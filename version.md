# Version — samirhvbr fork of ai-usagebar

**Current version:** `0.1.0`
**Upstream:** [akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar) @ `51f6b55a` (`main`) — 65 commits of ours past it

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
