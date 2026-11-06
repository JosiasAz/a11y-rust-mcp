use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use thiserror::Error;
use tracing::{debug, instrument, warn};

use super::models::{ChatRequest, ChatResponse, Message, Tool};

const DEFAULT_MAX_RETRIES: u32 = 3;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("variável de ambiente ausente: {0}")]
    MissingEnv(&'static str),
    #[error("falha HTTP ao falar com o LLM: {0}")]
    Http(#[from] reqwest::Error),
    #[error("resposta inválida do LLM: {0}")]
    InvalidResponse(String),
}

/// Backend assíncrono OpenAI-compatible (OpenAI, NVIDIA NIM, Ollama `/v1`, etc.).
#[async_trait]
pub trait LlmBackend: Send + Sync {
    fn model_name(&self) -> &str;

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;

    async fn chat_messages(
        &self,
        messages: &[Message],
        tools: Option<&[Tool]>,
    ) -> Result<ChatResponse> {
        self.chat(ChatRequest {
            model: self.model_name().to_string(),
            messages: messages.to_vec(),
            tools: tools.map(|t| t.to_vec()),
            temperature: Some(0.2),
        })
        .await
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiClient {
    http: Client,
    base_url: String,
    api_key: String,
    model: String,
    max_retries: u32,
}

impl OpenAiClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries.max(1);
        self
    }

    /// Modelos disponíveis no backend (`GET /models`, que o Ollama também oferece em `/v1`).
    pub async fn list_models(&self) -> Result<Vec<String>> {
        #[derive(serde::Deserialize)]
        struct Item {
            id: String,
        }
        #[derive(serde::Deserialize)]
        struct List {
            data: Vec<Item>,
        }
        let mut req = self
            .http
            .get(format!("{}/models", self.base_url))
            .bearer_auth(&self.api_key);
        // A rota /models nativa da Anthropic autentica por x-api-key, não por Bearer.
        if self.base_url.contains("api.anthropic.com") {
            req = req
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01");
        }
        let list: List = req
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .context("falha ao listar modelos")?
            .error_for_status()?
            .json()
            .await
            .context("lista de modelos inválida")?;
        Ok(list.data.into_iter().map(|m| m.id).collect())
    }

    async fn chat_once(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        debug!(%url, "enviando chat completion");

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .context("falha ao enviar requisição ao LLM")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("falha ao ler corpo da resposta do LLM")?;

        if !status.is_success() {
            if status.as_u16() == 404 && body.contains("not found") {
                return Err(anyhow!(
                    "modelo '{}' não encontrado no backend {}. \
                     Defina LLM_MODEL com um nome válido para esse provedor \
                     (no Ollama, veja `ollama list`). Detalhe: {}",
                    request.model,
                    self.base_url,
                    truncate(&body, 400)
                ));
            }
            return Err(anyhow!(
                "LLM retornou status {}: {}",
                status,
                truncate(&body, 800)
            ));
        }

        let parsed: ChatResponse = serde_json::from_str(&body).map_err(|err| {
            LlmError::InvalidResponse(format!("{err}: {}", truncate(&body, 400)))
        })?;

        if parsed.choices.is_empty() {
            return Err(LlmError::InvalidResponse("choices vazio".into()).into());
        }

        Ok(parsed)
    }
}

#[async_trait]
impl LlmBackend for OpenAiClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    #[instrument(skip(self, request), fields(model = %request.model, messages = request.messages.len()))]
    async fn chat(&self, mut request: ChatRequest) -> Result<ChatResponse> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }

        let mut attempt = 0u32;
        loop {
            match self.chat_once(&request).await {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    attempt += 1;
                    if attempt >= self.max_retries {
                        return Err(err).context("esgotadas as tentativas de chat com o LLM");
                    }
                    let wait = Duration::from_secs(2u64.pow(attempt.saturating_sub(1)));
                    warn!(attempt, ?wait, error = %err, "retry do LLM com backoff");
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }
}

pub const PROVIDERS: &str =
    "ollama, openai, anthropic (claude), nvidia, gemini, groq, openrouter, deepseek, mistral, custom";

/// (URL base, variável da chave, modelo padrão) de cada provedor com API OpenAI-compatible.
/// Chave vazia = provedor local, não exige chave.
fn preset(provider: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match provider {
        "ollama" => ("http://127.0.0.1:11434/v1", "", "qwen2.5:7b"),
        "openai" => ("https://api.openai.com/v1", "OPENAI_API_KEY", "gpt-4o-mini"),
        "anthropic" | "claude" => ("https://api.anthropic.com/v1", "ANTHROPIC_API_KEY", "claude-sonnet-5-5"),
        "nvidia" => ("https://integrate.api.nvidia.com/v1", "NVIDIA_API_KEY", "nvidia/nemotron-3-super-120b-a12b"),
        "gemini" | "google" => (
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "GEMINI_API_KEY",
            "gemini-2.5-flash",
        ),
        "groq" => ("https://api.groq.com/openai/v1", "GROQ_API_KEY", "llama-3.3-70b-versatile"),
        "openrouter" => ("https://openrouter.ai/api/v1", "OPENROUTER_API_KEY", "openrouter/auto"),
        "deepseek" => ("https://api.deepseek.com/v1", "DEEPSEEK_API_KEY", "deepseek-chat"),
        "mistral" => ("https://api.mistral.ai/v1", "MISTRAL_API_KEY", "mistral-small-latest"),
        // Qualquer outro servidor OpenAI-compatible (LM Studio, vLLM, Azure...): exige LLM_BASE_URL e LLM_MODEL.
        "custom" => ("", "", ""),
        _ => return None,
    })
}

/// Monta o cliente a partir de `LLM_PROVIDER`; `LLM_BASE_URL`, `LLM_API_KEY` e `LLM_MODEL` sobrescrevem o preset.
fn resolve(get: impl Fn(&str) -> Option<String>) -> Result<OpenAiClient> {
    let get = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let provider = get("LLM_PROVIDER").unwrap_or_else(|| "ollama".into()).to_lowercase();
    let (url, key_var, model) = preset(&provider)
        .ok_or_else(|| anyhow!("LLM_PROVIDER '{provider}' desconhecido. Use um destes: {PROVIDERS}"))?;

    let base_url = get("LLM_BASE_URL").or_else(|| (!url.is_empty()).then(|| url.into()))
        .ok_or(LlmError::MissingEnv("LLM_BASE_URL"))?;
    let model = get("LLM_MODEL").or_else(|| (!model.is_empty()).then(|| model.into()))
        .ok_or(LlmError::MissingEnv("LLM_MODEL"))?;
    let api_key = match get("LLM_API_KEY").or_else(|| (!key_var.is_empty()).then(|| get(key_var)).flatten()) {
        Some(k) => k,
        None if key_var.is_empty() => "local".into(),
        None => return Err(LlmError::MissingEnv(key_var).into()),
    };
    Ok(OpenAiClient::new(base_url, api_key, model))
}

/// Backend escolhido por `LLM_PROVIDER` (padrão: ollama).
pub fn backend_from_env() -> Result<Box<dyn LlmBackend>> {
    Ok(Box::new(shared_openai_compatible_from_env()?))
}

/// Cliente concreto compartilhado (MCP handler precisa de tipo `Clone`).
pub fn shared_openai_compatible_from_env() -> Result<OpenAiClient> {
    resolve(|k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn with(vars: &[(&str, &str)]) -> Result<OpenAiClient> {
        let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        resolve(|k| map.get(k).cloned())
    }

    #[test]
    fn padrao_e_ollama_sem_chave() {
        let c = with(&[]).unwrap();
        assert_eq!(c.base_url, "http://127.0.0.1:11434/v1");
        assert_eq!(c.model, "qwen2.5:7b");
    }

    #[test]
    fn provedor_usa_chave_propria_e_preset() {
        let c = with(&[("LLM_PROVIDER", "Claude"), ("ANTHROPIC_API_KEY", "sk-ant")]).unwrap();
        assert_eq!(c.base_url, "https://api.anthropic.com/v1");
        assert_eq!(c.api_key, "sk-ant");
        assert_eq!(c.model, "claude-sonnet-5-5");
    }

    #[test]
    fn llm_vars_sobrescrevem_preset() {
        let c = with(&[
            ("LLM_PROVIDER", "nvidia"),
            ("LLM_API_KEY", "k"),
            ("NVIDIA_API_KEY", "ignorada"),
            ("LLM_MODEL", "x/y"),
        ])
        .unwrap();
        assert_eq!(c.api_key, "k");
        assert_eq!(c.model, "x/y");
        assert_eq!(c.base_url, "https://integrate.api.nvidia.com/v1");
    }

    #[test]
    fn erros_claros() {
        assert!(with(&[("LLM_PROVIDER", "groq")]).unwrap_err().to_string().contains("GROQ_API_KEY"));
        assert!(with(&[("LLM_PROVIDER", "xyz")]).unwrap_err().to_string().contains("desconhecido"));
        assert!(with(&[("LLM_PROVIDER", "custom")]).unwrap_err().to_string().contains("LLM_BASE_URL"));
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let shortened: String = s.chars().take(max).collect();
        format!("{shortened}…")
    }
}
