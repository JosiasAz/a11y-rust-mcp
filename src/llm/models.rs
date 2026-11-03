use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mensagem no formato OpenAI-compatible (chat completions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_with_tools(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn default_tool_type() -> String {
    "function".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

impl ChatResponse {
    pub fn first_message(&self) -> Option<&Message> {
        self.choices.first().map(|c| &c.message)
    }

    pub fn content_text(&self) -> Option<String> {
        self.first_message()
            .and_then(|m| m.content.clone())
            .filter(|s| !s.trim().is_empty())
    }

    pub fn tool_calls(&self) -> Option<&[ToolCall]> {
        self.first_message()
            .and_then(|m| m.tool_calls.as_deref())
            .filter(|calls| !calls.is_empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub index: Option<u32>,
    pub message: Message,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: Option<u32>,
    #[serde(default)]
    pub completion_tokens: Option<u32>,
    #[serde(default)]
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionSchema,
}

impl Tool {
    pub fn function(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            kind: "function".into(),
            function: FunctionSchema {
                name: name.into(),
                description: Some(description.into()),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSchema {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

/// Plano tipado produzido na fase Plan-and-Solve.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanOutput {
    #[serde(default)]
    pub passos: Vec<String>,
    /// Pergunta ao usuário quando o pedido é ambíguo (só no desktop).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pergunta: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opcoes: Vec<String>,
}

/// Converte um tipo `JsonSchema` no objeto JSON Schema esperado pelo Function Calling.
pub fn schema_for_type<T: JsonSchema>() -> Value {
    let root = schemars::schema_for!(T);
    let mut value = serde_json::to_value(root).unwrap_or_else(|_| serde_json::json!({
        "type": "object",
        "properties": {}
    }));

    // OpenAI espera o schema do objeto (não o envelope RootSchema completo, quando houver).
    if let Some(obj) = value.as_object_mut() {
        if obj.contains_key("schema") {
            if let Some(inner) = obj.remove("schema") {
                return inner;
            }
        }
        obj.remove("$schema");
    }
    value
}
