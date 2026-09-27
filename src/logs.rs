//! Phase 3 — lecture de logs pour alimenter le diagnostic/self-healing.
//!
//! Le "self-healing itératif" de la roadmap (2.3) n'est pas une boucle
//! implémentée côté serveur : c'est le client MCP (l'IA) qui orchestre
//! exec_command -> lit stderr -> write_file pour corriger -> exec_command à
//! nouveau. Le rôle du serveur ici se limite à fournir un accès en lecture
//! seule et confiné aux logs système (hors workspace_root, donc hors du
//! confinement de `tools::confine_path`) — d'où l'allow-list explicite
//! `allowed_log_paths` plutôt qu'un accès filesystem libre.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Clone)]
pub struct LogTools {
    allowed_log_paths: Vec<PathBuf>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TailFileParams {
    pub path: String,
    #[serde(default = "default_lines")]
    pub lines: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ServiceLogsParams {
    pub unit: String,
    #[serde(default = "default_lines")]
    pub lines: usize,
}

fn default_lines() -> usize {
    200
}

#[derive(Debug, Serialize)]
pub struct LogOutput {
    pub content: String,
}

impl LogTools {
    pub fn new(allowed_log_paths: Vec<PathBuf>) -> Self {
        Self { allowed_log_paths }
    }

    /// tool: tail_file — lit les N dernières lignes d'un fichier de log
    /// explicitement autorisé (config.toml -> [logs] allowed_paths, à
    /// ajouter — non présent dans config.example.toml actuel, à ajouter
    /// toi-même selon les chemins de logs de ton setup, ex:
    /// /var/log/nginx/error.log).
    pub async fn tail_file(&self, params: TailFileParams) -> Result<LogOutput> {
        let target = PathBuf::from(&params.path);
        if !self.allowed_log_paths.iter().any(|p| &target == p) {
            bail!("chemin de log non autorisé: {} (ajoute-le à [logs] allowed_paths)", params.path);
        }

        let file = tokio::fs::File::open(&target).await?;
        let reader = BufReader::new(file);
        let mut all_lines = Vec::new();
        let mut stream = reader.lines();
        while let Some(line) = stream.next_line().await? {
            all_lines.push(line);
        }
        let start = all_lines.len().saturating_sub(params.lines);
        Ok(LogOutput { content: all_lines[start..].join("\n") })
    }

    /// tool: service_logs — wrapper autour de `journalctl -u <unit> -n <lines> --no-pager`.
    /// Lecture seule, pas de whitelist nécessaire côté exec_command (chemin
    /// dédié), mais le nom d'unité est passé tel quel à journalctl — à
    /// restreindre à une liste connue si le serveur est exposé à plusieurs
    /// opérateurs (Phase 4 durcissement).
    pub async fn service_logs(&self, params: ServiceLogsParams) -> Result<LogOutput> {
        let output = Command::new("journalctl")
            .arg("-u")
            .arg(&params.unit)
            .arg("-n")
            .arg(params.lines.to_string())
            .arg("--no-pager")
            .output()
            .await?;

        if !output.status.success() {
            bail!("journalctl a échoué: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(LogOutput { content: String::from_utf8_lossy(&output.stdout).into_owned() })
    }
}
