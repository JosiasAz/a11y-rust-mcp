//! Traduções das mensagens amigáveis do agente.
//!
//! Os textos ficam em `locales/<idioma>.json` (chave plana → texto com `{placeholders}`)
//! e são embutidos no binário com `include_str!`, então nenhum arquivo é procurado em runtime.
//! Idioma: `A11Y_LANG` (o desktop passa o idioma da interface) > `LC_ALL` > `LANG` > pt-BR.
//! Para adicionar um idioma: crie o JSON, uma variante em [`Lang`] e um braço em `table`.

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    PtBr,
    En,
}

type Table = HashMap<String, String>;

fn table(lang: Lang) -> &'static Table {
    static PT_BR: OnceLock<Table> = OnceLock::new();
    static EN: OnceLock<Table> = OnceLock::new();
    let (cell, src) = match lang {
        Lang::PtBr => (&PT_BR, include_str!("../locales/pt_br.json")),
        Lang::En => (&EN, include_str!("../locales/en.json")),
    };
    // Os JSONs são embutidos e cobertos por teste: um erro aqui é bug de build, não de usuário.
    cell.get_or_init(|| serde_json::from_str(src).expect("locale JSON inválido"))
}

/// Decide o idioma a partir dos valores de ambiente (pura, para teste).
pub fn detect(a11y_lang: Option<&str>, lc_all: Option<&str>, lang: Option<&str>) -> Lang {
    let chosen = [a11y_lang, lc_all, lang]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|v| !v.is_empty() && *v != "C" && *v != "POSIX");
    match chosen {
        Some(v) if v.to_ascii_lowercase().starts_with("en") => Lang::En,
        _ => Lang::PtBr,
    }
}

/// Idioma do processo, decidido uma vez.
pub fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        // Os testes fixam pt-BR para não depender do LANG da máquina que os roda.
        if cfg!(test) {
            return Lang::PtBr;
        }
        let env = |k| std::env::var(k).ok();
        detect(
            env("A11Y_LANG").as_deref(),
            env("LC_ALL").as_deref(),
            env("LANG").as_deref(),
        )
    })
}

/// Traduz `key` no idioma do processo, trocando `{nome}` pelos valores de `args`.
pub fn t(key: &str, args: &[(&str, &str)]) -> String {
    tr(lang(), key, args)
}

/// Igual a [`t`], com idioma explícito. Sem tradução: cai para pt-BR e depois para a própria chave.
pub fn tr(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let template = table(lang)
        .get(key)
        .or_else(|| table(Lang::PtBr).get(key))
        .map(String::as_str)
        .unwrap_or(key);
    args.iter().fold(template.to_string(), |s, (name, value)| {
        s.replace(&format!("{{{name}}}"), value)
    })
}

/// Existe tradução para esta chave?
pub fn has(key: &str) -> bool {
    table(Lang::PtBr).contains_key(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_locales_have_the_same_keys() {
        let mut pt: Vec<_> = table(Lang::PtBr).keys().collect();
        let mut en: Vec<_> = table(Lang::En).keys().collect();
        pt.sort();
        en.sort();
        assert_eq!(pt, en);
    }

    #[test]
    fn detects_language() {
        assert_eq!(detect(Some("en-US"), None, None), Lang::En);
        assert_eq!(detect(None, None, Some("en_GB.UTF-8")), Lang::En);
        assert_eq!(detect(Some("pt-BR"), None, Some("en_US")), Lang::PtBr);
        assert_eq!(detect(Some(""), Some("C"), Some("en_US")), Lang::En);
        assert_eq!(detect(None, None, None), Lang::PtBr);
    }

    #[test]
    fn translates_with_placeholders_and_fallback() {
        assert_eq!(
            tr(Lang::En, "tool.write_file", &[("path", "a.md")]),
            "Writing file: a.md"
        );
        assert_eq!(
            tr(Lang::PtBr, "tool.write_file", &[("path", "a.md")]),
            "Escrevendo arquivo: a.md"
        );
        assert_eq!(tr(Lang::En, "chave.inexistente", &[]), "chave.inexistente");
    }
}
