use std::{
    collections::{HashMap, VecDeque},
    io::ErrorKind,
    net::{SocketAddr, UdpSocket},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::{
    protocol::{ClientId, ClientMessage, PROTOCOL_VERSION, SERVER_TICK_RATE_HZ, ServerMessage},
    save::{WorldSave, WorldStore},
    server::{DeliveryTarget, GameServer, ServerEnvelope, ServerSettings},
    steam::{
        AuthMode, AuthenticatedUser, OfflineSteamBackend, ServerRegistrationRequest, SteamBackend,
    },
};

const MAX_PACKET_SIZE: usize = 16 * 1024;

#[derive(Debug)]
pub enum ClientSession {
    Local(Box<LocalGameSession>),
    Udp(UdpClient),
}

impl ClientSession {
    pub fn start_singleplayer(save: WorldSave, user: &AuthenticatedUser) -> Result<Self> {
        LocalGameSession::start(save, user).map(|session| Self::Local(Box::new(session)))
    }

    pub fn connect_udp(server_addr: SocketAddr, user: &AuthenticatedUser) -> Result<Self> {
        UdpClient::connect(server_addr, user).map(Self::Udp)
    }

    pub fn send(&mut self, message: ClientMessage) -> Result<()> {
        match self {
            Self::Local(session) => {
                session.send(message);
                Ok(())
            }
            Self::Udp(session) => session.send(&message),
        }
    }

    pub fn tick(&mut self, delta_seconds: f32) -> Result<Vec<ServerMessage>> {
        match self {
            Self::Local(session) => {
                session.tick(delta_seconds);
                Ok(session.drain())
            }
            Self::Udp(session) => session.poll(),
        }
    }

    pub fn shutdown(&mut self, store: &WorldStore) -> Result<()> {
        let _ = self.send(ClientMessage::Disconnect);
        if let Self::Local(session) = self {
            session.persist(store)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct LocalGameSession {
    server: GameServer,
    client_id: ClientId,
    inbox: VecDeque<ServerMessage>,
}

impl LocalGameSession {
    pub fn start(save: WorldSave, user: &AuthenticatedUser) -> Result<Self> {
        let mut server = GameServer::new(
            save,
            ServerSettings {
                auth_mode: AuthMode::Offline,
                singleplayer_host: Some(user.steam_id),
            },
        );

        let (client_id, envelopes) = server.connect(
            PROTOCOL_VERSION,
            user.steam_id,
            user.display_name.clone(),
            user.token.clone(),
        )?;

        let mut session = Self {
            server,
            client_id,
            inbox: VecDeque::new(),
        };
        session.ingest(envelopes);
        Ok(session)
    }

    pub fn send(&mut self, message: ClientMessage) {
        let envelopes = self.server.receive(self.client_id, message);
        self.ingest(envelopes);
    }

    pub fn tick(&mut self, delta_seconds: f32) {
        let envelopes = self.server.tick(delta_seconds);
        self.ingest(envelopes);
    }

    pub fn drain(&mut self) -> Vec<ServerMessage> {
        self.inbox.drain(..).collect()
    }

    pub fn persist(&self, store: &WorldStore) -> Result<()> {
        store.save_world(&self.server.world_save())
    }

    fn ingest(&mut self, envelopes: Vec<ServerEnvelope>) {
        for envelope in envelopes {
            match envelope.target {
                DeliveryTarget::Client(client_id) if client_id == self.client_id => {
                    self.inbox.push_back(envelope.message);
                }
                DeliveryTarget::Broadcast => {
                    self.inbox.push_back(envelope.message);
                }
                DeliveryTarget::Client(_) => {}
            }
        }
    }
}

#[derive(Debug)]
pub struct UdpClient {
    socket: UdpSocket,
    server_addr: SocketAddr,
}

impl UdpClient {
    pub fn connect(server_addr: SocketAddr, user: &AuthenticatedUser) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").context("could not bind UDP client socket")?;
        socket
            .set_nonblocking(true)
            .context("could not set UDP client socket nonblocking")?;

        let client = Self {
            socket,
            server_addr,
        };
        client.send(&ClientMessage::Auth {
            protocol_version: PROTOCOL_VERSION,
            steam_id: user.steam_id,
            display_name: user.display_name.clone(),
            token: user.token.clone(),
        })?;
        Ok(client)
    }

    pub fn send(&self, message: &ClientMessage) -> Result<()> {
        let bytes = encode(message)?;
        self.socket
            .send_to(&bytes, self.server_addr)
            .context("could not send UDP packet")?;
        Ok(())
    }

    pub fn poll(&self) -> Result<Vec<ServerMessage>> {
        let mut messages = Vec::new();
        let mut buffer = [0_u8; MAX_PACKET_SIZE];

        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, _addr)) => messages.push(decode(&buffer[..len])?),
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error).context("could not receive UDP packet"),
            }
        }

        Ok(messages)
    }
}

pub fn run_dedicated_server(
    bind_addr: SocketAddr,
    save: WorldSave,
    auth_mode: AuthMode,
) -> Result<()> {
    let socket =
        UdpSocket::bind(bind_addr).with_context(|| format!("could not bind {bind_addr}"))?;
    socket
        .set_nonblocking(true)
        .context("could not set server socket nonblocking")?;

    let steam = OfflineSteamBackend;
    let registration = steam.register_server(&ServerRegistrationRequest {
        name: "Game".to_owned(),
        bind_addr: bind_addr.to_string(),
        map: save.name.clone(),
        max_players: 32,
    })?;
    println!("server registration: {}", registration.detail);
    println!("authoritative server listening on {bind_addr}");

    let mut runner = DedicatedServer::new(socket, save, auth_mode);
    runner.run()
}

struct DedicatedServer {
    socket: UdpSocket,
    server: GameServer,
    addr_to_client: HashMap<SocketAddr, ClientId>,
    client_to_addr: HashMap<ClientId, SocketAddr>,
}

impl DedicatedServer {
    fn new(socket: UdpSocket, save: WorldSave, auth_mode: AuthMode) -> Self {
        Self {
            socket,
            server: GameServer::new(
                save,
                ServerSettings {
                    auth_mode,
                    singleplayer_host: None,
                },
            ),
            addr_to_client: HashMap::new(),
            client_to_addr: HashMap::new(),
        }
    }

    fn run(&mut self) -> Result<()> {
        let fixed_delta = 1.0 / SERVER_TICK_RATE_HZ;
        let tick_interval = Duration::from_secs_f32(fixed_delta);
        let mut next_tick = Instant::now();

        loop {
            self.receive_packets()?;

            let now = Instant::now();
            if now >= next_tick {
                let envelopes = self.server.tick(fixed_delta);
                self.dispatch(envelopes)?;
                next_tick = now + tick_interval;
            }

            thread::sleep(Duration::from_millis(2));
        }
    }

    fn receive_packets(&mut self) -> Result<()> {
        let mut buffer = [0_u8; MAX_PACKET_SIZE];
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => self.handle_packet(addr, &buffer[..len])?,
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error).context("could not receive server UDP packet"),
            }
        }

        Ok(())
    }

    fn handle_packet(&mut self, addr: SocketAddr, bytes: &[u8]) -> Result<()> {
        let message: ClientMessage = decode(bytes)?;
        match message {
            ClientMessage::Auth {
                protocol_version,
                steam_id,
                display_name,
                token,
            } => match self
                .server
                .connect(protocol_version, steam_id, display_name, token)
            {
                Ok((client_id, envelopes)) => {
                    self.addr_to_client.insert(addr, client_id);
                    self.client_to_addr.insert(client_id, addr);
                    self.dispatch(envelopes)?;
                }
                Err(error) => {
                    self.send_to(
                        addr,
                        &ServerMessage::AuthRejected {
                            reason: error.to_string(),
                        },
                    )?;
                }
            },
            other => {
                let Some(client_id) = self.addr_to_client.get(&addr).copied() else {
                    self.send_to(
                        addr,
                        &ServerMessage::AuthRejected {
                            reason: "client is not authenticated".to_owned(),
                        },
                    )?;
                    return Ok(());
                };

                let disconnecting = matches!(other, ClientMessage::Disconnect);
                let envelopes = self.server.receive(client_id, other);
                self.dispatch(envelopes)?;
                if disconnecting {
                    self.addr_to_client.remove(&addr);
                    self.client_to_addr.remove(&client_id);
                }
            }
        }

        Ok(())
    }

    fn dispatch(&self, envelopes: Vec<ServerEnvelope>) -> Result<()> {
        for envelope in envelopes {
            match envelope.target {
                DeliveryTarget::Client(client_id) => {
                    if let Some(addr) = self.client_to_addr.get(&client_id) {
                        self.send_to(*addr, &envelope.message)?;
                    }
                }
                DeliveryTarget::Broadcast => {
                    for addr in self.client_to_addr.values() {
                        self.send_to(*addr, &envelope.message)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn send_to(&self, addr: SocketAddr, message: &ServerMessage) -> Result<()> {
        let bytes = encode(message)?;
        self.socket
            .send_to(&bytes, addr)
            .with_context(|| format!("could not send UDP packet to {addr}"))?;
        Ok(())
    }
}

fn encode<T: serde::Serialize>(message: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(message).context("could not encode network packet")
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).context("could not decode network packet")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{save::WorldSave, steam::offline_auth_token};

    #[test]
    fn local_session_receives_welcome_and_snapshots() {
        let user = AuthenticatedUser {
            steam_id: 1,
            display_name: "Host".to_owned(),
            token: offline_auth_token(1),
        };
        let mut session =
            LocalGameSession::start(WorldSave::new("Local", Some(user.steam_id)), &user)
                .expect("local session should start");

        let initial = session.drain();
        assert!(matches!(initial[0], ServerMessage::Welcome { .. }));

        session.tick(0.05);
        let tick = session.drain();
        assert!(
            tick.iter()
                .any(|message| matches!(message, ServerMessage::Snapshot(_)))
        );
    }
}
