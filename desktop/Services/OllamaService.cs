using System.Net.Http;
using System.Net.Http.Json;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace A11yAgent.Desktop.Services;

public enum OllamaState
{
    /// O provedor configurado não é o Ollama: nada a verificar.
    NotApplicable,
    NotRunning,
    ModelMissing,
    Ready,
}

/// <summary>
/// Confere se o Ollama está aberto e se o modelo de <c>LLM_MODEL</c> está instalado.
/// Também baixa o modelo pela API <c>/api/pull</c>, informando o progresso em %, para o
/// usuário nunca precisar abrir o terminal.
/// </summary>
public sealed class OllamaService : IDisposable
{
    // Sem timeout global: o pull de um modelo leva vários minutos. A checagem tem timeout próprio.
    private readonly HttpClient _http = new() { Timeout = Timeout.InfiniteTimeSpan };

    public static bool IsOllamaProvider =>
        (Environment.GetEnvironmentVariable("LLM_PROVIDER") ?? "ollama")
        .Equals("ollama", StringComparison.OrdinalIgnoreCase);

    public static string ConfiguredModel =>
        Environment.GetEnvironmentVariable("LLM_MODEL") is { Length: > 0 } m ? m : "qwen2.5:7b";

    /// Raiz da API nativa do Ollama (LLM_BASE_URL aponta para a rota compatível /v1).
    public static string BaseUrl()
    {
        var url = (Environment.GetEnvironmentVariable("LLM_BASE_URL") ?? "http://127.0.0.1:11434/v1").TrimEnd('/');
        return url.EndsWith("/v1", StringComparison.Ordinal) ? url[..^3] : url;
    }

    public async Task<OllamaState> CheckAsync(string model, CancellationToken ct = default)
    {
        if (!IsOllamaProvider) return OllamaState.NotApplicable;

        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeout.CancelAfter(TimeSpan.FromSeconds(3));
        TagsDto? tags;
        try
        {
            tags = await _http.GetFromJsonAsync<TagsDto>($"{BaseUrl()}/api/tags", timeout.Token);
        }
        catch (Exception) when (!ct.IsCancellationRequested)
        {
            return OllamaState.NotRunning;
        }

        var wanted = Normalize(model);
        var installed = tags?.Models.Any(m => Normalize(m.Name) == wanted) == true;
        return installed ? OllamaState.Ready : OllamaState.ModelMissing;
    }

    /// <summary>
    /// Baixa <paramref name="model"/>. <paramref name="progress"/> recebe 0–100 quando o Ollama
    /// informa tamanho (as camadas pequenas de manifesto não informam).
    /// </summary>
    public async Task PullAsync(string model, IProgress<double> progress, CancellationToken ct = default)
    {
        using var req = new HttpRequestMessage(HttpMethod.Post, $"{BaseUrl()}/api/pull")
        {
            Content = JsonContent.Create(new { model, stream = true }),
        };
        using var resp = await _http.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, ct);
        if (!resp.IsSuccessStatusCode)
            throw new InvalidOperationException($"O Ollama recusou o download ({(int)resp.StatusCode}): {await resp.Content.ReadAsStringAsync(ct)}");

        await using var stream = await resp.Content.ReadAsStreamAsync(ct);
        using var reader = new StreamReader(stream);

        // O Ollama informa o progresso por camada (digest). A soma de todas evita que a barra
        // volte para trás quando uma camada pequena começa do zero.
        var layers = new Dictionary<string, (long Done, long Total)>();

        // A resposta é NDJSON: um objeto JSON por linha.
        while (await reader.ReadLineAsync(ct) is { } line)
        {
            if (string.IsNullOrWhiteSpace(line)) continue;
            var ev = JsonSerializer.Deserialize<PullEventDto>(line);
            if (ev is null) continue;

            if (!string.IsNullOrEmpty(ev.Error))
                throw new InvalidOperationException($"O download falhou: {ev.Error}");
            if (ev.Status == "success")
            {
                progress.Report(100);
                return;
            }
            if (ev.Digest is { Length: > 0 } digest && ev.Total is > 0 and var total)
            {
                layers[digest] = (ev.Completed ?? 0, total);
                var sumTotal = layers.Values.Sum(l => l.Total);
                var sumDone = layers.Values.Sum(l => l.Done);
                progress.Report(Math.Clamp(sumDone * 100.0 / sumTotal, 0, 100));
            }
        }

        throw new InvalidOperationException("O Ollama encerrou o download antes de terminar. Tente de novo.");
    }

    /// "qwen2.5" e "qwen2.5:latest" são o mesmo modelo para o Ollama.
    private static string Normalize(string name)
    {
        var n = name.Trim().ToLowerInvariant();
        return n.Contains(':') ? n : n + ":latest";
    }

    public void Dispose() => _http.Dispose();

    private sealed class TagsDto
    {
        [JsonPropertyName("models")] public List<TagDto> Models { get; set; } = [];
    }

    private sealed class TagDto
    {
        [JsonPropertyName("name")] public string Name { get; set; } = "";
    }

    private sealed class PullEventDto
    {
        [JsonPropertyName("status")] public string? Status { get; set; }
        [JsonPropertyName("error")] public string? Error { get; set; }
        [JsonPropertyName("digest")] public string? Digest { get; set; }
        [JsonPropertyName("total")] public long? Total { get; set; }
        [JsonPropertyName("completed")] public long? Completed { get; set; }
    }
}
