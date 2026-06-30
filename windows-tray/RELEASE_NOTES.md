<!-- Body for the tray GitHub Release (used by tray-release.yml as body_path).
     Update the "What's new" section before tagging the next v*-tray release.
     HTML comments like this one are not rendered in the published release. -->

Windows system-tray widget for ai-usagebar (Claude/GPT/GLM/OpenRouter/DeepSeek usage).

![Anthropic panel](https://raw.githubusercontent.com/EaeDave/ai-usagebar/main/windows-tray/screenshots/panel-anthropic.png)

## Download
`ai-usagebar-tray-win-x64.zip` — unzip and run `ai-usagebar-tray.exe`. Self-contained (no .NET install needed). Bundles the Rust backend (`ai-usagebar.exe`).

## What's new in v0.1.1
- Config error/status messages from the bundled backend now show the **resolved Windows config path** (via `directories::ProjectDirs`) instead of the hard-coded Unix `~/.config/...`; the `(chmod 600)` hint is Unix-only.
- The bundle is now **built and packaged by CI** on a native `windows-latest` runner (previously a manual cross-compile), so each release is reproducible.

## Setup
Populate credentials for at least one vendor, e.g. Anthropic at `%USERPROFILE%\.claude\.credentials.json` (run `claude` once), or OpenAI/Codex at `%USERPROFILE%\.codex\auth.json` (run `codex login` once), then launch the tray. Right-click the tray icon to pick a vendor.

## Screenshots

Tray icon shows the primary usage %, with a hover tooltip:

![Tray icon and tooltip](https://raw.githubusercontent.com/EaeDave/ai-usagebar/main/windows-tray/screenshots/tray-tooltip.png)

Right-click to switch vendor:

![Vendor menu](https://raw.githubusercontent.com/EaeDave/ai-usagebar/main/windows-tray/screenshots/vendor-menu.png)

Per-vendor panels (here OpenAI / ChatGPT):

![OpenAI panel](https://raw.githubusercontent.com/EaeDave/ai-usagebar/main/windows-tray/screenshots/panel-openai.png)

## Notes
- Tray shows the primary usage %; left-click for the detail panel.
- Built natively on `windows-latest` (Rust `x86_64-pc-windows-msvc` + .NET 8 self-contained WinForms).
