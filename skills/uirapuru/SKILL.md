---
name: uirapuru
description: Usa o servidor MCP do Uirapuru para acessibilidade cognitiva. Reescreve texto difícil (burocrático, jurídico, técnico) em linguagem simples, confere se a pessoa entendeu e cria lembretes. Use quando o usuário pedir "explica de um jeito simples", "não entendi esse documento", "resume para leigo", "linguagem simples", "plain language", "easy read", ou quando a pessoa disser que tem dificuldade de leitura, TDAH, dislexia, deficiência intelectual ou é idosa. Requer o servidor MCP `uirapuru` instalado.
---

# Uirapuru: linguagem simples e acessível

O servidor MCP `uirapuru` (https://github.com/JosiasAz/a11y-rust-mcp) oferece ferramentas para tornar informação compreensível para qualquer pessoa. Ele roda localmente com Ollama.

## Antes de começar

Verifique se as ferramentas `simplificar_texto`, `verificar_compreensao` e `criar_lembrete` estão disponíveis. Se não estiverem, diga ao usuário como instalar e pare:

```powershell
claude mcp add uirapuru --env A11Y_WORKSPACE="C:\caminho\da\pasta" --env LLM_MODEL=qwen2.5:7b -- "$env:LOCALAPPDATA\Programs\Uirapuru\agent\rust-a11y-agent.exe" mcp-server
```

O instalador está em https://github.com/JosiasAz/a11y-rust-mcp/releases/latest. O Ollama precisa estar aberto.

## Quando usar cada ferramenta

| Situação | Ferramenta | Argumentos |
|---|---|---|
| Texto difícil de entender (contrato, edital, bula, carta do banco, mensagem de erro) | `simplificar_texto` | `texto`, `nivel`: `"basico"` (padrão) ou `"intermediario"` |
| Depois de explicar algo importante, para conferir se a pessoa entendeu | `verificar_compreensao` | `texto_referencia`, `explicacao_usuario` (o que a pessoa disse com as palavras dela) |
| A pessoa precisa fazer algo depois (prazo, consulta, pagamento) | `criar_lembrete` | `titulo`, `quando`, `detalhes` (opcional) |

## Fluxo recomendado

1. **Simplifique.** Chame `simplificar_texto` com o texto original. Use `"basico"` se a pessoa pediu "bem simples" ou demonstrou dificuldade. Use `"intermediario"` se ela só quer um resumo mais claro.
2. **Mostre o resultado** sem acrescentar jargão de volta. Frases curtas, uma ideia por frase, voz ativa.
3. **Pergunte o que ela entendeu.** Peça que explique com as próprias palavras. Não pergunte apenas "entendeu?".
4. **Confira.** Chame `verificar_compreensao` com o texto simplificado e a explicação da pessoa. Se a avaliação mostrar lacunas, explique só a parte que faltou, de outro jeito.
5. **Ofereça lembrete** se o texto tiver prazo ou tarefa. Use `criar_lembrete` só se a pessoa aceitar.

## Como falar com a pessoa

- Diga o que vai fazer antes de fazer ("Vou reescrever esse texto em palavras mais simples").
- Não trate a pessoa como criança nem comente a dificuldade dela.
- Mantenha os fatos: datas, valores, nomes e obrigações não podem mudar ao simplificar. Se o resultado da ferramenta mudar algum desses dados, corrija e avise.
- Em documentos jurídicos ou médicos, lembre que a versão simples ajuda a entender, mas não substitui o documento original nem um profissional.

## Ferramentas de arquivo

O servidor também expõe `read_file`, `write_file`, `str_replace`, `list_dir`, `mkdir`, `delete_path`, `glob_files`, `grep_search`, `create_structure` e `run_terminal`, todas presas à pasta `A11Y_WORKSPACE`. Use `read_file` para abrir um documento que a pessoa quer simplificar e `write_file` para salvar a versão simples ao lado do original. Peça confirmação antes de sobrescrever ou apagar arquivos e antes de usar `run_terminal`.
