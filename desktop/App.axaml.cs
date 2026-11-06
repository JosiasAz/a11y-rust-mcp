using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using A11yAgent.Desktop.Services;
using A11yAgent.Desktop.ViewModels;
using A11yAgent.Desktop.Views;

namespace A11yAgent.Desktop;

public partial class App : Application
{
    private AgentHost? _host;

    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
    }

    public override void OnFrameworkInitializationCompleted()
    {
        AiSettings.Load(); // antes do agente: ele herda o provedor escolhido pelas variáveis de ambiente
        _host = new AgentHost();

        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var vm = new MainViewModel(_host);
            desktop.MainWindow = new MainWindow { DataContext = vm };
            desktop.Exit += (_, _) =>
            {
                _host.Dispose();
                _host = null;
            };
        }

        base.OnFrameworkInitializationCompleted();
    }
}
