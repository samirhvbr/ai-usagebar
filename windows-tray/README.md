# ai-usagebar tray (Windows)

> **Credit:** this Windows tray was created by
> **[EaeDave](https://github.com/EaeDave/ai-usagebar)**. It is vendored here
> (MIT) so this fork can build and test all three desktop integrations —
> GNOME, macOS, and Windows — together. Upstream credit for the Windows tray
> belongs to EaeDave. See **[TESTING.md](TESTING.md)** for a step-by-step.

A native Windows **system-tray** widget for
[`ai-usagebar`](../README.md). It shells out to the native `ai-usagebar.exe`
backend (`--json`) and renders:

- A **colored tray dot** whose color follows the usage severity
  (green → yellow → orange → red), matching the Waybar/TUI palette.
- A **hover tooltip** with the plan + Session/Weekly summary (plain text).
- **Left-click** → a bordered detail panel with progress bars.
- **Right-click** → menu: Open panel, Refresh now, Vendor picker,
  Start with Windows, Exit.

The tray never reimplements vendor logic; all auth + API work stays in the
Rust backend. This is a thin consumer of its JSON.

## Prerequisites

- **.NET SDK 8.0+** on Windows. Check / install:
  ```powershell
  dotnet --version
  # if missing:
  winget install --id Microsoft.DotNet.SDK.8 -e
  ```
- A built `ai-usagebar.exe` (see the repo root README — `cargo build --release`
  produces `target\release\ai-usagebar.exe`).
- A populated `%USERPROFILE%\.claude\.credentials.json` (or whichever vendor
  you select) so the backend has something to report.

## Build & run

From this directory (`windows-tray`):

```powershell
dotnet build -c Release
dotnet run -c Release          # or run the built exe directly
```

### Portable bundle (self-contained, no .NET needed)

The Release config publishes a compressed, self-contained single-file exe AND
copies the Rust backend (`ai-usagebar.exe`) next to it, so the publish folder is
a self-sufficient bundle:

```powershell
# Build the backend first (from the repo root):
cargo build --release
# Then publish the tray (from windows-tray\):
dotnet publish -c Release
```

Output folder — copy it whole to any machine and double-click the tray exe:

```
bin\Release\net8.0-windows\win-x64\publish\
  ├─ ai-usagebar-tray.exe   (UI; bundles the .NET runtime)
  └─ ai-usagebar.exe        (Rust backend; copied automatically)
```

The tray probes "next to me" first, so the bundled backend is always used.

> If `cargo build --release` hasn't been run, publish still succeeds but warns
> that `ai-usagebar.exe` wasn't bundled. Point at a custom backend with
> `dotnet publish -c Release -p:BackendExe=C:\path\to\ai-usagebar.exe`.

**Note:** these are still two executables (a Rust backend + a C# UI) living in
one folder — not a single merged binary. The tray runs the backend as a child
process and reads its `--json`. This keeps all auth/API logic in the audited
Rust binary.

## Backend discovery

The tray finds `ai-usagebar.exe` in this order:

1. The `BackendPath` in settings (if set).
2. Next to the tray exe.
3. Common dev/build locations (`..\..\..\..\target\release`,
   `%USERPROFILE%\dev\projects\ai-usagebar\target\release`,
   `%USERPROFILE%\.cargo\bin`).
4. `ai-usagebar` on `PATH`.

To pin it explicitly, edit
`%APPDATA%\ai-usagebar-tray\settings.json`:

```json
{
  "BackendPath": "C:\\Users\\David\\dev\\projects\\ai-usagebar\\target\\release\\ai-usagebar.exe",
  "Vendor": "anthropic",
  "IntervalSeconds": 300
}
```

(Auto-start is toggled from the tray menu and stored in the
`HKCU\…\Run` registry key, not in this file.)

## Notes

- **Poll interval** defaults to 300s. The Anthropic/OpenAI endpoints
  rate-limit aggressively below ~300s; the floor is 60s.
- **Auto-start** is implemented via the `HKCU\...\Run` registry key
  (no admin needed).
- The Windows tooltip is plain text (no Pango); the tray strips the
  backend's `<span>` markup and shows a compact summary.
