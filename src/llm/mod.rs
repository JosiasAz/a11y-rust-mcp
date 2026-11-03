pub mod client;
pub mod models;

pub use client::{backend_from_env, shared_openai_compatible_from_env, LlmBackend, OpenAiClient};
