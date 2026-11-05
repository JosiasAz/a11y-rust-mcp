use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::{
    transport::stdio,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ServiceExt,
};
use tracing::info;

use super::handler::A11yMcpServer;
use crate::api::auth::{require_bearer, ApiToken};
use crate::agent::workspace::Workspace;
use crate::llm::shared_openai_compatible_from_env;

fn workspace_from_env() -> Result<Arc<Workspace>> {
    let ws = Workspace::from_env_or(std::env::current_dir()?)?;
    info!(root = %ws.root().display(), "workspace MCP");
    Ok(Arc::new(ws))
}

/// Servidor MCP via stdio (JSON-RPC sobre stdin/stdout) usando o SDK oficial `rmcp`.
pub async fn run_mcp_server() -> Result<()> {
    info!("MCP server (rmcp / stdio) iniciando");
    let llm = Arc::new(shared_openai_compatible_from_env()?);
    let workspace = workspace_from_env()?;
    let server = A11yMcpServer::new(llm, workspace);
    let service = server
        .serve(stdio())
        .await
        .context("falha ao servir MCP via stdio")?;
    service
        .waiting()
        .await
        .context("serviço MCP stdio encerrou com erro")?;
    Ok(())
}

/// Servidor MCP Streamable HTTP (compartilhável em rede local).
pub async fn run_mcp_http(bind: &str) -> Result<()> {
    info!(%bind, "MCP server (rmcp / streamable HTTP) iniciando");
    let llm = Arc::new(shared_openai_compatible_from_env()?);
    let workspace = workspace_from_env()?;
    let ct = tokio_util::sync::CancellationToken::new();

    let llm_factory = llm.clone();
    let ws_factory = workspace.clone();
    let service = StreamableHttpService::new(
        move || Ok(A11yMcpServer::new(llm_factory.clone(), ws_factory.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_json_response(true)
            .with_cancellation_token(ct.child_token()),
    );

    let token = ApiToken::generate()?;
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn_with_state(token.clone(), require_bearer));
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("falha ao bind em {bind}"))?;
    // O cliente MCP precisa configurar este valor como header `Authorization: Bearer <token>`.
    token.announce()?;

    info!(%bind, path = "/mcp", "Streamable HTTP pronto");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            info!("sinal de shutdown recebido");
            ct.cancel();
        })
        .await
        .context("servidor HTTP encerrou com erro")?;

    Ok(())
}
