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
    ///
    /// Todas as ferramentas de arquivo e a API passam por aqui. Duas barreiras:
    /// 1. `..` é resolvido de forma léxica, sem tocar no disco (vale para arquivos que ainda vão ser criados);
    /// 2. o ancestral mais profundo que existe é canonicalizado, o que segue symlinks/junctions
    ///    e normaliza o prefixo `\\?\` do Windows. Se ele cair fora da raiz, o caminho é recusado.
    pub fn resolve(&self, user_path: &str) -> Result<PathBuf> {
        let trimmed = user_path.trim();
        if trimmed.is_empty() || trimmed.contains('\0') {
            return Err(anyhow!("Caminho inválido: {user_path:?}"));
        }
        let outside = || anyhow!("Esse caminho fica fora da pasta escolhida: {trimmed}");

        let raw = PathBuf::from(trimmed);
        let joined = if raw.is_absolute() {
            raw
        } else {
            self.root.join(raw)
        };

        // Depois disso não sobra nenhum `..` nos componentes que ainda não existem.
        let normalized = normalize_path(&joined);
        let (existing, rest) = deepest_existing(&normalized);
        let real = existing.canonicalize().map_err(|_| outside())?;
        if !real.starts_with(&self.root) {
            return Err(outside());
        }
        Ok(rest.iter().fold(real, |p, c| p.join(c)))
    }

    pub fn rel_display(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().into_owned())
    }
}

/// Separa `path` em (ancestral mais profundo que existe, componentes restantes ainda inexistentes).
fn deepest_existing(path: &Path) -> (PathBuf, Vec<std::ffi::OsString>) {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    rest.reverse();
    (existing, rest)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pasta temporária única (sem dependência de `tempfile`), apagada no fim do teste.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("a11y-ws-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(p.join("root/sub")).unwrap();
            Self(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn accepts_paths_inside_even_if_missing() {
        let t = TempDir::new("ok");
        let ws = Workspace::new(t.0.join("root")).unwrap();
        let p = ws.resolve("sub/novo/arquivo.txt").unwrap();
        assert!(p.starts_with(ws.root()));
        assert_eq!(ws.rel_display(&p), "sub/novo/arquivo.txt");
        assert!(ws.resolve("sub/../a.txt").unwrap().starts_with(ws.root()));
        let abs_inside = ws.root().join("sub").display().to_string();
        assert!(ws.resolve(&abs_inside).is_ok());
    }

    #[test]
    fn rejects_traversal_and_absolute_outside() {
        let t = TempDir::new("trav");
        let ws = Workspace::new(t.0.join("root")).unwrap();
        for bad in ["../x", "../../etc/passwd", "sub/../../x", "", "a\0b"] {
            let err = ws.resolve(bad);
            assert!(err.is_err(), "deveria recusar {bad:?}");
        }
        let outside = t.0.join("fora.txt").display().to_string();
        let msg = ws.resolve(&outside).unwrap_err().to_string();
        assert!(msg.contains("fora da pasta escolhida"), "{msg}");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let t = TempDir::new("link");
        std::fs::create_dir_all(t.0.join("segredo")).unwrap();
        std::os::unix::fs::symlink(t.0.join("segredo"), t.0.join("root/atalho")).unwrap();
        let ws = Workspace::new(t.0.join("root")).unwrap();
        assert!(ws.resolve("atalho/roubado.txt").is_err());
    }
}
