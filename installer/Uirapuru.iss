; Instalador do Uirapuru (Inno Setup 6).
; Não rode direto: use  build\empacotar.ps1 , que compila, assina e passa estes valores.
;   /DAppVersion=0.1.0      versão (vem do <Version> do desktop\A11yAgent.Desktop.csproj)
;   /DSourceDir=..\dist\app pasta publicada (Uirapuru.exe + agent\rust-a11y-agent.exe)
;   /DUseSign + /Suirapuru=...  assina o instalador e o desinstalador

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\app"
#endif

[Setup]
; Nunca mude o AppId: é ele que faz o Windows reconhecer atualizações e o desinstalador.
AppId={{E5487187-182F-4A4B-8A9A-0AFECC36FBF1}
AppName=Uirapuru
AppVersion={#AppVersion}
AppVerName=Uirapuru {#AppVersion}
AppPublisher=Josias Azevedo da Silva
AppCopyright=© 2026 Josias Azevedo da Silva
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Instalador do Uirapuru, assistente acessível
DefaultDirName={autopf}\Uirapuru
DefaultGroupName=Uirapuru
DisableProgramGroupPage=yes
; Instala só para o usuário sem pedir administrador; quem quiser pode escolher "todos os usuários".
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Uirapuru-Setup-{#AppVersion}
SetupIconFile=..\desktop\Assets\uirapuru.ico
UninstallDisplayIcon={app}\Uirapuru.exe
UninstallDisplayName=Uirapuru
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Fecha o app e o agente em segundo plano antes de atualizar ou desinstalar.
CloseApplications=yes
#ifdef UseSign
SignTool=uirapuru
SignedUninstaller=yes
#endif

[Languages]
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar um atalho na Área de Trabalho"; GroupDescription: "Atalhos:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Uirapuru"; Filename: "{app}\Uirapuru.exe"; Comment: "Assistente acessível"
Name: "{autodesktop}\Uirapuru"; Filename: "{app}\Uirapuru.exe"; Comment: "Assistente acessível"; Tasks: desktopicon

[Run]
; O Uirapuru precisa do Ollama para pensar. Se ele não estiver instalado, o instalador oferece a página oficial.
Filename: "https://ollama.com/download"; Description: "Abrir a página para baixar o Ollama (necessário para a IA funcionar)"; Flags: shellexec postinstall skipifsilent nowait; Check: not OllamaInstalado
Filename: "{app}\Uirapuru.exe"; Description: "Abrir o Uirapuru agora"; Flags: postinstall skipifsilent nowait

[Code]
function OllamaInstalado: Boolean;
begin
  Result := FileExists(ExpandConstant('{localappdata}\Programs\Ollama\ollama.exe'))
         or FileExists(ExpandConstant('{commonpf64}\Ollama\ollama.exe'));
end;
