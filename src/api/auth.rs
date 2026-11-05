//! Autenticação efêmera da API local.
//!
//! O processo gera um token aleatório forte ao subir, guarda só em memória e o imprime
//! uma única vez no stdout (`A11Y_API_TOKEN=<hex>`). O desktop, que iniciou o processo,
//! lê essa linha e manda `Authorization: Bearer <token>` em toda requisição (HTTP e SSE).
//! O token morre junto com o processo.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Prefixo da linha do stdout que o desktop procura.
pub const TOKEN_LINE_PREFIX: &str = "A11Y_API_TOKEN=";

#[derive(Clone)]
pub struct ApiToken(Arc<str>);

impl ApiToken {
    /// 32 bytes do CSPRNG do sistema operacional, em hex (64 caracteres).
    pub fn generate() -> Result<Self> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| anyhow!("CSPRNG indisponível: {e}"))?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Ok(Self(hex.into()))
    }

    /// Publica o token para o processo pai. Os logs vão para o stderr, então essa é a única linha no stdout.
    pub fn announce(&self) -> Result<()> {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{TOKEN_LINE_PREFIX}{}", self.0)?;
        out.flush()?;
        Ok(())
    }

    /// Valida o header `Authorization` sem vazar tempo de comparação.
    pub fn matches_header(&self, header_value: Option<&str>) -> bool {
        let Some(given) = header_value.and_then(|h| h.strip_prefix("Bearer ")) else {
            return false;
        };
        constant_time_eq(given.trim().as_bytes(), self.0.as_bytes())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    // O tamanho do token é público (sempre 64), então sair cedo aqui não vaza nada.
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Middleware do axum: responde 401 se o Bearer estiver ausente ou errado.
pub async fn require_bearer(State(token): State<ApiToken>, req: Request, next: Next) -> Response {
    let header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if token.matches_header(header) {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            "Acesso negado: token da sessão ausente ou inválido.",
        )
            .into_response()
    }
}

/// A API controla arquivos e o terminal: ela só pode escutar no próprio computador.
pub fn ensure_loopback(bind: &str) -> Result<()> {
    let addr: SocketAddr = bind
        .parse()
        .with_context(|| format!("endereço inválido: {bind} (use ex.: 127.0.0.1:8787)"))?;
    if !addr.ip().is_loopback() {
        return Err(anyhow!(
            "por segurança a API só escuta em 127.0.0.1 ou ::1 (recebido: {bind})"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_64_hex_and_unique() {
        let a = ApiToken::generate().unwrap();
        let b = ApiToken::generate().unwrap();
        assert_eq!(a.0.len(), 64);
        assert!(a.0.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn header_validation() {
        let t = ApiToken::generate().unwrap();
        let good = format!("Bearer {}", t.0);
        assert!(t.matches_header(Some(&good)));
        assert!(!t.matches_header(None));
        assert!(!t.matches_header(Some(&*t.0))); // sem "Bearer "
        assert!(!t.matches_header(Some("Bearer abc")));
        let mut wrong = t.0.to_string();
        wrong.replace_range(0..1, if wrong.starts_with('0') { "1" } else { "0" });
        assert!(!t.matches_header(Some(&format!("Bearer {wrong}"))));
    }

    #[test]
    fn only_loopback_bind() {
        assert!(ensure_loopback("127.0.0.1:8787").is_ok());
        assert!(ensure_loopback("[::1]:8787").is_ok());
        assert!(ensure_loopback("0.0.0.0:8787").is_err());
        assert!(ensure_loopback("192.168.0.10:8787").is_err());
        assert!(ensure_loopback("localhost:8787").is_err());
    }
}
