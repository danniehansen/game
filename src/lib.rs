pub mod app;
pub mod cli;
pub mod net;
pub mod protocol;
pub mod save;
pub mod server;
pub mod steam;

pub fn run() -> anyhow::Result<()> {
    cli::run()
}
