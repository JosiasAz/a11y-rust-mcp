use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rmcp::{
    model::CallToolRequestParams,
    service::RunningService,
    transport::{ConfigureCommandExt, TokioChildProcess},
    RoleClient, ServiceExt,
};
use serde_json::{Map, Value};
use tokio::process::Command;
use tracing::info;

use crate::llm::models::Tool;

/// Contrato mínimo para executar ferramentas (local ou via MCP).
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn call_tool(&self, name: &str, args: Value) -> Result<String, String>;
}

/// Sessão MCP em processo filho: descobre tools em runtime e as executa via `tools/call`.
pub struct McpToolSession {
    client: RunningService<RoleClient, ()>,
    tools: Vec<Tool>,
}

impl McpToolSession {
    /// Spawna o próprio binário em modo `mcp-server` e faz handshake MCP.
    ///
    /// `workspace` é repassado ao filho como `A11Y_WORKSPACE`; sem isso o filho usaria a
    /// variável herdada ou a pasta atual, e não a pasta escolhida em `chat --workspace`.
    pub async fn spawn_self(workspace: Option<&Path>) -> Result<Self> {
        let exe = std::env::current_exe().context("não foi possível resolver current_exe")?;
        let workspace = workspace.map(Path::to_path_buf);
        info!(?exe, ?workspace, "iniciando servidor MCP filho (stdio)");

        let transport = TokioChildProcess::new(Command::new(exe).configure(move |cmd| {
            cmd.arg("mcp-server");
            // Evita misturar logs do filho com o protocolo se o filtro for muito verboso no stdout.
            cmd.env("RUST_LOG", "warn,rust_a11y_agent=info");
            if let Some(ws) = &workspace {
                cmd.env("A11Y_WORKSPACE", ws);
            }
        }))
        .context("falha ao criar transporte stdio do processo filho")?;

        let client = ()
            .serve(transport)
            .await
            .context("falha no handshake MCP com o processo filho")?;

        if let Some(info) = client.peer_info() {
            info!(server = ?info.server_info, "conectado ao MCP server");
        }

        let listed = client
            .list_tools(Default::default())
            .await
            .context("tools/list falhou")?;

        let tools: Vec<Tool> = listed
            .tools
            .iter()
            .map(|t| {
                let parameters = serde_json::to_value(&t.input_schema)
                    .unwrap_or_else(|_| serde_json::json!({"type":"object","properties":{}}));
                Tool::function(
                    t.name.to_string(),
                    t.description.clone().unwrap_or_default(),
                    parameters,
                )
            })
            .collect();

        info!(count = tools.len(), "tools descobertas via MCP");
        for t in &tools {
            info!(name = %t.function.name, "tool MCP disponível");
        }

        Ok(Self { client, tools })
    }

    pub fn openai_tools(&self) -> Vec<Tool> {
        self.tools.clone()
    }

}

#[async_trait]
impl ToolExecutor for McpToolSession {
    async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        let map: Map<String, Value> = match args {
            Value::Object(m) => m,
            other => {
                let mut m = Map::new();
                m.insert("value".into(), other);
                m
            }
        };

        let result = self
            .client
            .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(map))
            .await
            .map_err(|e| e.to_string())?;

        if result.is_error.unwrap_or(false) {
            return Err(extract_text(&result).unwrap_or_else(|| "erro MCP sem detalhe".into()));
        }

        extract_text(&result).ok_or_else(|| "tools/call sem conteúdo textual".into())
    }
}

impl Drop for McpToolSession {
    fn drop(&mut self) {
        // Melhor esforço: o RunningService encerra o child ao dropar.
        info!("encerrando sessão MCP");
    }
}

fn extract_text(result: &rmcp::model::CallToolResult) -> Option<String> {
    let mut parts = Vec::new();
    for block in &result.content {
        // Compatível com diferentes shapes de ContentBlock / Annotated.
        let value = serde_json::to_value(block).ok()?;
        if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
            parts.push(text.to_string());
        } else if let Some(text) = value
            .pointer("/raw/text")
            .and_then(|t| t.as_str())
            .or_else(|| value.pointer("/0/text").and_then(|t| t.as_str()))
        {
            parts.push(text.to_string());
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}
