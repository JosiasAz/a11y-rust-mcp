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

    /// Lê `LLM_BASE_URL`, `LLM_API_KEY` e `LLM_MODEL`.
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("LLM_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let api_key = std::env::var("LLM_API_KEY").map_err(|_| LlmError::MissingEnv("LLM_API_KEY"))?;
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

        if model.trim().is_empty() {
            return Err(LlmError::MissingEnv("LLM_MODEL").into());
        }

        Ok(Self::new(base_url, api_key, model))
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
        let list: List = self
            .http
            .get(format!("{}/models", self.base_url))
            .bearer_auth(&self.api_key)
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
                     Rode `ollama list` e defina LLM_MODEL com um nome válido \
                     (ex: set LLM_MODEL=qwen2.5:7b). Detalhe: {}",
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

/// Cliente Ollama (API OpenAI-compatible em `/v1`).
#[derive(Debug, Clone)]
pub struct OllamaClient {
    inner: OpenAiClient,
}

impl OllamaClient {
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("LLM_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_string());
        let api_key = std::env::var("LLM_API_KEY").unwrap_or_else(|_| "ollama".to_string());
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_string());

        Ok(Self {
            inner: OpenAiClient::new(base_url, api_key, model),
        })
    }

    pub fn model(&self) -> &str {
        self.inner.model()
    }

    pub fn into_inner(self) -> OpenAiClient {
        self.inner
    }
}

#[async_trait]
impl LlmBackend for OllamaClient {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        self.inner.chat(request).await
    }
}

/// Escolhe o backend a partir de `LLM_PROVIDER` (`ollama` | `openai`, default: ollama).
pub fn backend_from_env() -> Result<Box<dyn LlmBackend>> {
    let provider = std::env::var("LLM_PROVIDER")
        .unwrap_or_else(|_| "ollama".to_string())
        .to_lowercase();

    match provider.as_str() {
        "openai" | "nvidia" => Ok(Box::new(OpenAiClient::from_env()?)),
        _ => Ok(Box::new(OllamaClient::from_env()?)),
    }
}

/// Cliente concreto compartilhado (MCP handler precisa de tipo `Clone`).
pub fn shared_openai_compatible_from_env() -> Result<OpenAiClient> {
    let provider = std::env::var("LLM_PROVIDER")
        .unwrap_or_else(|_| "ollama".to_string())
        .to_lowercase();

    match provider.as_str() {
        "openai" | "nvidia" => OpenAiClient::from_env(),
        _ => Ok(OllamaClient::from_env()?.into_inner()),
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
