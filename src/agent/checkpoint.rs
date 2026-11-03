use anyhow::{anyhow, Result};
use dialoguer::{theme::ColorfulTheme, Input, Select};
use tracing::info;

/// Human-in-the-Loop: (y) Aprovar, (n) Rejeitar com feedback, (e) Editar.
///
/// Retorna `(aprovado, texto)` — no caso de edição, `texto` é o plano editado.
pub fn human_in_the_loop(proposta: &str) -> Result<(bool, String)> {
    println!();
    println!("──────────── Checkpoint (Human-in-the-Loop) ────────────");
    println!("{proposta}");
    println!("───────────────────────────────────────────────────────");

    let opcoes = vec![
        "(y) Aprovar",
        "(n) Rejeitar com feedback",
        "(e) Editar",
    ];

    let selecao = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("O que deseja fazer com esta proposta?")
        .items(&opcoes)
        .default(0)
        .interact()?;

    match selecao {
        0 => {
            info!("plano aprovado pelo usuário");
            Ok((true, proposta.to_string()))
        }
        1 => {
            let feedback: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Descreva o motivo da rejeição / o que mudar")
                .interact_text()?;
            info!("plano rejeitado pelo usuário");
            Ok((false, feedback))
        }
        2 => {
            let editado: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Edite o texto/plano")
                .with_initial_text(proposta)
                .interact_text()?;
            info!("plano editado e aceito pelo usuário");
            Ok((true, editado))
        }
        _ => Err(anyhow!("opção de checkpoint inválida")),
    }
}
