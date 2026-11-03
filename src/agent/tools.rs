use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::debug;

use crate::agent::fs_tools::{
    execute_fs_tool, fs_tool_names, CreateStructureArgs, DeletePathArgs, GlobArgs, GrepArgs,
    ListDirArgs, MkDirArgs, ReadFileArgs, RunTerminalArgs, StrReplaceArgs, WriteFileArgs,
};
use crate::agent::workspace::Workspace;
use crate::llm::models::{schema_for_type, ChatRequest, Message, Tool};
use crate::llm::LlmBackend;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SimplificarTextoArgs {
    /// Texto original a ser simplificado.
    pub texto: String,
    /// Nível de simplificação: "basico" ou "intermediario".
    #[serde(default = "default_nivel")]
    pub nivel: String,
}

fn default_nivel() -> String {
    "basico".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VerificarCompreensaoArgs {
    /// Conteúdo que deveria ter sido compreendido.
    pub texto_referencia: String,
    /// Como o usuário explicou o que entendeu.
    pub explicacao_usuario: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CriarLembreteArgs {
    /// Título curto do lembrete.
    pub titulo: String,
    /// Quando lembrar (texto livre).
    pub quando: String,
    /// Detalhes opcionais.
    #[serde(default)]
    pub detalhes: String,
}

/// Metadados estáticos das ferramentas (a11y + coding estilo Cursor).
pub fn openai_tool_definitions() -> Vec<Tool> {
    vec![
        Tool::function(
            "simplificar_texto",
            "Reescreve texto burocrático em linguagem simples e acessível (few-shot).",
            schema_for_type::<SimplificarTextoArgs>(),
        ),
        Tool::function(
            "verificar_compreensao",
            "Avalia compreensão com Chain-of-Thought e subperguntas Least-to-Most.",
            schema_for_type::<VerificarCompreensaoArgs>(),
        ),
        Tool::function(
            "criar_lembrete",
            "Cria um lembrete simples imprimindo no console e confirmando sucesso.",
            schema_for_type::<CriarLembreteArgs>(),
        ),
        Tool::function(
            "read_file",
            "Lê um arquivo do workspace (com offset/limit opcionais).",
            schema_for_type::<ReadFileArgs>(),
        ),
        Tool::function(
            "write_file",
            "Cria ou sobrescreve um arquivo no workspace (cria pastas pai se preciso).",
            schema_for_type::<WriteFileArgs>(),
        ),
        Tool::function(
            "str_replace",
            "Edita trecho exato de um arquivo (old_string → new_string).",
            schema_for_type::<StrReplaceArgs>(),
        ),
        Tool::function(
            "list_dir",
            "Lista arquivos e pastas de um diretório do workspace.",
            schema_for_type::<ListDirArgs>(),
        ),
        Tool::function(
            "mkdir",
            "Cria diretório (incluindo pais) no workspace.",
            schema_for_type::<MkDirArgs>(),
        ),
        Tool::function(
            "delete_path",
            "Apaga arquivo ou pasta dentro do workspace (nunca a raiz).",
            schema_for_type::<DeletePathArgs>(),
        ),
        Tool::function(
            "glob_files",
            "Busca arquivos por padrão glob (ex: **/*.rs).",
            schema_for_type::<GlobArgs>(),
        ),
        Tool::function(
            "grep_search",
            "Busca texto/regex no workspace.",
            schema_for_type::<GrepArgs>(),
        ),
        Tool::function(
            "run_terminal",
            "Executa comando no workspace (PowerShell/sh) com bloqueios de segurança.",
            schema_for_type::<RunTerminalArgs>(),
        ),
        Tool::function(
            "create_structure",
            "Cria estrutura de pastas e arquivos do zero em lote.",
            schema_for_type::<CreateStructureArgs>(),
        ),
    ]
}

/// Executa uma ferramenta pelo nome (a11y via LLM ou fs no sandbox).
pub async fn execute_tool(
    backend: &dyn LlmBackend,
    workspace: Option<&Workspace>,
    name: &str,
    args: Value,
) -> Result<String, String> {
    if fs_tool_names().contains(&name) {
        let ws = workspace.ok_or_else(|| {
            "workspace não configurado — defina A11Y_WORKSPACE ou envie workspace na API".to_string()
        })?;
        return execute_fs_tool(ws, name, args);
    }

    match name {
        "simplificar_texto" => {
            let parsed: SimplificarTextoArgs =
                serde_json::from_value(args).map_err(|e| e.to_string())?;
            simplificar_texto(backend, &parsed).await
        }
        "verificar_compreensao" => {
            let parsed: VerificarCompreensaoArgs =
                serde_json::from_value(args).map_err(|e| e.to_string())?;
            verificar_compreensao(backend, &parsed).await
        }
        "criar_lembrete" => {
            let parsed: CriarLembreteArgs =
                serde_json::from_value(args).map_err(|e| e.to_string())?;
            criar_lembrete(&parsed)
        }
        other => Err(format!("ferramenta desconhecida: {other}")),
    }
}

/// Few-shot via LLM; cai para heurística local se o backend falhar.
pub async fn simplificar_texto(
    backend: &dyn LlmBackend,
    args: &SimplificarTextoArgs,
) -> Result<String, String> {
    let nivel = if args.nivel.trim().is_empty() {
        "basico"
    } else {
        args.nivel.as_str()
    };

    let prompt = format!(
        "Você simplifica textos para acessibilidade cognitiva.\n\n\
         Exemplos (few-shot):\n\
         Entrada: O paciente deve realizar a ingestão do medicamento subsequentemente.\n\
         Saída: A pessoa precisa tomar o remédio depois.\n\n\
         Entrada: Em virtude de intercorrências, efetue o contato com a equipe.\n\
         Saída: Se algo der errado, fale com a equipe.\n\n\
         Nível desejado: {nivel}\n\
         Texto:\n{}\n\n\
         Reescreva só o texto simplificado, em português claro.",
        args.texto
    );

    match chat_plain(backend, &prompt, 0.2).await {
        Ok(text) => Ok(text),
        Err(err) => {
            debug!(error = %err, "fallback local de simplificar_texto");
            Ok(fallback_simplify(&args.texto, nivel))
        }
    }
}

/// CoT + Least-to-Most: gera subperguntas e avalia a explicação do usuário.
pub async fn verificar_compreensao(
    backend: &dyn LlmBackend,
    args: &VerificarCompreensaoArgs,
) -> Result<String, String> {
    let prompt = format!(
        "Técnica: Chain-of-Thought + Least-to-Most.\n\
         1) Liste 3 subperguntas simples sobre o texto de referência.\n\
         2) Compare a explicação do usuário com cada subpergunta.\n\
         3) Dê um veredito: ADEQUADA | PARCIAL | INSUFICIENTE.\n\
         4) Sugira uma frase de reforço em linguagem fácil.\n\n\
         Texto de referência:\n{}\n\n\
         Explicação do usuário:\n{}\n",
        args.texto_referencia, args.explicacao_usuario
    );

    match chat_plain(backend, &prompt, 0.2).await {
        Ok(text) => Ok(text),
        Err(err) => {
            debug!(error = %err, "fallback local de verificar_compreensao");
            Ok(fallback_comprehension(
                &args.texto_referencia,
                &args.explicacao_usuario,
            ))
        }
    }
}

pub fn criar_lembrete(args: &CriarLembreteArgs) -> Result<String, String> {
    if args.titulo.trim().is_empty() || args.quando.trim().is_empty() {
        return Err("titulo e quando são obrigatórios".into());
    }

    eprintln!("══════════════════════════════════════");
    eprintln!("🔔 LEMBRETE CRIADO");
    eprintln!("   Título : {}", args.titulo);
    eprintln!("   Quando : {}", args.quando);
    if !args.detalhes.trim().is_empty() {
        eprintln!("   Detalhes: {}", args.detalhes);
    }
    eprintln!("══════════════════════════════════════");

    Ok(format!(
        "Lembrete '{}' agendado para '{}' com sucesso.",
        args.titulo, args.quando
    ))
}

async fn chat_plain(backend: &dyn LlmBackend, user: &str, temperature: f32) -> Result<String, String> {
    let request = ChatRequest {
        model: backend.model_name().to_string(),
        messages: vec![
            Message::system("Responda em português, de forma clara e objetiva."),
            Message::user(user),
        ],
        tools: None,
        temperature: Some(temperature),
    };

    let response = backend.chat(request).await.map_err(|e| e.to_string())?;
    response
        .content_text()
        .ok_or_else(|| "LLM não retornou conteúdo".into())
}

fn fallback_simplify(texto: &str, nivel: &str) -> String {
    let mut out = texto.trim().to_string();
    let replacements = [
        ("deve-se", "é bom"),
        ("deve ", "precisa "),
        ("necessário", "preciso"),
        ("realizar", "fazer"),
        ("medicamento", "remédio"),
        ("paciente", "pessoa"),
        ("subsequentemente", "depois"),
        ("portanto", "então"),
        ("em virtude de", "por causa de"),
        ("a fim de", "para"),
    ];
    for (from, to) in replacements {
        out = out.replace(from, to);
    }
    if nivel == "basico" {
        out = out.replace(';', ". ");
    }
    format!("Texto simplificado (fallback local, nível {nivel}):\n\n{out}")
}

fn fallback_comprehension(referencia: &str, explicacao: &str) -> String {
    let ref_tokens = tokenize(referencia);
    let exp_tokens = tokenize(explicacao);
    let keys: Vec<&String> = ref_tokens.iter().take(8).collect();
    let covered: Vec<&String> = keys
        .iter()
        .copied()
        .filter(|k| exp_tokens.iter().any(|t| t == *k))
        .collect();
    let score = if keys.is_empty() {
        0.0
    } else {
        covered.len() as f32 / keys.len() as f32
    };
    let veredito = if score >= 0.7 {
        "ADEQUADA"
    } else if score >= 0.4 {
        "PARCIAL"
    } else {
        "INSUFICIENTE"
    };
    format!(
        "Avaliação (fallback local)\n\
         Subperguntas LtM implícitas cobertas: {covered:?}\n\
         Score: {:.0}%\n\
         Veredito: {veredito}",
        score * 100.0
    )
}

fn tokenize(text: &str) -> Vec<String> {
    let stop = [
        "a", "o", "os", "as", "de", "da", "do", "e", "em", "um", "uma", "para", "com", "que", "se",
        "na", "no", "por",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !stop.contains(w))
        .map(|w| w.to_string())
        .collect()
}
