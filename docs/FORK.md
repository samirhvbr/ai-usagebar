# What this fork adds

`samirhvbr/ai-usagebar` tracks `akitaonrails/ai-usagebar`. This file exists so
the next update can tell, without reading 200 commits, what is ours and what is
merely older.

Base: **upstream v1.20.2**. `Cargo.toml` keeps upstream's version number, with
no fork suffix — the number answers "which upstream is this", which is the
question that matters when merging the next one. `CHANGELOG.md` is left exactly
as upstream ships it: a fork entry under `[Unreleased]` is precisely the shape
that merges *cleanly* into a published section and corrupts it, which upstream's
own `CLAUDE.md` documents happening three times. What the fork changes is
recorded here instead.

## Ours

| What | Where |
|---|---|
| **ShvIA vendor** — a self-hosted, OpenAI-compatible gateway; today/week/month rolling windows from `{base_url}/api/v1/usage` | `src/shvia/`, plus the usual vendor wiring (`vendor.rs`, `config.rs`, `usage.rs`, `active.rs`, `detect.rs`, `catalog.rs`, `lib.rs`, `widget/{cli,run}.rs`, `tui/{app,panels,settings}.rs`) |
| ShvIA's three windows in the macOS menu bar — today in the session slot, week in the weekly one, a *capped* month in the fourth-window slot | `macos/ai-usagebar-menubar.swift` (`FORMAT` fields 50-56, `shviaWindow`, the `case "shvia"`) |
| `format::compact_count` — the compact magnitude formatter the uncapped-window rows need | `src/format.rs` |

That is the whole fork. Everything else it used to carry is upstream's now.

## Was ours, is now upstream's

Do **not** re-apply these from an old branch — upstream's versions are further
along:

- **The vendor catalog** (`src/catalog.rs`, `ai-usagebar vendors --json`) —
  contributed as PR #158 and shipped in upstream v1.13.0, comment for comment.
  Both desktop frontends read it there. This was the fork's strongest argument
  and it no longer needs making.
- **`fix(copilot)`: follow `gh`'s own config-dir precedence** — same PR.
- **Named Codex accounts across the macOS provider selectors**, and
  **distinguishing a disabled provider with an explicit enable** — the
  `codex/macos-*` branches, both merged upstream.
- **"Status das APIs"** — dropped deliberately, not lost. Upstream's macOS menu
  bar now lists every provider with its status off `vendors --json` (which is
  our catalog), the KDE plasmoid keeps a failing vendor visible, and upstream
  turned the UI English-only in v1.18.0 — so a Portuguese copy of a section
  upstream already draws was a permanent divergence buying very little. The
  GNOME extension is kept byte-identical to upstream for the same reason.
- **MiniMax** (`src/minimax/`), **Anthropic API** (`src/anthropic_api/`), and
  the Kilo / Novita / Moonshot / Grok balance vendors.

## Updating to a newer upstream

1. `git fetch upstream --tags`
2. Branch from `upstream/main` and re-apply the three rows above. The vendor
   wiring is mechanical, and the compiler finds every site for you: add the
   `VendorId` variant first and then fix each non-exhaustive match it reports.
   `git grep -ln grokbot -- src` lists the touchpoints a recent upstream vendor
   occupies, which is the current shape of the list.
3. Run the gate upstream's `CONTRIBUTING.md` describes: `make test`,
   `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
   `cargo machete`, and `bash macos/run-tests.sh`.

`FORMAT` in the macOS menu bar is positional: ShvIA's fields sit at the **end**,
so an upstream release that appends its own fields shifts ours. The indices in
`case "shvia"`, in `shviaWindow`, and in `testShviaWindows` must move together —
the swift test is what catches it.

## Toolchain note (macOS)

`macos/build.sh` needs Xcode's toolchain, not the Command Line Tools: SwiftUI's
macro plugins (`SwiftUIMacros`) ship only with Xcode, and `swiftc` from
`/Library/Developer/CommandLineTools` fails with "plugin for module
'SwiftUIMacros' not found". Either `xcode-select -s` once, or prefix the build:

```
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer bash macos/build.sh
```

The pre-1.10 history is on `legacy/main-0.16`. Its diffs no longer apply, so it
is a reference, not a base.
