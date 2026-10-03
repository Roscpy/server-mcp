use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub filesystem: FilesystemConfig,
    pub audit: AuditConfig,
    pub commands: CommandsConfig,
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub logs: LogsConfig,
    #[serde(default)]
    pub snapshots: SnapshotsConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SecurityConfig {
    /// Sous-chaînes interdites dans une commande exec_command, quelle que
    /// soit la whitelist ou une confirmation humaine. Vérifié en premier,
    /// avant toute autre logique — dernier filet de sécurité.
    #[serde(default)]
    pub blocked_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LogsConfig {
    #[serde(default)]
    pub allowed_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotsConfig {
    #[serde(default = "default_snapshots_dir")]
    pub dir: PathBuf,
}

impl Default for SnapshotsConfig {
    fn default() -> Self {
        Self { dir: default_snapshots_dir() }
    }
}

fn default_snapshots_dir() -> PathBuf {
    PathBuf::from("/var/lib/mcp-vps-server/snapshots")
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub transport: String,
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
}

fn default_bind_addr() -> String {
    "0.0.0.0:8787".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub bearer_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FilesystemConfig {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuditConfig {
    pub log_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandsConfig {
    #[serde(default)]
    pub whitelist: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub chat_id: String,
    #[serde(default)]
    pub webhook_secret: String,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("impossible de lire le fichier de config: {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&raw)
            .with_context(|| "config.toml invalide — vérifie la syntaxe TOML")?;

        if let Ok(env_token) = std::env::var("MCP_VPS_TOKEN") {
            if !env_token.is_empty() {
                cfg.auth.bearer_token = env_token;
            }
        }

        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.server.transport == "stdio" || self.server.transport == "http",
            "server.transport doit valoir \"stdio\" ou \"http\""
        );
        anyhow::ensure!(
            self.auth.bearer_token != "change-me-before-deploy",
            "auth.bearer_token n'a pas été changé — refuse de démarrer avec la valeur par défaut"
        );
        anyhow::ensure!(
            self.filesystem.workspace_root.is_absolute(),
            "filesystem.workspace_root doit être un chemin absolu"
        );
        Ok(())
    }
}
