use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use crate::{
    app, net,
    save::{WorldSave, WorldStore},
    steam::{AuthMode, OfflineSteamBackend, SteamBackend},
};

#[derive(Debug, Parser)]
#[command(name = "Game", version, about = "Game client and authoritative server")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Client,
    Server {
        #[arg(long, default_value = "127.0.0.1:7777")]
        bind: SocketAddr,
        #[arg(long)]
        world: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = AuthModeArg::Offline)]
        auth: AuthModeArg,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AuthModeArg {
    Offline,
    Steam,
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
    match args.command.unwrap_or(Command::Client) {
        Command::Client => app::run_app(),
        Command::Server { bind, world, auth } => {
            let save = load_server_world(world)?;
            net::run_dedicated_server(bind, save, auth.into())
        }
    }
}

fn load_server_world(path: Option<PathBuf>) -> Result<WorldSave> {
    if let Some(path) = path {
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read world save {}", path.display()))?;
        return serde_json::from_str(&json)
            .with_context(|| format!("could not parse world save {}", path.display()));
    }

    let steam = OfflineSteamBackend;
    let user = steam.current_user()?;
    WorldStore::platform_default()?.load_or_create_dedicated(Some(user.steam_id))
}
