using A11yAgent.Desktop.ViewModels;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Threading;

namespace A11yAgent.Desktop.Views;

public partial class MainWindow : Window
{
    private MainViewModel? _boundVm;

    public MainWindow()
    {
        InitializeComponent();
        // Túnel: o TextBox consumiria o Enter antes de um KeyDown normal.
        this.FindControl<TextBox>("PromptBox")?.AddHandler(KeyDownEvent, OnPromptKeyDown, RoutingStrategies.Tunnel);

        DataContextChanged += (_, _) => BindViewModel();
        Opened += async (_, _) =>
        {
            BindViewModel();
            this.FindControl<TextBox>("PromptBox")?.Focus();
            if (_boundVm is not null)
                await _boundVm.BootCommand.ExecuteAsync(null);
        };
    }

    private void BindViewModel()
    {
        if (DataContext is not MainViewModel vm) return;
        if (ReferenceEquals(_boundVm, vm)) return;

        if (_boundVm is not null)
            _boundVm.ChatScrolled -= ScrollChatToEnd;

        _boundVm = vm;
        vm.AttachWindow(this);
        vm.ChatScrolled += ScrollChatToEnd;
    }

    private void ScrollChatToEnd()
    {
        Dispatcher.UIThread.Post(() =>
        {
            if (_boundVm is { Messages.Count: > 0 } vm && this.FindControl<ListBox>("ChatList") is { } list)
                list.ScrollIntoView(vm.Messages[^1]);
        }, DispatcherPriority.Background);
    }

    // Enter envia; Shift+Enter pula linha.
    private async void OnPromptKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter || e.KeyModifiers.HasFlag(KeyModifiers.Shift) || _boundVm is null) return;
        e.Handled = true;
        await _boundVm.SendCommand.ExecuteAsync(null);
        this.FindControl<TextBox>("PromptBox")?.Focus();
    }

    private async void OnFileDoubleTapped(object? sender, TappedEventArgs e) => await OpenSelectedFile(sender);

    private async void OnFileListKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter) return;
        e.Handled = true;
        await OpenSelectedFile(sender);
    }

    private async Task OpenSelectedFile(object? sender)
    {
        if (_boundVm is not null && sender is ListBox { SelectedItem: string entry })
            await _boundVm.OpenEntryCommand.ExecuteAsync(entry);
    }
}
