use std::{
    io::ErrorKind,
    net::{SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::{
    net::{
        MAX_PACKET_SIZE,
        codec::decode,
        codec::encode,
        local::LocalGameSession,
        reliability::{RELIABLE_RESEND_INTERVAL, ReceivedPacketWindow},
    },
    protocol::{
        ClientMessage, ClientPacket, PROTOCOL_VERSION, PacketDelivery, PacketSequence,
        ServerMessage, ServerPacket,
    },
    save::{WorldSave, WorldStore},
    steam::AuthenticatedUser,
};

const CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

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
            Self::Udp(session) => session.send(message),
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
pub struct UdpClient {
    socket: UdpSocket,
    server_addr: SocketAddr,
    next_sequence: PacketSequence,
    received_server_packets: ReceivedPacketWindow,
    pending_reliable: Vec<PendingClientPacket>,
    last_heartbeat: Instant,
}

impl UdpClient {
    pub fn connect(server_addr: SocketAddr, user: &AuthenticatedUser) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").context("could not bind UDP client socket")?;
        socket
            .set_nonblocking(true)
            .context("could not set UDP client socket nonblocking")?;

        let mut client = Self {
            socket,
            server_addr,
            next_sequence: 1,
            received_server_packets: ReceivedPacketWindow::new(),
            pending_reliable: Vec::new(),
            last_heartbeat: Instant::now(),
        };
        client.send(ClientMessage::Auth {
            protocol_version: PROTOCOL_VERSION,
            steam_id: user.steam_id,
            display_name: user.display_name.clone(),
            token: user.token.clone(),
        })?;
        Ok(client)
    }

    pub fn send(&mut self, message: ClientMessage) -> Result<()> {
        let packet = self.build_packet(message);
        self.send_packet(&packet)?;

        if packet.delivery == PacketDelivery::Reliable {
            self.pending_reliable.push(PendingClientPacket {
                packet,
                last_sent: Instant::now(),
            });
        }

        Ok(())
    }

    pub fn poll(&mut self) -> Result<Vec<ServerMessage>> {
        let now = Instant::now();
        self.send_heartbeat(now)?;
        self.resend_reliable(now)?;

        let mut messages = Vec::new();
        let mut buffer = [0_u8; MAX_PACKET_SIZE];

        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) if addr == self.server_addr => {
                    if let Some(message) = self.handle_packet(&buffer[..len])? {
                        messages.push(message);
                    }
                }
                Ok((_len, _addr)) => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error).context("could not receive UDP packet"),
            }
        }

        Ok(messages)
    }

    fn build_packet(&mut self, message: ClientMessage) -> ClientPacket {
        let packet = ClientPacket::new(
            self.next_sequence,
            self.received_server_packets.latest(),
            message,
        );
        self.next_sequence += 1;
        packet
    }

    fn send_packet(&self, packet: &ClientPacket) -> Result<()> {
        let bytes = encode(packet)?;
        self.socket
            .send_to(&bytes, self.server_addr)
            .context("could not send UDP packet")?;
        Ok(())
    }

    fn handle_packet(&mut self, bytes: &[u8]) -> Result<Option<ServerMessage>> {
        let packet: ServerPacket = decode(bytes)?;
        self.acknowledge(packet.ack);

        if packet.protocol_version != PROTOCOL_VERSION {
            return Ok(Some(ServerMessage::AuthRejected {
                reason: "protocol mismatch".to_owned(),
            }));
        }

        let is_new = self.received_server_packets.record(packet.sequence);
        let Some(message) = packet.into_message() else {
            return Ok(None);
        };

        if is_new { Ok(Some(message)) } else { Ok(None) }
    }

    fn acknowledge(&mut self, sequence: PacketSequence) {
        self.pending_reliable
            .retain(|pending| pending.packet.sequence != sequence);
    }

    fn send_heartbeat(&mut self, now: Instant) -> Result<()> {
        if now.duration_since(self.last_heartbeat) < CLIENT_HEARTBEAT_INTERVAL {
            return Ok(());
        }

        self.last_heartbeat = now;
        let packet = self.build_packet(ClientMessage::Heartbeat);
        self.send_packet(&packet)
    }

    fn resend_reliable(&mut self, now: Instant) -> Result<()> {
        let packets = self
            .pending_reliable
            .iter_mut()
            .filter_map(|pending| {
                (now.duration_since(pending.last_sent) >= RELIABLE_RESEND_INTERVAL).then(|| {
                    pending.packet.ack = self.received_server_packets.latest();
                    pending.last_sent = now;
                    pending.packet.clone()
                })
            })
            .collect::<Vec<_>>();

        for packet in packets {
            self.send_packet(&packet)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct PendingClientPacket {
    packet: ClientPacket,
    last_sent: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_ack_removes_only_matching_reliable_packet() {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("socket should bind");
        let server_addr = socket.local_addr().expect("socket should have addr");
        let mut client = UdpClient {
            socket,
            server_addr,
            next_sequence: 3,
            received_server_packets: ReceivedPacketWindow::new(),
            pending_reliable: vec![
                PendingClientPacket {
                    packet: ClientPacket::new(
                        1,
                        0,
                        ClientMessage::Chat {
                            text: "first".to_owned(),
                        },
                    ),
                    last_sent: Instant::now(),
                },
                PendingClientPacket {
                    packet: ClientPacket::new(
                        2,
                        0,
                        ClientMessage::Chat {
                            text: "second".to_owned(),
                        },
                    ),
                    last_sent: Instant::now(),
                },
            ],
            last_heartbeat: Instant::now(),
        };

        client.acknowledge(1);

        assert_eq!(client.pending_reliable.len(), 1);
        assert_eq!(client.pending_reliable[0].packet.sequence, 2);
    }
}
