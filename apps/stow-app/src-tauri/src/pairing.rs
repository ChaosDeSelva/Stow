//! Extension pairing requests waiting for the user's Allow/Deny. At most one is pending.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use tokio::sync::oneshot;

#[derive(Default)]
pub struct PairingState {
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl PairingState {
    /// Open a request; `None` while another one is still waiting.
    pub fn begin(&self) -> Option<(String, oneshot::Receiver<bool>)> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if !pending.is_empty() {
            return None;
        }
        let id = stow_core::ids::new_id();
        let (tx, rx) = oneshot::channel();
        pending.insert(id.clone(), tx);
        Some((id, rx))
    }

    /// Deliver the user's decision. False when no such request is waiting.
    pub fn respond(&self, id: &str, allow: bool) -> bool {
        let tx = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
        tx.is_some_and(|tx| tx.send(allow).is_ok())
    }

    /// Drop a request whose HTTP caller gave up or timed out.
    pub fn cancel(&self, id: &str) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pairing_allows_one_pending_and_delivers_decision() {
        let p = PairingState::default();
        let (id, rx) = p.begin().expect("first request");
        assert!(p.begin().is_none(), "second request must wait");
        assert!(!p.respond("unknown", true));
        assert!(p.respond(&id, true));
        assert!(rx.await.unwrap());
        assert!(!p.respond(&id, false), "already answered");
        let (id2, rx2) = p.begin().expect("slot free again");
        assert_ne!(id, id2);
        assert!(p.respond(&id2, false));
        assert!(!rx2.await.unwrap());
    }

    #[test]
    fn pairing_cancel_frees_slot() {
        let p = PairingState::default();
        let (id, rx) = p.begin().unwrap();
        p.cancel(&id);
        drop(rx);
        assert!(!p.respond(&id, true));
        assert!(p.begin().is_some());
    }

    #[test]
    fn pairing_respond_after_caller_gone_is_false() {
        let p = PairingState::default();
        let (id, rx) = p.begin().unwrap();
        drop(rx);
        assert!(!p.respond(&id, true));
        assert!(p.begin().is_some());
    }
}
