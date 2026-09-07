# What this fork adds

`samirhvbr/ai-usagebar` tracks `akitaonrails/ai-usagebar`. This file exists so
the next update can tell, without reading 60 commits, what is ours and what is
merely older.

Base: **upstream v1.12.0**. `Cargo.toml` keeps upstream's version number, with
no fork suffix — the number answers "which upstream is this", which is the
question that matters when merging the next one.

## Ours

| What | Where |
|---|---|
| **ShvIA vendor** — a self-hosted, OpenAI-compatible gateway; today/week/month rolling windows from `{base_url}/api/v1/usage` | `src/shvia/`, plus the usual vendor wiring (`vendor.rs`, `config.rs`, `usage.rs`, `active.rs`, `lib.rs`, `widget/{cli,run}.rs`, `tui/{app,panels,settings}.rs`) |
| ShvIA's three windows in the macOS menu bar (today / week / capped month), instead of the 5h+weekly pair it does not have | `macos/ai-usagebar-menubar.swift` (`FORMAT` tail, `parse`) |
| **"Status das APIs"** — a collapsible section listing every vendor's health, including the ones that are off or unconfigured | `macos/ai-usagebar-menubar.swift`, `gnome-extension/api-status-logic.js` + `extension.js` |
| `format::compact_count` — the compact magnitude formatter the uncapped-window rows need | `src/format.rs` |
| **The vendor catalog** — `ai-usagebar vendors --json`: every provider, how each authenticates, and whether it is enabled and credentialed here. What lets the section above carry no provider table | `src/catalog.rs`, `vendor.rs` (`AuthKind`, `auth_kind`/`api_key_env`/`login_command`), `config.rs` (`api_key_env_for`, `inline_api_key`) |

These are written to upstream's own conventions and are candidates for PRs.
The strongest is the **catalog**, because it is upstream's own `CLAUDE.md` rule
applied to the one table that had escaped it — "provider fetching, credentials,
canonical product names … belong in Rust; do not add a complete provider-name
table to a frontend". Both frontends had one, and both had drifted: the GNOME
extension listed sixteen of twenty-one providers, and the macOS menu bar
re-derived Claude's, Codex's, Cursor's and Antigravity's credential locations in
Swift, hard-coding a Cursor default path that Rust already resolves. The section
on top of it answers something neither the dropdown nor the Overview does, and
is built on `usage --json` rather than on any vendor's cache layout.

Pitch the catalog first and the section second: the catalog stands on its own
as a fix to a stated invariant, and it is the part with no Portuguese UI copy
to argue about.

## Was ours, is now upstream's

Do **not** re-apply these from an old branch — upstream's versions are further
along:

- **MiniMax** (`src/minimax/`) — contributed upstream and since evolved there.
- **Anthropic API** (`src/anthropic_api/`) — in upstream since v0.16.
- **Kilo / Novita / Moonshot / Grok** balance vendors.
- The macOS menu bar app and the GNOME extension themselves, including the
  per-vendor Overview mode, which covers most of what the fork's original
  "Status das APIs" panel was built to show.

Upstream's KDE plasmoid (v1.12.0) now draws one card per vendor off the same
`usage --json` report, and its tab strip deliberately keeps a failing vendor
visible — "hiding an errored vendor is what made it impossible to tell 'not
configured' from 'configured and broken'". That is the same question this
fork's section answers, so the section's remaining margin is narrower than it
was: it is the vendors `usage --json` never reports at all, because they are
switched off or have no credential yet. Pitch a PR on that, not on health
display in general.

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
