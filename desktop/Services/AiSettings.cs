using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace A11yAgent.Desktop.Services;

#pragma warning disable CA1416 // DPAPI é só do Windows, e o app só é distribuído para Windows (instalador Inno Setup)

/// <summary>Um provedor de IA. O Id é o mesmo LLM_PROVIDER que o agente Rust entende.</summary>
public sealed record AiProvider(string Id, string Name, string Description, string? KeyUrl, string LogoData)
{
    private Avalonia.Media.Geometry? _logo;

    /// Logo oficial (ProviderLogos), lido uma vez só.
    public Avalonia.Media.Geometry Logo => _logo ??= Avalonia.Media.StreamGeometry.Parse(LogoData);
    public bool IsLocal => KeyUrl is null;
    public string Badge => IsLocal ? "LOCAL" : "NUVEM";
    public string Tag => IsLocal ? "No seu computador · grátis" : "Na internet · precisa de chave";
}

/// <summary>
/// Escolha de provedor, modelo e chaves, guardada em %APPDATA%\Uirapuru\ia.json.
/// As chaves são criptografadas com DPAPI: só este usuário do Windows consegue lê-las.
/// A escolha vira variáveis de ambiente do processo, que o AgentHost repassa ao agente.
/// </summary>
public static class AiSettings
{
    public static IReadOnlyList<AiProvider> Providers { get; } =
    [
        new("ollama", "Ollama", "Roda no seu computador. Nada do que você escreve sai daqui.", null, ProviderLogos.Ollama),
        new("anthropic", "Claude (Anthropic)", "Ótimo para explicar e escrever com clareza.", "https://console.anthropic.com/settings/keys", ProviderLogos.Claude),
        new("openai", "OpenAI (ChatGPT)", "Modelos GPT da OpenAI.", "https://platform.openai.com/api-keys", ProviderLogos.OpenAI),
        new("nvidia", "NVIDIA", "Modelos abertos (Nemotron, Llama, Qwen) na nuvem da NVIDIA.", "https://build.nvidia.com/settings/api-keys", ProviderLogos.Nvidia),
        new("gemini", "Google Gemini", "Modelos Gemini do Google.", "https://aistudio.google.com/apikey", ProviderLogos.Gemini),
        new("groq", "Groq", "Respostas muito rápidas com modelos abertos.", "https://console.groq.com/keys", ProviderLogos.Groq),
        new("openrouter", "OpenRouter", "Uma chave só para centenas de modelos.", "https://openrouter.ai/keys", ProviderLogos.OpenRouter),
        new("deepseek", "DeepSeek", "Modelos DeepSeek.", "https://platform.deepseek.com/api_keys", ProviderLogos.DeepSeek),
        new("mistral", "Mistral", "Modelos da Mistral AI.", "https://console.mistral.ai/api-keys", ProviderLogos.Mistral),
    ];

    private static readonly string FilePath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Uirapuru", "ia.json");

    private sealed class Data
    {
        public string? Provider { get; set; }
        public string? Model { get; set; }
        /// Chave criptografada (DPAPI, base64) por provedor.
        public Dictionary<string, string> Keys { get; set; } = [];
        /// "Light", "Dark" ou nulo (segue o Windows).
        public string? Theme { get; set; }
    }

    public static string? Theme => _data.Theme;

    public static void SaveTheme(string theme)
    {
        _data.Theme = theme;
        Save();
    }

    private static Data _data = new();

    public static AiProvider Current =>
        Providers.FirstOrDefault(p => p.Id == (Environment.GetEnvironmentVariable("LLM_PROVIDER") ?? "ollama").ToLowerInvariant())
        ?? Providers[0];

    public static string? SavedModel => _data.Model;

    public static bool HasKey(string providerId) => _data.Keys.ContainsKey(providerId);

    /// Lê o arquivo (se existir) e aplica a escolha salva. Sem arquivo, valem as variáveis de ambiente.
    public static void Load()
    {
        try
        {
            if (!File.Exists(FilePath)) return;
            _data = JsonSerializer.Deserialize<Data>(File.ReadAllText(FilePath)) ?? new();
            if (_data.Provider is { } p) ApplyEnv(p, _data.Model);
        }
        catch
        {
            _data = new(); // arquivo corrompido: volta ao padrão em vez de travar a abertura
        }
    }

    /// Troca de provedor. Chave nula = manter a que já está salva.
    public static void SaveProvider(string providerId, string? newKey)
    {
        if (!string.IsNullOrWhiteSpace(newKey))
            _data.Keys[providerId] = Convert.ToBase64String(
                ProtectedData.Protect(Encoding.UTF8.GetBytes(newKey.Trim()), null, DataProtectionScope.CurrentUser));
        if (_data.Provider != providerId) _data.Model = null; // modelo de outro provedor não serve aqui
        _data.Provider = providerId;
        ApplyEnv(providerId, _data.Model);
        Save();
    }

    public static void SaveModel(string? model)
    {
        if (_data.Model == model) return;
        _data.Model = model;
        _data.Provider ??= Current.Id;
        Save();
    }

    public static void ForgetKey(string providerId)
    {
        if (_data.Keys.Remove(providerId)) Save();
    }

    private static void ApplyEnv(string providerId, string? model)
    {
        Environment.SetEnvironmentVariable("LLM_PROVIDER", providerId);
        Environment.SetEnvironmentVariable("LLM_MODEL", string.IsNullOrWhiteSpace(model) ? null : model);
        Environment.SetEnvironmentVariable("LLM_API_KEY", ReadKey(providerId));
        // Uma URL do Ollama definida à mão não pode valer para um provedor da nuvem.
        if (providerId != "ollama") Environment.SetEnvironmentVariable("LLM_BASE_URL", null);
    }

    private static string? ReadKey(string providerId)
    {
        if (!_data.Keys.TryGetValue(providerId, out var enc)) return null;
        try
        {
            return Encoding.UTF8.GetString(
                ProtectedData.Unprotect(Convert.FromBase64String(enc), null, DataProtectionScope.CurrentUser));
        }
        catch
        {
            return null; // criada por outro usuário do Windows: pede a chave de novo
        }
    }

    private static void Save()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
        File.WriteAllText(FilePath, JsonSerializer.Serialize(_data, new JsonSerializerOptions { WriteIndented = true }));
    }
}
