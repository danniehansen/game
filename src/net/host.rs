mod handle;
mod routing;

use std::{
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use bevy::{app::TerminalCtrlCHandlerPlugin, prelude::*};
use lightyear::prelude::{
    LocalAddr, MessageSender,
    server::{self, ClientOf},
};

use crate::{
    protocol::{SERVER_TICK_RATE_HZ, ServerMessage},
    save::WorldSave,
    server::{GameServer, ServerSettings},
    steam::AuthMode,
};

pub(super) use self::handle::{GameServerHandle, SpawnedGameServer};
use self::{
    handle::HostCommand,
    routing::{
        ServerConnections, handle_disconnected_clients, receive_client_messages, route_envelopes,
    },
};
use super::protocol::{LIGHTYEAR_PROTOCOL_ID, LightyearProtocolPlugin, private_key};

const HOST_SLEEP: Duration = Duration::from_millis(1);
const MAX_SERVER_TICKS_PER_LOOP: f32 = 5.0;

#[derive(Resource)]
pub(super) struct AuthoritativeServer(GameServer);

#[derive(Resource)]
struct HostCommandInbox(Mutex<mpsc::Receiver<HostCommand>>);

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
            if let Err(error) = run_host(addr, save, settings, command_rx, false) {
                eprintln!("Lightyear game server stopped: {error:#}");
            }
        })
        .context("could not spawn loopback Lightyear game server")?;

    Ok(SpawnedGameServer {
        addr,
        handle: GameServerHandle::new(command_tx, thread),
    })
}

pub(super) fn run_game_server(
    bind_addr: SocketAddr,
    save: WorldSave,
    auth_mode: AuthMode,
) -> Result<WorldSave> {
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
        true,
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
    install_terminal_shutdown: bool,
) -> Result<WorldSave> {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    if install_terminal_shutdown {
        app.add_plugins(TerminalCtrlCHandlerPlugin);
    }
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
        if host_should_shutdown(&app) {
            return Ok(app.world().resource::<AuthoritativeServer>().0.world_save());
        }
        thread::sleep(HOST_SLEEP);
    }
}

fn host_should_shutdown(app: &App) -> bool {
    app.world().resource::<HostShutdown>().requested || app.should_exit().is_some()
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
