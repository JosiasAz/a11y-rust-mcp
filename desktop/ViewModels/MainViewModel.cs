using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using A11yAgent.Desktop.Models;
using A11yAgent.Desktop.Services;
using Avalonia.Controls;
using Avalonia.Platform.Storage;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace A11yAgent.Desktop.ViewModels;

public sealed record ExampleIdea(string Title, string Description, string Prompt);

public static class Ui
{
    /// "C:\...\docs.md" → "📄 a.md" nos botões de arquivo (o caminho completo vai na dica).
    public static Avalonia.Data.Converters.FuncValueConverter<string, string> FileButton { get; } =
        new(p => "📄 " + Path.GetFileName(p ?? ""));
}

public partial class MainViewModel : ViewModelBase
{
    private readonly AgentHost _host;
    private readonly AgentApiClient _api = new();
    private readonly OllamaService _ollama = new();
    private CancellationTokenSource? _pullCts;
    private Window? _window;
    private CancellationTokenSource? _treeDebounce;
    private CancellationTokenSource? _runCts;
    private ChatMessage? _pendingPlan;
    private bool _modelsLoaded;

    // Relógio do cartão em andamento: mostra que o agente não travou.
    private readonly DispatcherTimer _clock = new() { Interval = TimeSpan.FromSeconds(1) };
    private readonly Stopwatch _clockWatch = new();
    private ChatMessage? _clockTarget;

    /// Pede à View para rolar até a última mensagem.
    public event Action? ChatScrolled;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasWorkspace), nameof(WorkspaceName), nameof(FolderButtonText), nameof(StepOneMark))]
    private string _workspacePath = "";

    [ObservableProperty] private string _statusText = "Ligando o assistente…";
    [ObservableProperty] private string _promptText = "";
    [ObservableProperty] private string _fileContent = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPreview))]
    private string _selectedFile = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanSend), nameof(BadgeText))]
    private bool _isBusy;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanSend), nameof(BadgeText), nameof(IsOffline))]
    private bool _agentOnline;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanSend), nameof(BadgeText), nameof(IsOffline))]
    private bool _isStarting = true;

    /// Modelo em uso; vazio = padrão do provedor.
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasModels), nameof(ModelSummary))]
    private string? _selectedModel;

    // ───── Escolha da IA: provedor, chave e modelo (tela aberta pelo cartão da barra lateral) ─────

    public IReadOnlyList<AiProvider> Providers => AiSettings.Providers;

    /// Provedor que o agente está usando agora.
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsSwitching), nameof(PrivacyText), nameof(ShowConnect), nameof(ConnectText), nameof(ModelHint))]
    private AiProvider _activeProvider = AiSettings.Current;

    /// Provedor marcado no painel (só vira o ativo ao clicar em "Usar").
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsSwitching), nameof(NeedsKey), nameof(KeyPlaceholder), nameof(PickedPrivacy),
        nameof(ShowConnect), nameof(ConnectText), nameof(ShowModelPicker), nameof(ModelStepNumber))]
    private AiProvider _pickedProvider = AiSettings.Current;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowConnect), nameof(ConnectText))]
    private string _apiKeyInput = "";

    [ObservableProperty] private string _providerMessage = "";
    [ObservableProperty] private string _modelFilter = "";
    [ObservableProperty] private bool _isConnecting;

    /// Item marcado na lista filtrada. Separado de SelectedModel: filtrar a lista não pode desfazer a escolha.
    [ObservableProperty] private string? _pickedModel;

    public ObservableCollection<string> FilteredModels { get; } = [];

    public bool IsSwitching => PickedProvider != ActiveProvider;
    public bool NeedsKey => !PickedProvider.IsLocal;
    public bool ShowConnect => IsSwitching || (NeedsKey && !string.IsNullOrWhiteSpace(ApiKeyInput));
    public string ConnectText => IsSwitching ? $"Usar {PickedProvider.Name}" : "Salvar nova chave";
    public bool ShowModelPicker => !IsSwitching && HasModels;
    public string KeyPlaceholder => AiSettings.HasKey(PickedProvider.Id)
        ? "Chave já salva. Cole outra só se quiser trocar."
        : "Cole aqui a sua chave";
    public string PickedPrivacy =>
        $"O que você escrever será enviado para os servidores de {PickedProvider.Name}. Use só com textos que você pode compartilhar.";
    public string PrivacyText => ActiveProvider.IsLocal
        ? "🔒  Tudo roda no seu computador. Nada do que você escreve sai daqui."
        : $"🌐  Você está usando {ActiveProvider.Name}. O que você escreve é enviado pela internet para esse serviço.";
    public string ModelSummary => string.IsNullOrWhiteSpace(SelectedModel) ? "Modelo padrão" : SelectedModel;
    public string ModelCountText => FilteredModels.Count switch
    {
        0 => "Nenhum modelo com esse nome.",
        1 => "1 modelo",
        var n => $"{n} modelos",
    };

    // ───── Ollama: aberto? modelo instalado? download com progresso ─────

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsOllamaDown), nameof(IsModelMissing))]
    private OllamaState _ollamaState = OllamaState.NotApplicable;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanSend))]
    private bool _isPullingModel;

    [ObservableProperty] private double _pullPercent;
    /// Texto anunciado pelo leitor de tela; só muda a cada 5% para não inundar o usuário.
    [ObservableProperty] private string _pullText = "";

    public bool IsOllamaDown => OllamaState == OllamaState.NotRunning;
    public bool IsModelMissing => OllamaState == OllamaState.ModelMissing;
    public string ModelToDownload => OllamaService.ConfiguredModel;

    public ObservableCollection<string> FileEntries { get; } = [];
    public ObservableCollection<ChatMessage> Messages { get; } = [];
    public ObservableCollection<string> Models { get; } = [];

    public IReadOnlyList<ExampleIdea> Examples { get; } =
    [
        new("Simplificar um texto difícil",
            "Transforma um texto complicado em linguagem fácil.",
            "Simplifique este texto em linguagem fácil e salve em docs/texto-simples.md:\n\nO paciente deverá proceder à ingestão do medicamento subsequentemente à refeição."),
        new("Revisar acessibilidade de uma página",
            "Procura problemas de acessibilidade em um arquivo HTML e corrige.",
            "Revise o arquivo index.html procurando problemas de acessibilidade (texto alternativo em imagens, contraste, rótulos de formulário, títulos). Corrija o que puder e me explique cada mudança."),
        new("Criar um projeto novo",
            "Monta pastas e um README explicando tudo.",
            "Crie as pastas src/ e tests/ e um README.md explicando o projeto em linguagem clara."),
        new("Explicar o que tem nesta pasta",
            "Lista os arquivos e diz para que serve cada um.",
            "Olhe os arquivos desta pasta e me explique, em linguagem simples, para que serve cada um."),
    ];

    public bool HasWorkspace => !string.IsNullOrWhiteSpace(WorkspacePath);
    public string WorkspaceName => HasWorkspace ? Path.GetFileName(WorkspacePath.TrimEnd('\\', '/')) : "";
    public string FolderButtonText => HasWorkspace ? "Trocar pasta" : "Escolher pasta";
    public string StepOneMark => HasWorkspace ? "✓" : "1";
    public bool HasConversation => Messages.Count > 0;
    public bool HasPreview => !string.IsNullOrWhiteSpace(SelectedFile);
    public bool HasModels => Models.Count > 0;
    public bool IsOffline => !AgentOnline && !IsStarting;
    public bool CanSend => AgentOnline && !IsBusy && !IsStarting && !IsPullingModel;

    public string BadgeText =>
        IsStarting ? "Ligando…" :
        !AgentOnline ? "Desligado" :
        IsBusy ? "Trabalhando" : "Pronto";

    public MainViewModel() : this(new AgentHost()) { }

    public MainViewModel(AgentHost host)
    {
        _host = host;
        Messages.CollectionChanged += (_, _) => OnPropertyChanged(nameof(HasConversation));
        Models.CollectionChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(HasModels));
            OnPropertyChanged(nameof(ShowModelPicker));
        };
        FilteredModels.CollectionChanged += (_, _) => OnPropertyChanged(nameof(ModelCountText));
        // Também acompanha o tema do Windows quando a pessoa ainda não escolheu um.
        if (Avalonia.Application.Current is { } app)
            app.ActualThemeVariantChanged += (_, _) =>
            {
                OnPropertyChanged(nameof(IsDark));
                OnPropertyChanged(nameof(ThemeToggleText));
            };
        _clock.Tick += (_, _) =>
        {
            if (_clockTarget is null) return;
            var t = _clockWatch.Elapsed;
            _clockTarget.Elapsed = t.TotalMinutes >= 1 ? $"{(int)t.TotalMinutes} min {t.Seconds:00} s" : $"{t.Seconds} s";
        };
    }

    public void AttachWindow(Window window) => _window = window;

    // ───────────── Ligar o agente e listar modelos ─────────────

    [RelayCommand]
    private async Task BootAsync()
    {
        IsStarting = true;
        StatusText = "Ligando o assistente…";
        // Só mostra o progresso do boot enquanto ele dura (Progress<T> entrega atrasado).
        var progress = new Progress<string>(msg => Dispatcher.UIThread.Post(() => { if (IsStarting) StatusText = msg; }));

        // Sem o Ollama aberto o agente não tem com quem conversar: orienta antes de ligar.
        OllamaState = await _ollama.CheckAsync(OllamaService.ConfiguredModel);
        if (IsOllamaDown)
        {
            AgentOnline = false;
            IsStarting = false;
            StatusText = "O programa Ollama não está aberto. Abra o Ollama e clique em \"Tentar ligar de novo\".";
            return;
        }

        try
        {
            AgentOnline = await _host.EnsureRunningAsync(progress);
            if (_host.ApiToken is { } token) _api.UseToken(token);
        }
        catch (Exception ex)
        {
            AgentOnline = false;
            StatusText = ex.Message;
        }
        finally
        {
            IsStarting = false;
        }

        if (AgentOnline)
        {
            StatusText = IsModelMissing
                ? $"Falta baixar o modelo de IA {ModelToDownload}. Use o botão \"Baixar modelo agora\"."
                : HasWorkspace ? "Pronto. Escreva seu pedido." : "Pronto. Comece escolhendo uma pasta.";
            _modelsLoaded = await LoadModelsAsync();
        }
        else
        {
            StatusText = "O assistente não ligou. Veja as instruções na conversa.";
            AddError("Não consegui ligar o assistente",
                "Confira estes passos e depois clique em \"Tentar ligar de novo\", no topo:\n" +
                (ActiveProvider.IsLocal
                    ? "1. Abra o programa Ollama (ele roda o modelo de IA no seu computador).\n"
                    : $"1. Confira a chave de {ActiveProvider.Name} em \"Modelo de IA\", embaixo na barra lateral.\n") +
                "2. Se ainda não funcionar, reinstale o Uirapuru pelo instalador.",
                StatusText);
        }
    }

    // shortcut: filtro pelo nome; um modelo de conversa com uma destas palavras no nome fica de fora. Trocar por metadados do provedor se isso acontecer.
    private static readonly string[] NotChatModels =
        ["embed", "rerank", "guard", "reward", "safety", "moderation", "whisper", "tts", "dall-e", "transcribe", "parse"];

    /// Devolve false se o provedor não respondeu (na nuvem, quase sempre é chave errada).
    private async Task<bool> LoadModelsAsync()
    {
        var ok = true;
        try
        {
            var list = await _api.ModelsAsync();
            var keep = SelectedModel ?? AiSettings.SavedModel;
            Models.Clear();
            foreach (var m in list.Models.Where(m => !NotChatModels.Any(w => m.Contains(w, StringComparison.OrdinalIgnoreCase))).OrderBy(m => m))
                Models.Add(m);
            // O padrão do provedor entra na lista mesmo se a API não o listar.
            var pick = keep is not null && Models.Contains(keep) ? keep
                : !string.IsNullOrWhiteSpace(list.Current) ? list.Current
                : Models.FirstOrDefault();
            if (pick is not null && !Models.Contains(pick)) Models.Insert(0, pick);
            SelectedModel = pick;
        }
        catch
        {
            ok = false; // sem lista: o agente usa o modelo padrão dele
        }
        ApplyModelFilter();
        return ok;
    }

    partial void OnSelectedModelChanged(string? value)
    {
        if (value is not null) AiSettings.SaveModel(value);
    }

    partial void OnPickedModelChanged(string? value)
    {
        if (value is not null && value != SelectedModel)
        {
            SelectedModel = value;
            ProviderMessage = $"Pronto: os próximos pedidos usam {value}.";
        }
    }

    partial void OnModelFilterChanged(string value) => ApplyModelFilter();

    private void ApplyModelFilter()
    {
        var q = ModelFilter.Trim();
        FilteredModels.Clear();
        foreach (var m in Models.Where(m => m.Contains(q, StringComparison.OrdinalIgnoreCase)))
            FilteredModels.Add(m);
        PickedModel = SelectedModel is not null && FilteredModels.Contains(SelectedModel) ? SelectedModel : null;
    }

    /// A tela "Modelo de IA" ocupa o lugar do chat enquanto estiver aberta.
    [ObservableProperty] private bool _isModelPanelOpen;

    public string ModelStepNumber => NeedsKey ? "3" : "2";
    public string ModelHint => ActiveProvider.IsLocal
        ? "Modelos menores respondem mais rápido. Para baixar outro, use o programa Ollama."
        : "Cada provedor tem vários modelos. Se não souber qual escolher, fique com o que já está marcado.";

    [RelayCommand]
    private void OpenModelPanel()
    {
        if (IsBusy) return;
        // Abre mostrando o provedor em uso, sem resto de digitação anterior.
        PickedProvider = ActiveProvider;
        ApiKeyInput = "";
        ModelFilter = "";
        ProviderMessage = "";
        IsModelPanelOpen = true;
    }

    [RelayCommand]
    private void CloseModelPanel()
    {
        if (IsConnecting) return; // trocar no meio da conexão deixaria o agente pela metade
        ApiKeyInput = "";
        IsModelPanelOpen = false;
    }

    // ───── Tema claro/escuro ─────

    public bool IsDark => Avalonia.Application.Current?.ActualThemeVariant == Avalonia.Styling.ThemeVariant.Dark;
    public string ThemeToggleText => IsDark ? "Usar tema claro" : "Usar tema escuro";

    [RelayCommand]
    private void ToggleTheme()
    {
        if (Avalonia.Application.Current is not { } app) return;
        var dark = !IsDark;
        app.RequestedThemeVariant = dark ? Avalonia.Styling.ThemeVariant.Dark : Avalonia.Styling.ThemeVariant.Light;
        AiSettings.SaveTheme(dark ? "Dark" : "Light");
    }

    [RelayCommand]
    private async Task ConnectProviderAsync()
    {
        if (IsBusy || IsConnecting) return;
        var p = PickedProvider;
        if (NeedsKey && !AiSettings.HasKey(p.Id) && string.IsNullOrWhiteSpace(ApiKeyInput))
        {
            ProviderMessage = $"Cole a chave de {p.Name} antes de continuar. Não tem uma? Use o botão \"Criar uma chave\".";
            return;
        }

        IsConnecting = true;
        ProviderMessage = $"Conectando a {p.Name}…";
        try
        {
            AiSettings.SaveProvider(p.Id, ApiKeyInput);
            ApiKeyInput = "";
            if (p != ActiveProvider) SelectedModel = null; // modelo de outro provedor não existe neste
            ActiveProvider = p;
            Models.Clear();
            OllamaState = OllamaState.NotApplicable;
            _host.Stop();
            await BootAsync();
            ProviderMessage = !AgentOnline
                ? (p.IsLocal ? "Não consegui ligar. Veja se o Ollama está aberto." : $"Não consegui conectar a {p.Name}. Confira a chave e tente de novo.")
                : !p.IsLocal && !_modelsLoaded ? $"{p.Name} não aceitou a chave (ou está fora do ar). Confira a chave e tente de novo."
                : HasModels ? $"Conectado a {p.Name}. Escolha o modelo abaixo." : $"Conectado a {p.Name}.";
        }
        catch (Exception ex)
        {
            ProviderMessage = $"Não consegui salvar a escolha: {ex.Message}";
        }
        finally
        {
            IsConnecting = false;
            OnPropertyChanged(nameof(KeyPlaceholder));
        }
    }

    [RelayCommand]
    private async Task OpenKeyPageAsync()
    {
        if (_window is not null && PickedProvider.KeyUrl is { } url)
            await _window.Launcher.LaunchUriAsync(new Uri(url));
    }

    [RelayCommand]
    private async Task CheckHealthAsync() => await BootAsync();

    [RelayCommand]
    private Task RefreshModelsAsync() => LoadModelsAsync();

    // ───────────── Pasta e arquivos ─────────────

    [RelayCommand]
    private async Task PickWorkspaceAsync()
    {
        // Trocar de pasta no meio do trabalho abriria o resultado na pasta errada.
        if (_window is null || IsBusy || IsModelPanelOpen) return;

        try
        {
            var folders = await _window.StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
            {
                Title = "Escolha a pasta onde o assistente pode trabalhar",
                AllowMultiple = false
            });
            if (folders.Count == 0) return;

            var path = folders[0].TryGetLocalPath() ?? folders[0].Path.LocalPath;
            if (string.IsNullOrWhiteSpace(path))
            {
                StatusText = "Não consegui ler o caminho dessa pasta. Tente outra.";
                return;
            }

            WorkspacePath = path;
            SelectedFile = "";
            FileContent = "";
            StatusText = $"Pasta \"{WorkspaceName}\" aberta. Agora escreva seu pedido.";
            await RefreshTreeAsync();
        }
        catch (Exception ex)
        {
            StatusText = $"Não consegui abrir a pasta: {ex.Message}";
        }
    }

    [RelayCommand]
    private async Task RefreshTreeAsync()
    {
        if (!HasWorkspace) return;

        List<string> entries;
        try
        {
            entries = (await _api.TreeAsync(WorkspacePath)).Entries;
        }
        catch
        {
            entries =
            [
                .. Directory.EnumerateDirectories(WorkspacePath).Take(80)
                    .Select(d => "📁 " + Path.GetRelativePath(WorkspacePath, d).Replace('\\', '/')),
                .. Directory.EnumerateFiles(WorkspacePath).Take(120)
                    .Select(f => "📄 " + Path.GetRelativePath(WorkspacePath, f).Replace('\\', '/')),
            ];
        }

        await Dispatcher.UIThread.InvokeAsync(() =>
        {
            FileEntries.Clear();
            foreach (var e in entries) FileEntries.Add(e);
        });
    }

    private void RequestTreeRefreshSoon()
    {
        _treeDebounce?.Cancel();
        _treeDebounce = new CancellationTokenSource();
        var token = _treeDebounce.Token;
        _ = Task.Run(async () =>
        {
            try
            {
                await Task.Delay(400, token);
                await RefreshTreeAsync();
            }
            catch (TaskCanceledException) { }
        });
    }

    /// Item da lista de arquivos ("📄 caminho" ou "📁 caminho").
    [RelayCommand]
    private async Task OpenEntryAsync(string? entry)
    {
        if (string.IsNullOrWhiteSpace(entry)) return;
        var idx = entry.IndexOf(' ');
        var path = idx > 0 ? entry[(idx + 1)..] : entry;
        if (entry.StartsWith("📁"))
        {
            StatusText = $"\"{path}\" é uma pasta.";
            return;
        }
        await OpenFileAsync(path);
    }

    /// Abre na prévia. Aceita caminho relativo à pasta atual ou caminho completo.
    [RelayCommand]
    private async Task OpenFileAsync(string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return;

        try
        {
            if (Path.IsPathRooted(path))
            {
                FileContent = await File.ReadAllTextAsync(path);
                SelectedFile = HasWorkspace && path.StartsWith(WorkspacePath, StringComparison.OrdinalIgnoreCase)
                    ? Path.GetRelativePath(WorkspacePath, path).Replace('\\', '/')
                    : path;
            }
            else
            {
                if (!HasWorkspace) return;
                var file = await _api.ReadFileAsync(WorkspacePath, path);
                SelectedFile = file.Path;
                FileContent = file.Content;
            }
        }
        catch (Exception ex)
        {
            StatusText = $"Não consegui abrir \"{path}\": {ex.Message}";
            return;
        }
        StatusText = $"Mostrando \"{SelectedFile}\" na prévia.";
    }

    [RelayCommand]
    private void UseExample(ExampleIdea? idea)
    {
        if (idea is null) return;
        PromptText = idea.Prompt;
        StatusText = HasWorkspace
            ? "Exemplo colocado na caixa de pedido. Ajuste se quiser e aperte Enter."
            : "Exemplo colocado. Antes de enviar, escolha uma pasta.";
    }

    [RelayCommand]
    private void ClosePreview() => SelectedFile = "";

    [RelayCommand]
    private void Cancel()
    {
        // Na tela "Modelo de IA", o Esc só volta para o chat.
        if (IsModelPanelOpen)
        {
            CloseModelPanel();
            return;
        }
        _runCts?.Cancel();
        _pullCts?.Cancel();
    }

    /// Baixa o modelo configurado pelo Ollama, só depois do clique (são vários GB).
    [RelayCommand]
    private async Task PullModelAsync()
    {
        if (IsPullingModel) return;
        var model = ModelToDownload;
        IsPullingModel = true;
        PullPercent = 0;
        PullText = $"Baixando o modelo {model}: 0%";
        StatusText = "Baixando o modelo de IA. Isso pode levar alguns minutos. Esc cancela.";
        _pullCts = new CancellationTokenSource();
        var announced = 0;

        // Progress<T> devolve na thread da tela, porque foi criado nela.
        var progress = new Progress<double>(p =>
        {
            PullPercent = p;
            var step = (int)p / 5 * 5;
            if (step > announced)
            {
                announced = step;
                PullText = $"Baixando o modelo {model}: {step}%";
            }
        });

        try
        {
            await _ollama.PullAsync(model, progress, _pullCts.Token);
            OllamaState = OllamaState.Ready;
            PullText = "";
            StatusText = $"Modelo {model} instalado. Pronto para usar.";
            await LoadModelsAsync();
        }
        catch (OperationCanceledException)
        {
            PullText = "Download cancelado. O que já foi baixado fica guardado e continua da próxima vez.";
            StatusText = "Download cancelado.";
        }
        catch (Exception ex)
        {
            PullText = ex.Message;
            StatusText = "Não consegui baixar o modelo. Veja o motivo no aviso.";
        }
        finally
        {
            IsPullingModel = false;
            _pullCts.Dispose();
            _pullCts = null;
        }
    }

    [RelayCommand]
    private void NewConversation()
    {
        if (IsBusy) return;
        Messages.Clear();
        _pendingPlan = null;
        StatusText = "Conversa limpa. Escreva um novo pedido.";
    }

    // ───────────── Pedido → plano → aprovação → execução ─────────────

    [RelayCommand]
    private async Task SendAsync()
    {
        if (!CanSend) return;

        if (!HasWorkspace)
        {
            StatusText = "Antes de enviar, escolha uma pasta (botão \"Escolher pasta\").";
            return;
        }
        if (string.IsNullOrWhiteSpace(PromptText))
        {
            StatusText = "Escreva o que você precisa na caixa de pedido.";
            return;
        }
        if (!await _host.IsHealthyAsync())
        {
            await BootAsync();
            if (!AgentOnline) return;
        }

        DiscardPendingPlan();
        var task = PromptText.Trim();
        PromptText = "";
        Messages.Add(new ChatMessage(ChatRole.User, "Seu pedido", task));
        await PlanAsync(task);
    }

    /// Pede o plano ao modelo e mostra no cartão: etapas para aprovar, ou uma pergunta com opções.
    private async Task PlanAsync(string task)
    {
        var reply = new ChatMessage(ChatRole.Agent, "Montando um plano")
        {
            IsWorking = true,
            CurrentActivity = "Dividindo o pedido em etapas…",
            PendingTask = task
        };
        Messages.Add(reply);

        await RunWorkAsync(reply, async ct =>
            {
                var plan = await _api.PlanAsync(task, SelectedModel, ct);
                if (plan.Pergunta is { Length: > 0 } q && plan.Opcoes.Count >= 2)
                {
                    reply.Title = "Preciso que você escolha";
                    reply.Text = q;
                    foreach (var o in plan.Opcoes) reply.Options.Add(new ChatOption(reply, o));
                    StatusText = "O assistente fez uma pergunta. Escolha uma opção na conversa.";
                }
                else if (plan.Passos.Count > 0)
                {
                    reply.Title = "Meu plano";
                    reply.Text = "Vou fazer nesta ordem. Se estiver bom, clique em Começar.";
                    reply.SetPlan(plan.Passos);
                    reply.IsAwaitingApproval = true;
                    _pendingPlan = reply;
                    StatusText = $"Plano pronto, com {plan.Passos.Count} etapas. Clique em Começar ou ajuste o pedido.";
                }
                else
                {
                    throw new InvalidOperationException("o modelo não devolveu um plano");
                }
            });
    }

    [RelayCommand]
    private async Task ChooseOptionAsync(ChatOption? option)
    {
        if (option is null || !CanSend) return;
        var owner = option.Owner;
        owner.Options.Clear();
        owner.Title = "Você escolheu";
        owner.Text = $"{owner.Text}\n→ {option.Text}";
        await PlanAsync($"{owner.PendingTask}\n\nEscolha da pessoa: {option.Text}");
    }

    [RelayCommand]
    private async Task StartPlanAsync(ChatMessage? msg)
    {
        if (msg is null || !msg.IsAwaitingApproval || !CanSend) return;
        msg.IsAwaitingApproval = false;
        _pendingPlan = null;
        await ExecuteAsync(msg, msg.PendingTask, msg.Plan.Select(p => p.Text).ToList());
    }

    [RelayCommand]
    private void DiscardPlan(ChatMessage? msg)
    {
        if (msg is null || !msg.IsAwaitingApproval) return;
        msg.IsAwaitingApproval = false;
        msg.Title = "Plano descartado";
        msg.Text = "Tudo bem. Escreva o pedido de outro jeito e eu monto um plano novo.";
        if (ReferenceEquals(_pendingPlan, msg)) _pendingPlan = null;
        StatusText = "Plano descartado. Escreva um novo pedido.";
    }

    private void DiscardPendingPlan()
    {
        if (_pendingPlan is { } p) DiscardPlan(p);
    }

    /// Executa as etapas aprovadas, atualizando o mesmo cartão.
    private async Task ExecuteAsync(ChatMessage reply, string task, IReadOnlyList<string> steps)
    {
        reply.Title = "Trabalhando no seu pedido";
        reply.Text = "";
        reply.IsWorking = true;
        reply.CurrentActivity = "Começando…";
        _host.ActiveModel = SelectedModel;
        StatusText = "Trabalhando. Você pode acompanhar cada etapa na conversa.";
        var gotFinal = false;
        var workspace = WorkspacePath;

        await RunWorkAsync(reply, ct => _api.RunStreamAsync(workspace, task, steps, SelectedModel, ev =>
        {
            switch (ev.Kind)
            {
                case "step_start":
                    reply.StartNextPlanStep();
                    reply.CurrentActivity = ev.Message;
                    StatusText = $"Etapa: {ev.Message}";
                    break;
                case "step_done":
                    reply.FinishPlanStep(true);
                    break;
                case "tool":
                    reply.AddStep(ev.Message);
                    reply.CurrentActivity = ev.Message;
                    StatusText = ev.Message;
                    if (ev.Path is { Length: > 0 } p && ev.Tool is "write_file" or "str_replace")
                        reply.AddChangedFile(Path.IsPathRooted(p) ? p : Path.Combine(workspace, p));
                    break;
                case "tool_done":
                    reply.FinishLastStep(true);
                    break;
                case "tool_failed":
                    reply.FinishLastStep(false, ev.Message);
                    break;
                case "status":
                    reply.CurrentActivity = ev.Message;
                    StatusText = ev.Message;
                    break;
                case "final":
                    gotFinal = true;
                    reply.Title = "Pronto! Veja o que eu fiz";
                    reply.Text = ev.Message.Trim();
                    StatusText = "Pronto. Leia a explicação na conversa.";
                    break;
                case "error":
                    gotFinal = true;
                    ShowFailure(reply, ev.Message);
                    break;
            }
            if (ev.RefreshTree == true) RequestTreeRefreshSoon();
            ChatScrolled?.Invoke();
        }, ct));

        if (!gotFinal && reply.Title == "Trabalhando no seu pedido")
        {
            reply.Title = "Terminei";
            reply.Text = "O trabalho acabou, mas não recebi uma explicação final. Confira os arquivos alterados abaixo.";
        }

        await RefreshTreeAsync();
        if (reply.ChangedFiles.LastOrDefault() is { } last)
            await OpenFileAsync(last);
    }

    /// Roda um trabalho do agente no cartão: liga o relógio, permite cancelar e trata erros.
    /// Retorna false se falhou ou foi cancelado.
    private async Task<bool> RunWorkAsync(ChatMessage reply, Func<CancellationToken, Task> work)
    {
        IsBusy = true;
        reply.IsWorking = true;
        _runCts = new CancellationTokenSource();
        _clockTarget = reply;
        _clockWatch.Restart();
        reply.Elapsed = "0 s";
        _clock.Start();
        ChatScrolled?.Invoke();

        var ok = false;
        try
        {
            await work(_runCts.Token);
            ok = true;
        }
        catch (OperationCanceledException)
        {
            reply.Title = "Cancelado";
            reply.Text = "Parei a pedido seu. O que já foi feito até aqui continua na pasta.";
            StatusText = "Cancelado.";
        }
        catch (Exception ex)
        {
            ShowFailure(reply, ex.Message);
        }
        finally
        {
            _clock.Stop();
            _clockTarget = null;
            reply.StopAllRunning();
            reply.IsWorking = false;
            reply.CurrentActivity = "";
            IsBusy = false;
            _runCts.Dispose();
            _runCts = null;
            ChatScrolled?.Invoke();
        }
        return ok;
    }

    private void ShowFailure(ChatMessage reply, string raw)
    {
        reply.Title = "Não deu certo";
        reply.Text = FriendlyError(raw);
        reply.Detail = raw;
        StatusText = "Algo deu errado. Veja a explicação na conversa.";
    }

    private void AddError(string title, string text, string detail)
    {
        Messages.Add(new ChatMessage(ChatRole.Error, title, text) { Detail = detail });
        ChatScrolled?.Invoke();
    }

    /// Traduz erros técnicos para algo que a pessoa consiga resolver.
    internal static string FriendlyError(string raw)
    {
        var r = raw.ToLowerInvariant();
        if (r.Contains("does not support tools") || (r.Contains("tool") && r.Contains("not supported")))
            return "Esse modelo não sabe usar ferramentas (criar e editar arquivos). Escolha outro em \"Modelo de IA\", na barra lateral.";
        if (r.Contains("status 401") || r.Contains("status 403") || r.Contains("invalid api key") || r.Contains("unauthorized"))
            return "A chave da API foi recusada. Confira se ela está certa e ativa em \"Modelo de IA\", na barra lateral.";
        if (r.Contains("status 429") || r.Contains("rate limit") || r.Contains("quota"))
            return "O serviço de IA recusou por excesso de uso ou falta de créditos. Espere um pouco ou confira o saldo da sua conta.";
        if (r.Contains("connection refused") || r.Contains("10061") || r.Contains("error sending request")
            || r.Contains("11434") || r.Contains("conexão"))
            return "Não consegui falar com o modelo de IA. Se você usa o Ollama, abra o programa. Se usa um serviço na internet, confira sua conexão.";
        if (r.Contains("não encontrado no backend") || r.Contains("ollama list"))
            return "Esse modelo não existe ou não está instalado. Escolha outro em \"Modelo de IA\", na barra lateral.";
        if (r.Contains("limite de repetições"))
            return "Fiquei repetindo a mesma ação sem avançar. Tente pedir de forma mais específica, por exemplo dizendo o nome do arquivo.";
        if (r.Contains("limite de passos"))
            return "O pedido era grande demais para uma vez só. Tente dividir em partes menores.";
        if (r.Contains("fora do workspace") || r.Contains("fora da pasta escolhida"))
            return "Esse pedido tentou mexer em algo fora da pasta escolhida, e eu só posso trabalhar dentro dela.";
        if (r.Contains("interpretar o plano") || r.Contains("plano vazio") || r.Contains("não devolveu um plano"))
            return "Não consegui montar um plano para esse pedido. Tente escrever de outro jeito, dizendo o que você quer no final.";
        if (r.Contains("timeout") || r.Contains("timed out"))
            return "O modelo demorou demais para responder. Tente um pedido menor ou um modelo mais leve.";
        return "Algo deu errado enquanto eu trabalhava. Tente de novo. Se repetir, o detalhe técnico abaixo ajuda a investigar.";
    }
}
