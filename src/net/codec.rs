use anyhow::{Context, Result};

pub(super) fn encode<T: serde::Serialize>(message: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(message).context("could not encode network packet")
}

pub(super) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).context("could not decode network packet")
}
