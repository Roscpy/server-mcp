//! Câblage rmcp <-> nos tools. Un seul struct `McpVpsHandler` regroupe
//! tous les modules ; chaque `#[tool]` reste une méthode fine qui délègue
//! à FsTools/ExecTools/LogTools/SnapshotManager/ResourceMonitor et
//! sérialise le résultat en `CallToolResult`.

use crate::exec::{ExecCommandParams, ExecTools};
use crate::logs::{LogTools, ServiceLogsParams, TailFileParams};
use crate::monitor::ResourceMonitor;
use crate::native_confirm;
use rmcp::service::{Peer, RoleServer};
use crate::snapshot::SnapshotManager;
use crate::tools::fs_tools::{FsTools, ListDirParams, ReadFileParams, WriteFileParams};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

fn ok_json(value: impl Serialize) -> Result<CallToolResult, ErrorData> {
    let text = serde_json::to_string_pretty(&value)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

fn err_text(msg: impl ToString) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![Content::text(msg.to_string())]))
}

#[derive(Clone)]
pub struct McpVpsHandler {
    fs_tools: Arc<FsTools>,
    exec_tools: Arc<ExecTools>,
    log_tools: Arc<LogTools>,
    snapshot_manager: Arc<SnapshotManager>,
    monitor: Arc<Mutex<ResourceMonitor>>,
    tool_router: ToolRouter<Self>,
}

impl McpVpsHandler {
    pub fn new(
        fs_tools: Arc<FsTools>,
        exec_tools: Arc<ExecTools>,
        log_tools: Arc<LogTools>,
        snapshot_manager: Arc<SnapshotManager>,
        monitor: Arc<Mutex<ResourceMonitor>>,
    ) -> Self {
        Self {
            fs_tools,
            exec_tools,
            log_tools,
            snapshot_manager,
            monitor,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl McpVpsHandler {
    // ---------- Phase 1 : filesystem non destructif ----------

    #[tool(description = "Lit le contenu d'un fichier texte, confiné à workspace_root.")]
    async fn read_file(&self, Parameters(params): Parameters<ReadFileParams>) -> Result<CallToolResult, ErrorData> {
        match self.fs_tools.read_file(params).await {
            Ok(content) => Ok(CallToolResult::success(vec![Content::text(content)])),
            Err(e) => err_text(e),
        }
    }

    #[tool(description = "Écrit un fichier (sauvegarde .bak automatique si le fichier existe déjà), confiné à workspace_root.")]
    async fn write_file(&self, Parameters(params): Parameters<WriteFileParams>) -> Result<CallToolResult, ErrorData> {
        match self.fs_tools.write_file(params).await {
            Ok(msg) => Ok(CallToolResult::success(vec![Content::text(msg)])),
            Err(e) => err_text(e),
        }
    }

    #[tool(description = "Liste le contenu d'un dossier, confiné à workspace_root.")]
    async fn list_dir(&self, Parameters(params): Parameters<ListDirParams>) -> Result<CallToolResult, ErrorData> {
        match self.fs_tools.list_dir(params).await {
            Ok(entries) => ok_json(entries),
            Err(e) => err_text(e),
        }
    }

    // ---------- Phase 2 : exécution de commandes ----------

    #[tool(description = "Exécute une commande shell. Si elle n'est pas dans la whitelist, met l'action en pause et demande une confirmation humaine (prompt MCP natif si le client le supporte, sinon Telegram).")]
    async fn exec_command(
        &self,
        Parameters(params): Parameters<ExecCommandParams>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if self.exec_tools.is_whitelisted(&params.command).await {
            return match self.exec_tools.exec_command(params).await {
                Ok(result) => ok_json(result),
                Err(e) => err_text(e),
            };
        }
        match native_confirm::ask(&peer, &params.command, params.remember_if_approved).await {
            native_confirm::Native::Approved => match self.exec_tools.run_approved(params).await {
                Ok(result) => ok_json(result),
                Err(e) => err_text(e),
            },
            native_confirm::Native::Denied(reason) => {
                self.exec_tools.record_denied(&params.command, &reason).await;
                err_text(format!("commande refusée: {reason}"))
            }
            // Client sans élicitation : repli sur le canal externe (Telegram).
            native_confirm::Native::Unsupported => match self.exec_tools.exec_command(params).await {
                Ok(result) => ok_json(result),
                Err(e) => err_text(e),
            },
        }
    }
    
    // ---------- Phase 3 : logs & snapshots ----------

    #[tool(description = "Lit les dernières lignes d'un fichier de log explicitement autorisé (voir [logs] allowed_paths dans config.toml).")]
    async fn tail_file(&self, Parameters(params): Parameters<TailFileParams>) -> Result<CallToolResult, ErrorData> {
        match self.log_tools.tail_file(params).await {
            Ok(out) => Ok(CallToolResult::success(vec![Content::text(out.content)])),
            Err(e) => err_text(e),
        }
    }

    #[tool(description = "Lit les logs d'un service systemd via journalctl.")]
    async fn service_logs(&self, Parameters(params): Parameters<ServiceLogsParams>) -> Result<CallToolResult, ErrorData> {
        match self.log_tools.service_logs(params).await {
            Ok(out) => Ok(CallToolResult::success(vec![Content::text(out.content)])),
            Err(e) => err_text(e),
        }
    }

    #[tool(description = "Crée une copie complète du workspace avant une opération risquée.")]
    async fn snapshot_workspace(&self) -> Result<CallToolResult, ErrorData> {
        match self.snapshot_manager.create(None).await {
            Ok(info) => ok_json(info),
            Err(e) => err_text(e),
        }
    }

    #[tool(description = "Liste les snapshots existants du workspace.")]
    async fn list_snapshots(&self) -> Result<CallToolResult, ErrorData> {
        match self.snapshot_manager.list().await {
            Ok(list) => ok_json(list),
            Err(e) => err_text(e),
        }
    }

    // ---------- Phase 4 : monitoring ----------

    #[tool(description = "Renvoie l'utilisation courante CPU/RAM/disque du serveur.")]
    async fn get_resource_usage(&self) -> Result<CallToolResult, ErrorData> {
        let snap = self.monitor.lock().await.snapshot();
        ok_json(snap)
    }
}

#[tool_handler]
impl ServerHandler for McpVpsHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Serveur MCP d'administration système pour VPS/Termux. Les commandes hors \
                 whitelist déclenchent une confirmation humaine avant exécution."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
