using System.Net.Http;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace A11yAgent.Desktop.Services;

public sealed class AgentApiClient
{
    private readonly HttpClient _http;
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNameCaseInsensitive = true
    };

    public AgentApiClient(string baseUrl = "http://127.0.0.1:8787")
    {
        _http = new HttpClient
        {
            BaseAddress = new Uri(baseUrl.TrimEnd('/') + "/"),
            Timeout = TimeSpan.FromMinutes(15)
        };
    }

    /// Envia o token efêmero da sessão em toda requisição (HTTP e SSE). Sem ele a API responde 401.
    public void UseToken(string token) =>
        _http.DefaultRequestHeaders.Authorization = new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", token);

    /// Plano em etapas, ou uma pergunta com opções se o pedido for ambíguo.
    public async Task<PlanDto> PlanAsync(string task, string? model, CancellationToken ct = default)
    {
        using var resp = await _http.PostAsJsonAsync("api/agent/plan", new { task, model }, ct);
        if (!resp.IsSuccessStatusCode)
            throw new InvalidOperationException(await resp.Content.ReadAsStringAsync(ct));
        return await resp.Content.ReadFromJsonAsync<PlanDto>(JsonOpts, ct) ?? new PlanDto();
    }

    public async Task<ModelsDto> ModelsAsync(CancellationToken ct = default) =>
        await _http.GetFromJsonAsync<ModelsDto>("api/models", JsonOpts, ct) ?? new ModelsDto();

    public async Task RunStreamAsync(
        string workspace,
        string task,
        IReadOnlyList<string> steps,
        string? model,
        Action<ProgressEventDto> onProgress,
        CancellationToken ct = default)
    {
        var payload = new RunRequest
        {
            Task = task,
            Workspace = workspace,
            AutoApprove = true,
            FastMode = true,
            Steps = [.. steps],
            Model = model
        };

        using var req = new HttpRequestMessage(HttpMethod.Post, "api/agent/run-stream")
        {
            Content = JsonContent.Create(payload)
        };

        using var resp = await _http.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, ct);
        if (!resp.IsSuccessStatusCode)
        {
            var body = await resp.Content.ReadAsStringAsync(ct);
            throw new InvalidOperationException($"Agente falhou ({(int)resp.StatusCode}): {body}");
        }

        await using var stream = await resp.Content.ReadAsStreamAsync(ct);
        using var reader = new StreamReader(stream, Encoding.UTF8);

        string? eventName = null;
        var data = new StringBuilder();

        while (true)
        {
            ct.ThrowIfCancellationRequested();
            var line = await reader.ReadLineAsync(ct);
            if (line is null)
            {
                FlushEvent();
                break;
            }

            if (line.StartsWith(':'))
                continue; // keep-alive SSE (:ping)

            if (line.StartsWith("event:", StringComparison.OrdinalIgnoreCase))
            {
                eventName = line["event:".Length..].Trim();
            }
            else if (line.StartsWith("data:", StringComparison.OrdinalIgnoreCase))
            {
                if (data.Length > 0) data.Append('\n');
                data.Append(line["data:".Length..].TrimStart());
            }
            else if (string.IsNullOrWhiteSpace(line))
            {
                FlushEvent();
            }
        }

        void FlushEvent()
        {
            if (data.Length > 0 && (eventName is null || eventName == "progress"))
            {
                var json = data.ToString();
                var ev = JsonSerializer.Deserialize<ProgressEventDto>(json, JsonOpts);
                if (ev is not null)
                    onProgress(ev);
            }
            eventName = null;
            data.Clear();
        }
    }

    public async Task<TreeResponse> TreeAsync(string workspace, int depth = 3, CancellationToken ct = default)
    {
        var url = $"api/workspace/tree?workspace={Uri.EscapeDataString(workspace)}&depth={depth}";
        return await _http.GetFromJsonAsync<TreeResponse>(url, ct)
               ?? new TreeResponse { Root = workspace, Entries = [] };
    }

    public async Task<FileResponse> ReadFileAsync(string workspace, string path, CancellationToken ct = default)
    {
        var url = $"api/workspace/file?workspace={Uri.EscapeDataString(workspace)}&path={Uri.EscapeDataString(path)}";
        return await _http.GetFromJsonAsync<FileResponse>(url, ct)
               ?? new FileResponse { Path = path, Content = "" };
    }
}

public sealed class RunRequest
{
    [JsonPropertyName("task")] public string Task { get; set; } = "";
    [JsonPropertyName("workspace")] public string Workspace { get; set; } = "";
    [JsonPropertyName("auto_approve")] public bool AutoApprove { get; set; } = true;
    [JsonPropertyName("fast_mode")] public bool FastMode { get; set; } = true;
    [JsonPropertyName("steps")] public List<string> Steps { get; set; } = [];
    [JsonPropertyName("model")] public string? Model { get; set; }
}

public sealed class PlanDto
{
    [JsonPropertyName("passos")] public List<string> Passos { get; set; } = [];
    [JsonPropertyName("pergunta")] public string? Pergunta { get; set; }
    [JsonPropertyName("opcoes")] public List<string> Opcoes { get; set; } = [];
}

public sealed class ModelsDto
{
    [JsonPropertyName("current")] public string Current { get; set; } = "";
    [JsonPropertyName("models")] public List<string> Models { get; set; } = [];
}

public sealed class ProgressEventDto
{
    [JsonPropertyName("kind")] public string Kind { get; set; } = "";
    [JsonPropertyName("message")] public string Message { get; set; } = "";
    [JsonPropertyName("tool")] public string? Tool { get; set; }
    [JsonPropertyName("refresh_tree")] public bool? RefreshTree { get; set; }
    [JsonPropertyName("path")] public string? Path { get; set; }
}

public sealed class TreeResponse
{
    [JsonPropertyName("root")] public string Root { get; set; } = "";
    [JsonPropertyName("entries")] public List<string> Entries { get; set; } = [];
}

public sealed class FileResponse
{
    [JsonPropertyName("path")] public string Path { get; set; } = "";
    [JsonPropertyName("content")] public string Content { get; set; } = "";
}
