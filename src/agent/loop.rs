use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::agent::checkpoint::human_in_the_loop;
use crate::agent::progress::{ProgressEvent, ProgressSink};
use crate::agent::state::AgentState;
use crate::agent::tools::{execute_tool, openai_tool_definitions};
use crate::agent::workspace::Workspace;
use crate::llm::models::{ChatRequest, Message, PlanOutput, Tool};
use crate::llm::LlmBackend;
use crate::mcp_server::bridge::{McpToolSession, ToolExecutor};

const MAX_PASSOS: usize = 16;
const MAX_REPETICOES: usize = 3;
const MAX_PLAN_RETRIES: u32 = 3;

/// Formato da resposta final no desktop: ensina a pessoa a conferir o resultado.
const FORMATO_FINAL: &str = "Escreva a resposta final em português simples, com estes 3 blocos curtos:\n\
O que eu fiz: lista com cada arquivo/pasta criado ou alterado e para que serve.\n\
Como usar ou conferir: passos numerados que uma pessoa leiga consiga seguir.\n\
Próximo passo sugerido: uma ideia do que pedir depois.";

/// Laço de tool-use: LLM pede ferramenta → código executa → resultado volta como `role: tool`.
pub async fn run_agent(
    task: &str,
    backend: &dyn LlmBackend,
    tools: &[Tool],
    executor: &dyn ToolExecutor,
) -> Result<String> {
    run_agent_with_progress(task, backend, tools, executor, None).await
}

pub async fn run_agent_with_progress(
    task: &str,
    backend: &dyn LlmBackend,
    tools: &[Tool],
    executor: &dyn ToolExecutor,
    on_progress: Option<ProgressSink>,
) -> Result<String> {
    let system = Message::system(
        "Você é o Rust-A11y-Agent, um agente de acessibilidade cognitiva E de engenharia de software.\n\
         Você pode: planejar, ler/editar/criar arquivos, montar estruturas de pastas, buscar no código e rodar comandos no workspace.\n\
         Prefira mudanças pequenas e verificáveis. Use ferramentas em vez de inventar conteúdo de arquivo.\n\
         Para texto voltado a pessoas, use linguagem clara (simplificar_texto / verificar_compreensao).\n\
         Responda sempre em português simples, sem jargão; se precisar de um termo técnico, explique-o em poucas palavras.",
    );

    let mut messages = vec![system, Message::user(task)];
    let mut last_signature: Option<String> = None;
    let mut repeat_count = 0usize;

    for passo in 1..=MAX_PASSOS {
        info!(passo, "executando passo do agente");
        if let Some(cb) = &on_progress {
            cb(ProgressEvent::status(if passo == 1 {
                "Lendo seu pedido e decidindo o que fazer…".to_string()
            } else {
                "Pensando no próximo passo…".to_string()
            }));
        }

        let request = ChatRequest {
            model: backend.model_name().to_string(),
            messages: messages.clone(),
            tools: if tools.is_empty() {
                None
            } else {
                Some(tools.to_vec())
            },
            temperature: Some(0.2),
        };

        let response = backend.chat(request).await.context("falha no chat do agente")?;
        let assistant_msg = response
            .first_message()
            .cloned()
            .ok_or_else(|| anyhow!("LLM não retornou mensagem"))?;

        if let Some(calls) = assistant_msg.tool_calls.clone() {
            let signature = calls
                .iter()
                .map(|c| format!("{}:{}", c.function.name, c.function.arguments))
                .collect::<Vec<_>>()
                .join("|");

            if last_signature.as_deref() == Some(signature.as_str()) {
                repeat_count += 1;
                warn!(repeat_count, "repetição de tool_calls detectada");
                if repeat_count >= MAX_REPETICOES {
                    return Err(anyhow!(
                        "limite de repetições atingido ({MAX_REPETICOES}); interrompendo o laço"
                    ));
                }
            } else {
                repeat_count = 0;
                last_signature = Some(signature);
            }

            messages.push(assistant_msg);

            for call in calls {
                let args: Value = serde_json::from_str(&call.function.arguments)
                    .unwrap_or_else(|_| Value::Object(Default::default()));

                debug!(tool = %call.function.name, "executando ferramenta");
                let result = match executor.call_tool(&call.function.name, args).await {
                    Ok(ok) => ok,
                    Err(err) => {
                        warn!(error = %err, tool = %call.function.name, "ferramenta falhou");
                        format!("ERRO ao executar ferramenta '{}': {err}", call.function.name)
                    }
                };
                messages.push(Message::tool(&call.id, result));
            }

            continue;
        }

        messages.push(assistant_msg);
        if let Some(text) = response.content_text() {
            return Ok(text);
        }

        return Err(anyhow!(
            "passo {passo}: LLM respondeu sem conteúdo e sem tool_calls"
        ));
    }

    Err(anyhow!(
        "limite de passos atingido ({MAX_PASSOS}) sem resposta final"
    ))
}

pub struct AgentRunOptions {
    pub via_mcp: bool,
    pub auto_approve: bool,
    pub workspace: Option<Workspace>,
}

impl Default for AgentRunOptions {
    fn default() -> Self {
        Self {
            via_mcp: false,
            auto_approve: false,
            workspace: None,
        }
    }
}

/// Plan-and-Solve com máquina de estados. `auto_approve` pula o HITL (desktop/API).
pub async fn plan_and_solve(
    task: &str,
    backend: &dyn LlmBackend,
    opts: AgentRunOptions,
) -> Result<String> {
    info!(
        via_mcp = opts.via_mcp,
        auto_approve = opts.auto_approve,
        "iniciando plan_and_solve"
    );

    let mut state = AgentState::Planning;
    info!(state = state.name(), "transição de estado");

    let plan = plan_phase(task, backend).await?;
    state = AgentState::AwaitingApproval {
        plan: plan.clone(),
    };
    info!(state = state.name(), passos = plan.len(), "transição de estado");

    let plan_display = plan
        .iter()
        .enumerate()
        .map(|(i, step)| format!("{}. {step}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");

    let steps = if opts.auto_approve {
        info!("auto_approve ativo — pulando checkpoint interativo");
        plan
    } else {
        println!("\n📋 Plano proposto:\n{plan_display}\n");
        let (aprovado, feedback_ou_plano) = human_in_the_loop(&plan_display)?;
        if !aprovado {
            state = AgentState::Cancelled {
                feedback: feedback_ou_plano.clone(),
            };
            info!(state = state.name(), "transição de estado");
            return Ok(format!(
                "Execução cancelada pelo usuário.\nFeedback: {feedback_ou_plano}"
            ));
        }
        parse_plan_lines(&feedback_ou_plano).unwrap_or(plan)
    };

    let mcp_session = if opts.via_mcp {
        Some(McpToolSession::spawn_self().await.context(
            "falha ao iniciar sessão MCP (child process). Use --local-tools / auto_approve local.",
        )?)
    } else {
        None
    };

    let tools = if let Some(session) = mcp_session.as_ref() {
        info!("ferramentas descobertas via MCP tools/list");
        session.openai_tools()
    } else {
        openai_tool_definitions()
    };

    let local_exec = LocalToolExecutor {
        backend,
        workspace: opts.workspace,
        on_progress: None,
    };
    let executor: &dyn ToolExecutor = if let Some(session) = mcp_session.as_ref() {
        session
    } else {
        &local_exec
    };

    let mut accumulated = String::new();
    let mut step_results: Vec<String> = Vec::new();

    for (idx, step) in steps.iter().enumerate() {
        state = AgentState::Executing {
            plan: steps.clone(),
            current_step: idx,
        };
        info!(
            state = state.name(),
            etapa = idx + 1,
            %step,
            "transição de estado"
        );

        let prompt = format!(
            "Tarefa original: {task}\n\n\
             Contexto acumulado até agora:\n{accumulated}\n\n\
             Etapa atual ({}/{}): {step}\n\n\
             Resolva APENAS esta etapa (Least-to-Most). Use ferramentas de arquivo/terminal se necessário.",
            idx + 1,
            steps.len()
        );

        let result = run_agent(&prompt, backend, &tools, executor)
            .await
            .with_context(|| format!("falha ao executar etapa {}: {step}", idx + 1))?;

        step_results.push(format!("Etapa {}: {}\nResultado: {result}", idx + 1, step));
        if !accumulated.is_empty() {
            accumulated.push_str("\n---\n");
        }
        accumulated.push_str(&result);
    }

    state = AgentState::Synthesizing {
        step_results: step_results.clone(),
    };
    info!(state = state.name(), "transição de estado");

    let final_text = synthesize(task, &step_results, backend).await?;
    state = AgentState::Done {
        result: final_text.clone(),
    };
    info!(state = state.name(), "transição de estado");

    drop(mcp_session);
    Ok(final_text)
}

/// Desktop: sem `steps`, um único laço; com `steps` (plano aprovado na tela), uma etapa por vez.
pub async fn run_desktop_agent(
    task: &str,
    steps: &[String],
    backend: &dyn LlmBackend,
    workspace: Workspace,
    on_progress: Option<ProgressSink>,
) -> Result<String> {
    let emit = |ev: ProgressEvent| {
        if let Some(cb) = &on_progress {
            cb(ev);
        }
    };
    // Sem terminal no desktop: o comando não fica preso à pasta (risco) e um programa com
    // janela nunca termina (trava o cartão). A pessoa executa seguindo "Como usar ou conferir".
    let tools: Vec<Tool> = openai_tool_definitions()
        .into_iter()
        .filter(|t| t.function.name != "run_terminal")
        .collect();
    let exec = LocalToolExecutor {
        backend,
        workspace: Some(workspace),
        on_progress: on_progress.clone(),
    };

    let result = if steps.is_empty() {
        let prompt = format!("{task}\n\n{FORMATO_FINAL}");
        run_agent_with_progress(&prompt, backend, &tools, &exec, on_progress.clone()).await?
    } else {
        let mut feito: Vec<String> = Vec::new();
        for (idx, step) in steps.iter().enumerate() {
            emit(ProgressEvent::step("step_start", step.as_str()));
            let anteriores = if feito.is_empty() {
                "(nenhuma)".to_string()
            } else {
                feito.join("\n")
            };
            let prompt = format!(
                "Pedido original: {task}\n\n\
                 Etapas já feitas:\n{anteriores}\n\n\
                 Faça APENAS a etapa {}/{}: {step}\n\
                 Use as ferramentas. Para mudar um arquivo pequeno, leia-o com read_file e grave a versão \
                 completa com write_file (é mais seguro que str_replace).\n\
                 No fim, responda em uma frase curta o que você fez nesta etapa.",
                idx + 1,
                steps.len()
            );
            let resumo =
                run_agent_with_progress(&prompt, backend, &tools, &exec, on_progress.clone())
                    .await
                    .with_context(|| format!("falha na etapa {}: {step}", idx + 1))?;
            emit(ProgressEvent::step("step_done", step.as_str()));
            feito.push(format!("{}. {step}: {}", idx + 1, resumo.trim()));
        }
        emit(ProgressEvent::status("Escrevendo a explicação final…"));
        let prompt = format!(
            "Pedido original: {task}\n\nO que foi feito em cada etapa:\n{}\n\n{FORMATO_FINAL}",
            feito.join("\n")
        );
        chat_text(backend, &prompt).await?
    };

    emit(ProgressEvent::done("Pronto! Trabalho concluído."));
    Ok(result)
}

/// Plano para o desktop: etapas curtas ou, se o pedido for ambíguo, uma pergunta com opções.
pub async fn plan_desktop(task: &str, backend: &dyn LlmBackend) -> Result<PlanOutput> {
    let prompt = format!(
        "Você planeja tarefas para uma pessoa que não programa.\n\
         Se o pedido estiver ambíguo de um jeito que muda muito o resultado (ex.: janela no computador ou página web), \
         devolva {{\"pergunta\":\"...\",\"opcoes\":[\"...\",\"...\"]}} com 2 a 4 opções curtas.\n\
         Senão, devolva {{\"passos\":[\"...\"]}} com 2 a 5 etapas concretas, ordenadas e em linguagem simples. \
         Cada etapa começa com um verbo, ex.: \"Criar o arquivo app.py com a janela principal\".\n\
         Se o pedido já traz a escolha da pessoa, NÃO pergunte de novo: devolva os passos.\n\
         As etapas só podem criar, editar, ler ou organizar arquivos. NÃO inclua etapas de instalar \
         bibliotecas nem de executar ou testar o programa: isso a pessoa faz depois, seguindo as instruções finais.\n\
         Responda APENAS o JSON.\n\nPedido: {task}"
    );
    plan_with_retries(&prompt, backend).await
}

async fn chat_text(backend: &dyn LlmBackend, prompt: &str) -> Result<String> {
    let response = backend
        .chat(ChatRequest {
            model: backend.model_name().to_string(),
            messages: vec![
                Message::system("Você explica com clareza, para pessoas que não programam."),
                Message::user(prompt),
            ],
            tools: None,
            temperature: Some(0.3),
        })
        .await
        .context("falha ao escrever a resposta final")?;
    response
        .content_text()
        .ok_or_else(|| anyhow!("resposta final sem conteúdo"))
}

struct LocalToolExecutor<'a> {
    backend: &'a dyn LlmBackend,
    workspace: Option<Workspace>,
    on_progress: Option<ProgressSink>,
}

#[async_trait::async_trait]
impl ToolExecutor for LocalToolExecutor<'_> {
    async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        if let Some(cb) = &self.on_progress {
            cb(ProgressEvent::tool_start(name, &args));
        }
        let result = execute_tool(self.backend, self.workspace.as_ref(), name, args).await;
        if let Some(cb) = &self.on_progress {
            cb(ProgressEvent::tool_end(name, &result));
        }
        result
    }
}

async fn plan_phase(task: &str, backend: &dyn LlmBackend) -> Result<Vec<String>> {
    let prompt = format!(
        "Você é um planejador. Pense passo a passo (Chain-of-Thought) e depois \
         devolva APENAS um JSON válido no formato {{\"passos\":[\"...\"]}} com 2 a 6 \
         etapas concretas e ordenadas (Least-to-Most) para resolver a tarefa abaixo.\n\n\
         Tarefa: {task}"
    );
    let plan = plan_with_retries(&prompt, backend).await?;
    if plan.passos.is_empty() {
        return Err(anyhow!("plano sem etapas"));
    }
    Ok(plan.passos)
}

async fn plan_with_retries(prompt: &str, backend: &dyn LlmBackend) -> Result<PlanOutput> {
    let mut last_err = None;
    for attempt in 1..=MAX_PLAN_RETRIES {
        let request = ChatRequest {
            model: backend.model_name().to_string(),
            messages: vec![
                Message::system("Responda somente com JSON válido. Sem markdown."),
                Message::user(format!(
                    "{prompt}\n\nTentativa {attempt}/{MAX_PLAN_RETRIES}."
                )),
            ],
            tools: None,
            temperature: Some(0.1),
        };

        let response = backend
            .chat(request)
            .await
            .context("falha na fase de planejamento")?;
        let text = match response.content_text() {
            Some(t) => t,
            None => {
                last_err = Some(anyhow!("planejamento sem conteúdo"));
                continue;
            }
        };

        match parse_plan_typed(&text) {
            Ok(plan) => return Ok(plan),
            Err(err) => {
                warn!(attempt, error = %err, "plano inválido; retry");
                last_err = Some(err);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("falha ao obter plano")))
}

async fn synthesize(
    task: &str,
    step_results: &[String],
    backend: &dyn LlmBackend,
) -> Result<String> {
    let joined = step_results.join("\n\n");
    let prompt = format!(
        "Tarefa original: {task}\n\n\
         Resultados intermediários:\n{joined}\n\n\
         Sintetize uma resposta final clara, acessível e em português, \
         adequada a leitores com necessidades de acessibilidade cognitiva. \
         Se arquivos foram alterados, liste-os."
    );

    let request = ChatRequest {
        model: backend.model_name().to_string(),
        messages: vec![
            Message::system("Você escreve respostas finais curtas, claras e empáticas."),
            Message::user(prompt),
        ],
        tools: None,
        temperature: Some(0.3),
    };

    let response = backend
        .chat(request)
        .await
        .context("falha na fase de sintetização")?;

    response
        .content_text()
        .ok_or_else(|| anyhow!("sintetização sem conteúdo"))
}

fn parse_plan_typed(text: &str) -> Result<PlanOutput> {
    let trimmed = text.trim();
    let json_slice = extract_json_object(trimmed).unwrap_or(trimmed);

    if let Ok(mut plan) = serde_json::from_str::<PlanOutput>(json_slice) {
        let clean = |v: Vec<String>| -> Vec<String> {
            v.into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        plan.passos = clean(plan.passos);
        plan.opcoes = clean(plan.opcoes);
        plan.pergunta = plan.pergunta.filter(|q| !q.trim().is_empty());
        // Pergunta sem opções não dá para responder com um clique.
        if plan.opcoes.len() < 2 {
            plan.pergunta = None;
            plan.opcoes.clear();
        }
        if !plan.passos.is_empty() || plan.pergunta.is_some() {
            return Ok(plan);
        }
        return Err(anyhow!("plano vazio: {text}"));
    }

    parse_plan_lines(trimmed)
        .map(|passos| PlanOutput {
            passos,
            pergunta: None,
            opcoes: vec![],
        })
        .ok_or_else(|| anyhow!("não foi possível interpretar o plano: {text}"))
}

fn parse_plan_lines(text: &str) -> Option<Vec<String>> {
    let steps: Vec<String> = text
        .lines()
        .map(|l| {
            let l = l.trim();
            l.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')' || c == '-')
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty() && s.len() > 2)
        .collect();

    if steps.is_empty() {
        None
    } else {
        Some(steps)
    }
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plan_json_ok() {
        let raw = r#"{"passos":["Ler o texto","Simplificar","Criar lembrete"]}"#;
        let plan = parse_plan_typed(raw).unwrap();
        assert_eq!(plan.passos.len(), 3);
    }

    #[test]
    fn parse_plan_question_with_options() {
        let raw = r#"Claro! {"pergunta":"Janela ou página web?","opcoes":["Janela","Página web"]}"#;
        let plan = parse_plan_typed(raw).unwrap();
        assert_eq!(plan.pergunta.as_deref(), Some("Janela ou página web?"));
        assert_eq!(plan.opcoes.len(), 2);
        assert!(plan.passos.is_empty());
    }

    #[test]
    fn parse_plan_question_without_options_is_rejected() {
        assert!(parse_plan_typed(r#"{"pergunta":"Qual?","opcoes":[]}"#).is_err());
    }
}
