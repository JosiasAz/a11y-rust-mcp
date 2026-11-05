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
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
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

/// Depois que o shell termina, quanto esperamos o resto da saída de processos que ele deixou
/// rodando (ex.: `notepad`, `npm start &`), que herdam o pipe e o mantêm aberto.
const PIPE_GRACE: Duration = Duration::from_secs(2);

const BACKGROUND_WARNING: &str = "\n[aviso: o comando deixou um processo rodando em segundo plano; \
     a saída pode estar incompleta]";

/// Um pedaço lido de stdout (`false`) ou stderr (`true`).
type Chunk = (bool, Vec<u8>);

/// Lê `r` em pedaços e manda pelo canal até o pipe fechar.
fn pump(mut r: impl Read + Send + 'static, is_err: bool, tx: mpsc::Sender<Chunk>) {
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((is_err, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

fn push_chunk((is_err, bytes): Chunk, out: &mut Vec<u8>, err: &mut Vec<u8>) {
    if is_err {
        err.extend_from_slice(&bytes);
    } else {
        out.extend_from_slice(&bytes);
    }
}

/// Mata o processo e os filhos dele (o `child.kill()` sozinho deixaria netos vivos).
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // O filho foi criado como líder do próprio grupo (`process_group(0)`).
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// Roda o processo com prazo real.
///
/// stdout/stderr são lidos em threads (para o filho não travar com o pipe cheio) e chegam por
/// canal, então o prazo vale também para a leitura: um neto que segura o pipe aberto não
/// trava mais a ferramenta (antes o `join()` das threads esperava para sempre).
fn spawn_with_timeout(
    mut cmd: Command,
    timeout: Duration,
    on_timeout: impl FnOnce(),
) -> Result<CommandOutput, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não consegui iniciar o comando: {e}"))?;

    let (tx, rx) = mpsc::channel::<Chunk>();
    pump(child.stdout.take().expect("stdout piped"), false, tx.clone());
    pump(child.stderr.take().expect("stderr piped"), true, tx);

    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut pipes_open = true;
    let deadline = Instant::now() + timeout;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                kill_tree(&mut child);
                return Err(format!("erro esperando o comando: {e}"));
            }
        }
        let now = Instant::now();
        if now >= deadline {
            on_timeout();
            kill_tree(&mut child);
            let _ = child.wait();
            return Err(format!(
                "o comando passou do limite de {}s e foi interrompido",
                timeout.as_secs()
            ));
        }
        let wait = (deadline - now).min(Duration::from_millis(100));
        if pipes_open {
            match rx.recv_timeout(wait) {
                Ok(chunk) => push_chunk(chunk, &mut out, &mut err),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => pipes_open = false,
            }
        } else {
            thread::sleep(wait);
        }
    };

    // O shell terminou: drena o resto, mas sem esperar para sempre por um neto que segura o pipe.
    let grace_end = Instant::now() + PIPE_GRACE;
    let mut lingering = false;
    while pipes_open {
        let now = Instant::now();
        if now >= grace_end {
            lingering = true;
            break;
        }
        match rx.recv_timeout(grace_end - now) {
            Ok(chunk) => push_chunk(chunk, &mut out, &mut err),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lingering = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => pipes_open = false,
        }
    }

    let mut stderr = String::from_utf8_lossy(&err).into_owned();
    if lingering {
        stderr.push_str(BACKGROUND_WARNING);
    }
    Ok(CommandOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr,
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

    /// Antes, um processo em segundo plano segurando o stdout travava a ferramenta para sempre.
    #[cfg(unix)]
    #[test]
    fn host_shell_does_not_hang_on_background_process() {
        let started = Instant::now();
        let out = HostShell
            .run("sleep 30 & echo pronto", &std::env::temp_dir(), Duration::from_secs(60))
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(10), "travou esperando o pipe");
        assert!(out.stdout.contains("pronto"));
        assert!(out.stderr.contains("segundo plano"), "{}", out.stderr);
    }

    #[cfg(unix)]
    #[test]
    fn host_shell_timeout_kills_grandchildren() {
        let marker = std::env::temp_dir().join(format!("a11y-neto-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let cmd = format!("(sleep 3; touch '{}') & sleep 10", marker.display());
        let err = HostShell
            .run(&cmd, &std::env::temp_dir(), Duration::from_secs(1))
            .unwrap_err();
        assert!(err.contains("limite"), "{err}");
        thread::sleep(Duration::from_secs(4));
        assert!(!marker.exists(), "o neto sobreviveu ao timeout");
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
