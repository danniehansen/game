use bevy::prelude::*;
use uuid::Uuid;

use crate::{
    controller::PlayerController,
    net::ClientSession,
    protocol::{
        ChatMessage, ClientId, PlayerEvent, PlayerState, ServerMessage, Vec3Net, WorldSnapshot,
    },
    save::{WorldStore, WorldSummary},
    steam::AuthenticatedUser,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    MainMenu,
    Worlds,
    Multiplayer,
    InGame,
}

#[derive(Resource)]
pub(crate) struct SaveStore(pub(crate) WorldStore);

#[derive(Resource)]
pub(crate) struct SteamUser(pub(crate) AuthenticatedUser);

#[derive(Resource)]
pub(crate) struct MenuState {
    pub(crate) screen: Screen,
    pub(crate) worlds: Vec<WorldSummary>,
    pub(crate) new_world_name: String,
    pub(crate) multiplayer_addr: String,
    pub(crate) status: Option<String>,
    pub(crate) pause_open: bool,
    pub(crate) chat_input: String,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            screen: Screen::MainMenu,
            worlds: Vec::new(),
            new_world_name: "New World".to_owned(),
            multiplayer_addr: "127.0.0.1:7777".to_owned(),
            status: None,
            pause_open: false,
            chat_input: String::new(),
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct ClientRuntime {
    pub(crate) session: Option<ClientSession>,
    pub(crate) active_world_id: Option<Uuid>,
    pub(crate) client_id: Option<ClientId>,
    pub(crate) is_admin: bool,
    pub(crate) snapshot: Option<WorldSnapshot>,
    pub(crate) predicted_local: Option<PlayerController>,
    pub(crate) messages: Vec<String>,
    pub(crate) input_sequence: u64,
}

impl ClientRuntime {
    pub(crate) fn start_session(&mut self, session: ClientSession, world_id: Option<Uuid>) {
        self.session = Some(session);
        self.active_world_id = world_id;
        self.client_id = None;
        self.is_admin = false;
        self.snapshot = None;
        self.predicted_local = None;
        self.messages.clear();
        self.input_sequence = 0;
    }

    pub(crate) fn shutdown(&mut self, store: &WorldStore) {
        if let Some(session) = self.session.as_mut()
            && let Err(error) = session.shutdown(store)
        {
            self.messages.push(format!("save/shutdown error: {error}"));
        }

        self.session = None;
        self.active_world_id = None;
        self.client_id = None;
        self.snapshot = None;
        self.predicted_local = None;
        self.is_admin = false;
    }

    pub(crate) fn apply_message(&mut self, message: ServerMessage) {
        match message {
            ServerMessage::Welcome {
                client_id,
                is_admin,
                snapshot,
                ..
            } => {
                self.client_id = Some(client_id);
                self.is_admin = is_admin;
                self.sync_prediction_from_snapshot(&snapshot, true);
                self.snapshot = Some(snapshot);
                self.messages
                    .push(format!("connected as player {client_id}"));
            }
            ServerMessage::AuthRejected { reason } => {
                self.messages.push(format!("auth rejected: {reason}"));
            }
            ServerMessage::PlayerEvent(event) => self.messages.push(format_player_event(event)),
            ServerMessage::Snapshot(snapshot) => {
                self.sync_prediction_from_snapshot(&snapshot, false);
                self.snapshot = Some(snapshot);
            }
            ServerMessage::Chat(ChatMessage { from, text }) => {
                self.messages.push(format!("{from}: {text}"));
            }
        }

        if self.messages.len() > 80 {
            let drain_count = self.messages.len() - 80;
            self.messages.drain(0..drain_count);
        }
    }

    pub(crate) fn local_player(&self) -> Option<&PlayerState> {
        let client_id = self.client_id?;
        self.snapshot
            .as_ref()?
            .players
            .iter()
            .find(|player| player.client_id == client_id)
    }

    pub(crate) fn local_view(&self) -> Option<LocalPlayerView> {
        if let Some(predicted) = &self.predicted_local {
            return Some(LocalPlayerView {
                position: predicted.position,
                health: predicted.health,
                stamina: predicted.stamina,
            });
        }

        let player = self.local_player()?;
        Some(LocalPlayerView {
            position: player.position,
            health: player.health,
            stamina: player.stamina,
        })
    }

    fn sync_prediction_from_snapshot(&mut self, snapshot: &WorldSnapshot, force: bool) {
        let Some(client_id) = self.client_id else {
            return;
        };
        let Some(server_player) = snapshot
            .players
            .iter()
            .find(|player| player.client_id == client_id)
        else {
            return;
        };

        if force || self.predicted_local.is_none() {
            self.predicted_local = Some(PlayerController::from_player_state(server_player));
            return;
        }

        if let Some(predicted) = self.predicted_local.as_mut() {
            predicted.reconcile(server_player);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalPlayerView {
    pub(crate) position: Vec3Net,
    pub(crate) health: f32,
    pub(crate) stamina: f32,
}

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct LookState {
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) sensitivity: Vec2,
}

impl Default for LookState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: -0.04,
            sensitivity: Vec2::new(0.0024, 0.0020),
        }
    }
}

fn format_player_event(event: PlayerEvent) -> String {
    match event {
        PlayerEvent::Joined { name, .. } => format!("{name} joined"),
        PlayerEvent::Left { name, .. } => format!("{name} left"),
    }
}
