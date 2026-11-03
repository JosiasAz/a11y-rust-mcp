using System.Diagnostics;
using System.Net.Http;

namespace A11yAgent.Desktop.Services;

/// <summary>
/// Sobe a API Rust em segundo plano (sem janela) e encerra ao fechar o app.
/// </summary>
public sealed class AgentHost : IDisposable
{
    public const string BaseUrl = "http://127.0.0.1:8787";

    private Process? _process;
    private bool _startedByUs;
    private readonly HttpClient _http = new() { Timeout = TimeSpan.FromSeconds(2) };

    /// Modelo usado no último pedido; é descarregado da memória do Ollama ao fechar o app.
    public string? ActiveModel { get; set; }

    public async Task<bool> EnsureRunningAsync(IProgress<string>? progress = null, CancellationToken ct = default)
    {
        if (await IsHealthyAsync(ct))
        {
            progress?.Report("Agente já estava ligado.");
            return true;
        }

        var exe = FindAgentExe();
        if (exe is null)
        {
            progress?.Report("Agente não encontrado. Rode iniciar.bat rebuild uma vez para gerar o app.");
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
            psi.Environment["LLM_MODEL"] = Environment.GetEnvironmentVariable("LLM_MODEL") ?? "qwen2.5:7b";
            psi.Environment["RUST_LOG"] = Environment.GetEnvironmentVariable("RUST_LOG") ?? "warn";

            _process = Process.Start(psi);
            if (_process is null)
            {
                progress?.Report("Falha ao iniciar o agente.");
                return false;
            }

            _startedByUs = true;
            _ = DrainAsync(_process.StandardOutput);
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
                progress?.Report("O agente encerrou sozinho. Veja se o Ollama está aberto.");
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

    public async Task<bool> IsHealthyAsync(CancellationToken ct = default)
    {
        try
        {
            using var resp = await _http.GetAsync($"{BaseUrl}/api/health", ct);
            return resp.IsSuccessStatusCode;
        }
        catch
        {
            return false;
        }
    }

    public void Dispose()
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
        _http.Dispose();
    }

    /// Pede ao Ollama para liberar a RAM/CPU do modelo agora (o padrão dele é segurar por 5 min).
    /// Não encerra o Ollama: ele é um serviço do usuário e pode estar servindo outros programas.
    private void UnloadOllamaModel()
    {
        var provider = Environment.GetEnvironmentVariable("LLM_PROVIDER") ?? "ollama";
        if (string.IsNullOrWhiteSpace(ActiveModel) || !provider.Equals("ollama", StringComparison.OrdinalIgnoreCase))
            return;

        var baseUrl = (Environment.GetEnvironmentVariable("LLM_BASE_URL") ?? "http://127.0.0.1:11434/v1")
            .TrimEnd('/');
        if (baseUrl.EndsWith("/v1")) baseUrl = baseUrl[..^3];

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
