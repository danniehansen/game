use std::net::SocketAddr;

use anyhow::Result;

use crate::{save::WorldSave, steam::AuthMode};

use super::host::run_game_server;

pub fn run_dedicated_server(
    bind_addr: SocketAddr,
    save: WorldSave,
    auth_mode: AuthMode,
) -> Result<()> {
    run_game_server(bind_addr, save, auth_mode)
}
