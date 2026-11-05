using System.Collections.ObjectModel;
using Avalonia.Automation;
using CommunityToolkit.Mvvm.ComponentModel;

namespace A11yAgent.Desktop.Models;

public enum ChatRole { User, Agent, Error }

public enum StepState { Pending, Running, Ok, Failed }

/// Um passo: etapa do plano ("Criar a janela principal") ou ação ("Escrevendo arquivo: app.py").
public partial class ChatStep : ObservableObject
{
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Icon), nameof(AccessibleName), nameof(IsCurrent), nameof(LiveSetting),
        nameof(IsDone), nameof(IsFailed), nameof(IsPending))]
    private StepState _state = StepState.Running;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(AccessibleName))]
    private string _text = "";

    public bool IsCurrent => State == StepState.Running;
    // Usados só para a cor do ícone; o estado também está no texto e no nome acessível.
    public bool IsDone => State == StepState.Ok;
    public bool IsFailed => State == StepState.Failed;
    public bool IsPending => State == StepState.Pending;

    public string Icon => State switch
    {
        StepState.Pending => "○",
        StepState.Ok => "✓",
        StepState.Failed => "⚠",
        _ => "…"
    };

    public string AccessibleName => State switch
    {
        StepState.Pending => $"Ainda não começou: {Text}",
        StepState.Ok => $"Concluído: {Text}",
        StepState.Failed => $"Falhou: {Text}",
        _ => $"Em andamento: {Text}"
    };

    /// Falha interrompe o leitor de tela (Assertive); o resto espera a fala atual (Polite).
    public AutomationLiveSetting LiveSetting =>
        State == StepState.Failed ? AutomationLiveSetting.Assertive : AutomationLiveSetting.Polite;
}

/// Uma resposta possível a uma pergunta do agente (vira um botão).
public sealed record ChatOption(ChatMessage Owner, string Text);

/// Uma mensagem da conversa. Mensagens do agente guardam o plano, os passos e os arquivos.
public partial class ChatMessage : ObservableObject
{
    public ChatMessage(ChatRole role, string title, string text = "")
    {
        Role = role;
        _title = title;
        _text = text;
        Plan.CollectionChanged += (_, _) => OnPropertyChanged(nameof(HasPlan));
        Options.CollectionChanged += (_, _) => OnPropertyChanged(nameof(HasOptions));
    }

    public ChatRole Role { get; }
    public bool IsUser => Role == ChatRole.User;
    public bool IsError => Role == ChatRole.Error;
    public AutomationLiveSetting TitleLiveSetting =>
        IsError ? AutomationLiveSetting.Assertive : AutomationLiveSetting.Polite;

    /// Pedido que gerou o plano/pergunta (usado ao aprovar ou ao responder).
    public string PendingTask { get; set; } = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(AccessibleName))]
    private string _title;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(AccessibleName), nameof(HasText))]
    private string _text;

    [ObservableProperty] private bool _isWorking;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(AccessibleName))]
    private string _currentActivity = "";

    /// Tempo desde o início do trabalho ("1 min 05 s"), para mostrar que não travou.
    [ObservableProperty] private string _elapsed = "";

    /// Plano mostrado e esperando o clique em "Começar".
    [ObservableProperty] private bool _isAwaitingApproval;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasDetail))]
    private string _detail = "";

    public bool HasText => !string.IsNullOrWhiteSpace(Text);
    public bool HasDetail => !string.IsNullOrWhiteSpace(Detail);
    public bool HasSteps => Steps.Count > 0;
    public bool HasChangedFiles => ChangedFiles.Count > 0;
    public bool HasPlan => Plan.Count > 0;
    public bool HasOptions => Options.Count > 0;

    /// Etapas do plano (○ → … → ✓).
    public ObservableCollection<ChatStep> Plan { get; } = [];
    /// Ações de arquivo feitas pelo agente.
    public ObservableCollection<ChatStep> Steps { get; } = [];
    public ObservableCollection<string> ChangedFiles { get; } = [];
    public ObservableCollection<ChatOption> Options { get; } = [];

    public string AccessibleName
    {
        get
        {
            var who = Role switch { ChatRole.User => "Você disse", ChatRole.Error => "Problema", _ => "Assistente" };
            var body = HasText ? Text : CurrentActivity;
            var plan = HasPlan ? " Plano: " + string.Join(". ", Plan.Select(p => p.AccessibleName)) + "." : "";
            return $"{who}. {Title}. {body}{plan}";
        }
    }

    public void SetPlan(IEnumerable<string> steps)
    {
        Plan.Clear();
        foreach (var s in steps) Plan.Add(new ChatStep { Text = s, State = StepState.Pending });
        OnPropertyChanged(nameof(AccessibleName));
    }

    /// Marca a próxima etapa pendente como "em andamento".
    public void StartNextPlanStep()
    {
        if (Plan.FirstOrDefault(s => s.State == StepState.Pending) is { } step)
            step.State = StepState.Running;
    }

    public void FinishPlanStep(bool ok)
    {
        if (Plan.FirstOrDefault(s => s.State == StepState.Running) is { } step)
            step.State = ok ? StepState.Ok : StepState.Failed;
    }

    public void AddStep(string text)
    {
        Steps.Add(new ChatStep { Text = text });
        OnPropertyChanged(nameof(HasSteps));
    }

    public void FinishLastStep(bool ok, string? resultText = null)
    {
        var step = Steps.LastOrDefault(s => s.State == StepState.Running);
        if (step is null) return;
        step.State = ok ? StepState.Ok : StepState.Failed;
        if (!ok && !string.IsNullOrWhiteSpace(resultText))
            step.Text = $"{step.Text} — {resultText}";
    }

    /// Ao parar (fim, erro ou cancelamento), nada fica "em andamento" para sempre.
    public void StopAllRunning()
    {
        foreach (var s in Steps.Concat(Plan).Where(s => s.State == StepState.Running))
            s.State = StepState.Failed;
    }

    public void AddChangedFile(string path)
    {
        if (ChangedFiles.Contains(path)) return;
        ChangedFiles.Add(path);
        OnPropertyChanged(nameof(HasChangedFiles));
    }
}
