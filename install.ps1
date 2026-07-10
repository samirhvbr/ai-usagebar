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
# Native tools (git/cargo/dotnet) print progress to stderr; don't let that abort
# the script on PS 7.3+ (no-op on 5.1). We check $LASTEXITCODE ourselves.
$PSNativeCommandUseErrorActionPreference = $false
Set-Location -Path $PSScriptRoot

function Need($cmd, $hint) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Host "X $cmd nao encontrado. $hint" -ForegroundColor Red
        exit 1
    }
}

# Update the checkout — best-effort, with diagnostics when the pull fails. The
# pull is the flakiest step (missing/wrong remote, auth, or no git on PATH), so
# test each precondition and point at the likely culprit instead of moving on mute.
function Update-Repo {
    if ($env:SKIP_PULL -eq '1') { Write-Host '> git pull pulado (SKIP_PULL=1).'; return }
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Write-Warning 'git nao esta no PATH — pulando o update. Instale o Git for Windows ou rode com SKIP_PULL=1.'
        return
    }
    if (-not (Test-Path .git)) { Write-Warning 'isto nao e um checkout git — pulando o update.'; return }
    $remotes = git remote
    if (-not $remotes) {
        Write-Warning 'nenhum remote configurado — nao da pra atualizar.'
        Write-Host   '    conserto:  git remote add origin <URL-do-fork>'
        return
    }
    $branch = (git rev-parse --abbrev-ref HEAD)
    $remote = git config "branch.$branch.remote"
    if (-not $remote) { $remote = 'origin' }
    $url = git remote get-url $remote
    Write-Host "> git pull ($remote/$branch)..."
    git pull --ff-only $remote $branch
    if ($LASTEXITCODE -ne 0) {
        Write-Warning 'git pull FALHOU — seguindo com o que ja esta no disco. Cheque:'
        Write-Host   "     - remote   -> ${remote}: ${url}      (git remote -v)"
        Write-Host   "     - alcanca? -> git ls-remote $remote    (rede / auth / URL errada)"
        Write-Host   "     - local    -> git status               (mudanca nao commitada / conflito)"
    } else {
        Write-Host '  OK atualizado.'
    }
}

Need cargo  'Instale o Rust: https://rustup.rs'
Need dotnet 'Instale o .NET 8 SDK: https://dotnet.microsoft.com/download'

Update-Repo

# 1. Build + install the backend (ai-usagebar.exe -> %USERPROFILE%\.cargo\bin).
Write-Host '> cargo install (backend)...'
cargo install --path . --force
if ($LASTEXITCODE -ne 0) { throw "cargo install falhou (exit $LASTEXITCODE)." }
$backend = Join-Path $env:USERPROFILE '.cargo\bin\ai-usagebar.exe'
if (-not (Test-Path $backend)) { throw "backend nao encontrado em $backend" }

# 2. Publish the tray (self-contained single-file) bundling THIS backend next to it.
Write-Host '> dotnet publish (tray)...'
dotnet publish 'windows-tray\AiUsagebarTray.csproj' -c Release -p:BackendExe="$backend"
if ($LASTEXITCODE -ne 0) { throw "dotnet publish falhou (exit $LASTEXITCODE)." }
$publishDir = 'windows-tray\bin\Release\net8.0-windows\win-x64\publish'
if (-not (Test-Path (Join-Path $publishDir 'ai-usagebar-tray.exe'))) {
    throw "publish falhou (nao achei ai-usagebar-tray.exe em $publishDir)"
}

# 3. Copy the bundle to a stable per-user location so the auto-start path never moves.
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\ai-usagebar-tray'
Write-Host "> instalando em $installDir ..."
Get-Process 'ai-usagebar-tray' -ErrorAction SilentlyContinue | Stop-Process -Force  # solta o exe travado
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item -Path (Join-Path $publishDir '*') -Destination $installDir -Recurse -Force
$exe = Join-Path $installDir 'ai-usagebar-tray.exe'

# 4. Auto-start on login — same HKCU Run key/value the tray's "Start with Windows"
#    toggle uses, so the checkbox stays in sync.
Write-Host '> habilitando auto-start no login...'
New-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' `
    -Name 'AiUsagebarTray' -Value "`"$exe`"" -PropertyType String -Force | Out-Null

# 5. Launch now.
Start-Process $exe
$ver = ((Get-Content VERSION -ErrorAction SilentlyContinue) -join '').Trim()
Write-Host ''
Write-Host "OK - tray instalado e no ar; sobe no login. Versao: $ver"
