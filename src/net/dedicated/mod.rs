use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;

use crate::{
    save::{WorldSave, WorldStore, save_world_file},
    steam::AuthMode,
};

use super::host::run_game_server;

#[derive(Debug, Clone)]
pub enum DedicatedWorldPersistence {
    Store(WorldStore),
    File(PathBuf),
}

impl DedicatedWorldPersistence {
    fn save(&self, world: &WorldSave) -> Result<()> {
        match self {
            Self::Store(store) => store.save_world(world),
            Self::File(path) => save_world_file(path, world),
        }
    }
}

pub fn run_dedicated_server(
    bind_addr: SocketAddr,
    save: WorldSave,
    auth_mode: AuthMode,
    persistence: DedicatedWorldPersistence,
) -> Result<()> {
    let final_save = run_game_server(bind_addr, save, auth_mode)?;
    persistence.save(&final_save)
}
