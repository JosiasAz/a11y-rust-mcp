use serde::Serialize;
use std::sync::Arc;

use crate::i18n::{self, t};

/// Evento de progresso enviado ao desktop (SSE).
#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_tree: Option<bool>,
    /// Arquivo/pasta tocado pela ferramenta (relativo ao workspace), para o desktop abrir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl ProgressEvent {
    pub fn status(message: impl Into<String>) -> Self {
        Self {
            kind: "status".into(),
            message: message.into(),
            tool: None,
            refresh_tree: None,
            path: None,
        }
    }

    pub fn tool_start(tool: &str, args: &serde_json::Value) -> Self {
        Self {
            kind: "tool".into(),
            message: friendly_tool_message(tool, args),
            tool: Some(tool.into()),
            refresh_tree: Some(changes_files(tool)),
            path: args.get("path").and_then(|v| v.as_str()).map(String::from),
        }
    }

    /// Fim de uma ferramenta: `tool_done` (ok) ou `tool_failed`.
    pub fn tool_end(tool: &str, result: &Result<String, String>) -> Self {
        let (kind, message) = match result {
            Ok(ok) => ("tool_done", friendly_tool_result(tool, ok)),
            Err(err) => ("tool_failed", t("tool.failed", &[("error", &truncate(err, 120))])),
        };
        Self {
            kind: kind.into(),
            message,
            tool: Some(tool.into()),
            refresh_tree: Some(changes_files(tool) && result.is_ok()),
            path: None,
        }
    }

    /// Início/fim de uma etapa do plano aprovado (`step_start` / `step_done`).
    pub fn step(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            tool: None,
            refresh_tree: None,
            path: None,
        }
    }

    pub fn done(message: impl Into<String>) -> Self {
        Self {
            kind: "done".into(),
            message: message.into(),
            tool: None,
            refresh_tree: Some(true),
            path: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            kind: "error".into(),
            message: message.into(),
            tool: None,
            refresh_tree: None,
            path: None,
        }
    }
}

fn changes_files(tool: &str) -> bool {
    matches!(
        tool,
        "write_file" | "mkdir" | "create_structure" | "delete_path" | "str_replace"
    )
}

pub type ProgressSink = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

/// Frase amigável, no idioma do processo, sobre o que a ferramenta vai fazer.
pub fn friendly_tool_message(tool: &str, args: &serde_json::Value) -> String {
    let arg = |k: &str, default: &str| {
        args.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    };
    let key = format!("tool.{tool}");
    if !i18n::has(&key) {
        return t("tool.other", &[("tool", tool)]);
    }
    let path = arg("path", if tool == "list_dir" { "." } else { "…" });
    let command = truncate(&arg("command", "…"), 80);
    t(&key, &[("path", &path), ("command", &command)])
}

/// Resumo curto do resultado da ferramenta (evita despejar arquivo inteiro no chat).
pub fn friendly_tool_result(tool: &str, result: &str) -> String {
    let one_line = result.lines().next().unwrap_or(result).trim();
    let short = truncate(one_line, 100);
    match tool {
        "write_file" | "mkdir" | "create_structure" | "str_replace" | "delete_path" => {
            t("result.changed", &[("summary", &short)])
        }
        "read_file" => t("result.read_file", &[("size", &format_bytes(result.len()))]),
        "list_dir" | "glob_files" | "grep_search" => {
            t("result.found", &[("count", &result.lines().count().to_string())])
        }
        "run_terminal" => t("result.run_terminal", &[("summary", &short)]),
        _ => t("result.other", &[("summary", &short)]),
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn format_bytes(n: usize) -> String {
    if n < 1024 {
        t("size.chars", &[("n", &n.to_string())])
    } else {
        t("size.kb", &[("n", &format!("{:.1}", n as f64 / 1024.0))])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_start_carries_path_and_friendly_text() {
        let ev = ProgressEvent::tool_start("write_file", &json!({"path": "docs/a.md"}));
        assert_eq!(ev.kind, "tool");
        assert_eq!(ev.path.as_deref(), Some("docs/a.md"));
        assert_eq!(ev.refresh_tree, Some(true));
        assert_eq!(ev.message, "Escrevendo arquivo: docs/a.md");
    }

    #[test]
    fn tool_end_reports_ok_and_failure() {
        let ok = ProgressEvent::tool_end("read_file", &Ok("abc".into()));
        assert_eq!(ok.kind, "tool_done");
        assert_eq!(ok.refresh_tree, Some(false));
        let fail = ProgressEvent::tool_end("write_file", &Err("sem permissão".into()));
        assert_eq!(fail.kind, "tool_failed");
        assert!(fail.message.contains("sem permissão"));
        assert_eq!(fail.refresh_tree, Some(false));
    }
}
