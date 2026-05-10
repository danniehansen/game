use std::{
    sync::mpsc::{self, Sender},
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Context, Result};

use crate::save::WorldSave;

const HOST_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(in crate::net) struct SpawnedGameServer {
    pub(in crate::net) addr: std::net::SocketAddr,
    pub(in crate::net) handle: GameServerHandle,
}

pub(in crate::net) struct GameServerHandle {
    command_tx: Sender<HostCommand>,
    thread: Option<JoinHandle<()>>,
}

impl GameServerHandle {
    pub(super) fn new(command_tx: Sender<HostCommand>, thread: JoinHandle<()>) -> Self {
        Self {
            command_tx,
            thread: Some(thread),
        }
    }

    pub(in crate::net) fn world_save(&self) -> Result<WorldSave> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(HostCommand::WorldSave(reply_tx))
            .context("game server host is not running")?;
        reply_rx
            .recv_timeout(HOST_COMMAND_TIMEOUT)
            .context("game server host did not return a world save")
    }

    pub(in crate::net) fn shutdown(&mut self) -> Result<()> {
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

impl std::fmt::Debug for GameServerHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GameServerHandle")
            .field("running", &self.thread.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for GameServerHandle {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Debug)]
pub(super) enum HostCommand {
    WorldSave(Sender<WorldSave>),
    Shutdown(Sender<()>),
}
