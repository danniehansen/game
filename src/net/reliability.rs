use std::{collections::VecDeque, time::Duration};

use crate::protocol::PacketSequence;

pub(super) const RELIABLE_RESEND_INTERVAL: Duration = Duration::from_millis(250);

const RECEIVED_PACKET_HISTORY: usize = 256;

#[derive(Debug, Default)]
pub(super) struct ReceivedPacketWindow {
    latest: PacketSequence,
    history: VecDeque<PacketSequence>,
}

impl ReceivedPacketWindow {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn with_initial(sequence: PacketSequence) -> Self {
        let mut window = Self::new();
        window.record(sequence);
        window
    }

    pub(super) fn latest(&self) -> PacketSequence {
        self.latest
    }

    pub(super) fn record(&mut self, sequence: PacketSequence) -> bool {
        self.latest = sequence;
        if self.history.contains(&sequence) {
            return false;
        }

        self.history.push_back(sequence);
        while self.history.len() > RECEIVED_PACKET_HISTORY {
            self.history.pop_front();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_latest_and_flags_duplicates() {
        let mut window = ReceivedPacketWindow::new();

        assert!(window.record(3));
        assert_eq!(window.latest(), 3);
        assert!(!window.record(3));
        assert_eq!(window.latest(), 3);
    }
}
