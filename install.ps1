#Requires -Version 5.1
<#
  One-shot installer for the ai-usagebar Windows tray — the Windows counterpart
  to install.sh. Does the whole dance:
    git pull -> cargo install (backend) -> dotnet publish (self-contained tray,
    bundles the backend next to it) -> copy to a stable per-user location ->
    register auto-start (HKCU Run) -> launch.

  Run from an ordinary PowerShell (no admin needed):
    powershell -ExecutionPolicy Bypass -File .\install.ps1

  Env: set SKIP_PULL=1 to skip the git pull.
#>

$ErrorActionPreference = 'Stop'
Set-Location -Path $PSScriptRoot

function Need($cmd, $hint) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Error "$cmd nao encontrado. $hint"
        exit 1
    }
}
Need cargo  'Instale o Rust: https://rustup.rs'
Need dotnet 'Instale o .NET 8 SDK: https://dotnet.microsoft.com/download'

# 1. Update the checkout (best-effort; a dirty tree shouldn't abort the install).
if ($env:SKIP_PULL -ne '1' -and (Test-Path .git)) {
    Write-Host '> git pull...'
    try { git pull --ff-only } catch { Write-Warning 'git pull pulou; seguindo com o que esta no disco.' }
}

# 2. Build + install the backend (ai-usagebar.exe -> %USERPROFILE%\.cargo\bin).
Write-Host '> cargo install (backend)...'
cargo install --path . --force
$backend = Join-Path $env:USERPROFILE '.cargo\bin\ai-usagebar.exe'
if (-not (Test-Path $backend)) { Write-Error "backend nao encontrado em $backend"; exit 1 }

# 3. Publish the tray (self-contained single-file) bundling THIS backend next to it.
Write-Host '> dotnet publish (tray)...'
dotnet publish 'windows-tray\AiUsagebarTray.csproj' -c Release -p:BackendExe="$backend"
$publishDir = 'windows-tray\bin\Release\net8.0-windows\win-x64\publish'
if (-not (Test-Path (Join-Path $publishDir 'ai-usagebar-tray.exe'))) {
    Write-Error "publish falhou (nao achei ai-usagebar-tray.exe em $publishDir)"; exit 1
}

# 4. Copy the bundle to a stable per-user location so the auto-start path never moves.
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\ai-usagebar-tray'
Write-Host "> instalando em $installDir ..."
Get-Process 'ai-usagebar-tray' -ErrorAction SilentlyContinue | Stop-Process -Force  # solta o exe travado
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item -Path (Join-Path $publishDir '*') -Destination $installDir -Recurse -Force
$exe = Join-Path $installDir 'ai-usagebar-tray.exe'

# 5. Auto-start on login — same HKCU Run key/value the tray's own "Start with
#    Windows" toggle uses, so the checkbox stays in sync.
Write-Host '> habilitando auto-start no login...'
New-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' `
    -Name 'AiUsagebarTray' -Value "`"$exe`"" -PropertyType String -Force | Out-Null

# 6. Launch now.
Start-Process $exe
$ver = ((Get-Content VERSION -ErrorAction SilentlyContinue) -join '').Trim()
Write-Host ''
Write-Host "OK - tray instalado e no ar; sobe no login. Versao: $ver"
