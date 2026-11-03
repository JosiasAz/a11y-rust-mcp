@echo off
chcp 65001 >nul
setlocal EnableExtensions
cd /d "%~dp0"

set "APPDIR=%~dp0app"
set "APPEXE=%APPDIR%\A11yAgent.exe"
set "AGENTEXE=%APPDIR%\agent\rust-a11y-agent.exe"
set "RUSTEXE=%~dp0target\release\rust-a11y-agent.exe"

echo ============================================
echo   A11y Agent
echo ============================================
echo.

REM Se o app ja foi gerado, so abre a interface (usuario comum).
if /I not "%~1"=="rebuild" if exist "%APPEXE%" if exist "%AGENTEXE%" (
  echo Abrindo a interface...
  start "" "%APPEXE%"
  exit /b 0
)

echo Gerando o app pela primeira vez ^(ou rebuild^)...
echo Depois voce so da dois cliques no atalho / A11yAgent.exe
echo.

if "%LLM_MODEL%"=="" set "LLM_MODEL=qwen2.5:7b"
if "%LLM_PROVIDER%"=="" set "LLM_PROVIDER=ollama"

where cargo >nul 2>&1
if errorlevel 1 (
  echo [ERRO] cargo nao encontrado. Instale Rust: https://rustup.rs/
  pause
  exit /b 1
)

where dotnet >nul 2>&1
if errorlevel 1 (
  echo [ERRO] dotnet nao encontrado. Instale .NET: https://dotnet.microsoft.com/download
  pause
  exit /b 1
)

echo [1/3] Compilando o agente Rust...
cargo build --release
if errorlevel 1 (
  echo [ERRO] Falha ao compilar o agente.
  pause
  exit /b 1
)

echo [2/3] Empacotando a interface ^(pode demorar na 1a vez^)...
dotnet publish "%~dp0desktop\A11yAgent.Desktop.csproj" -c Release -r win-x64 --self-contained true -o "%APPDIR%" -p:PublishSingleFile=false
if errorlevel 1 (
  echo [ERRO] Falha ao publicar o desktop.
  pause
  exit /b 1
)

if not exist "%APPDIR%\agent" mkdir "%APPDIR%\agent"
copy /Y "%RUSTEXE%" "%AGENTEXE%" >nul
if errorlevel 1 (
  echo [ERRO] Nao copiei rust-a11y-agent.exe para app\agent\
  pause
  exit /b 1
)

echo [3/3] Criando atalho na Area de Trabalho...
powershell -NoProfile -Command ^
  "$desk=[Environment]::GetFolderPath('Desktop');" ^
  "$lnk=Join-Path $desk 'A11y Agent.lnk';" ^
  "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($lnk);" ^
  "$s.TargetPath='%~dp0app\A11yAgent.exe';" ^
  "$s.WorkingDirectory='%~dp0app';" ^
  "$s.Description='A11y Agent';" ^
  "$s.Save()"

echo.
echo Pronto.
echo - Atalho: Area de Trabalho \ A11y Agent
echo - Ou abra: app\A11yAgent.exe
echo.
echo Abrindo agora...
start "" "%APPEXE%"
timeout /t 2 /nobreak >nul
endlocal
