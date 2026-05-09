use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        Mutex,
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result};
use bevy::prelude::*;
use lightyear::prelude::{
    Disconnected, LocalAddr, MessageReceiver, MessageSender,
    server::{self, ClientOf},
};

use crate::{
    protocol::{ClientId, ClientMessage, SERVER_TICK_RATE_HZ, ServerMessage},
    save::WorldSave,
    server::{DeliveryTarget, GameServer, ServerEnvelope, ServerSettings},
    steam::AuthMode,
};

use super::protocol::{
    LIGHTYEAR_PROTOCOL_ID, LightyearProtocolPlugin, private_key, send_server_message,
};

const HOST_SLEEP: Duration = Duration::from_millis(1);
const MAX_SERVER_TICKS_PER_LOOP: f32 = 5.0;
const HOST_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(super) struct SpawnedGameServer {
    pub(super) addr: SocketAddr,
    pub(super) handle: GameServerHandle,
}

pub(super) struct GameServerHandle {
    command_tx: Sender<HostCommand>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for GameServerHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GameServerHandle")
            .field("running", &self.thread.is_some())
            .finish_non_exhaustive()
    }
}

impl GameServerHandle {
    pub(super) fn world_save(&self) -> Result<WorldSave> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(HostCommand::WorldSave(reply_tx))
            .context("game server host is not running")?;
        reply_rx
            .recv_timeout(HOST_COMMAND_TIMEOUT)
            .context("game server host did not return a world save")
    }

    pub(super) fn shutdown(&mut self) -> Result<()> {
        if let Some(thread) = self.thread.take() {
            let (reply_tx, reply_rx) = mpsc::channel();
            let _ = self.command_tx.send(HostCommand::Shutdown(reply_tx));
            let _ = reply_rx.recv_timeout(HOST_COMMAND_TIMEOUT);
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("game server host thread panicked"))?;
        }
        Ok(())
    }
}

impl Drop for GameServerHandle {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Debug)]
enum HostCommand {
    WorldSave(Sender<WorldSave>),
    Shutdown(Sender<()>),
}

#[derive(Resource)]
struct HostCommandInbox(Mutex<mpsc::Receiver<HostCommand>>);

#[derive(Resource)]
struct AuthoritativeServer(GameServer);

#[derive(Resource, Default)]
struct ServerConnections {
    by_entity: HashMap<Entity, ClientId>,
    client_to_entity: HashMap<ClientId, Entity>,
}

#[derive(Resource, Default)]
struct TickAccumulator(Duration);

#[derive(Resource, Default)]
struct HostShutdown {
    requested: bool,
}

pub(super) fn spawn_loopback_server(
    save: WorldSave,
    settings: ServerSettings,
) -> Result<SpawnedGameServer> {
    let addr = reserve_udp_addr(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .context("could not reserve loopback Lightyear server address")?;
    let (command_tx, command_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("lightyear-game-server".to_owned())
        .spawn(move || {
            if let Err(error) = run_host(addr, save, settings, command_rx) {
                eprintln!("Lightyear game server stopped: {error:#}");
            }
        })
        .context("could not spawn loopback Lightyear game server")?;

    Ok(SpawnedGameServer {
        addr,
        handle: GameServerHandle {
            command_tx,
            thread: Some(thread),
        },
    })
}

pub(super) fn run_game_server(
    bind_addr: SocketAddr,
    save: WorldSave,
    auth_mode: AuthMode,
) -> Result<()> {
    let bind_addr = reserve_udp_addr(bind_addr)
        .with_context(|| format!("could not reserve Lightyear server address {bind_addr}"))?;
    let (_command_tx, command_rx) = mpsc::channel();
    println!("Lightyear game server listening on {bind_addr} ({auth_mode:?})");
    run_host(
        bind_addr,
        save,
        ServerSettings {
            auth_mode,
            singleplayer_host: None,
        },
        command_rx,
    )
}

fn reserve_udp_addr(addr: SocketAddr) -> Result<SocketAddr> {
    if addr.port() != 0 {
        return Ok(addr);
    }
    let socket = UdpSocket::bind(addr).with_context(|| format!("could not bind {addr}"))?;
    socket
        .local_addr()
        .context("could not read reserved UDP address")
}

fn run_host(
    bind_addr: SocketAddr,
    save: WorldSave,
    settings: ServerSettings,
    command_rx: mpsc::Receiver<HostCommand>,
) -> Result<()> {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(server::ServerPlugins {
        tick_duration: Duration::from_secs_f32(1.0 / SERVER_TICK_RATE_HZ),
    });
    app.add_plugins(LightyearProtocolPlugin);

    let server_entity = app
        .world_mut()
        .spawn((
            Name::new("Lightyear Game Server"),
            LocalAddr(bind_addr),
            server::ServerUdpIo::default(),
            server::NetcodeServer::new(
                server::NetcodeConfig::default()
                    .with_protocol_id(LIGHTYEAR_PROTOCOL_ID)
                    .with_key(private_key()),
            ),
        ))
        .id();

    app.insert_resource(HostCommandInbox(Mutex::new(command_rx)));
    app.insert_resource(AuthoritativeServer(GameServer::new(save, settings)));
    app.insert_resource(ServerConnections::default());
    app.insert_resource(TickAccumulator::default());
    app.insert_resource(HostShutdown::default());

    app.add_systems(Startup, move |mut commands: Commands| {
        commands.trigger(server::Start {
            entity: server_entity,
        });
    });
    app.add_systems(
        Update,
        (
            drain_host_commands,
            receive_client_messages,
            handle_disconnected_clients,
            tick_authoritative_server,
        )
            .chain(),
    );
    app.finish();
    app.cleanup();

    loop {
        app.update();
        if app.world().resource::<HostShutdown>().requested {
            return Ok(());
        }
        thread::sleep(HOST_SLEEP);
    }
}

fn drain_host_commands(
    inbox: Res<HostCommandInbox>,
    mut shutdown: ResMut<HostShutdown>,
    server: Res<AuthoritativeServer>,
) {
    let commands = {
        let Ok(receiver) = inbox.0.lock() else {
            shutdown.requested = true;
            return;
        };
        receiver.try_iter().collect::<Vec<_>>()
    };

    for command in commands {
        match command {
            HostCommand::WorldSave(reply_tx) => {
                let _ = reply_tx.send(server.0.world_save());
            }
            HostCommand::Shutdown(reply_tx) => {
                shutdown.requested = true;
                let _ = reply_tx.send(());
            }
        }
    }
}

fn receive_client_messages(
    mut server: ResMut<AuthoritativeServer>,
    mut connections: ResMut<ServerConnections>,
    mut receivers: Query<(Entity, &mut MessageReceiver<ClientMessage>), With<ClientOf>>,
    mut senders: Query<&mut MessageSender<ServerMessage>, With<ClientOf>>,
) {
    for (entity, mut receiver) in &mut receivers {
        let messages: Vec<ClientMessage> = receiver.receive().collect();
        for message in messages {
            handle_client_message(
                entity,
                message,
                &mut server.0,
                &mut connections,
                &mut senders,
            );
        }
    }
}

fn handle_client_message(
    entity: Entity,
    message: ClientMessage,
    server: &mut GameServer,
    connections: &mut ServerConnections,
    senders: &mut Query<&mut MessageSender<ServerMessage>, With<ClientOf>>,
) {
    let Some(client_id) = connections.by_entity.get(&entity).copied() else {
        handle_unauthenticated_message(entity, message, server, connections, senders);
        return;
    };

    if matches!(message, ClientMessage::Disconnect) {
        let envelopes = server.disconnect(client_id);
        forget_connection(entity, connections);
        route_envelopes(connections, senders, envelopes);
        return;
    }

    let envelopes = server.receive(client_id, message);
    route_envelopes(connections, senders, envelopes);
}

fn handle_unauthenticated_message(
    entity: Entity,
    message: ClientMessage,
    server: &mut GameServer,
    connections: &mut ServerConnections,
    senders: &mut Query<&mut MessageSender<ServerMessage>, With<ClientOf>>,
) {
    let ClientMessage::Auth {
        protocol_version,
        steam_id,
        display_name,
        token,
    } = message
    else {
        send_to_entity(
            senders,
            entity,
            ServerMessage::AuthRejected {
                reason: "client is not authenticated".to_owned(),
            },
        );
        return;
    };

    match server.connect(protocol_version, steam_id, display_name, token) {
        Ok((client_id, envelopes)) => {
            connections.by_entity.insert(entity, client_id);
            connections.client_to_entity.insert(client_id, entity);
            route_envelopes(connections, senders, envelopes);
        }
        Err(error) => {
            send_to_entity(
                senders,
                entity,
                ServerMessage::AuthRejected {
                    reason: error.to_string(),
                },
            );
        }
    }
}

fn handle_disconnected_clients(
    mut server: ResMut<AuthoritativeServer>,
    mut connections: ResMut<ServerConnections>,
    disconnected: Query<Entity, (With<ClientOf>, Added<Disconnected>)>,
    mut senders: Query<&mut MessageSender<ServerMessage>, With<ClientOf>>,
) {
    for entity in &disconnected {
        let Some(client_id) = forget_connection(entity, &mut connections) else {
            continue;
        };
        let envelopes = server.0.disconnect(client_id);
        route_envelopes(&connections, &mut senders, envelopes);
    }
}

fn tick_authoritative_server(
    time: Res<Time>,
    mut accumulator: ResMut<TickAccumulator>,
    mut server: ResMut<AuthoritativeServer>,
    connections: Res<ServerConnections>,
    mut senders: Query<&mut MessageSender<ServerMessage>, With<ClientOf>>,
) {
    let fixed_delta = Duration::from_secs_f32(1.0 / SERVER_TICK_RATE_HZ);
    let max_accumulator = fixed_delta.mul_f32(MAX_SERVER_TICKS_PER_LOOP);
    accumulator.0 = (accumulator.0 + time.delta()).min(max_accumulator);

    while accumulator.0 >= fixed_delta {
        let envelopes = server.0.tick(fixed_delta.as_secs_f32());
        route_envelopes(&connections, &mut senders, envelopes);
        accumulator.0 -= fixed_delta;
    }
}

fn forget_connection(entity: Entity, connections: &mut ServerConnections) -> Option<ClientId> {
    let client_id = connections.by_entity.remove(&entity)?;
    connections.client_to_entity.remove(&client_id);
    Some(client_id)
}

fn route_envelopes(
    connections: &ServerConnections,
    senders: &mut Query<&mut MessageSender<ServerMessage>, With<ClientOf>>,
    envelopes: Vec<ServerEnvelope>,
) {
    for envelope in envelopes {
        match envelope.target {
            DeliveryTarget::Client(client_id) => {
                if let Some(entity) = connections.client_to_entity.get(&client_id).copied() {
                    send_to_entity(senders, entity, envelope.message);
                }
            }
            DeliveryTarget::Broadcast => {
                let entities = connections.by_entity.keys().copied().collect::<Vec<_>>();
                for entity in entities {
                    send_to_entity(senders, entity, envelope.message.clone());
                }
            }
        }
    }
}

fn send_to_entity(
    senders: &mut Query<&mut MessageSender<ServerMessage>, With<ClientOf>>,
    entity: Entity,
    message: ServerMessage,
) {
    if let Ok(mut sender) = senders.get_mut(entity) {
        send_server_message(&mut sender, message);
    }
}
