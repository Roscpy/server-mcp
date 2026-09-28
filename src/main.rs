mod audit;
mod auth;
mod config;
mod confirmation;
mod exec;
mod handler;
mod logs;
mod monitor;
mod native_confirm;
mod oauth;
mod snapshot;
mod telegram;
mod tools;

use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use clap::Parser;
use confirmation::{ConfirmationStore, Decision};
use exec::ExecTools;
use handler::McpVpsHandler;
use logs::LogTools;
use monitor::ResourceMonitor;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::ServiceExt;
use snapshot::SnapshotManager;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tools::fs_tools::FsTools;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "mcp-vps-server")]
struct Cli {
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,
    #[arg(long)]
    transport: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let cli = Cli::parse();
    let mut cfg = config::Config::load(&cli.config)
        .with_context(|| format!("échec du chargement de {}", cli.config.display()))?;
    if let Some(t) = cli.transport {
        cfg.server.transport = t;
    }

    std::fs::create_dir_all(&cfg.filesystem.workspace_root).context("création workspace_root")?;
    std::fs::create_dir_all(&cfg.snapshots.dir).context("création snapshots.dir")?;

    let audit_log = audit::AuditLog::new(&cfg.audit.log_path).await?;
    let confirmations = ConfirmationStore::new();

    let fs_tools = Arc::new(FsTools::new(cfg.filesystem.workspace_root.clone(), audit_log.clone()));
    let exec_tools = Arc::new(ExecTools::new(
        cfg.filesystem.workspace_root.clone(),
        cfg.commands.whitelist.clone(),
        audit_log.clone(),
        confirmations.clone(),
        cfg.telegram.clone(),
    ));
    let log_tools = Arc::new(LogTools::new(cfg.logs.allowed_paths.clone()));
    let snapshot_manager = Arc::new(SnapshotManager::new(cfg.filesystem.workspace_root.clone(), cfg.snapshots.dir.clone()));
    let monitor = Arc::new(Mutex::new(ResourceMonitor::new()));

    let mcp_handler = McpVpsHandler::new(fs_tools, exec_tools.clone(), log_tools, snapshot_manager, monitor);

    match cfg.server.transport.as_str() {
        "stdio" => run_stdio(mcp_handler).await,
        "http" => run_http(mcp_handler, exec_tools, &cfg).await,
        other => anyhow::bail!("transport inconnu: {other} (attendu: stdio | http)"),
    }
}

async fn run_stdio(handler: McpVpsHandler) -> Result<()> {
    tracing::info!("démarrage en mode stdio");
    let service = handler.serve(stdio()).await.context("échec du démarrage rmcp (stdio)")?;
    service.waiting().await.context("le service rmcp s'est arrêté avec une erreur")?;
    Ok(())
}

async fn run_http(handler: McpVpsHandler, exec_tools: Arc<ExecTools>, cfg: &config::Config) -> Result<()> {
    // URL publique (tunnel ou domaine), utilisée par OAuth. Ex: MCP_PUBLIC_URL=https://xxx.trycloudflare.com
    let base_url = std::env::var("MCP_PUBLIC_URL")
        .unwrap_or_else(|_| format!("http://{}", cfg.server.bind_addr))
        .trim_end_matches('/')
        .to_string();
    tracing::info!(addr = %cfg.server.bind_addr, public_url = %base_url, "démarrage en mode http");

    let oauth_state = oauth::OAuthState {
        secret: Arc::new(cfg.auth.bearer_token.clone()),
        base_url: Arc::new(base_url),
    };

    let session_manager = Arc::new(LocalSessionManager::default());
    let mcp_service = StreamableHttpService::new(move || Ok(handler.clone()), session_manager, Default::default());

    let telegram_state = Arc::new(TelegramWebhookState {
        confirmations: exec_tools.confirmation_store(),
    });

    let app = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(oauth_state.clone(), bearer_auth_middleware))
        // Hors du middleware d'auth : webhook Telegram et routes OAuth.
        .route("/telegram/webhook", post(telegram_webhook_handler))
        .with_state(telegram_state)
        .merge(oauth::router(oauth_state));

    let listener = tokio::net::TcpListener::bind(&cfg.server.bind_addr)
        .await
        .with_context(|| format!("impossible d'écouter sur {}", cfg.server.bind_addr))?;
    axum::serve(listener, app).await.context("erreur du serveur axum")?;
    Ok(())
}

/// Accepte soit le token statique (Inspector, Claude Code), soit un token OAuth signé (claude.ai).
async fn bearer_auth_middleware(
    State(oauth): State<oauth::OAuthState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let provided = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(auth::extract_bearer);

    let ok = match provided {
        Some(token) => auth::tokens_match(token, &oauth.secret) || oauth::is_valid_access_token(&oauth, token),
        None => false,
    };
    if ok {
        return next.run(request).await;
    }

    let challenge = format!(
        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
        oauth.base_url
    );
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, challenge)],
        "invalid or missing bearer token",
    )
        .into_response()
}

#[derive(Clone)]
struct TelegramWebhookState {
    confirmations: ConfirmationStore,
}

async fn telegram_webhook_handler(
    State(state): State<Arc<TelegramWebhookState>>,
    Json(payload): Json<telegram::TelegramWebhookPayload>,
) -> impl IntoResponse {
    if let Some(cb) = payload.callback_query {
        if let Some((id, approved)) = telegram::parse_callback_data(&cb.data) {
            let decision = if approved { Decision::Approved } else { Decision::Denied };
            state.confirmations.resolve(id, decision).await;
        }
    }
    StatusCode::OK
}
