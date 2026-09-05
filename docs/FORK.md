# What this fork adds

`samirhvbr/ai-usagebar` tracks `akitaonrails/ai-usagebar`. This file exists so
the next update can tell, without reading 60 commits, what is ours and what is
merely older.

Base: **upstream v1.10.0**. `Cargo.toml` keeps upstream's version number, with
no fork suffix — the number answers "which upstream is this", which is the
question that matters when merging the next one.

## Ours

| What | Where |
|---|---|
| **ShvIA vendor** — a self-hosted, OpenAI-compatible gateway; today/week/month rolling windows from `{base_url}/api/v1/usage` | `src/shvia/`, plus the usual vendor wiring (`vendor.rs`, `config.rs`, `usage.rs`, `active.rs`, `lib.rs`, `widget/{cli,run}.rs`, `tui/{app,panels,settings}.rs`) |
| ShvIA's three windows in the macOS menu bar (today / week / capped month), instead of the 5h+weekly pair it does not have | `macos/ai-usagebar-menubar.swift` (`FORMAT` tail, `parse`) |
| **"Status das APIs"** — a collapsible section listing every vendor's health, including the ones that are off or unconfigured | `macos/ai-usagebar-menubar.swift`, `gnome-extension/api-status-logic.js` + `extension.js` |
| `format::compact_count` — the compact magnitude formatter the uncapped-window rows need | `src/format.rs` |

Both features are written to upstream's own conventions and are candidates for
PRs: the section answers something neither the dropdown nor the Overview does,
and it is built on `usage --json` rather than on any vendor's cache layout.

## Was ours, is now upstream's

Do **not** re-apply these from an old branch — upstream's versions are further
along:

- **MiniMax** (`src/minimax/`) — contributed upstream and since evolved there.
- **Anthropic API** (`src/anthropic_api/`) — in upstream since v0.16.
- **Kilo / Novita / Moonshot / Grok** balance vendors.
- The macOS menu bar app and the GNOME extension themselves, including the
  per-vendor Overview mode, which covers most of what the fork's original
  "Status das APIs" panel was built to show.

## Updating to a newer upstream

1. `git fetch upstream --tags`
2. Branch from `upstream/main`, then re-apply the table above. The vendor
   wiring is mechanical — `git grep -n novita -- src` lists every touchpoint a
   vendor occupies.
3. Run the gate upstream's `CONTRIBUTING.md` describes: `make test`,
   `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
   `cargo machete`, and `bash macos/run-tests.sh`.

The pre-1.10 history is on `legacy/main-0.16`. Its diffs no longer apply —
upstream rewrote the fetch policy (`outcome.rs`), added multi-account support
(`account.rs`) and reorganised the TUI — so it is a reference, not a base.
