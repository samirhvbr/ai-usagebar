# Testing the Windows tray (step-by-step)

How to build and run the tray from **this fork** on a Windows machine. The
tray is a thin consumer of `ai-usagebar.exe --json`, so you build the Rust
backend (already Windows-ready in this fork, v0.7.1) and then the C# tray.

Tray authored by [EaeDave](https://github.com/EaeDave/ai-usagebar).

## 1. Prerequisites (install once, in PowerShell)

```powershell
# Rust toolchain (builds ai-usagebar.exe)
winget install --id Rustlang.Rustup -e
# .NET 8 SDK (builds the C# tray)
winget install --id Microsoft.DotNet.SDK.8 -e
```

Open a **new** PowerShell after installing, then verify:

```powershell
rustc --version
dotnet --version
```

You also need credentials for the vendor you'll display. For Anthropic, run
the Claude Code CLI once so the OAuth file exists:

```powershell
claude        # creates %USERPROFILE%\.claude\.credentials.json
# (or: codex login  -> %USERPROFILE%\.codex\auth.json  for OpenAI)
```

## 2. Clone this fork

```powershell
git clone https://github.com/samirhvbr/ai-usagebar.git
cd ai-usagebar
```

## 3. Build the Rust backend

```powershell
cargo build --release
# produces target\release\ai-usagebar.exe
```

Quick sanity check that the backend works on Windows:

```powershell
.\target\release\ai-usagebar.exe --vendor anthropic --pretty
```

You should see the bars / plan summary.

## 4. Build & run the tray

```powershell
cd windows-tray
dotnet run -c Release
```

A colored dot appears in the system tray (color follows usage severity):

- **Hover** → tooltip with plan + Session/Weekly.
- **Left-click** → detail panel with progress bars.
- **Right-click** → menu: Open panel, Refresh now, **Vendor picker**,
  Start with Windows, Exit.

## 5. (Optional) Portable bundle — no .NET needed to run

```powershell
# from the repo root, after `cargo build --release`:
cd windows-tray
dotnet publish -c Release
```

Output folder (self-contained UI + backend, copy anywhere and double-click):

```
windows-tray\bin\Release\net8.0-windows\win-x64\publish\
  ├─ ai-usagebar-tray.exe   (UI; bundles the .NET runtime)
  └─ ai-usagebar.exe        (Rust backend; copied automatically)
```

## Notes / troubleshooting

| Symptom | Fix |
|---|---|
| `cargo` not found | install Rustup (step 1), open a new shell |
| `dotnet` not found | install .NET 8 SDK (step 1), open a new shell |
| Tray dot is grey / no data | run `claude` (or `codex login`) once; test with `ai-usagebar.exe --pretty` |
| Wrong/old backend used | set `BackendPath` in `%APPDATA%\ai-usagebar-tray\settings.json` |

Backend discovery order, poll-interval floor (60s, default 300s), and the
full feature list are in **[README.md](README.md)**.
