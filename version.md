# Version — samirhvbr fork of ai-usagebar

**Current version:** `0.2.0`
**Upstream:** [akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar) @ `7eabc94f` (`v1.34.0`) — 67 commits of ours past it

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
