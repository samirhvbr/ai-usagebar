# Muse Code subscription quota

Muse Code is a native, opt-in provider. It reads the official CLI's existing
OAuth login and reports its subscription session and weekly used percentages,
reset times, and plan. The shared display settings can show remaining allowance.

After signing in with `muse login`, enable:

```toml
[muse-code]
enabled = true
# credentials_path = "~/.config/muse/auth.json"
```

```bash
ai-usagebar --vendor muse-code --pretty
ai-usagebar --vendor muse-code --json
ai-usagebar usage --json
ai-usagebar-tui
```

The report/catalog ID is `muse-code`; the short name is `msc`. The separate
names preserve compatibility with the older local-token `[[custom]]` recipe
whose ID/short name were `muse`/`mus`. No local counters are used by this provider.

The Omarchy bar and panel use Meta's infinity mark when brand icons are enabled.
The provider filter must be empty and **Show all providers** enabled to display
Muse alongside the other enabled providers. Selecting `muse-code` in that filter
limits both the bar and panel to Muse.

The login path is `credentials_path`, then `MUSE_AUTH_PATH`, then an absolute
`XDG_CONFIG_HOME` plus `muse/auth.json`, then `~/.config/muse/auth.json`.
The file must contain the CLI's `providers.meta` OAuth `access_token` (a `dca:`
device credential) and a pre-existing issued `api_key`. An inference/API key
alone cannot read subscription quota. ai-usagebar does not onboard, log in,
refresh a credential, rewrite the file, or use its configurable API origin.

This initial integration supports file-based logins. A macOS login that keeps
its tokens only in Keychain is not read; it reports a credential error. Linux
was tested live; macOS and Windows were not tested locally. Windows users can
set `credentials_path` to the file written by their Muse CLI.

## Missing data and request frequency

**Meta may omit all subscription usage while the five-hour window is idle,
even when weekly allowance has been consumed.** ai-usagebar reports quota as
unknown, never as 0% used or 100% remaining. If a previous observation exists
for the same login, it is shown with the usual stale indication. A window whose
reported reset has passed is omitted rather than projected into a new window.
If no unexpired observation remains, the provider reports unavailable.

The endpoint also returns the account's standing inference key. That field is
skipped during parsing and never cached or printed. Requests are bounded to
one per login every five minutes across processes sharing the vendor cache,
including failures, process interruptions, and forced refreshes. Within that
interval the last observation/error is replayed. A successful quota update
stores only plan/quota fields and a one-way login fingerprint. HTTP redirects
are rejected; error bodies and account fields are not displayed or persisted.

The provider remains opt-in even if a login file is detected. No browser
cookies or model calls are needed. A rejected login instructs you to sign in
again using the official CLI.

## Endpoint investigation

Investigated 2026-10-13. The published Model API does not document account quota
as a REST resource. The official Muse 1.4.0+ SDK does expose last-observed
subscription windows through MSP `usage/read` / `usage/changed`, but fresh
1.4.7 hosts returned no observation, including after `model/list` and a
memory-only `session/start` without a prompt. This cache-only method was not a
working refresh route on the test machine.
[Official protocol](https://meta-models.github.io/muse-code-sdk/next/generated/msp/methods/usage-read/),
[SDK usage schema](https://github.com/meta-models/muse-code-sdk/blob/a0c8611b382584eb5091942b477f30e92e1c8d18/schema/msp/msp.d.ts#L2535).

A direct request with an existing Muse OAuth login succeeded:

```http
POST https://api.meta.ai/muse-code/key
Authorization: Bearer <existing access_token>
x-api-version: 1.0.0
Content-Type: application/json

{"onboard": false}
```

The verified HTTP 200 response contained `is_subs_active: true`,
`subs_tier_name`, and `subs_usage.window` / `.weekly` with `used_percent` and
Unix-second `resets_at`; the session also had `window_duration_mins: 300`.
The returned key matched the existing saved key in an in-memory comparison;
no credential values were printed or written. The native CLI's constants and
these independent implementations corroborate the undocumented contract:
[quota-axi](https://github.com/kunchenguid/quota-axi/blob/5cb1c94c6c26166a35f4628eb59858b5e080729a/src/providers/muse.ts#L895),
[CodexBar](https://github.com/steipete/CodexBar/blob/910dc82e78e212f8904f3725a756ad03fa86c551/Sources/CodexBarCore/Resources/Plugins/muse.ts#L17).
First-use key issuance and rotation cadence are undocumented, which is why this
integration requires a standing CLI key and bounds every request.
[Empirical key comparison](https://github.com/kunchenguid/quota-axi/blob/5cb1c94c6c26166a35f4628eb59858b5e080729a/README.md#L1080),
[idle-window omission report](https://github.com/steipete/CodexBar/issues/4002).

## Omarchy logo investigation

Investigated 2026-10-14. Omarchy owns its SVG artwork in `omarchy/icons/` and
resolves the report's `brand` (or provider ID for older reports) in
`Model.brandIconFile`. An unregistered brand falls back to its short name, which
is why Muse originally appeared as `msc` despite brand icons being enabled.

No separate Muse Code product mark was found in the
[official Muse Code docs](https://dev.meta.ai/docs/muse-code) or the
[Lobe Icons source tree](https://github.com/lobehub/lobe-icons/tree/05a9b8c41e4735a4a2b6d2f1e43574bd8ec6c8cb/src).
The integration uses the parent-company infinity mark described in
[Meta's company brand resources](https://www.meta.com/brand/resources/meta/company-brand/).
Its path comes from [Lobe Icons' Meta SVG](https://github.com/lobehub/lobe-icons/blob/05a9b8c41e4735a4a2b6d2f1e43574bd8ec6c8cb/packages/static-svg/icons/meta.svg)
under [MIT](https://github.com/lobehub/lobe-icons/blob/05a9b8c41e4735a4a2b6d2f1e43574bd8ec6c8cb/LICENSE),
with the existing LobeHub notice retained. The path is unchanged; a white fill
and fixed 24px dimensions follow the plugin's theme-tinting and Qt contracts.
The legacy custom `muse` recipe gains no brand unless explicitly configured
with `brand = "muse-code"`.
