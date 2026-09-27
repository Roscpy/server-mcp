//! Gère le cycle de vie d'une demande de confirmation humaine pour une
//! action sensible (commande hors whitelist, suppression de fichier, etc.).
//!
//! Flux :
//!   1. Le tool détecte qu'une confirmation est requise, crée une entrée
//!      `Pending` via `ConfirmationStore::create`, notifie (prompt MCP natif
//!      et/ou Telegram), puis attend via `wait_for_decision` avec un délai
//!      d'expiration.
//!   2. La décision arrive soit par le canal MCP natif (si le client la
//!      supporte), soit via le webhook Telegram (`src/telegram.rs`), qui
//!      appelle `ConfirmationStore::resolve`.
//!   3. `wait_for_decision` se débloque dès que la décision est posée, ou
//!      renvoie `Decision::Expired` après le délai.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approved,
    Denied,
    Expired,
}

pub struct PendingConfirmation {
    pub description: String,
    responder: Option<oneshot::Sender<Decision>>,
}

#[derive(Clone)]
pub struct ConfirmationStore {
    inner: Arc<Mutex<HashMap<String, PendingConfirmation>>>,
}

impl ConfirmationStore {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Crée une demande et renvoie (id, receiver) — le receiver se résout
    /// quand `resolve()` est appelé avec cet id, ou après `timeout`.
    pub async fn create(&self, description: impl Into<String>) -> (String, oneshot::Receiver<Decision>) {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.inner.lock().await.insert(
            id.clone(),
            PendingConfirmation { description: description.into(), responder: Some(tx) },
        );
        (id, rx)
    }

    /// Appelé par le canal externe (Telegram) ou natif MCP quand l'humain a tranché.
    pub async fn resolve(&self, id: &str, decision: Decision) -> bool {
        if let Some(mut pending) = self.inner.lock().await.remove(id) {
            if let Some(tx) = pending.responder.take() {
                let _ = tx.send(decision);
                return true;
            }
        }
        false
    }

    pub async fn describe(&self, id: &str) -> Option<String> {
        self.inner.lock().await.get(id).map(|p| p.description.clone())
    }

    /// Bloque jusqu'à décision ou expiration après `timeout`.
    pub async fn wait_for_decision(rx: oneshot::Receiver<Decision>, timeout: Duration) -> Decision {
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(decision)) => decision,
            _ => Decision::Expired,
        }
    }
}
