mod agent;
mod api;
mod i18n;
mod llm;
mod mcp_server;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use agent::{plan_and_solve, AgentRunOptions, Workspace};
use api::run_desktop_api;
use llm::backend_from_env;
use mcp_server::{run_mcp_http, run_mcp_server};

#[derive(Debug, Parser)]
#[command(
    name = "rust-a11y-agent",
    version,
    about = "Rust-A11y-Agent — agente acessível com tools de engenharia + MCP"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Servidor MCP via stdio (`rmcp`).
    McpServer,
    /// Servidor MCP Streamable HTTP (`rmcp` + axum).
    McpHttp {
        #[arg(long, default_value = "127.0.0.1:8000")]
        bind: String,
    },
    /// API HTTP para o desktop Avalonia.
    ServeApi {
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,
    },
    /// Agente interativo no terminal.
    Chat {
        #[arg(long)]
        task: String,
        #[arg(long, default_value_t = false)]
        local_tools: bool,
        /// Pula Human-in-the-Loop.
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
        /// Pasta sandbox das tools de arquivo (default: cwd).
        #[arg(long)]
        workspace: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Commands::McpServer => {
            info!("modo mcp-server (stdio / rmcp)");
            run_mcp_server()
                .await
                .context("servidor MCP stdio encerrou com erro")?;
        }
        Commands::McpHttp { bind } => {
            info!(%bind, "modo mcp-http");
            run_mcp_http(&bind)
                .await
                .context("servidor MCP HTTP encerrou com erro")?;
        }
        Commands::ServeApi { bind } => {
            info!(%bind, "modo serve-api (desktop)");
            run_desktop_api(&bind)
                .await
                .context("API desktop encerrou com erro")?;
        }
        Commands::Chat {
            task,
            local_tools,
            auto_approve,
            workspace,
        } => {
            let via_mcp = !local_tools;
            info!(%task, via_mcp, auto_approve, "modo chat");

            let backend = backend_from_env().context(
                "configure LLM_PROVIDER / LLM_BASE_URL / LLM_API_KEY / LLM_MODEL",
            )?;
            info!(model = %backend.model_name(), "backend LLM carregado");

            let ws = match workspace {
                Some(p) => Some(Workspace::new(p)?),
                None => Workspace::from_env_or(std::env::current_dir()?).ok(),
            };

            let result = plan_and_solve(
                &task,
                backend.as_ref(),
                AgentRunOptions {
                    via_mcp,
                    auto_approve,
                    workspace: ws,
                },
            )
            .await
            .context("falha em plan_and_solve")?;

            println!("\n======== Resposta final ========\n{result}\n");
        }
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,rust_a11y_agent=debug"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .compact()
        .init();
}
