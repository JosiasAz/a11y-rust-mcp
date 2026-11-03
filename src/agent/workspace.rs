use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};

/// Raiz de trabalho sandboxed: todas as tools de arquivo/shell ficam dentro dela.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root
            .as_ref()
            .canonicalize()
            .with_context(|| format!("workspace inválido: {}", root.as_ref().display()))?;
        if !root.is_dir() {
            return Err(anyhow!("workspace não é diretório: {}", root.display()));
        }
        Ok(Self { root })
    }

    pub fn from_env_or(default: impl AsRef<Path>) -> Result<Self> {
        if let Ok(p) = std::env::var("A11Y_WORKSPACE") {
            Self::new(p)
        } else {
            Self::new(default)
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve um caminho relativo (ou absoluto dentro da raiz) e impede escape do sandbox.
    pub fn resolve(&self, user_path: &str) -> Result<PathBuf> {
        let raw = PathBuf::from(user_path.trim());
        let joined = if raw.is_absolute() {
            raw
        } else {
            self.root.join(raw)
        };

        // Normaliza sem exigir que o arquivo exista (para write/mkdir).
        let normalized = normalize_path(&joined);
        let root_norm = normalize_path(&self.root);

        if !normalized.starts_with(&root_norm) {
            return Err(anyhow!(
                "caminho fora do workspace: {user_path} (root={})",
                self.root.display()
            ));
        }
        Ok(normalized)
    }

    pub fn rel_display(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().into_owned())
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
