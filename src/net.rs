mod client;
mod codec;
mod dedicated;
mod local;
mod reliability;

pub use client::{ClientSession, UdpClient};
pub use dedicated::run_dedicated_server;
pub use local::LocalGameSession;

const MAX_PACKET_SIZE: usize = 16 * 1024;

#[cfg(test)]
mod tests;
