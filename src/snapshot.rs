//! Phase 3 — snapshot du workspace entier avant une opération risquée
//! (au-delà du `.bak.<timestamp>` par-fichier déjà fait dans `write_file`).
//! Complète `exec_command` : l'IA peut appeler `snapshot_workspace` avant
//! une commande destructive dont elle ne maîtrise pas tous les effets de
//! bord (ex: un script de migration), puis proposer un rollback en cas
//! d'échec (rollback = copie manuelle de la snapshot, pas encore automatisé
//! ici — voir note en fin de fichier).

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct SnapshotInfo {
    pub id: String,
    pub path: String,
    pub created_at: String,
}

pub struct SnapshotManager {
    workspace_root: PathBuf,
    snapshots_dir: PathBuf,
}

impl SnapshotManager {
    pub fn new(workspace_root: PathBuf, snapshots_dir: PathBuf) -> Self {
        Self { workspace_root, snapshots_dir }
    }

    /// tool: snapshot_workspace
    /// Copie récursive du workspace dans snapshots_dir/<timestamp>/.
    /// Implémentation volontairement simple (copie fichier par fichier) —
    /// pour un gros workspace, remplacer par un appel à `tar` via
    /// `tokio::process::Command` serait plus rapide, à faire si nécessaire.
    pub async fn create(&self, label: Option<&str>) -> Result<SnapshotInfo> {
        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let id = match label {
            Some(l) => format!("{timestamp}_{l}"),
            None => timestamp.clone(),
        };
        let dest = self.snapshots_dir.join(&id);

        tokio::fs::create_dir_all(&dest).await.context("création du dossier de snapshot")?;
        copy_dir_recursive(&self.workspace_root, &dest).await?;

        Ok(SnapshotInfo {
            id,
            path: dest.to_string_lossy().into_owned(),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    pub async fn list(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if !self.snapshots_dir.exists() {
            return Ok(out);
        }
        let mut entries = tokio::fs::read_dir(&self.snapshots_dir).await?;
        while let Some(e) = entries.next_entry().await? {
            out.push(e.file_name().to_string_lossy().into_owned());
        }
        out.sort();
        Ok(out)
    }
}

fn copy_dir_recursive<'a>(src: &'a Path, dst: &'a Path) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let target = dst.join(entry.file_name());
            if file_type.is_dir() {
                // On ignore les snapshots imbriqués si snapshots_dir est sous workspace_root.
                if entry.path() == dst {
                    continue;
                }
                tokio::fs::create_dir_all(&target).await?;
                copy_dir_recursive(&entry.path(), &target).await?;
            } else if file_type.is_file() {
                tokio::fs::copy(entry.path(), &target).await?;
            }
        }
        Ok(())
    })
}

// Rollback automatique (restaurer une snapshot par-dessus le workspace)
// n'est pas implémenté : c'est une opération destructive par nature (elle
// écrase l'état courant), donc elle doit passer par le même circuit de
// confirmation que exec_command plutôt qu'être un simple aller-retour de
// fichiers. À brancher en Phase 3.5 une fois le circuit de confirmation
// validé en usage réel.
