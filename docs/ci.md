# CI — the fleet's self-hosted runner

> **Status:** `ACTIVE` · Where this fork's CI runs. The machine's own runbook is
> the source and is **not** copied here.

This repository is a fork of [akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar) that we own and
modify. The fleet it belongs to has one CI machine.

| | |
|---|---|
| Machine | `cicd` — a self-hosted GitHub Actions runner, so jobs do not spend the account's hosted minutes |
| Address | `100.64.100.240` — the office network only. RFC 6598 shared space: not routable from the internet |
| Access | `ssh samir@100.64.100.240` |
| Dashboard | `http://100.64.100.240:8080/` — read-only, office network only. It shows the jobs; it is not how a repository joins |
| Source | [samirhvbr/repodocs → `docs/ci.md`](https://github.com/samirhvbr/repodocs/blob/master/docs/ci.md) — the full rules and the joining procedure |

**This fork may use it.** Since 07/10/2026 the runner is open to every repository
of the fleet, public ones included.

🔴 **And this repository is public, which makes one setting mandatory.** A pull
request from any fork runs its author's code on that machine, where jobs have
passwordless `sudo` and `docker` — effectively root inside the office network.
Before any job of this repository runs on `cicd`, turn on *Settings → Actions →
Fork pull request workflows → **Require approval for all external
contributors***. It is not a recommendation; it is what stands where a
prohibition used to.

**The switch is a repository variable, never an edit of the workflow.** A job
reads `runs-on: ${{ vars.CI_RUNNER || 'ubuntu-latest' }}`;
`gh variable set CI_RUNNER --body shvia-ci -R samirhvbr/ai-usagebar` sends the jobs to `cicd`,
and `gh variable delete CI_RUNNER -R samirhvbr/ai-usagebar` hands them back to GitHub. Nothing
moves until the variable exists.

**Registering the runner is the owner's act.** It needs a one-hour token and a
shell on the machine. An agent **never registers a runner on its own
initiative** — it says what is needed and asks.

**Nothing here belongs to the upstream.** This file is ours and has no
counterpart in [akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar), so it never conflicts on a
sync — which is why the fleet's rule arrives in a file of our own instead of in
the upstream's `CLAUDE.md` or `README.md`.
