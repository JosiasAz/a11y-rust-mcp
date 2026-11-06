<p align="center">
  <img src="docs/logo-uirapuru.png" width="128" height="128" alt="Logo do Uirapuru: um pássaro uirapuru cor de ferrugem, com a garganta dourada e uma nota musical, sobre fundo marrom-escuro.">
</p>

<h1 align="center">Uirapuru</h1>

<p align="center">
  <strong>Um agente de IA que trabalha no seu computador e explica o que fez, para quem nunca abriu um terminal.</strong><br>
  Roda 100% local com o Ollama. Feito em Manaus, Amazonas.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/vers%C3%A3o-0.1.0-8A3B12" alt="Versão 0.1.0">
  <img src="https://img.shields.io/badge/Rust-1.89%2B-21170F?logo=rust&logoColor=white" alt="Rust 1.89 ou mais novo">
  <img src="https://img.shields.io/badge/.NET-10-512BD4?logo=dotnet&logoColor=white" alt=".NET 10">
  <img src="https://img.shields.io/badge/Avalonia-12-8B44AC" alt="Avalonia 12">
  <img src="https://img.shields.io/badge/Windows-10%20%7C%2011%20(x64)-0078D4" alt="Windows 10 e 11, 64 bits">
  <br>
  <img src="https://img.shields.io/badge/MCP-stdio%20%7C%20HTTP-1F6B4F" alt="Servidor MCP via stdio e HTTP">
  <img src="https://img.shields.io/badge/IA%20local-Ollama-21170F?logo=ollama&logoColor=white" alt="IA local com Ollama">
  <img src="https://img.shields.io/badge/contraste-WCAG%20AA%20medido-1F6B4F" alt="Contraste WCAG AA medido">
  <img src="https://img.shields.io/badge/licen%C3%A7a-MIT-4E3B2E" alt="Licença MIT">
</p>

<p align="center">
  <a href="https://github.com/JosiasAz/a11y-rust-mcp/releases/latest">
    <img src="https://img.shields.io/badge/Baixar-para%20Windows-8A3B12?style=for-the-badge" alt="Baixar o Uirapuru para Windows">
  </a>
</p>

<p align="center">
  <a href="#começar-a-usar">Começar a usar</a> ·
  <a href="#usar-via-mcp">Usar via MCP</a> ·
  <a href="#por-dentro">Por dentro</a> ·
  <a href="#o-que-ainda-não-está-bom">Limites conhecidos</a> ·
  <a href="#licença">Licença</a>
</p>

![Tela inicial do Uirapuru. No topo, uma barra de título marrom-escura de ponta a ponta com o logo do pássaro uirapuru, o nome "Uirapuru", o selo dourado "MANAUS", o texto "Nenhuma pasta aberta", o selo de estado "Pronto" e os botões de minimizar, maximizar e fechar. À esquerda, a barra lateral da mesma cor com o botão dourado "Nova conversa", o cartão "Nenhuma pasta ainda" com o botão "Escolher pasta" e, embaixo, o seletor de modelo de IA. À direita, em fundo bege-claro, a faixa de status, a saudação "Olá! Eu sou o Uirapuru.", quatro ideias de pedido e o aviso de que tudo roda no computador. Embaixo, a caixa de pedido com os botões "Escolher pasta" e "Enviar pedido", este em cor de ferrugem.](docs/tela-inicial.png)

## Destaques

- **Linguagem simples:** cada ação vira uma frase ("Escrevendo arquivo: index.html"), e a resposta final sempre diz o que foi feito e como conferir.
- **Plano antes de agir:** o agente mostra as etapas e só começa depois do seu clique. Se o pedido for ambíguo, ele pergunta.
- **Só mexe na pasta que você escolher:** caminhos fora dela são recusados, inclusive por atalhos (symlinks e junctions).
- **Privado:** a IA roda no seu computador. A API local exige um token novo a cada início e só escuta em `127.0.0.1`.
- **Feito para leitor de tela:** o progresso é anunciado ao vivo, o foco tem contraste de pelo menos 3:1 e a fonte é a Atkinson Hyperlegible.
- **Também é servidor MCP:** Claude Desktop, Claude Code, Cursor e outros clientes podem usar as mesmas ferramentas.

## Requisitos

| Para usar | Para desenvolver |
| --- | --- |
| Windows 10 ou 11, 64 bits | Rust **1.89** ou mais novo |
| [Ollama](https://ollama.com/download) | .NET SDK **10** |
| Cerca de 5 GB livres para o modelo `qwen2.5:7b` (8 GB de RAM ou mais recomendados) | Inno Setup 6 e `signtool` do Windows SDK (só para gerar o instalador) |

---

## Uma conversa de verdade

Esta é uma execução real, gravada em 2 de novembro de 2026 com o modelo `qwen2.5:7b` rodando só na CPU (levou 5 minutos). O texto abaixo não foi editado.

> **Pedido**
>
> Crie uma página simples e acessível para a minha cafeteria, a Café da Praça. Ela abre das 7h às 18h.

> **O que a pessoa vê enquanto o agente trabalha**
>
> Lendo seu pedido e decidindo o que fazer…> ✓ Escrevendo arquivo: index.html> Pensando no próximo passo…

> **Resposta final**
>
> **O que eu fiz:**> Criei um arquivo `index.html` no diretório raiz do seu projeto. Esse arquivo contém um código HTML simples e acessível para a sua cafeteria, Café da Praça, com informações sobre o horário de funcionamento.
>
> **Como usar ou conferir:**
> 1. Abra o arquivo `index.html` em um navegador web.
> 2. Verifique se o título da página é "Café da Praça".
> 3. Confira se o horário de funcionamento está corretamente exibido como "7h às 18h".
> 4. Teste a legibilidade da página, especialmente para pessoas com deficiência visual, para garantir que ela seja acessível.
>
> **Próximo passo sugerido:**> Agora que a página básica foi criada, você pode adicionar mais informações, como menu, localização, ou imagens da cafeteria. Quer adicionar mais detalhes ou ajustar o design?

O arquivo criado já vem com `lang="pt-BR"`, `<title>` e modo escuro. Mesmo assim, a resposta ainda escorrega: "diretório raiz" é jargão que a pessoa não precisava ler. Melhorar isso é parte do trabalho deste projeto.

---

## O mesmo momento, em duas telas

Quando o agente grava o arquivo, o servidor envia este evento:

```json
{"kind":"tool","message":"Escrevendo arquivo: index.html","tool":"write_file","refresh_tree":true,"path":"index.html"}
```

| O que o sistema produz por dentro                              | O que a pessoa vê                                         |
| -------------------------------------------------------------- | --------------------------------------------------------- |
| `"tool":"write_file"`, `"path":"index.html"`                   | ✓ Escrevendo arquivo: index.html                          |
| `falha ao enviar requisição ao LLM`                            | Não consegui falar com o modelo de IA (Ollama). Abra o programa Ollama e tente de novo. |
| `limite de repetições atingido (3); interrompendo o laço`      | Fiquei repetindo a mesma ação sem avançar. Tente pedir de forma mais específica, por exemplo dizendo o nome do arquivo. |
| `limite de passos atingido (16) sem resposta final`            | O pedido era grande demais para uma vez só. Tente dividir em partes menores. |

As mensagens da esquerda estão no código, exatamente assim. A informação é a mesma; o que muda é quem consegue entendê-la. O projeto inteiro gira em torno dessa tradução.

---

## Em leitura fácil

Este programa ajuda você a criar arquivos no computador.

Você escreve o que quer, com as suas palavras.

O programa faz o trabalho e mostra cada passo.

No fim, ele explica o que fez e como conferir.

Ele só mexe na pasta que você escolher.

Tudo fica no seu computador. Nada vai para a internet.

---

## Cinco regras que o código segue

Cada regra aponta para o lugar do código onde ela está implementada, para você conferir.

1. **Ninguém lê nome de ferramenta.** Toda ação vira uma frase. Exemplo: `str_replace` aparece como "Editando arquivo: …".
   Código: `friendly_tool_message` em [`src/agent/progress.rs`](src/agent/progress.rs).

2. **Todo passo tem um fim visível.** Cada ação começa com "…" e termina com ✓ ou ⚠. Nenhuma fica pendurada.
   Código: os eventos `tool`, `tool_done` e `tool_failed` em [`progress.rs`](src/agent/progress.rs), e `ChatMessage.FinishLastStep` em [`desktop/Models/ChatMessage.cs`](desktop/Models/ChatMessage.cs).

3. **A resposta final ensina a conferir.** O modelo é instruído a sempre responder com *o que eu fiz*, *como usar ou conferir* e *próximo passo*.
   Código: prompt de sistema em [`src/agent/loop.rs`](src/agent/loop.rs).

4. **Erro é instrução, não desabafo.** Mensagens técnicas viram uma ação que a pessoa consegue fazer. O detalhe técnico fica recolhido, para quem quiser ajudar.
   Código: `FriendlyError` em [`desktop/ViewModels/MainViewModel.cs`](desktop/ViewModels/MainViewModel.cs).

5. **O leitor de tela ouve o mesmo que os olhos veem.** O status é uma *live region*, cada mensagem tem nome acessível e cada ✓ é falado como "Concluído: …".
   Código: `AutomationProperties` em [`desktop/Views/MainWindow.axaml`](desktop/Views/MainWindow.axaml).

---

## Começar a usar

Você precisa do **[Ollama](https://ollama.com/download)**, o programa que roda a IA no seu computador. Se o modelo ainda não estiver instalado, o app mostra o botão **Baixar modelo agora** com uma barra de progresso, sem precisar abrir o terminal.

Depois:

1. Baixe o `Uirapuru-Setup-<versão>.exe` na [página de versões](https://github.com/JosiasAz/a11y-rust-mcp/releases/latest) e instale. Abra o Ollama e depois o **Uirapuru** pelo menu Iniciar.
   O instalador é assinado com um certificado de teste, então o Windows pode mostrar "O Windows protegeu o computador": clique em **Mais informações → Executar assim mesmo**. Para conferir o arquivo, compare o SHA-256 publicado junto na versão.
2. Clique em **Escolher pasta**.
3. Escreva o pedido e aperte **Enter**.
4. Em menos de um minuto aparece **Meu plano**, com as etapas. Se estiver bom, clique em **Começar**. Se o pedido for ambíguo, o agente pergunta antes ("janela no computador ou página web?") e você responde com um clique.
5. Acompanhe: cada etapa vai de ○ para … e depois para ✓, e o cartão mostra há quanto tempo o agente está trabalhando.

Na barra lateral, em **Modelo de IA**, você escolhe qualquer modelo instalado no Ollama. Modelos menores respondem mais rápido. Para instalar outro, digite no terminal `ollama pull nome-do-modelo` e clique em ⟳.

Sem placa de vídeo, um pedido de três etapas leva uns 5 minutos com o `qwen2.5:7b`. **Cancelar** (ou `Esc`) para o trabalho na hora, e o Ollama deixa de usar a CPU. Ao fechar o app, o modelo também sai da memória.

| Tecla           | O que faz                       |
| --------------- | ------------------------------- |
| `Enter`         | Envia o pedido                  |
| `Shift + Enter` | Pula linha                      |
| `Ctrl + O`      | Escolhe ou troca a pasta        |
| `Esc`           | Cancela                         |
| Setas           | Passam de uma mensagem para outra na conversa |

<details>
<summary><strong>Gerar o instalador (para quem desenvolve)</strong></summary>

Precisa de [Rust](https://rustup.rs/), do [.NET SDK 10+](https://dotnet.microsoft.com/download), do Inno Setup 6 (`winget install JRSoftware.InnoSetup`) e do `signtool` do Windows SDK.

```powershell
powershell -ExecutionPolicy Bypass -File build\empacotar.ps1
```

O script compila o agente, publica o app em `dist\app`, assina `Uirapuru.exe`, `Uirapuru.dll` e `agent\rust-a11y-agent.exe`, e gera `dist\Uirapuru-Setup-<versão>.exe` (também assinado, junto com o desinstalador). A versão vem do `<Version>` em `desktop\A11yAgent.Desktop.csproj`.

O instalador:
- instala só para o usuário, sem pedir administrador (dá para escolher "todos os usuários");
- cria o atalho no menu Iniciar e, se você marcar, na Área de Trabalho;
- se o Ollama não estiver instalado, oferece abrir a página de download dele no final;
- aparece em "Programas instalados" para desinstalar.

**Certificado de assinatura**, nesta ordem:

| Variável de ambiente | Quando usar |
| --- | --- |
| `UIRAPURU_PFX` + `UIRAPURU_PFX_SENHA` | certificado comprado em arquivo `.pfx` |
| `UIRAPURU_CERT_THUMBPRINT` | certificado já instalado no Windows (inclui token USB) |
| nenhuma | usa o certificado de teste "Uirapuru - assinatura de teste" desta máquina; crie com `-CriarCertificadoDeTeste` |

Com o certificado de teste, o Windows mostra o editor, mas ainda avisa "Editor desconhecido" em outras máquinas. Para isso sumir, é preciso um certificado de assinatura de código de uma autoridade certificadora. Use `-SemAssinatura` para gerar sem assinar.

</details>

<details>
<summary><strong>Quando algo não funciona</strong></summary>

| O que aparece                       | O que fazer                                                        |
| ----------------------------------- | ------------------------------------------------------------------ |
| Selo **Desligado**                  | Abra o Ollama e clique em **Tentar ligar de novo**                  |
| "Falta baixar o modelo de IA"       | Clique em **Baixar modelo agora** (Esc cancela)                     |
| "Fiquei repetindo a mesma ação"     | Peça de novo dizendo o nome do arquivo                              |
| `Acesso negado` ao recompilar       | Feche o app e o processo `rust-a11y-agent.exe`                      |
| "Outra cópia do agente já está aberta" | Feche o processo `rust-a11y-agent.exe` que sobrou e tente de novo |

</details>

---

## Usar via MCP

O mesmo executável do agente é um servidor [MCP](https://modelcontextprotocol.io) (Model Context Protocol). Assim, Claude Desktop, Claude Code, Cursor e outros clientes usam as ferramentas do Uirapuru. Ele se apresenta como `uirapuru`.

Depois de instalar, o executável fica em `%LOCALAPPDATA%\Programs\Uirapuru\agent\rust-a11y-agent.exe`.

**Claude Desktop, Cursor e similares** (configuração MCP do cliente):

```json
{
  "mcpServers": {
    "uirapuru": {
      "command": "C:\\Users\\SEU_USUARIO\\AppData\\Local\\Programs\\Uirapuru\\agent\\rust-a11y-agent.exe",
      "args": ["mcp-server"],
      "env": {
        "A11Y_WORKSPACE": "C:\\caminho\\da\\pasta\\permitida",
        "LLM_MODEL": "qwen2.5:7b"
      }
    }
  }
}
```

**Claude Code:**

```powershell
claude mcp add uirapuru --env A11Y_WORKSPACE="C:\caminho\da\pasta" --env LLM_MODEL=qwen2.5:7b -- "$env:LOCALAPPDATA\Programs\Uirapuru\agent\rust-a11y-agent.exe" mcp-server
```

**Skill (opcional):** o arquivo [`skills/uirapuru/SKILL.md`](skills/uirapuru/SKILL.md) ensina o agente a usar as ferramentas na ordem certa: simplificar, perguntar o que a pessoa entendeu e conferir. No Claude Code, copie a pasta `skills/uirapuru` para `~/.claude/skills/`.

**Por HTTP** (clientes que se conectam por URL): rode `rust-a11y-agent.exe mcp-http --bind 127.0.0.1:8000`. Ao iniciar, ele imprime `A11Y_API_TOKEN=<token>`; configure o cliente com a URL `http://127.0.0.1:8000/mcp` e o header `Authorization: Bearer <token>`. O token muda a cada início.

| Ferramenta | O que faz |
| --- | --- |
| `simplificar_texto` | Reescreve um texto difícil em linguagem simples (usa o modelo do Ollama) |
| `verificar_compreensao` | Confere se um texto ficou compreensível, por partes (usa o modelo) |
| `criar_lembrete` | Cria um lembrete curto e claro |
| `read_file`, `write_file`, `str_replace` | Lê, cria e edita arquivos |
| `list_dir`, `mkdir`, `delete_path`, `create_structure` | Lista, cria e apaga pastas e arquivos |
| `glob_files`, `grep_search` | Procura arquivos por nome e texto |
| `run_terminal` | Roda um comando na pasta (veja o aviso abaixo) |

Todas as ferramentas ficam presas à pasta `A11Y_WORKSPACE`. Se ela não for definida, vale a pasta de onde o cliente abriu o processo. **`run_terminal` executa comandos de verdade:** no modo padrão, só a pasta inicial e o tempo-limite limitam o comando. Para isolar de fato, adicione `"A11Y_SANDBOX": "docker"` no `env` (precisa do Docker Desktop aberto).

---

## O que ainda não está bom

Este projeto quer servir de referência em acessibilidade, então os limites ficam à vista:

- **No app, o agente não roda comandos no terminal.** Ele cria e edita arquivos, e a explicação final diz como abrir o resultado. Nos modos `chat` e MCP o `run_terminal` continua disponível. Por padrão ele roda no shell local (`A11Y_SANDBOX=host`), que **não isola nada** além da pasta inicial e do tempo-limite. Com `A11Y_SANDBOX=docker` o comando roda num container sem rede que só enxerga a pasta.
- **O `mcp-http` usa HTTP sem TLS.** O token protege contra quem não o conhece, mas na rede local ele passa em texto aberto. Por isso ele só aceita `127.0.0.1` ou `::1`; para escutar na rede (ex.: `--bind 0.0.0.0:8000`) é preciso passar `--allow-remote`, sabendo que qualquer um com o token pode criar, apagar arquivos e rodar comandos na pasta.
- **Ainda não houve teste com NVDA, Narrador nem com pessoas usuárias.** Tudo acima é intenção de projeto, não resultado medido. A única exceção são os contrastes, que foram medidos: texto principal 13,2:1, texto de apoio 8,5:1 e 5,7:1, botão de ação 7,7:1 e anel de foco 5,8:1 (os valores estão comentados em `desktop/App.axaml`).
- **Modelos pequenos erram e usam jargão**, como o "diretório raiz" da conversa lá em cima.

Se você usa leitor de tela, tem deficiência cognitiva ou trabalha com quem tem, o seu relato vale mais que qualquer código. Abra uma *issue* contando o que aconteceu, sem precisar saber programar.

---

## Por dentro

```mermaid
sequenceDiagram
    actor P as Pessoa
    participant UI as Interface (Avalonia)
    participant A as Agente (Rust)
    participant M as Modelo (Ollama)
    P->>UI: "Crie uma página para a minha cafeteria"
    UI->>A: POST /api/agent/run-stream
    A->>M: pedido + ferramentas disponíveis
    M-->>A: chamar write_file(index.html)
    A-->>UI: tool: "Escrevendo arquivo: index.html"
    A->>A: grava o arquivo dentro da pasta escolhida
    A-->>UI: tool_done
    A->>M: resultado da ferramenta
    M-->>A: resposta em três blocos
    A-->>UI: final
    UI-->>P: cartão com ✓, botão do arquivo e explicação
```

A mesma caixa de ferramentas também está disponível via **MCP**; veja [Usar via MCP](#usar-via-mcp).

<details>
<summary><strong>Compilar, testar e estrutura do código</strong></summary>

```bat
cargo build --release
cargo test
cd desktop && dotnet run
```

```text
src/
  api/          API HTTP (Axum) e eventos ao vivo (SSE) para a interface
  agent/        laço do agente, ferramentas, pasta permitida, mensagens amigáveis
  llm/          cliente compatível com a API da OpenAI (Ollama por padrão)
  mcp_server/   servidor MCP via stdio e HTTP
desktop/
  Models/       ChatMessage e ChatStep (um cartão, seus passos e seus arquivos)
  Services/     AgentHost (liga e desliga o agente) e AgentApiClient (HTTP + SSE)
  ViewModels/   estado da tela
  Views/        layout e acessibilidade (AutomationProperties)
```

**Comandos do agente**

| Comando                                   | Para quê                           |
| ----------------------------------------- | ---------------------------------- |
| `serve-api --bind 127.0.0.1:8787`         | API usada pela interface           |
| `chat --task "..." --workspace .`         | agente no terminal, com aprovação do plano |
| `mcp-server`                              | MCP via stdio                      |
| `mcp-http --bind 127.0.0.1:8000`          | MCP via HTTP                       |

`serve-api` e `mcp-http` geram um token novo a cada início e imprimem `A11Y_API_TOKEN=<token>` no stdout. Toda requisição precisa do header `Authorization: Bearer <token>`; sem ele a resposta é 401. O app desktop lê esse token sozinho. O `serve-api` só aceita endereços de loopback (`127.0.0.1` ou `::1`).

**Configuração**

| Variável         | Padrão                      |
| ---------------- | --------------------------- |
| `LLM_PROVIDER`   | `ollama` (também `openai`, `nvidia`) |
| `LLM_MODEL`      | `qwen2.5:7b`                |
| `LLM_BASE_URL`   | `http://127.0.0.1:11434/v1` |
| `LLM_API_KEY`    | `ollama`                    |
| `A11Y_WORKSPACE` | pasta atual (modos MCP e `chat`) |
| `A11Y_AGENT_EXE` | busca automática do binário |
| `A11Y_SANDBOX`   | `host` (também `docker`)    |
| `A11Y_SANDBOX_IMAGE` | `alpine:3`              |
| `A11Y_LANG`      | idioma da interface (o app envia); senão `LC_ALL`/`LANG`; senão pt-BR. Aceita `en` |

</details>

<details>
<summary><strong>Eventos ao vivo e ferramentas</strong></summary>

**Eventos** (`POST /api/agent/run-stream`, `event: progress`)

| `kind`        | Significa                   | Na tela                     |
| ------------- | --------------------------- | --------------------------- |
| `status`      | O agente está pensando      | Atividade atual, em itálico |
| `step_start`  | Uma etapa do plano começou  | A etapa vai de ○ para …     |
| `step_done`   | A etapa terminou            | A etapa vira ✓              |
| `tool`        | Uma ação começou            | Passo "…"                   |
| `tool_done`   | A ação deu certo            | O passo vira ✓              |
| `tool_failed` | A ação falhou               | O passo vira ⚠, com o motivo |
| `final`       | Resposta em três blocos     | Texto do cartão             |
| `error`       | O pedido inteiro falhou     | Cartão de erro              |

**Ferramentas de acessibilidade cognitiva:** `simplificar_texto`, `verificar_compreensao`, `criar_lembrete`.

**Ferramentas de arquivo:** `read_file`, `write_file`, `str_replace`, `list_dir`, `mkdir`, `delete_path`, `glob_files`, `grep_search`, `create_structure` e `run_terminal` (este só nos modos `chat` e MCP).

**Plano:** `POST /api/agent/plan` com `{"task": "...", "model": "..."}` devolve `{"passos": [...]}` ou `{"pergunta": "...", "opcoes": [...]}`. As etapas aprovadas vão em `steps` no `run-stream`. `GET /api/models` lista os modelos do Ollama.

**Como adicionar uma ferramenta:** defina os argumentos em `src/agent/tools.rs` ou `fs_tools.rs`, registre em `openai_tool_definitions()` e no MCP (`src/mcp_server/handler.rs`). Depois escreva a frase que a pessoa vai ler em `progress.rs`, leia em voz alta e reescreva até soar natural.

</details>

---

## Licença

[MIT](./LICENSE). Criado e mantido por **Josias Azevedo da Silva** ([@JosiasAz](https://github.com/JosiasAz)).
