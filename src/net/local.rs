use std::collections::VecDeque;

use anyhow::Result;

use crate::{
    protocol::{ClientId, ClientMessage, PROTOCOL_VERSION, SERVER_TICK_RATE_HZ, ServerMessage},
    save::{WorldSave, WorldStore},
    server::{DeliveryTarget, GameServer, ServerEnvelope, ServerSettings},
    steam::{AuthMode, AuthenticatedUser},
};

const MAX_LOCAL_SERVER_TICKS_PER_FRAME: usize = 5;

#[derive(Debug)]
pub struct LocalGameSession {
    server: GameServer,
    client_id: ClientId,
    inbox: VecDeque<ServerMessage>,
    tick_accumulator: f32,
}

impl LocalGameSession {
    pub fn start(save: WorldSave, user: &AuthenticatedUser) -> Result<Self> {
        let mut server = GameServer::new(
            save,
            ServerSettings {
                auth_mode: AuthMode::Offline,
                singleplayer_host: Some(user.steam_id),
            },
        );

        let (client_id, envelopes) = server.connect(
            PROTOCOL_VERSION,
            user.steam_id,
            user.display_name.clone(),
            user.token.clone(),
        )?;

        let mut session = Self {
            server,
            client_id,
            inbox: VecDeque::new(),
            tick_accumulator: 0.0,
        };
        session.ingest(envelopes);
        Ok(session)
    }

    pub fn send(&mut self, message: ClientMessage) {
        let envelopes = self.server.receive(self.client_id, message);
        self.ingest(envelopes);
    }

    pub fn tick(&mut self, delta_seconds: f32) {
        let fixed_delta = 1.0 / SERVER_TICK_RATE_HZ;
        let max_accumulator = fixed_delta * MAX_LOCAL_SERVER_TICKS_PER_FRAME as f32;
        self.tick_accumulator =
            (self.tick_accumulator + delta_seconds.max(0.0)).min(max_accumulator);

        while self.tick_accumulator >= fixed_delta {
            let envelopes = self.server.tick(fixed_delta);
            self.ingest(envelopes);
            self.tick_accumulator -= fixed_delta;
        }
    }

    pub fn drain(&mut self) -> Vec<ServerMessage> {
        self.inbox.drain(..).collect()
    }

    pub fn persist(&self, store: &WorldStore) -> Result<()> {
        store.save_world(&self.server.world_save())
    }

    fn ingest(&mut self, envelopes: Vec<ServerEnvelope>) {
        for envelope in envelopes {
            match envelope.target {
                DeliveryTarget::Client(client_id) if client_id == self.client_id => {
                    self.inbox.push_back(envelope.message);
                }
                DeliveryTarget::Broadcast => {
                    self.inbox.push_back(envelope.message);
                }
                DeliveryTarget::Client(_) => {}
            }
        }
    }
}
