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

For testing, run **framework-dependent** (uses the .NET 8 runtime you just
installed — no extra downloads):

```powershell
cd windows-tray
dotnet run
```

> Don't use `-c Release` for `run`. The Release config is set up for a
> **self-contained** publish (RID `win-x64`), so `run -c Release` tries to
> restore the win-x64 runtime packs and can fail with `NU1100: unable to
> resolve Microsoft.*.Runtime.win-x64 (= 8.0.x)`. Plain `dotnet run` avoids
> that; `-c Release` is only for the portable bundle (step 5).

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
| `NU1100` resolving `*.Runtime.win-x64` | you used `-c Release` (self-contained) to *run*; use plain `dotnet run` to test |
| Tray dot is grey / no data | run `claude` (or `codex login`) once; test with `ai-usagebar.exe --pretty` |
| Wrong/old backend used | set `BackendPath` in `%APPDATA%\ai-usagebar-tray\settings.json` |

Backend discovery order, poll-interval floor (60s, default 300s), and the
full feature list are in **[README.md](README.md)**.

## Build gotchas (real-world, Windows 11)

These bit us on a first real run — documented so they don't bite you:

1. **`NU1100` resolving `Microsoft.*.Runtime.win-x64 (= 8.0.x)`** — a stale
   `obj/` from a previous `-c Release` (self-contained) build poisons later
   builds, even Debug ones. **Fix:** clean and build framework-dependent:
   ```cmd
   rmdir /s /q bin
   rmdir /s /q obj
   dotnet build -c Debug
   ```
   Only `dotnet publish -c Release` (the portable bundle) legitimately needs
   the win-x64 runtime packs.

2. **`MSB3021: cannot copy … ai-usagebar-tray.exe — being used by another
   process`** — you're rebuilding while the tray is running; it locks its own
   exe. **Fix:** kill it first, then build:
   ```cmd
   taskkill /F /IM ai-usagebar-tray.exe
   ```

3. **"You must install .NET Desktop Runtime"** — usually a false alarm from a
   half-written exe after a locked rebuild. The runtime ships with the SDK
   (`dotnet --list-runtimes` shows `Microsoft.WindowsDesktop.App 8.0.x`); a
   clean rebuild (after the `taskkill` above) fixes it.

4. **`Remove-Item` / `'#'` not recognized** — those are **PowerShell**. In
   **CMD** use `rmdir /s /q` and `::` for comments (or just use PowerShell).

5. **Terminal "hangs" after `dotnet run`** — expected: it's a GUI app, so the
   console blocks until the app exits, and `Ctrl+C` won't kill a WinForms app.
   To keep the prompt free, run the **built exe** instead — it returns
   immediately and the app lives in the **system tray**, not the console:
   ```cmd
   start "" "bin\Debug\net8.0-windows\ai-usagebar-tray.exe"
   ```

6. **"Where's the window?"** — there isn't one. Look at the **system tray**
   (bottom-right; click the `^` to show hidden icons) for the colored dot.
   Left-click → panel; right-click → menu (Refresh, Vendor picker, …).
