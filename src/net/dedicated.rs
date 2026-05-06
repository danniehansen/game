use std::{
    collections::HashMap,
    io::ErrorKind,
    net::{SocketAddr, UdpSocket},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::{
    net::{
        MAX_PACKET_SIZE,
        codec::decode,
        codec::encode,
        reliability::{RELIABLE_RESEND_INTERVAL, ReceivedPacketWindow},
    },
    protocol::{
        ClientId, ClientMessage, ClientPacket, PROTOCOL_VERSION, PacketDelivery, PacketSequence,
        SERVER_TICK_RATE_HZ, ServerMessage, ServerPacket,
    },
    save::WorldSave,
    server::{DeliveryTarget, GameServer, ServerEnvelope, ServerSettings},
    steam::{AuthMode, OfflineSteamBackend, ServerRegistrationRequest, SteamBackend},
};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MOVEMENT_MESSAGES_PER_SECOND: u32 = 240;
const MAX_PACKETS_PER_SERVER_TICK: usize = 256;

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
    clients: HashMap<ClientId, DedicatedClient>,
    next_server_sequence: PacketSequence,
    packets_processed_this_tick: usize,
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
            clients: HashMap::new(),
            next_server_sequence: 1,
            packets_processed_this_tick: 0,
        }
    }

    fn run(&mut self) -> Result<()> {
        let fixed_delta = 1.0 / SERVER_TICK_RATE_HZ;
        let tick_interval = Duration::from_secs_f32(fixed_delta);
        let mut next_tick = Instant::now();

        loop {
            let now = Instant::now();
            self.receive_packets(now)?;

            let now = Instant::now();
            if now >= next_tick {
                self.packets_processed_this_tick = 0;
                let timeout_envelopes = self.disconnect_timed_out(now);
                self.dispatch(timeout_envelopes)?;

                let envelopes = self.server.tick(fixed_delta);
                self.dispatch(envelopes)?;
                next_tick = now + tick_interval;
            }

            self.resend_reliable(now)?;
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn receive_packets(&mut self, now: Instant) -> Result<()> {
        let mut buffer = [0_u8; MAX_PACKET_SIZE];
        while self.packets_processed_this_tick < MAX_PACKETS_PER_SERVER_TICK {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    self.packets_processed_this_tick += 1;
                    self.handle_packet(addr, &buffer[..len], now)?;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error).context("could not receive server UDP packet"),
            }
        }

        Ok(())
    }

    fn handle_packet(&mut self, addr: SocketAddr, bytes: &[u8], now: Instant) -> Result<()> {
        let packet: ClientPacket = decode(bytes)?;
        if packet.protocol_version != PROTOCOL_VERSION {
            self.send_untracked(
                addr,
                packet.sequence,
                ServerMessage::AuthRejected {
                    reason: format!(
                        "protocol mismatch: client {}, server {PROTOCOL_VERSION}",
                        packet.protocol_version
                    ),
                },
            )?;
            return Ok(());
        }

        let sequence = packet.sequence;
        let ack = packet.ack;
        let Some(message) = packet.into_message() else {
            return Ok(());
        };

        if let Some(client_id) = self.addr_to_client.get(&addr).copied() {
            let is_new_packet = self.record_client_packet(client_id, sequence, ack, now);
            if !is_new_packet {
                return Ok(());
            }

            self.handle_authenticated_message(client_id, message, now)?;
            return Ok(());
        }

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
                    self.clients
                        .insert(client_id, DedicatedClient::new(addr, now, sequence));
                    self.dispatch(envelopes)?;
                }
                Err(error) => {
                    self.send_untracked(
                        addr,
                        sequence,
                        ServerMessage::AuthRejected {
                            reason: error.to_string(),
                        },
                    )?;
                }
            },
            _ => {
                self.send_untracked(
                    addr,
                    sequence,
                    ServerMessage::AuthRejected {
                        reason: "client is not authenticated".to_owned(),
                    },
                )?;
            }
        }

        Ok(())
    }

    fn handle_authenticated_message(
        &mut self,
        client_id: ClientId,
        message: ClientMessage,
        now: Instant,
    ) -> Result<()> {
        match message {
            ClientMessage::Auth { .. } => {}
            ClientMessage::Heartbeat => {
                self.send_to_client(client_id, ServerMessage::Heartbeat)?;
            }
            ClientMessage::Movement(movement) => {
                if self.allow_movement(client_id, now) {
                    let envelopes = self
                        .server
                        .receive(client_id, ClientMessage::Movement(movement));
                    self.dispatch(envelopes)?;
                }
            }
            ClientMessage::Chat { text } => {
                let envelopes = self.server.receive(client_id, ClientMessage::Chat { text });
                self.dispatch(envelopes)?;
            }
            ClientMessage::Disconnect => {
                let envelopes = self.server.receive(client_id, ClientMessage::Disconnect);
                self.dispatch(envelopes)?;
                self.remove_network_client(client_id);
            }
        }

        Ok(())
    }

    fn record_client_packet(
        &mut self,
        client_id: ClientId,
        sequence: PacketSequence,
        ack: PacketSequence,
        now: Instant,
    ) -> bool {
        self.clients
            .get_mut(&client_id)
            .is_some_and(|client| client.record_packet(sequence, ack, now))
    }

    fn allow_movement(&mut self, client_id: ClientId, now: Instant) -> bool {
        self.clients
            .get_mut(&client_id)
            .is_some_and(|client| client.movement_rate_limiter.allow(now))
    }

    fn disconnect_timed_out(&mut self, now: Instant) -> Vec<ServerEnvelope> {
        let timed_out = self
            .clients
            .iter()
            .filter_map(|(client_id, client)| {
                (now.duration_since(client.last_heard) >= CLIENT_TIMEOUT).then_some(*client_id)
            })
            .collect::<Vec<_>>();

        let mut envelopes = Vec::new();
        for client_id in timed_out {
            self.remove_network_client(client_id);
            envelopes.extend(self.server.disconnect(client_id));
        }

        envelopes
    }

    fn remove_network_client(&mut self, client_id: ClientId) {
        if let Some(client) = self.clients.remove(&client_id) {
            self.addr_to_client.remove(&client.addr);
        }
    }

    fn dispatch(&mut self, envelopes: Vec<ServerEnvelope>) -> Result<()> {
        let mut sends = Vec::new();
        for envelope in envelopes {
            match envelope.target {
                DeliveryTarget::Client(client_id) => {
                    if let Some(send) = self.queue_server_packet(client_id, envelope.message) {
                        sends.push(send);
                    }
                }
                DeliveryTarget::Broadcast => {
                    let client_ids = self.clients.keys().copied().collect::<Vec<_>>();
                    for client_id in client_ids {
                        if let Some(send) =
                            self.queue_server_packet(client_id, envelope.message.clone())
                        {
                            sends.push(send);
                        }
                    }
                }
            }
        }

        self.send_packets(sends)
    }

    fn send_to_client(&mut self, client_id: ClientId, message: ServerMessage) -> Result<()> {
        let Some(send) = self.queue_server_packet(client_id, message) else {
            return Ok(());
        };
        self.send_packets(vec![send])
    }

    fn queue_server_packet(
        &mut self,
        client_id: ClientId,
        message: ServerMessage,
    ) -> Option<(SocketAddr, ServerPacket)> {
        let sequence = self.next_server_sequence;
        self.next_server_sequence += 1;

        let client = self.clients.get_mut(&client_id)?;
        let packet = ServerPacket::new(sequence, client.received_packets.latest(), message);
        if packet.delivery == PacketDelivery::Reliable {
            client.pending_reliable.push(PendingServerPacket {
                sequence,
                message: packet.message.clone(),
                last_sent: Instant::now(),
            });
        }

        Some((client.addr, packet))
    }

    fn send_untracked(
        &mut self,
        addr: SocketAddr,
        ack: PacketSequence,
        message: ServerMessage,
    ) -> Result<()> {
        let packet = ServerPacket::new(self.next_server_sequence, ack, message);
        self.next_server_sequence += 1;
        self.send_packets(vec![(addr, packet)])
    }

    fn resend_reliable(&mut self, now: Instant) -> Result<()> {
        let mut sends = Vec::new();
        for client in self.clients.values_mut() {
            for pending in &mut client.pending_reliable {
                if now.duration_since(pending.last_sent) < RELIABLE_RESEND_INTERVAL {
                    continue;
                }

                pending.last_sent = now;
                sends.push((
                    client.addr,
                    ServerPacket::new(
                        pending.sequence,
                        client.received_packets.latest(),
                        pending.message.clone(),
                    ),
                ));
            }
        }

        self.send_packets(sends)
    }

    fn send_packets(&self, packets: Vec<(SocketAddr, ServerPacket)>) -> Result<()> {
        for (addr, packet) in packets {
            send_server_packet(&self.socket, addr, &packet)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
struct DedicatedClient {
    addr: SocketAddr,
    last_heard: Instant,
    received_packets: ReceivedPacketWindow,
    pending_reliable: Vec<PendingServerPacket>,
    movement_rate_limiter: MovementRateLimiter,
}

impl DedicatedClient {
    fn new(addr: SocketAddr, now: Instant, initial_sequence: PacketSequence) -> Self {
        Self {
            addr,
            last_heard: now,
            received_packets: ReceivedPacketWindow::with_initial(initial_sequence),
            pending_reliable: Vec::new(),
            movement_rate_limiter: MovementRateLimiter::new(now),
        }
    }

    fn record_packet(
        &mut self,
        sequence: PacketSequence,
        ack: PacketSequence,
        now: Instant,
    ) -> bool {
        self.last_heard = now;
        self.pending_reliable
            .retain(|pending| pending.sequence != ack);
        self.received_packets.record(sequence)
    }
}

#[derive(Debug, Clone)]
struct PendingServerPacket {
    sequence: PacketSequence,
    message: ServerMessage,
    last_sent: Instant,
}

#[derive(Debug)]
struct MovementRateLimiter {
    window_started: Instant,
    count: u32,
}

impl MovementRateLimiter {
    fn new(now: Instant) -> Self {
        Self {
            window_started: now,
            count: 0,
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window_started) >= Duration::from_secs(1) {
            self.window_started = now;
            self.count = 0;
        }

        if self.count >= MAX_MOVEMENT_MESSAGES_PER_SECOND {
            return false;
        }

        self.count += 1;
        true
    }
}

fn send_server_packet(socket: &UdpSocket, addr: SocketAddr, packet: &ServerPacket) -> Result<()> {
    let bytes = encode(packet)?;
    socket
        .send_to(&bytes, addr)
        .with_context(|| format!("could not send UDP packet to {addr}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_rate_limiter_caps_and_resets() {
        let start = Instant::now();
        let mut limiter = MovementRateLimiter::new(start);

        for _ in 0..MAX_MOVEMENT_MESSAGES_PER_SECOND {
            assert!(limiter.allow(start));
        }
        assert!(!limiter.allow(start));
        assert!(limiter.allow(start + Duration::from_secs(1)));
    }

    #[test]
    fn client_records_duplicate_packets_and_exact_acks() {
        let now = Instant::now();
        let addr = "127.0.0.1:7777".parse().expect("addr should parse");
        let mut client = DedicatedClient::new(addr, now, 1);
        client.pending_reliable.push(PendingServerPacket {
            sequence: 7,
            message: ServerMessage::Heartbeat,
            last_sent: now,
        });

        assert!(client.record_packet(2, 7, now));
        assert!(client.pending_reliable.is_empty());
        assert!(!client.record_packet(2, 0, now));
    }
}
