mod multiplayer_test;

use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use crate::{
    app, net,
    save::{WorldSave, WorldStore, load_world_file, save_world_file},
    steam::{AuthMode, OfflineSteamBackend, SteamBackend},
    world_time::parse_time_token,
};

use self::multiplayer_test::run_multiplayer_test;

const DEFAULT_ADMIN_SOCKET: &str = "/run/game-server/admin.sock";
const DEFAULT_SHUTDOWN_REASON: &str =
    "Server is stopping for maintenance. Please reconnect after it restarts.";

#[derive(Debug, Parser)]
#[command(name = "Game", version, about = "Game client and authoritative server")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Client {
        /// When set, skip the main menu and connect directly to the given
        /// address as soon as the client window is ready. Used by the
        /// `multiplayer-test` helper so spawned windows enter the test
        /// world without any clicking.
        #[arg(long)]
        connect: Option<SocketAddr>,
    },
    Server {
        #[arg(long, default_value = "127.0.0.1:7777")]
        bind: SocketAddr,
        #[arg(long)]
        world: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = AuthModeArg::Offline)]
        auth: AuthModeArg,
        #[arg(long)]
        admin_socket: Option<PathBuf>,
    },
    Admin {
        #[arg(long, default_value = DEFAULT_ADMIN_SOCKET)]
        socket: PathBuf,
        #[command(subcommand)]
        command: AdminCommand,
    },
    /// Developer helper: launch a fresh local server with a brand-new test
    /// world and two client windows that auto-connect with distinct names.
    /// Use to exercise multiplayer visuals (movement, nametags, chat
    /// bubbles, player models) without manual menu work.
    MultiplayerTest {
        /// Port the temporary server listens on. Defaults to a free port.
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Names assigned to the two test clients. Pass twice to override
        /// both, once to override the first. Defaults: `Alpha`, `Bravo`.
        #[arg(long, num_args = 1..=2)]
        names: Option<Vec<String>>,
    },
}

#[derive(Debug, Subcommand)]
enum AdminCommand {
    Announce {
        #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
        message: Vec<String>,
    },
    Shutdown {
        #[arg(long, default_value = DEFAULT_SHUTDOWN_REASON)]
        reason: String,
    },
    /// Set the day/night clock. Accepts `HH:MM` or an integer/decimal
    /// hour (`/admin time 18` for 6 pm).
    Time { time: String },
    /// Set the day/night cycle speed multiplier. `1.0` is the default
    /// (one cycle per 30 real minutes). `0` pauses the cycle.
    Speed { multiplier: f32 },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AuthModeArg {
    Offline,
    Steam,
}

struct ServerWorld {
    save: WorldSave,
    persistence: net::DedicatedWorldPersistence,
}

impl From<AuthModeArg> for AuthMode {
    fn from(value: AuthModeArg) -> Self {
        match value {
            AuthModeArg::Offline => Self::Offline,
            AuthModeArg::Steam => Self::Steam,
        }
    }
}

pub fn run() -> Result<()> {
    let args = Args::parse();
    match args.command.unwrap_or(Command::Client { connect: None }) {
        Command::Client { connect } => app::run_app(connect),
        Command::Server {
            bind,
            world,
            auth,
            admin_socket,
        } => {
            let world = load_server_world(world)?;
            net::run_dedicated_server(
                bind,
                world.save,
                auth.into(),
                world.persistence,
                admin_socket,
            )
        }
        Command::Admin { socket, command } => run_admin_command(socket, command),
        Command::MultiplayerTest { port, names } => run_multiplayer_test(port, names),
    }
}

fn load_server_world(path: Option<PathBuf>) -> Result<ServerWorld> {
    if let Some(path) = path {
        let save = if path.exists() {
            match load_world_file(&path) {
                Ok(save) => save,
                Err(error) => {
                    // Dedicated servers run unattended — when a save format
                    // version bump (or any other unreadable state) makes the
                    // existing file unloadable, drop it and start fresh
                    // rather than crash-looping on every restart. There is no
                    // migration path for save format bumps yet.
                    eprintln!(
                        "could not load world save {}: {error:#}. Replacing with a fresh world.",
                        path.display()
                    );
                    std::fs::remove_file(&path).with_context(|| {
                        format!("could not remove unloadable world save {}", path.display())
                    })?;
                    let save = WorldSave::new("Dedicated File", None);
                    save_world_file(&path, &save)?;
                    save
                }
            }
        } else {
            let save = WorldSave::new("Dedicated File", None);
            save_world_file(&path, &save)?;
            save
        };
        return Ok(ServerWorld {
            save,
            persistence: net::DedicatedWorldPersistence::File(path),
        });
    }

    let steam = OfflineSteamBackend;
    let user = steam.current_user()?;
    let store = WorldStore::platform_default()?;
    let save = store.load_or_create_dedicated(Some(user.steam_id))?;
    Ok(ServerWorld {
        save,
        persistence: net::DedicatedWorldPersistence::Store(store),
    })
}

fn run_admin_command(socket: PathBuf, command: AdminCommand) -> Result<()> {
    let request = match command {
        AdminCommand::Announce { message } => net::DedicatedAdminRequest::Announce {
            text: message.join(" "),
        },
        AdminCommand::Shutdown { reason } => net::DedicatedAdminRequest::Shutdown { reason },
        AdminCommand::Time { time } => {
            let Some(seconds_of_day) = parse_time_token(&time) else {
                anyhow::bail!("could not parse '{time}'; expected HH:MM or an hour like 14");
            };
            net::DedicatedAdminRequest::SetTime { seconds_of_day }
        }
        AdminCommand::Speed { multiplier } => {
            net::DedicatedAdminRequest::SetTimeMultiplier { multiplier }
        }
    };
    let response = net::send_dedicated_admin_request(&socket, request)
        .with_context(|| format!("could not send admin command to {}", socket.display()))?;
    println!("{}", response.message);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;
    use crate::net::DedicatedWorldPersistence;

    fn temp_world_path() -> PathBuf {
        std::env::temp_dir().join(format!("game-cli-world-test-{}.save", Uuid::new_v4()))
    }

    #[test]
    fn load_server_world_creates_fresh_save_when_path_missing() {
        let path = temp_world_path();
        let world = load_server_world(Some(path.clone())).expect("fresh world should load");

        assert!(matches!(
            world.persistence,
            DedicatedWorldPersistence::File(_)
        ));
        assert!(path.exists(), "save file should have been created");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn load_server_world_replaces_unloadable_save_with_fresh_world() {
        let path = temp_world_path();
        fs::write(&path, b"not a real save file").expect("garbage save should be written");

        let world =
            load_server_world(Some(path.clone())).expect("unloadable save should be replaced");

        // The fresh save should be loadable on a second call, proving the
        // unreadable file was removed and a valid one written in its place.
        let reloaded = load_server_world(Some(path.clone())).expect("fresh save should reload");
        assert_eq!(world.save.id, reloaded.save.id);

        let _ = fs::remove_file(&path);
    }
}
