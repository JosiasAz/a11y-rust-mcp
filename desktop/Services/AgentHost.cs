using System.Diagnostics;
using System.Globalization;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;

namespace A11yAgent.Desktop.Services;

/// <summary>
/// Sobe a API Rust em segundo plano (sem janela) e encerra ao fechar o app.
/// A API exige um token efêmero que ela mesma gera e imprime no stdout ao subir;
/// só quem iniciou o processo consegue lê-lo.
/// </summary>
public sealed class AgentHost : IDisposable
{
    public const string BaseUrl = "http://127.0.0.1:8787";

    private Process? _process;
    private bool _startedByUs;
    private readonly HttpClient _http = new() { Timeout = TimeSpan.FromSeconds(2) };
    private TaskCompletionSource<string> _token = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private const string TokenPrefix = "A11Y_API_TOKEN=";

    /// Token da sessão para o header Authorization. Nulo até o agente subir.
    public string? ApiToken => _token.Task.IsCompletedSuccessfully ? _token.Task.Result : null;

    /// Modelo usado no último pedido; é descarregado da memória do Ollama ao fechar o app.
    public string? ActiveModel { get; set; }

    public async Task<bool> EnsureRunningAsync(IProgress<string>? progress = null, CancellationToken ct = default)
    {
        // Sem token não dá para usar uma instância antiga: ela é de outro processo (ou ficou órfã).
        if (await ProbeAsync(ct) != ProbeResult.Down)
        {
            progress?.Report("Outra cópia do agente já está aberta. Feche-a (ou reinicie o computador) e tente de novo.");
            return false;
        }

        var exe = FindAgentExe();
        if (exe is null)
        {
            progress?.Report("Uma parte do Uirapuru está faltando. Reinstale o Uirapuru pelo instalador.");
            return false;
        }

        progress?.Report("Ligando o agente em segundo plano…");
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = exe,
                Arguments = "serve-api --bind 127.0.0.1:8787",
                WorkingDirectory = Path.GetDirectoryName(exe)!,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
            };
            psi.Environment["LLM_PROVIDER"] = Environment.GetEnvironmentVariable("LLM_PROVIDER") ?? "ollama";
            // Fora do Ollama, sem LLM_MODEL o agente usa o modelo padrão do provedor.
            if (OllamaService.IsOllamaProvider || Environment.GetEnvironmentVariable("LLM_MODEL") is { Length: > 0 })
                psi.Environment["LLM_MODEL"] = OllamaService.ConfiguredModel;
            psi.Environment["RUST_LOG"] = Environment.GetEnvironmentVariable("RUST_LOG") ?? "warn";
            // Mensagens de progresso no idioma da interface.
            psi.Environment["A11Y_LANG"] = CultureInfo.CurrentUICulture.Name;

            _process = Process.Start(psi);
            if (_process is null)
            {
                progress?.Report("Falha ao iniciar o agente.");
                return false;
            }

            _startedByUs = true;
            _ = ReadTokenAsync(_process.StandardOutput);
            _ = DrainAsync(_process.StandardError);
        }
        catch (Exception ex)
        {
            progress?.Report($"Erro ao ligar o agente: {ex.Message}");
            return false;
        }

        for (var i = 0; i < 40; i++)
        {
            ct.ThrowIfCancellationRequested();
            if (_process.HasExited)
            {
                progress?.Report(OllamaService.IsOllamaProvider
                    ? "O agente encerrou sozinho. Veja se o Ollama está aberto."
                    : "O agente encerrou sozinho. Confira a chave da API em \"Modelo de IA\".");
                return false;
            }
            if (await IsHealthyAsync(ct))
            {
                progress?.Report("Agente pronto.");
                return true;
            }
            await Task.Delay(500, ct);
        }

        progress?.Report("Agente demorou demais para responder.");
        return false;
    }

    public async Task<bool> IsHealthyAsync(CancellationToken ct = default) =>
        await ProbeAsync(ct) == ProbeResult.Ok;

    private enum ProbeResult { Down, Ok, Unauthorized }

    private async Task<ProbeResult> ProbeAsync(CancellationToken ct)
    {
        try
        {
            using var req = new HttpRequestMessage(HttpMethod.Get, $"{BaseUrl}/api/health");
            if (ApiToken is { } token)
                req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token);
            using var resp = await _http.SendAsync(req, ct);
            if (resp.StatusCode == HttpStatusCode.Unauthorized) return ProbeResult.Unauthorized;
            return resp.IsSuccessStatusCode ? ProbeResult.Ok : ProbeResult.Down;
        }
        catch
        {
            return ProbeResult.Down;
        }
    }

    /// Procura a linha do token no stdout do agente e continua drenando o resto.
    private async Task ReadTokenAsync(StreamReader reader)
    {
        try
        {
            while (await reader.ReadLineAsync() is { } line)
            {
                if (line.StartsWith(TokenPrefix, StringComparison.Ordinal))
                    _token.TrySetResult(line[TokenPrefix.Length..].Trim());
            }
        }
        catch { /* processo encerrou */ }
    }

    /// Desliga o agente para que ele volte a subir lendo o provedor e a chave novos.
    public void Stop()
    {
        UnloadOllamaModel();
        if (_startedByUs && _process is { HasExited: false })
        {
            try
            {
                _process.Kill(entireProcessTree: true);
                _process.WaitForExit(3000);
            }
            catch { /* ignore */ }
        }
        _process?.Dispose();
        _process = null;
        _startedByUs = false;
        _token = new(TaskCreationOptions.RunContinuationsAsynchronously);
    }

    public void Dispose()
    {
        Stop();
        _http.Dispose();
    }

    /// Pede ao Ollama para liberar a RAM/CPU do modelo agora (o padrão dele é segurar por 5 min).
    /// Não encerra o Ollama: ele é um serviço do usuário e pode estar servindo outros programas.
    private void UnloadOllamaModel()
    {
        if (string.IsNullOrWhiteSpace(ActiveModel) || !OllamaService.IsOllamaProvider)
            return;

        var baseUrl = OllamaService.BaseUrl();

        try
        {
            var body = new StringContent(
                System.Text.Json.JsonSerializer.Serialize(new { model = ActiveModel, keep_alive = 0 }),
                System.Text.Encoding.UTF8, "application/json");
            _http.PostAsync($"{baseUrl}/api/generate", body).Wait(TimeSpan.FromSeconds(2));
        }
        catch { /* Ollama fechado: nada para liberar */ }
    }

    private static async Task DrainAsync(StreamReader reader)
    {
        try { while (await reader.ReadLineAsync() is not null) { } }
        catch { /* ignore */ }
    }

    private static string? FindAgentExe()
    {
        var name = OperatingSystem.IsWindows() ? "rust-a11y-agent.exe" : "rust-a11y-agent";
        var candidates = new List<string>();

        // Ao lado do desktop (deploy)
        var baseDir = AppContext.BaseDirectory;
        candidates.Add(Path.Combine(baseDir, name));
        candidates.Add(Path.Combine(baseDir, "agent", name));

        // Dev: .../desktop/bin/... → repo/target/release|debug
        var dir = new DirectoryInfo(baseDir);
        for (var i = 0; i < 8 && dir is not null; i++, dir = dir.Parent)
        {
            candidates.Add(Path.Combine(dir.FullName, "target", "release", name));
            candidates.Add(Path.Combine(dir.FullName, "target", "debug", name));
            candidates.Add(Path.Combine(dir.FullName, name));
        }

        var env = Environment.GetEnvironmentVariable("A11Y_AGENT_EXE");
        if (!string.IsNullOrWhiteSpace(env))
            candidates.Insert(0, env);

        return candidates.FirstOrDefault(File.Exists);
    }
}
