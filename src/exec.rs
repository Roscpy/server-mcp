//! Phase 2 — exécution de commandes shell avec whitelist + confirmation.
//!
//! Politique volontairement stricte pour la sécurité :
//! - Pas d'interprétation shell (pas de `sh -c`, pas de pipes/redirections/
//!   substitutions). Chaque commande est tokenisée puis exécutée directement
//!   via `tokio::process::Command`. Si tu as besoin de pipes, whiteliste un
//!   script dédié plutôt que d'activer un shell complet.
//! - Le matching whitelist est une égalité exacte sur la commande complète
//!   (ex: "cargo build"), pas un prefix-match — un prefix-match sur des
//!   commandes shell est une source classique de contournement
//!   (ex: whitelister "git" permettrait "git config --global ..."). À
//!   discuter si tu veux un modèle par sous-commande plus fin en Phase 2.5.

use crate::audit::{AuditLog, AuditOutcome};
use crate::confirmation::{ConfirmationStore, Decision};
use crate::config::TelegramConfig;
use crate::telegram;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::Mutex;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecCommandParams {
    /// Commande complète, ex: "cargo build" ou "systemctl restart nginx"
    pub command: String,
    /// Si la commande était hors-whitelist et a été approuvée, l'ajoute
    /// durablement à la whitelist en mémoire (persistée au redémarrage
    /// suivant si tu branches la sauvegarde vers config.toml — non fait
    /// automatiquement ici pour éviter d'écrire un fichier de config à
    /// l'aveugle).
    #[serde(default)]
    pub remember_if_approved: bool,
}

#[derive(Debug, Serialize)]
pub struct ExecCommandResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone)]
pub struct ExecTools {
    workspace_root: PathBuf,
    whitelist: Arc<Mutex<Vec<String>>>,
    audit: AuditLog,
    confirmations: ConfirmationStore,
    telegram_cfg: TelegramConfig,
    confirmation_timeout: Duration,
}

impl ExecTools {
    pub fn new(
        workspace_root: PathBuf,
        initial_whitelist: Vec<String>,
        audit: AuditLog,
        confirmations: ConfirmationStore,
        telegram_cfg: TelegramConfig,
    ) -> Self {
        Self {
            workspace_root,
            whitelist: Arc::new(Mutex::new(initial_whitelist)),
            audit,
            confirmations,
            telegram_cfg,
            confirmation_timeout: Duration::from_secs(300),
        }
    }

    pub fn confirmation_store(&self) -> ConfirmationStore {
        self.confirmations.clone()
    }

    /// tool: exec_command
    pub async fn exec_command(&self, params: ExecCommandParams) -> Result<ExecCommandResult> {
        let is_whitelisted = self.whitelist.lock().await.iter().any(|c| c == &params.command);

        if !is_whitelisted {
            let approved = self.request_confirmation(&params.command).await?;
            if !approved {
                self.audit
                    .record(
                        "exec_command",
                        serde_json::json!({ "command": params.command }),
                        AuditOutcome::Denied { reason: "refusé ou expiré".to_string() },
                        true,
                    )
                    .await
                    .ok();
                bail!("commande refusée ou confirmation expirée: {}", params.command);
            }
            if params.remember_if_approved {
                self.whitelist.lock().await.push(params.command.clone());
            }
        }

        self.run(&params.command, !is_whitelisted).await
    }

    async fn request_confirmation(&self, command: &str) -> Result<bool> {
        if !self.telegram_cfg.enabled {
            return Ok(false);
        }
        let (id, rx) = self.confirmations.create(format!("exec_command: {command}")).await;

        // Canal 1: notification Telegram si activé.
        if let Err(e) = telegram::notify_pending(&self.telegram_cfg, &id, &format!("Commande: `{command}`")).await {
            tracing::warn!(error = %e, "échec notification Telegram (on continue, le prompt MCP natif reste possible)");
        }

        // Canal 2 (prompt MCP natif): à brancher dans le handler — si le
        // client MCP supporte les elicitations, il peut appeler
        // ConfirmationStore::resolve(id, ...) directement depuis le handler
        // `#[tool]` correspondant. Voir TODO dans src/handler.rs.

        let decision = ConfirmationStore::wait_for_decision(rx, self.confirmation_timeout).await;
        Ok(decision == Decision::Approved)
    }

    async fn run(&self, command: &str, was_confirmed: bool) -> Result<ExecCommandResult> {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or_else(|| anyhow::anyhow!("commande vide"))?;
        let args: Vec<&str> = parts.collect();

        let output = Command::new(program)
            .args(&args)
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        let result = ExecCommandResult {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };

        self.audit
            .record(
                "exec_command",
                serde_json::json!({ "command": command }),
                AuditOutcome::Success { summary: format!("exit_code={:?}", result.exit_code) },
                was_confirmed,
            )
            .await
            .ok();

        Ok(result)
    }
}

impl ExecTools {
    pub async fn is_whitelisted(&self, command: &str) -> bool {
        self.whitelist.lock().await.iter().any(|c| c == command)
    }

    /// Exécute une commande déjà approuvée par un humain via le canal natif.
    pub async fn run_approved(&self, params: ExecCommandParams) -> Result<ExecCommandResult> {
        if params.remember_if_approved {
            self.whitelist.lock().await.push(params.command.clone());
        }
        self.run(&params.command, true).await
    }

    pub async fn record_denied(&self, command: &str, reason: &str) {
        self.audit
            .record(
                "exec_command",
                serde_json::json!({ "command": command }),
                AuditOutcome::Denied { reason: reason.to_string() },
                true,
            )
            .await
            .ok();
    }
}
