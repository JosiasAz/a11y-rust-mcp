use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router, ServerHandler,
};
use tracing::error;

use crate::agent::fs_tools::{
    execute_fs_tool, CreateStructureArgs, DeletePathArgs, GlobArgs, GrepArgs, ListDirArgs, MkDirArgs,
    ReadFileArgs, RunTerminalArgs, StrReplaceArgs, WriteFileArgs,
};
use crate::agent::tools::{
    criar_lembrete as criar_lembrete_impl, simplificar_texto as simplificar_texto_impl,
    verificar_compreensao as verificar_compreensao_impl, CriarLembreteArgs, SimplificarTextoArgs,
    VerificarCompreensaoArgs,
};
use crate::agent::workspace::Workspace;
use crate::llm::OpenAiClient;

/// Servidor MCP oficial (`rmcp`) — a11y + tools de engenharia (sandbox).
#[derive(Clone)]
pub struct A11yMcpServer {
    tool_router: ToolRouter<Self>,
    llm: Arc<OpenAiClient>,
    workspace: Arc<Workspace>,
}

#[tool_router]
impl A11yMcpServer {
    pub fn new(llm: Arc<OpenAiClient>, workspace: Arc<Workspace>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            llm,
            workspace,
        }
    }

    #[tool(
        name = "simplificar_texto",
        description = "Reescreve texto burocrático em linguagem simples e acessível (few-shot via LLM)."
    )]
    async fn simplificar_texto(
        &self,
        Parameters(args): Parameters<SimplificarTextoArgs>,
    ) -> String {
        match simplificar_texto_impl(self.llm.as_ref(), &args).await {
            Ok(text) => text,
            Err(err) => format!("ERRO: {err}"),
        }
    }

    #[tool(
        name = "verificar_compreensao",
        description = "Avalia compreensão com Chain-of-Thought e subperguntas Least-to-Most."
    )]
    async fn verificar_compreensao(
        &self,
        Parameters(args): Parameters<VerificarCompreensaoArgs>,
    ) -> String {
        match verificar_compreensao_impl(self.llm.as_ref(), &args).await {
            Ok(text) => text,
            Err(err) => format!("ERRO: {err}"),
        }
    }

    #[tool(
        name = "criar_lembrete",
        description = "Cria um lembrete simples imprimindo no console e confirmando sucesso."
    )]
    fn criar_lembrete(&self, Parameters(args): Parameters<CriarLembreteArgs>) -> String {
        match criar_lembrete_impl(&args) {
            Ok(text) => text,
            Err(err) => format!("ERRO: {err}"),
        }
    }

    #[tool(description = "Lê um arquivo do workspace.")]
    fn read_file(&self, Parameters(args): Parameters<ReadFileArgs>) -> String {
        fs_call(self, "read_file", args)
    }

    #[tool(description = "Cria ou sobrescreve arquivo no workspace.")]
    fn write_file(&self, Parameters(args): Parameters<WriteFileArgs>) -> String {
        fs_call(self, "write_file", args)
    }

    #[tool(description = "Edita trecho exato de um arquivo.")]
    fn str_replace(&self, Parameters(args): Parameters<StrReplaceArgs>) -> String {
        fs_call(self, "str_replace", args)
    }

    #[tool(description = "Lista diretório do workspace.")]
    fn list_dir(&self, Parameters(args): Parameters<ListDirArgs>) -> String {
        fs_call(self, "list_dir", args)
    }

    #[tool(description = "Cria diretório no workspace.")]
    fn mkdir(&self, Parameters(args): Parameters<MkDirArgs>) -> String {
        fs_call(self, "mkdir", args)
    }

    #[tool(description = "Apaga arquivo/pasta no workspace (não a raiz).")]
    fn delete_path(&self, Parameters(args): Parameters<DeletePathArgs>) -> String {
        fs_call(self, "delete_path", args)
    }

    #[tool(description = "Busca arquivos por glob.")]
    fn glob_files(&self, Parameters(args): Parameters<GlobArgs>) -> String {
        fs_call(self, "glob_files", args)
    }

    #[tool(description = "Busca texto/regex no workspace.")]
    fn grep_search(&self, Parameters(args): Parameters<GrepArgs>) -> String {
        fs_call(self, "grep_search", args)
    }

    #[tool(description = "Executa comando no workspace (com bloqueios).")]
    fn run_terminal(&self, Parameters(args): Parameters<RunTerminalArgs>) -> String {
        fs_call(self, "run_terminal", args)
    }

    #[tool(description = "Cria estrutura de pastas/arquivos em lote.")]
    fn create_structure(&self, Parameters(args): Parameters<CreateStructureArgs>) -> String {
        fs_call(self, "create_structure", args)
    }
}

fn fs_call<T: serde::Serialize>(server: &A11yMcpServer, name: &str, args: T) -> String {
    let value = serde_json::to_value(args).unwrap_or_default();
    match execute_fs_tool(server.workspace.as_ref(), name, value) {
        Ok(text) => text,
        Err(err) => {
            error!(tool = name, error = %err, "tool fs falhou");
            format!("ERRO: {err}")
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for A11yMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Rust-A11y-Agent: acessibilidade cognitiva + agente de código \
                 (read/write/edit/list/mkdir/glob/grep/terminal/structure) sandboxed no workspace.",
            )
    }
}
