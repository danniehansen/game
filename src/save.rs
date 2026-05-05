use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::protocol::SteamId;

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "Game";
const APPLICATION: &str = "Game";

#[derive(Debug, Clone)]
pub struct WorldStore {
    root: PathBuf,
}

impl WorldStore {
    pub fn platform_default() -> Result<Self> {
        let project_dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .context("could not resolve the platform data directory")?;
        Ok(Self::new(project_dirs.data_dir().join("worlds")))
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_exists(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("could not create world directory {}", self.root.display()))
    }

    pub fn list_worlds(&self) -> Result<Vec<WorldSummary>> {
        self.ensure_exists()?;

        let mut worlds = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("could not read world directory {}", self.root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let save = self.load_world_file(&path)?;
            worlds.push(WorldSummary::from_save(&save, path));
        }

        worlds.sort_by(|a, b| {
            b.created_at_unix
                .cmp(&a.created_at_unix)
                .then(a.name.cmp(&b.name))
        });
        Ok(worlds)
    }

    pub fn create_world(&self, name: &str, owner_steam_id: Option<SteamId>) -> Result<WorldSave> {
        self.ensure_exists()?;

        let save = WorldSave::new(name, owner_steam_id);
        self.save_world(&save)?;
        Ok(save)
    }

    pub fn load_world(&self, id: Uuid) -> Result<WorldSave> {
        self.load_world_file(&self.world_path(id))
    }

    pub fn save_world(&self, save: &WorldSave) -> Result<()> {
        self.ensure_exists()?;

        let path = self.world_path(save.id);
        let json = serde_json::to_string_pretty(save).context("could not serialize world save")?;
        fs::write(&path, json).with_context(|| format!("could not write world {}", path.display()))
    }

    pub fn delete_world(&self, id: Uuid) -> Result<()> {
        let path = self.world_path(id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("could not delete world {}", path.display()))?;
        }
        Ok(())
    }

    pub fn load_or_create_dedicated(&self, owner_steam_id: Option<SteamId>) -> Result<WorldSave> {
        let worlds = self.list_worlds()?;
        if let Some(world) = worlds.into_iter().find(|world| world.name == "Dedicated") {
            return self.load_world(world.id);
        }

        self.create_world("Dedicated", owner_steam_id)
    }

    fn world_path(&self, id: Uuid) -> PathBuf {
        self.root.join(format!("{id}.json"))
    }

    fn load_world_file(&self, path: &Path) -> Result<WorldSave> {
        let json = fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        serde_json::from_str(&json).with_context(|| format!("could not parse {}", path.display()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldSave {
    pub id: Uuid,
    pub name: String,
    pub seed: u64,
    pub created_at_unix: u64,
    pub admins: Vec<SteamId>,
    pub state: WorldStateSave,
}

impl WorldSave {
    pub fn new(name: &str, owner_steam_id: Option<SteamId>) -> Self {
        let id = Uuid::new_v4();
        let mut admins = Vec::new();
        if let Some(owner_steam_id) = owner_steam_id {
            admins.push(owner_steam_id);
        }

        Self {
            id,
            name: normalize_world_name(name),
            seed: seed_from_uuid(id),
            created_at_unix: now_unix(),
            admins,
            state: WorldStateSave::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WorldStateSave {
    pub last_authoritative_tick: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorldSummary {
    pub id: Uuid,
    pub name: String,
    pub seed: u64,
    pub created_at_unix: u64,
    pub admin_count: usize,
    pub path: PathBuf,
}

impl WorldSummary {
    fn from_save(save: &WorldSave, path: PathBuf) -> Self {
        Self {
            id: save.id,
            name: save.name.clone(),
            seed: save.seed,
            created_at_unix: save.created_at_unix,
            admin_count: save.admins.len(),
            path,
        }
    }
}

fn normalize_world_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "New World".to_owned()
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn seed_from_uuid(id: Uuid) -> u64 {
    let raw = id.as_u128();
    (raw as u64) ^ ((raw >> 64) as u64)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> WorldStore {
        WorldStore::new(std::env::temp_dir().join(format!("game-save-test-{}", Uuid::new_v4())))
    }

    #[test]
    fn create_load_and_delete_world() {
        let store = temp_store();
        let save = store
            .create_world("  Test World  ", Some(123))
            .expect("world should be created");

        assert_eq!(save.name, "Test World");
        assert_eq!(save.admins, vec![123]);

        let loaded = store.load_world(save.id).expect("world should load");
        assert_eq!(loaded.id, save.id);

        let listed = store.list_worlds().expect("world list should load");
        assert_eq!(listed.len(), 1);

        store.delete_world(save.id).expect("world should delete");
        assert!(
            store
                .list_worlds()
                .expect("world list should load")
                .is_empty()
        );

        let _ = fs::remove_dir_all(store.root());
    }
}
