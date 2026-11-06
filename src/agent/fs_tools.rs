use std::fs;
use std::io::Write;
use std::path::{Component, Path};
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use super::workspace::Workspace;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadFileArgs {
    /// Caminho relativo ao workspace.
    pub path: String,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StrReplaceArgs {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListDirArgs {
    /// Diretório relativo (`. ` = raiz).
    #[serde(default = "default_dot")]
    pub path: String,
}

fn default_dot() -> String {
    ".".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MkDirArgs {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeletePathArgs {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GlobArgs {
    /// Padrão glob, ex: `**/*.rs`
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GrepArgs {
    pub pattern: String,
    #[serde(default = "default_dot")]
    pub path: String,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default = "default_grep_limit")]
    pub max_results: usize,
}

fn default_grep_limit() -> usize {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunTerminalArgs {
    /// Comando (PowerShell no Windows / sh no Unix). Roda com cwd = workspace.
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateStructureArgs {
    /// Lista de caminhos de pastas a criar.
    #[serde(default)]
    pub directories: Vec<String>,
    /// Lista de arquivos `{ "path": "...", "content": "..." }`.
    #[serde(default)]
    pub files: Vec<StructureFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StructureFile {
    pub path: String,
    #[serde(default)]
    pub content: String,
}

pub fn execute_fs_tool(ws: &Workspace, name: &str, args: Value) -> Result<String, String> {
    match name {
        "read_file" => {
            let a: ReadFileArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            read_file(ws, &a)
        }
        "write_file" => {
            let a: WriteFileArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            write_file(ws, &a)
        }
        "str_replace" => {
            let a: StrReplaceArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            str_replace(ws, &a)
        }
        "list_dir" => {
            let a: ListDirArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            list_dir(ws, &a)
        }
        "mkdir" => {
            let a: MkDirArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            mkdir(ws, &a)
        }
        "delete_path" => {
            let a: DeletePathArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_path(ws, &a)
        }
        "glob_files" => {
            let a: GlobArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            glob_files(ws, &a)
        }
        "grep_search" => {
            let a: GrepArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            grep_search(ws, &a)
        }
        "run_terminal" => {
            let a: RunTerminalArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            run_terminal(ws, &a)
        }
        "create_structure" => {
            let a: CreateStructureArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            create_structure(ws, &a)
        }
        other => Err(format!("ferramenta fs desconhecida: {other}")),
    }
}

pub fn fs_tool_names() -> &'static [&'static str] {
    &[
        "read_file",
        "write_file",
        "str_replace",
        "list_dir",
        "mkdir",
        "delete_path",
        "glob_files",
        "grep_search",
        "run_terminal",
        "create_structure",
    ]
}

fn read_file(ws: &Workspace, args: &ReadFileArgs) -> Result<String, String> {
    let path = ws.resolve(&args.path).map_err(|e| e.to_string())?;
    let text = fs::read_to_string(&path).map_err(|e| format!("read_file: {e}"))?;
    let lines: Vec<&str> = text.lines().collect();
    let start = args.offset.unwrap_or(0).min(lines.len());
    let end = args
        .limit
        .map(|l| (start + l).min(lines.len()))
        .unwrap_or(lines.len());
    let slice = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>4}|{l}", start + i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "Arquivo {} (linhas {}-{} de {}):\n{slice}",
        ws.rel_display(&path),
        start + 1,
        end,
        lines.len()
    ))
}

fn write_file(ws: &Workspace, args: &WriteFileArgs) -> Result<String, String> {
    let path = ws.resolve(&args.path).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir pai: {e}"))?;
    }
    // Sempre UTF-8 (evita lixo de encoding no Windows).
    fs::write(&path, args.content.as_bytes()).map_err(|e| format!("write_file: {e}"))?;
    Ok(format!(
        "Arquivo escrito: {} ({} bytes)",
        ws.rel_display(&path),
        args.content.len()
    ))
}

fn str_replace(ws: &Workspace, args: &StrReplaceArgs) -> Result<String, String> {
    // Texto vazio "casa" entre cada letra: com replace_all o arquivo inteiro vira lixo.
    if args.old_string.is_empty() {
        return Err(
            "old_string vazio; para reescrever o arquivo inteiro use write_file com o conteúdo completo"
                .into(),
        );
    }
    let path = ws.resolve(&args.path).map_err(|e| e.to_string())?;
    let original = fs::read_to_string(&path).map_err(|e| format!("str_replace read: {e}"))?;
    let count = original.matches(&args.old_string).count();
    if count == 0 {
        return Err("old_string não encontrado no arquivo".into());
    }
    if !args.replace_all && count > 1 {
        return Err(format!(
            "old_string aparece {count} vezes; use replace_all=true ou trecho único"
        ));
    }
    let updated = if args.replace_all {
        original.replace(&args.old_string, &args.new_string)
    } else {
        original.replacen(&args.old_string, &args.new_string, 1)
    };
    fs::write(&path, &updated).map_err(|e| format!("str_replace write: {e}"))?;
    Ok(format!(
        "Edição aplicada em {} ({} ocorrência(s))",
        ws.rel_display(&path),
        if args.replace_all { count } else { 1 }
    ))
}

fn list_dir(ws: &Workspace, args: &ListDirArgs) -> Result<String, String> {
    let path = ws.resolve(&args.path).map_err(|e| e.to_string())?;
    let mut entries = fs::read_dir(&path)
        .map_err(|e| format!("list_dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let kind = if e.path().is_dir() { "dir" } else { "file" };
            format!("{kind}\t{name}")
        })
        .collect::<Vec<_>>();
    entries.sort();
    Ok(format!(
        "Conteúdo de {} ({} itens):\n{}",
        ws.rel_display(&path),
        entries.len(),
        entries.join("\n")
    ))
}

fn mkdir(ws: &Workspace, args: &MkDirArgs) -> Result<String, String> {
    let path = ws.resolve(&args.path).map_err(|e| e.to_string())?;
    fs::create_dir_all(&path).map_err(|e| format!("mkdir: {e}"))?;
    Ok(format!("Diretório criado: {}", ws.rel_display(&path)))
}

fn delete_path(ws: &Workspace, args: &DeletePathArgs) -> Result<String, String> {
    let path = ws.resolve(&args.path).map_err(|e| e.to_string())?;
    if path == ws.root() {
        return Err("recusado: não é permitido apagar a raiz do workspace".into());
    }
    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| format!("delete_path dir: {e}"))?;
    } else {
        fs::remove_file(&path).map_err(|e| format!("delete_path file: {e}"))?;
    }
    Ok(format!("Removido: {}", ws.rel_display(&path)))
}

const MAX_GLOB_RESULTS: usize = 200;

/// Percorre só o workspace e compara o caminho relativo com o padrão.
///
/// Antes o padrão era colado na raiz e passado a `glob::glob`, o que tinha três problemas:
/// padrões absolutos ou com `..` varriam o disco fora da pasta; uma única pasta sem
/// permissão abortava a busca inteira; e no Windows a raiz canônica `\\?\C:\...` virava
/// `//?/C:/...`, em que o `?` é curinga.
fn glob_files(ws: &Workspace, args: &GlobArgs) -> Result<String, String> {
    let normalized = args.pattern.trim().replace('\\', "/");
    let raw = normalized.trim_start_matches("./");
    if raw.is_empty() {
        return Err("padrão glob vazio".into());
    }
    let escapes = Path::new(raw).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if escapes {
        return Err(format!(
            "padrão fora da pasta: use um caminho relativo à pasta, sem '..' (ex: **/*.rs). Recebido: {}",
            args.pattern
        ));
    }
    let pattern = glob::Pattern::new(raw).map_err(|e| format!("padrão glob inválido: {e}"))?;
    let opts = glob::MatchOptions {
        case_sensitive: !cfg!(windows),
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };

    let mut matches = Vec::new();
    let mut truncated = false;
    // WalkDir não segue symlinks por padrão; entradas ilegíveis são puladas, não abortam a busca.
    for entry in WalkDir::new(ws.root())
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = ws.rel_display(entry.path());
        if pattern.matches_with(&rel, opts) {
            if matches.len() >= MAX_GLOB_RESULTS {
                truncated = true;
                break;
            }
            matches.push(rel);
        }
    }
    let count = matches.len();
    if truncated {
        matches.push(format!("… (truncado em {MAX_GLOB_RESULTS})"));
    }
    Ok(format!(
        "Glob '{}' → {} arquivo(s):\n{}",
        args.pattern,
        count,
        matches.join("\n")
    ))
}

fn grep_search(ws: &Workspace, args: &GrepArgs) -> Result<String, String> {
    let re = Regex::new(&args.pattern).map_err(|e| format!("regex inválida: {e}"))?;
    let root = ws.resolve(&args.path).map_err(|e| e.to_string())?;
    let mut hits = Vec::new();

    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if let Some(g) = &args.glob {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !glob_match_simple(g, name) {
                continue;
            }
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for (idx, line) in content.lines().enumerate() {
            if re.is_match(line) {
                hits.push(format!(
                    "{}:{}:{}",
                    ws.rel_display(path),
                    idx + 1,
                    truncate(line, 200)
                ));
                if hits.len() >= args.max_results {
                    return Ok(format!(
                        "grep '{}' ({} hits, limite):\n{}",
                        args.pattern,
                        hits.len(),
                        hits.join("\n")
                    ));
                }
            }
        }
    }
    Ok(format!(
        "grep '{}' ({} hits):\n{}",
        args.pattern,
        hits.len(),
        hits.join("\n")
    ))
}

fn run_terminal(ws: &Workspace, args: &RunTerminalArgs) -> Result<String, String> {
    let cmd = args.command.trim();
    if cmd.is_empty() {
        return Err("comando vazio".into());
    }
    if is_dangerous(cmd) {
        return Err(format!("comando bloqueado por política de segurança: {cmd}"));
    }

    let sandbox = super::sandbox::from_env();
    let timeout = Duration::from_secs(args.timeout_secs.clamp(1, 300));
    let output = sandbox.run(cmd, ws.root(), timeout)?;
    Ok(format!(
        "exit={} (sandbox={})
--- stdout ---
{}
--- stderr ---
{}",
        output.exit_code,
        sandbox.name(),
        truncate(&output.stdout, 8000),
        truncate(&output.stderr, 4000)
    ))
}

fn create_structure(ws: &Workspace, args: &CreateStructureArgs) -> Result<String, String> {
    let mut log = Vec::new();
    for dir in &args.directories {
        let path = ws.resolve(dir).map_err(|e| e.to_string())?;
        fs::create_dir_all(&path).map_err(|e| format!("dir {dir}: {e}"))?;
        log.push(format!("dir  {}", ws.rel_display(&path)));
    }
    for file in &args.files {
        let path = ws.resolve(&file.path).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = fs::File::create(&path).map_err(|e| e.to_string())?;
        f.write_all(file.content.as_bytes())
            .map_err(|e| e.to_string())?;
        log.push(format!("file {}", ws.rel_display(&path)));
    }
    Ok(format!(
        "Estrutura criada ({} itens):\n{}",
        log.len(),
        log.join("\n")
    ))
}

fn is_dangerous(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    const BLOCK: &[&str] = &[
        "format ",
        "mkfs",
        "shutdown",
        "reboot",
        "rm -rf /",
        "del /s /q c:\\",
        "reg delete",
        "diskpart",
        "invoke-webrequest",
        "curl ",
        "wget ",
        "start-process",
    ];
    BLOCK.iter().any(|b| lower.contains(b))
}

fn glob_match_simple(pattern: &str, name: &str) -> bool {
    // suporte mínimo: *.ext
    if let Some(ext) = pattern.strip_prefix("*.") {
        return name.ends_with(&format!(".{ext}"));
    }
    pattern == name || pattern == "*"
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_replace_rejects_empty_old_string_and_keeps_file() {
        let dir = std::env::temp_dir().join(format!("a11y-str-replace-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("app.py"), "print('oi')").unwrap();
        let ws = Workspace::new(&dir).unwrap();

        let args = StrReplaceArgs {
            path: "app.py".into(),
            old_string: String::new(),
            new_string: "lixo".into(),
            replace_all: true,
        };
        assert!(str_replace(&ws, &args).is_err());
        assert_eq!(fs::read_to_string(dir.join("app.py")).unwrap(), "print('oi')");
        let _ = fs::remove_dir_all(&dir);
    }

    fn run_glob(ws: &Workspace, pattern: &str) -> Result<String, String> {
        glob_files(ws, &GlobArgs { pattern: pattern.into() })
    }

    #[test]
    fn glob_finds_files_inside_workspace() {
        let dir = std::env::temp_dir().join(format!("a11y-glob-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/agent")).unwrap();
        fs::write(dir.join("main.rs"), "").unwrap();
        fs::write(dir.join("src/agent/mod.rs"), "").unwrap();
        fs::write(dir.join("src/notas.txt"), "").unwrap();
        let ws = Workspace::new(&dir).unwrap();

        let all = run_glob(&ws, "**/*.rs").unwrap();
        assert!(all.contains("main.rs") && all.contains("src/agent/mod.rs"), "{all}");
        assert!(!all.contains("notas.txt"), "{all}");

        let top = run_glob(&ws, "*.rs").unwrap();
        assert!(top.contains("main.rs") && !top.contains("src/agent/mod.rs"), "{top}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn glob_rejects_patterns_outside_workspace() {
        let dir = std::env::temp_dir().join(format!("a11y-glob-out-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let ws = Workspace::new(&dir).unwrap();
        for bad in ["../*", "src/../../*", "/etc/*", "", "   "] {
            assert!(run_glob(&ws, bad).is_err(), "deveria recusar {bad:?}");
        }
        #[cfg(windows)]
        assert!(run_glob(&ws, "C:/Windows/*").is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
