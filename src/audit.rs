use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Une entrée d'audit — une ligne JSON par action.
/// Format volontairement plat pour être facile à streamer / grep / réinjecter
/// dans un modèle pour le diagnostic (cf. roadmap section 2.3).
#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: String,
    pub tool: String,
    pub params: serde_json::Value,
    pub outcome: AuditOutcome,
    pub requires_confirmation: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AuditOutcome {
    Success { summary: String },
    Denied { reason: String },
    Error { message: String },
    PendingConfirmation,
}

#[derive(Clone)]
pub struct AuditLog {
    path: Arc<std::path::PathBuf>,
    lock: Arc<Mutex<()>>,
}

impl AuditLog {
    pub async fn new(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("impossible de créer le dossier de logs: {}", parent.display()))?;
        }
        Ok(Self {
            path: Arc::new(path.to_path_buf()),
            lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn new_entry_id() -> String {
        Uuid::new_v4().to_string()
    }

    pub async fn record(&self, tool: &str, params: serde_json::Value, outcome: AuditOutcome, requires_confirmation: bool) -> Result<()> {
        let entry = AuditEntry {
            id: Self::new_entry_id(),
            timestamp: Utc::now().to_rfc3339(),
            tool: tool.to_string(),
            params,
            outcome,
            requires_confirmation,
        };
        let line = serde_json::to_string(&entry)?;

        let _guard = self.lock.lock().await;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&*self.path)
            .await
            .with_context(|| format!("impossible d'ouvrir le fichier d'audit: {}", self.path.display()))?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }
}
