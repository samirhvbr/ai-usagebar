# Version — samirhvbr fork of ai-usagebar

**Current version:** `0.5.0`
**Upstream:** [akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar) @ `7418f4c` (`v1.51.0`) — 74 commits of ours past it

The **first semver in this file is ours**, and that is not cosmetic: everything
that reads a version here takes the first one it finds. Pointing that at the
upstream's number would hand our tooling a value we do not control, cannot bump,
and that moves on somebody else's schedule — including backwards, when an
upstream reverts ([repodocs ADR-012 and ADR-023][adr]).

**Our history starts where our changes start**, not at the upstream's number, so
this begins at `0.1.0` regardless of how far along they are.

**The upstream's own version fields are theirs and are not edited here.**
Editing them conflicts on every sync.

**A sync is a delivery.** Pulling from `upstream` changes what this fork is, so
it bumps the version above and its entry names the upstream point we moved to —
even when not a line of our own code changed.

Where this fork's CI runs: [`docs/ci.md`](docs/ci.md).

[adr]: https://github.com/samirhvbr/repodocs/blob/master/docs/decisions.md

---

## Changelog

Newest first. Each `##` heading is literally the commit subject. The upstream's
own changelog, where it has one, is left alone: it is their record, not ours.

## 0.5.0 - adopt upstream v1.51.0, preserving ShvIA and fork panels

Merges the official `v1.51.0` release, exactly
`7418f4c29245dd29660d40e0d1f03071183861a5`, into fork main
`91b004361d5b44aef7dd66c81fc171bd8f22e4fe`. The upstream point moves from
`a983efd99c514b7b13018900b21a0ead5c841f18` (`v1.50.0`), importing 31 commits.
This brings Kiro Google/GitHub social sessions, Omarchy pace markers, scoped
Claude Keychain account detection, Anthropic API named accounts in the native
menu, and the released Mistral and Kimi fixes.

Resolution and preservation notes:

- The only conflict was `CHANGELOG.md`: the fork's Kimi entry was already
  released upstream through #619 (`3d0ce41`, original patch `7348cbb`). The
  official release changelog is retained byte for byte, with no duplicate
  `[Unreleased]` entry. Upstream version fields and packaging metadata also
  match the release exactly; only this file carries fork version `0.5.0`.
- Shared Rust files merged without manual source changes. ShvIA remains
  opt-in, with its configuration, provider registration, fetch/report/TUI
  integration and credential-permission handling preserved. Its provider
  directory is byte-identical to the previous fork main.
- The entire GNOME extension is byte-identical to the previous fork main:
  saved aggregate panel entries, named-account selection, "Status das APIs",
  dark styling and the single background API-refresh owner are retained.
  The official release did not change that tree.
- Native macOS source, tests and README now match the release exactly. The
  fork's prior Kimi monthly patch is already included there, alongside the
  new Anthropic API named-account mirror; it is not reapplied separately.
- The fork-only CI tag fetch and `docs/ci.md` are unchanged. They fetch
  upstream tags only into disposable checkouts for changelog validation;
  synchronization does not push upstream tags or change runner settings.

Preparation checks passed: rustfmt, cargo-machete, immutable changelog/version
checks, `git diff --check`, GNOME/KDE/popover/Omarchy Node contracts, 18 Linux
Mint model tests and four macOS packaging tests. Rust builds, Clippy, the
Swift suite and platform integration checks are left to the separate build
and CI lanes; this preparation did not run heavyweight local builds. Live
GNOME, KDE, Omarchy and GTK behavior has not been exercised on this macOS
host. Runtime installation and the settings-preserving reload are verified
separately from this source merge.

## 0.4.1 - include Kimi monthly quota in the legacy macOS menu

Applies upstream contribution [#619](https://github.com/akitaonrails/ai-usagebar/pull/619)
(`7348cbb`, based on `a983efd`) without changing the shared quota fetch path.
The Swift format now carries Kimi's monthly pool into its existing third
quota row and Overview's maximum-window headline. Older weekly-only output
keeps its existing rows; a missing monthly pool never becomes a fabricated 0%.

The full Swift test harness and production native build pass on macOS arm64.
Synthetic parser regressions fail on the unchanged base and pass here;
light/dark Ring and Block fixture renders are attached to the upstream PR.
Full Rust and platform CI is tracked on that PR. No credentials or local
account data are included in the contribution. The installed Swift binary
was rebuilt and the LaunchAgent reloaded with the existing CLI and settings.

## 0.4.0 - fetch upstream tags locally for fork changelog validation

The first CI run of the sync found only the fork's early remote tags and
compared old release sections instead of the current upstream releases.
Linux and macOS now fetch the official `v*` tags into their disposable
checkouts before the unchanged changelog gate. This never pushes tags or
starts the fork's Release workflow; upstream released history stays intact.

## 0.4.0 - adopt upstream v1.50.0, preserving ShvIA and GNOME account panels

Moves the upstream point from `v1.36.0` (`22975d7f`) to `v1.50.0`
(`a983efd`), 504 upstream commits. This includes the Console organization
support, app-icon menu-bar mode, columned usage report, new providers, and
quota/cache/account reliability fixes released in that interval.

Resolution notes, for whoever syncs next:

- Six files conflicted: the example provider list, the TUI outside-click
  test, and GNOME's extension, marker tests, preferences and stylesheet.
  ShvIA stays registered alongside the new upstream providers. The TUI test
  adopts upstream's newer geometry-based fix for the same growing-picker
  problem that our previous sync worked around.
- GNOME adopts upstream's tabbed provider menu and per-provider indicators,
  while keeping named Claude account selection, "Status das APIs", the dark
  menu and the saved `panel-entries` combined strip. A nonempty combined
  selection still draws exactly one indicator; an empty selection uses the
  new provider switches. Existing setting keys are retained.
- Named selections still pass `--account` separately to the binary. Only the
  first indicator owns the background API-refresh timer, so additional
  provider indicators do not multiply those requests. The empty combined
  strip retains its icon fallback without duplicating separate indicators'
  already-prepended brand marks.
- GNOME contract tests exercise both selection modes, named-account command
  arguments, the icon fallback and the single background timer. Desktop,
  Omarchy, Mint and macOS packaging model tests pass locally. The GNOME
  extension has not been exercised in a live GNOME Shell on this macOS host.
- `CHANGELOG.md` and upstream version fields are v1.50.0's, byte for byte.
  The immutable-changelog gate passes without a fork exception or changes
  to released history.

## 0.3.0 - adopt upstream v1.36.0, with Cline, Ollama credit plans and Claude Desktop switching

Moves the upstream point from `v1.34.0` (`7eabc94f`) to `v1.36.0` (`22975d7f`),
81 upstream commits. Taken at the release tag rather than `main`, which was
one `CLAUDE.md` commit past it (`75d2d4a`). Nothing in it was urgent for this
fork, unlike 0.2.0; it keeps the next sync small. What it brings: Cline
(ClinePass) as a vendor, Ollama Cloud's credit-based plans through
`/api/balance`, switching and preparing Claude Desktop accounts from the macOS
tray, TUI tabs ordered by `[ui] overview_vendors`, the Cursor WAF headers, and
the placeholder-escaping fixes (Anthropic, OpenAI, Ollama).

Resolution notes, for whoever syncs next:

- Five conflicts, all ShvIA and Cline landing on the same line of a per-vendor
  list: `config.example.toml`, `src/catalog.rs`, `src/detect.rs`,
  `src/usage.rs`, `src/vendor.rs`. Both kept. In `catalog.rs` OpenCode Go left
  the key-only arm upstream (it now reads the OpenCode CLI's `auth.json`), so
  only ShvIA stays there. In `usage.rs` the Ollama doc comment is upstream's
  new one.
- Upstream did not touch `gnome-extension/` in this range, so our extension
  came across untouched. Our delta over `v1.36.0` is byte-identical to the one
  we had over `v1.34.0`, apart from the test below.
- `clicking_outside_the_picker_closes_it_without_selecting` (TUI) clicked the
  Novita row assuming the centred picker popup left it uncovered. One more
  vendor (ShvIA, on top of Cline) grows the popup by a row and puts it over
  Novita, so the click selected Cline. The test now clicks the first provider
  row clear of the popup's choice rows; it passes on pure `v1.36.0` as well.
- ShvIA's `{plan}` / `{shvia_plan}` stay unescaped: unlike the plans upstream
  started escaping, it comes from our own `[shvia] plan` config, not from the
  API.
- `CHANGELOG.md` is v1.36.0's, byte for byte.

## 0.2.0 - adopt upstream v1.34.0, whose Keychain fix stops the morning login hang

Moves the upstream point from `51f6b55a` to `v1.34.0` (`7eabc94f`), 507 upstream
commits. The reason it could not wait: up to 1.21.1 the upstream wrote a refreshed
Claude credential through Security.framework whenever the `security -i` line
passed 4000 bytes, which `mcpOAuth` makes the normal case (our two items are
12–16 KB). That write stamps the item with ai-usagebar's `cdhash:` partition, the
next `/usr/bin/security` read raises a Keychain dialog, and on 09/10 and 10/10 a
dialog raised overnight with the screen locked held `securityd`'s Keychain lock
until morning: the login window waited on it and the Mac had to be forced off.
v1.23.0 (`c69cc3c`) routes oversized blobs through `security(1)` argv instead.

Resolution notes, for whoever syncs next:

- `CHANGELOG.md` is v1.34.0's, byte for byte. The entries this fork had written
  under its `[Unreleased]` would have merged into published sections; they stay
  in our history (`84491e5`, `097a598`, `862f8ac`, `a50a4ba`).
- ShvIA gains the arms upstream's per-vendor tables now require, including
  `has_inline_secrets` — an inline `[shvia] api_key` had not triggered the
  config's `chmod 600`, which the upstream guard test caught — and a
  `macos_mirror` entry: `Generic`, because this line's macOS app has no ShvIA
  slot.
- The fork's old `Minimax` arm in `tui/app.rs` is dropped for upstream's
  account-aware one; `active.rs` now resolves through `VendorId::from_slug`.
- GNOME is ported onto upstream's rewritten extension (job slots, provider
  submenus, `_setPanelMarkup`). Kept: named accounts, several entries on the
  panel from one `usage --json`, "Status das APIs", the dark menu, pt-BR labels.
  Not ported: the split colour of a bar past its pace marker, since upstream's
  `barWidget` replaced the bars it was drawn on. `version-name` follows upstream
  (`0.3.0`): the version fields are theirs. Not yet run in a live GNOME Shell.

## 0.1.0 - docs/ci.md says where this fork's CI runs

The fleet's CI machine is open to every repository since 07/10/2026, this
one included. The rule arrives in a file of **our own** rather than in the
upstream's `CLAUDE.md` or `README.md`: a file they do not have never
conflicts on a sync, and their agent context is theirs.

This repository is **public**, so the page says the part that is not
optional: fork pull request approval has to be on before any job of this
repository runs on that machine. A public repository on a self-hosted
runner without it executes a stranger's pull request with effective root
inside the office network.

## 0.1.0 - the fork gets a version of its own, and the upstream point it sits on

Until now this repository had no version of ours at all. It is a fork we own and
modify, so the `X.Y.Z` of the upstream is theirs and there was nothing recording
what *we* had done to it, or from which point. Both questions matter the moment
the fork breaks: *which upstream point is this, and how far past it are we?*

This file answers both, in the shape [repodocs ADR-023][adr] prescribes: our
version first, the upstream point declared on its own line with the project and
the ref, and no edit to the upstream's own version fields.

At this point the fork carries **65 commits of our own** on top of that
upstream point, and none of them were recorded anywhere. This file starts
that record; it does not reconstruct them, because reconstructing entries
after the fact produces a plausible history rather than a true one.
