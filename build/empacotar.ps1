<#
.SYNOPSIS
  Gera o instalador assinado do Uirapuru: dist\Uirapuru-Setup-<versão>.exe

.DESCRIPTION
  1. Compila o agente Rust (release).
  2. Publica o app desktop (self-contained, win-x64) em dist\app.
  3. Assina Uirapuru.exe, Uirapuru.dll e agent\rust-a11y-agent.exe.
  4. Gera o instalador com o Inno Setup e assina o instalador e o desinstalador.

  Certificado, nesta ordem:
    UIRAPURU_PFX + UIRAPURU_PFX_SENHA    arquivo .pfx (certificado comprado)
    UIRAPURU_CERT_THUMBPRINT             certificado já instalado no Windows
    "Uirapuru - assinatura de teste"     certificado autoassinado desta máquina

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File build\empacotar.ps1 -CriarCertificadoDeTeste   # primeira vez
  powershell -ExecutionPolicy Bypass -File build\empacotar.ps1
  powershell -ExecutionPolicy Bypass -File build\empacotar.ps1 -SemAssinatura
#>
param(
  [switch]$SemAssinatura,
  [switch]$CriarCertificadoDeTeste
)
$ErrorActionPreference = 'Stop'
$raiz = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $raiz 'dist'
$app = Join-Path $dist 'app'
$nomeCertTeste = 'Uirapuru - assinatura de teste'
$carimboDeTempo = 'http://timestamp.digicert.com'

function Etapa($texto) { Write-Host "`n==> $texto" -ForegroundColor Cyan }
function Rodar($exe, [string[]]$argumentos) {
  & $exe @argumentos
  if ($LASTEXITCODE -ne 0) { throw "Falhou ($LASTEXITCODE): $exe $($argumentos -join ' ')" }
}

function Achar-SignTool {
  $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  $st = Get-ChildItem $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName | Select-Object -Last 1
  if (-not $st) { throw 'signtool.exe não encontrado. Instale o Windows SDK (winget install Microsoft.WindowsSDK.10.0.26100).' }
  $st.FullName
}

function Achar-Iscc {
  $c = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") |
       Where-Object { Test-Path $_ } | Select-Object -First 1
  if (-not $c) { throw 'Inno Setup 6 não encontrado. Instale com: winget install JRSoftware.InnoSetup' }
  $c
}

# Argumentos do signtool que escolhem o certificado (sem a parte "sign ... arquivo").
function Argumentos-Certificado {
  if ($env:UIRAPURU_PFX) {
    if (-not (Test-Path $env:UIRAPURU_PFX)) { throw "UIRAPURU_PFX aponta para um arquivo que não existe: $env:UIRAPURU_PFX" }
    return @('/f', $env:UIRAPURU_PFX, '/p', $env:UIRAPURU_PFX_SENHA)
  }
  if ($env:UIRAPURU_CERT_THUMBPRINT) { return @('/sha1', $env:UIRAPURU_CERT_THUMBPRINT) }
  $cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert |
          Where-Object { $_.FriendlyName -eq $nomeCertTeste -and $_.NotAfter -gt (Get-Date) } |
          Sort-Object NotAfter | Select-Object -Last 1
  if (-not $cert) {
    throw "Nenhum certificado encontrado. Rode com -CriarCertificadoDeTeste, defina UIRAPURU_PFX/UIRAPURU_PFX_SENHA ou UIRAPURU_CERT_THUMBPRINT, ou use -SemAssinatura."
  }
  Write-Host "Certificado de TESTE (autoassinado): $($cert.Subject)" -ForegroundColor Yellow
  return @('/sha1', $cert.Thumbprint)
}

# ---------------------------------------------------------------------------------------------
if ($CriarCertificadoDeTeste) {
  Etapa 'Criando certificado autoassinado de teste (CurrentUser\My)'
  $existente = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Where-Object { $_.FriendlyName -eq $nomeCertTeste -and $_.NotAfter -gt (Get-Date) }
  if ($existente) {
    Write-Host "Já existe: $($existente[0].Thumbprint)"
  } else {
    $novo = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=Josias Azevedo da Silva, O=Uirapuru' `
      -FriendlyName $nomeCertTeste -CertStoreLocation Cert:\CurrentUser\My `
      -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddYears(3)
    Write-Host "Criado: $($novo.Thumbprint) (válido até $($novo.NotAfter.ToString('dd/MM/yyyy')))"
  }
}

$versao = ([xml](Get-Content (Join-Path $raiz 'desktop\A11yAgent.Desktop.csproj') -Raw)).Project.PropertyGroup.Version | Where-Object { $_ } | Select-Object -First 1
if (-not $versao) { throw 'Não achei <Version> em desktop\A11yAgent.Desktop.csproj' }
Write-Host "Uirapuru $versao"
$iscc = Achar-Iscc
if (-not $SemAssinatura) { $signtool = Achar-SignTool; $certArgs = Argumentos-Certificado }

Etapa '1/4 Compilando o agente (Rust, release)'
Push-Location $raiz
try { Rodar 'cargo' @('build', '--release') } finally { Pop-Location }

Etapa '2/4 Publicando o app desktop em dist\app'
if (Test-Path $app) { Remove-Item $app -Recurse -Force }
Rodar 'dotnet' @('publish', (Join-Path $raiz 'desktop\A11yAgent.Desktop.csproj'), '-c', 'Release', '-r', 'win-x64',
  '--self-contained', 'true', '-p:PublishSingleFile=false', '-o', $app, '-v', 'q')
New-Item -ItemType Directory -Force (Join-Path $app 'agent') | Out-Null
Copy-Item (Join-Path $raiz 'target\release\rust-a11y-agent.exe') (Join-Path $app 'agent\rust-a11y-agent.exe')

$nossos = @('Uirapuru.exe', 'Uirapuru.dll', 'agent\rust-a11y-agent.exe') | ForEach-Object { Join-Path $app $_ }
if ($SemAssinatura) {
  Etapa '3/4 Assinatura pulada (-SemAssinatura)'
} else {
  Etapa '3/4 Assinando os executáveis'
  Rodar $signtool (@('sign', '/fd', 'sha256', '/tr', $carimboDeTempo, '/td', 'sha256', '/d', 'Uirapuru') + $certArgs + $nossos)
}

Etapa '4/4 Gerando o instalador (Inno Setup)'
$issArgs = @("/DAppVersion=$versao", "/DSourceDir=$app", '/Q', (Join-Path $raiz 'installer\Uirapuru.iss'))
if (-not $SemAssinatura) {
  # $q e $f são variáveis do próprio Inno Setup (aspas e arquivo a assinar), por isso o acento grave.
  $cmd = "`$q$signtool`$q sign /fd sha256 /tr $carimboDeTempo /td sha256 /d `$qUirapuru`$q " +
         (($certArgs | ForEach-Object { if ($_ -match '\s') { "`$q$_`$q" } else { $_ } }) -join ' ') + ' $f'
  $issArgs = @('/DUseSign', "/Suirapuru=$cmd") + $issArgs
}
Rodar $iscc $issArgs

$setup = Join-Path $dist "Uirapuru-Setup-$versao.exe"
Etapa 'Conferindo assinaturas'
foreach ($f in $nossos + $setup) {
  $s = Get-AuthenticodeSignature $f
  $quem = if ($s.SignerCertificate) { $s.SignerCertificate.Subject } else { '(sem assinatura)' }
  '{0,-28} {1,-14} {2}' -f (Split-Path $f -Leaf), $s.Status, $quem
}
Write-Host "`nPronto: $setup" -ForegroundColor Green
if (-not $SemAssinatura -and -not $env:UIRAPURU_PFX -and -not $env:UIRAPURU_CERT_THUMBPRINT) {
  Write-Host 'Assinado com certificado de TESTE: outras máquinas vão mostrar "Editor desconhecido" até você usar um certificado comprado.' -ForegroundColor Yellow
}
