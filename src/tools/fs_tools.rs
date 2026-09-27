//! Tools MCP de la Phase 1 (lecture/écriture non destructive).
//!
//! NB API rmcp: la macro `#[tool]` / `#[tool_router]` et la signature exacte
//! de `ServerHandler` ont bougé plusieurs fois pendant que le SDK était
//! < 1.0. Vérifie la forme actuelle sur https://docs.rs/rmcp avant de
//! compiler — la structure ci-dessous (un struct de state + une méthode par
//! tool + retour JSON) reste le squelette correct quel que soit le détail
//! de macro exact.

use super::confine_path;
use crate::audit::{AuditLog, AuditOutcome};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone)]
pub struct FsTools {
    workspace_root: PathBuf,
    audit: AuditLog,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadFileParams {
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WriteFileParams {
    pub path: String,
    pub content: String,
    /// Si true et que le fichier existe déjà, une sauvegarde .bak
    /// horodatée est créée avant écrasement (cf. roadmap 2.4).
    #[serde(default = "default_true")]
    pub backup_if_exists: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListDirParams {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

impl FsTools {
    pub fn new(workspace_root: PathBuf, audit: AuditLog) -> Self {
        Self { workspace_root, audit }
    }

    /// tool: read_file
    pub async fn read_file(&self, params: ReadFileParams) -> Result<String> {
        let target = confine_path(&self.workspace_root, &params.path)
            .context("chemin invalide pour read_file")?;

        let result = tokio::fs::read_to_string(&target).await;
        match &result {
            Ok(content) => {
                self.audit
                    .record(
                        "read_file",
                        serde_json::json!({ "path": params.path }),
                        AuditOutcome::Success { summary: format!("{} octets lus", content.len()) },
                        false,
                    )
                    .await
                    .ok();
            }
            Err(e) => {
                self.audit
                    .record(
                        "read_file",
                        serde_json::json!({ "path": params.path }),
                        AuditOutcome::Error { message: e.to_string() },
                        false,
                    )
                    .await
                    .ok();
            }
        }
        result.map_err(|e| e.into())
    }

    /// tool: write_file
    /// Non destructif par défaut: sauvegarde .bak.<timestamp> avant d'écraser.
    pub async fn write_file(&self, params: WriteFileParams) -> Result<String> {
        let target = confine_path(&self.workspace_root, &params.path)
            .context("chemin invalide pour write_file")?;

        if params.backup_if_exists && target.exists() {
            let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
            let backup_path = target.with_extension(format!(
                "{}.bak.{}",
                target.extension().and_then(|e| e.to_str()).unwrap_or(""),
                stamp
            ));
            tokio::fs::copy(&target, &backup_path).await.ok();
        }

        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        let result = tokio::fs::write(&target, &params.content).await;
        match &result {
            Ok(_) => {
                self.audit
                    .record(
                        "write_file",
                        serde_json::json!({ "path": params.path, "bytes": params.content.len() }),
                        AuditOutcome::Success { summary: "fichier écrit".to_string() },
                        false,
                    )
                    .await
                    .ok();
            }
            Err(e) => {
                self.audit
                    .record(
                        "write_file",
                        serde_json::json!({ "path": params.path }),
                        AuditOutcome::Error { message: e.to_string() },
                        false,
                    )
                    .await
                    .ok();
            }
        }
        result.map(|_| "ok".to_string()).map_err(|e| e.into())
    }

    /// tool: list_dir
    pub async fn list_dir(&self, params: ListDirParams) -> Result<Vec<DirEntry>> {
        let target = confine_path(&self.workspace_root, &params.path)
            .context("chemin invalide pour list_dir")?;

        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&target).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let meta = entry.metadata().await?;
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size_bytes: meta.len(),
            });
        }

        self.audit
            .record(
                "list_dir",
                serde_json::json!({ "path": params.path }),
                AuditOutcome::Success { summary: format!("{} entrées", entries.len()) },
                false,
            )
            .await
            .ok();

        Ok(entries)
    }
}
