use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::agent::progress::{ProgressEvent, ProgressSink};
use crate::agent::r#loop::{plan_and_solve, plan_desktop, run_desktop_agent, AgentRunOptions};
use crate::agent::workspace::Workspace;
use crate::llm::{shared_openai_compatible_from_env, LlmBackend, OpenAiClient};

#[derive(Clone)]
struct AppState {
    llm: Arc<OpenAiClient>,
}

#[derive(Debug, Deserialize)]
pub struct RunRequest {
    pub task: String,
    pub workspace: String,
    #[serde(default = "default_true")]
    pub auto_approve: bool,
    #[serde(default = "default_true")]
    pub fast_mode: bool,
    /// Plano aprovado na tela; vazio = laço único.
    #[serde(default)]
    pub steps: Vec<String>,
    /// Modelo escolhido na tela; vazio = padrão do servidor.
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlanRequest {
    pub task: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub current: String,
    pub models: Vec<String>,
}

/// Cliente com o modelo pedido pela tela (clone barato: o `reqwest::Client` é compartilhado).
fn llm_for(state: &AppState, model: &Option<String>) -> Arc<OpenAiClient> {
    match model.as_deref().map(str::trim) {
        Some(m) if !m.is_empty() => Arc::new(state.llm.as_ref().clone().with_model(m)),
        _ => state.llm.clone(),
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct RunResponse {
    pub ok: bool,
    pub result: String,
    pub model: String,
    pub workspace: String,
}

#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    pub workspace: String,
    #[serde(default = "default_depth")]
    pub depth: usize,
}

fn default_depth() -> usize {
    3
}

#[derive(Debug, Serialize)]
pub struct TreeResponse {
    pub root: String,
    pub entries: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    pub workspace: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct FileResponse {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct WriteFileRequest {
    pub workspace: String,
    pub path: String,
    pub content: String,
}

/// API HTTP para o desktop Avalonia (CORS liberado em localhost).
pub async fn run_desktop_api(bind: &str) -> Result<()> {
    let llm = Arc::new(shared_openai_compatible_from_env()?);
    let state = AppState { llm };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/agent/run", post(agent_run))
        .route("/api/agent/run-stream", post(agent_run_stream))
        .route("/api/agent/plan", post(agent_plan))
        .route("/api/models", get(list_models))
        .route("/api/workspace/tree", get(workspace_tree))
        .route("/api/workspace/file", get(read_file).put(write_file))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("bind falhou em {bind}"))?;
    info!(%bind, "API desktop pronta — abra o app e use normalmente");
    axum::serve(listener, app)
        .await
        .context("servidor API encerrou com erro")?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "service": "rust-a11y-agent",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn agent_plan(
    State(state): State<AppState>,
    Json(req): Json<PlanRequest>,
) -> Result<Json<crate::llm::models::PlanOutput>, (axum::http::StatusCode, String)> {
    let llm = llm_for(&state, &req.model);
    plan_desktop(&req.task, llm.as_ref())
        .await
        .map(Json)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))
}

async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<ModelsResponse>, (axum::http::StatusCode, String)> {
    let models = state
        .llm
        .list_models()
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    Ok(Json(ModelsResponse {
        current: state.llm.model_name().to_string(),
        models,
    }))
}

async fn agent_run(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> Result<Json<RunResponse>, (axum::http::StatusCode, String)> {
    let ws = Workspace::new(&req.workspace).map_err(map_err)?;
    let model = state.llm.model_name().to_string();

    let result = if req.fast_mode {
        run_desktop_agent(&req.task, &req.steps, state.llm.as_ref(), ws.clone(), None).await
    } else {
        plan_and_solve(
            &req.task,
            state.llm.as_ref(),
            AgentRunOptions {
                via_mcp: false,
                auto_approve: req.auto_approve,
                workspace: Some(ws.clone()),
            },
        )
        .await
    }
    .map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
    })?;

    Ok(Json(RunResponse {
        ok: true,
        result,
        model,
        workspace: ws.root().display().to_string(),
    }))
}

/// Execução com progresso ao vivo (Server-Sent Events).
async fn agent_run_stream(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (axum::http::StatusCode, String)> {
    let ws = Workspace::new(&req.workspace).map_err(map_err)?;
    let llm = llm_for(&state, &req.model);
    let model = llm.model_name().to_string();
    let task = req.task.clone();
    let steps = req.steps.clone();
    let fast = req.fast_mode;
    let auto_approve = req.auto_approve;
    let workspace_display = ws.root().display().to_string();

    let (tx, rx) = mpsc::channel::<ProgressEvent>(64);

    tokio::spawn(async move {
        let (async_tx, mut async_rx) = mpsc::unbounded_channel::<ProgressEvent>();
        let bridge = tokio::spawn({
            let tx = tx.clone();
            async move {
                while let Some(ev) = async_rx.recv().await {
                    if tx.send(ev).await.is_err() {
                        break;
                    }
                }
            }
        });

        let async_sink: ProgressSink = Arc::new(move |ev: ProgressEvent| {
            let _ = async_tx.send(ev);
        });

        tracing::info!(%model, workspace = %workspace_display, "desktop run");
        async_sink(ProgressEvent::status("Comecei a trabalhar no seu pedido."));

        let run = async {
            if fast {
                run_desktop_agent(&task, &steps, llm.as_ref(), ws, Some(async_sink.clone())).await
            } else {
            async_sink(ProgressEvent::status("Planejando e executando…"));
            plan_and_solve(
                &task,
                llm.as_ref(),
                AgentRunOptions {
                    via_mcp: false,
                    auto_approve,
                    workspace: Some(ws),
                },
            )
            .await
            }
        };

        // Se a tela cancelar ou fechar, o SSE cai: descartar `run` derruba a requisição ao
        // modelo, e o Ollama para de gerar na hora.
        let result = tokio::select! {
            r = run => r,
            _ = tx.closed() => {
                info!("cliente desconectou; trabalho do agente interrompido");
                return;
            }
        };

        match result {
            Ok(text) => {
                async_sink(ProgressEvent {
                    kind: "final".into(),
                    message: text,
                    tool: None,
                    refresh_tree: Some(true),
                    path: None,
                });
            }
            Err(err) => {
                async_sink(ProgressEvent::error(format!("{err:#}")));
            }
        }

        drop(async_sink);
        let _ = bridge.await;
    });

    let stream = ReceiverStream::new(rx).map(|ev| {
        let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
        Ok::<Event, Infallible>(Event::default().event("progress").data(data))
    });

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

async fn workspace_tree(
    Query(q): Query<TreeQuery>,
) -> Result<Json<TreeResponse>, (axum::http::StatusCode, String)> {
    let ws = Workspace::new(&q.workspace).map_err(map_err)?;
    let mut entries = Vec::new();
    collect_tree(ws.root(), ws.root(), q.depth, 0, &mut entries);
    Ok(Json(TreeResponse {
        root: ws.root().display().to_string(),
        entries,
    }))
}

async fn read_file(
    Query(q): Query<FileQuery>,
) -> Result<Json<FileResponse>, (axum::http::StatusCode, String)> {
    let ws = Workspace::new(&q.workspace).map_err(map_err)?;
    let path = ws.resolve(&q.path).map_err(map_err)?;
    let content = std::fs::read_to_string(&path).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("read: {e}"),
        )
    })?;
    Ok(Json(FileResponse {
        path: ws.rel_display(&path),
        content,
    }))
}

async fn write_file(
    Json(req): Json<WriteFileRequest>,
) -> Result<Json<FileResponse>, (axum::http::StatusCode, String)> {
    let ws = Workspace::new(&req.workspace).map_err(map_err)?;
    let path = ws.resolve(&req.path).map_err(map_err)?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, req.content.as_bytes()).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("write: {e}"),
        )
    })?;
    Ok(Json(FileResponse {
        path: ws.rel_display(&path),
        content: req.content,
    }))
}

fn collect_tree(
    root: &std::path::Path,
    current: &std::path::Path,
    max_depth: usize,
    depth: usize,
    out: &mut Vec<String>,
) {
    if depth > max_depth {
        return;
    }
    let Ok(rd) = std::fs::read_dir(current) else {
        return;
    };
    let mut items: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name());
    for entry in items {
        let path = entry.path();
        let name = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());
        let marker = if path.is_dir() { "📁" } else { "📄" };
        out.push(format!("{marker} {name}"));
        if path.is_dir() {
            let skip = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "target" || n == "node_modules" || n == ".git")
                .unwrap_or(false);
            if !skip {
                collect_tree(root, &path, max_depth, depth + 1, out);
            }
        }
    }
}

fn map_err(e: impl ToString) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::BAD_REQUEST, e.to_string())
}
