//! Isolamento do `run_terminal`.
//!
//! A ferramenta não chama `std::process::Command` diretamente: ela pede a um [`CommandSandbox`].
//! A escolha vem de `A11Y_SANDBOX`:
//! - `host` (padrão): shell local (PowerShell/sh) com cwd = workspace e timeout real;
//! - `docker`: container descartável sem rede, que só enxerga o workspace montado em `/workspace`.
//!
//! shortcut: não há backend WASM (o wasmtime adiciona ~10 MB e o WASI não roda shell);
//! basta implementar `CommandSandbox` quando houver demanda.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait CommandSandbox: Send + Sync {
    /// Nome curto exibido nos logs/resultado (ex.: "host", "docker:alpine:3").
    fn name(&self) -> String;
    fn run(&self, cmd: &str, cwd: &Path, timeout: Duration) -> Result<CommandOutput, String>;
}

/// Shell do próprio computador. Não isola nada além do cwd e do timeout.
pub struct HostShell;

impl CommandSandbox for HostShell {
    fn name(&self) -> String {
        "host".into()
    }

    fn run(&self, cmd: &str, cwd: &Path, timeout: Duration) -> Result<CommandOutput, String> {
        #[cfg(windows)]
        let mut c = {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", cmd]);
            c
        };
        #[cfg(not(windows))]
        let mut c = {
            let mut c = Command::new("sh");
            c.args(["-lc", cmd]);
            c
        };
        c.current_dir(cwd);
        spawn_with_timeout(c, timeout, || {})
    }
}

/// Container Docker descartável: sem rede, memória/processos limitados, raiz só-leitura,
/// e só o workspace montado (leitura e escrita) em `/workspace`.
pub struct DockerSandbox {
    pub image: String,
}

impl CommandSandbox for DockerSandbox {
    fn name(&self) -> String {
        format!("docker:{}", self.image)
    }

    fn run(&self, cmd: &str, cwd: &Path, timeout: Duration) -> Result<CommandOutput, String> {
        let mount = format!("{}:/workspace", docker_host_path(cwd));
        // Nome único: matar o cliente `docker` não para o container, então o timeout usa `docker kill`.
        let name = format!(
            "a11y-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        let mut c = Command::new("docker");
        c.args([
            "run", "--rm", "--name", &name,
            "--network", "none",
            "--memory", "512m",
            "--cpus", "1",
            "--pids-limit", "128",
            "--read-only",
            "--tmpfs", "/tmp",
            "--cap-drop", "ALL",
            "--security-opt", "no-new-privileges",
            "-v", &mount,
            "-w", "/workspace",
            &self.image,
            "sh", "-lc", cmd,
        ]);
        let kill = || {
            let _ = Command::new("docker")
                .args(["kill", &name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        };
        spawn_with_timeout(c, timeout, kill).map_err(|e| {
            format!("{e} (o Docker Desktop está aberto? Para usar o shell local, defina A11Y_SANDBOX=host)")
        })
    }
}

/// O root canônico do Windows vem como `\\?\C:\...`, que o `docker -v` não aceita.
fn docker_host_path(p: &Path) -> String {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// Lê `A11Y_SANDBOX` (`host` | `docker`) e `A11Y_SANDBOX_IMAGE` (padrão `alpine:3`).
pub fn from_env() -> Box<dyn CommandSandbox> {
    match std::env::var("A11Y_SANDBOX").as_deref().map(str::trim) {
        Ok(v) if v.eq_ignore_ascii_case("docker") => Box::new(DockerSandbox {
            image: std::env::var("A11Y_SANDBOX_IMAGE").unwrap_or_else(|_| "alpine:3".into()),
        }),
        _ => Box::new(HostShell),
    }
}

/// Roda o processo e mata o filho direto se passar do prazo.
/// stdout/stderr são drenados em threads para o filho não travar com o pipe cheio.
fn spawn_with_timeout(
    mut cmd: Command,
    timeout: Duration,
    on_timeout: impl FnOnce(),
) -> Result<CommandOutput, String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não consegui iniciar o comando: {e}"))?;

    let drain = |mut r: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = drain(Box::new(child.stdout.take().expect("stdout piped")));
    let err = drain(Box::new(child.stderr.take().expect("stderr piped")));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                on_timeout();
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "o comando passou do limite de {}s e foi interrompido",
                    timeout.as_secs()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("erro esperando o comando: {e}")),
        }
    };

    Ok(CommandOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_shell_runs_and_captures() {
        let out = HostShell
            .run("echo ola", &std::env::temp_dir(), Duration::from_secs(30))
            .unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.contains("ola"));
    }

    #[test]
    fn host_shell_enforces_timeout() {
        #[cfg(windows)]
        let cmd = "Start-Sleep -Seconds 10";
        #[cfg(not(windows))]
        let cmd = "sleep 10";
        let started = Instant::now();
        let err = HostShell
            .run(cmd, &std::env::temp_dir(), Duration::from_secs(1))
            .unwrap_err();
        assert!(err.contains("limite"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    /// Precisa do Docker Desktop aberto: `cargo test -- --ignored docker_isolates`.
    #[test]
    #[ignore]
    fn docker_isolates_network_and_mounts_workspace() {
        let sb = DockerSandbox { image: "alpine:3".into() };
        let ws = std::env::temp_dir().join("a11y-docker-test");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("marca.txt"), "ok").unwrap();
        let t = Duration::from_secs(120);
        let out = sb.run("cat /workspace/marca.txt", &ws, t).unwrap();
        assert_eq!(out.stdout.trim(), "ok");
        let net = sb.run("wget -T 3 -q -O- http://example.com", &ws, t).unwrap();
        assert_ne!(net.exit_code, 0, "o container não deveria ter rede");
    }

    #[test]
    fn docker_path_drops_verbatim_prefix() {
        assert_eq!(docker_host_path(Path::new(r"\\?\C:\ws")), r"C:\ws");
        assert_eq!(docker_host_path(Path::new("/home/u/ws")), "/home/u/ws");
    }
}
