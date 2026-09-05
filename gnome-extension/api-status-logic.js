// Pure logic for the dropdown's "Status das APIs" section — a row per vendor
// with a health state, including the vendors that are switched off or have no
// credential yet. The GNOME counterpart of the macOS menu bar panel of the
// same name.
//
// The figures come from one `ai-usagebar usage --json` sweep. The binary
// already walks every configured vendor and account there and reports each
// one's status, error and staleness, so nothing here needs to know how any
// vendor's cache payload is shaped — a previous version of this file decoded
// six of them by hand, and that is precisely what stopped matching when the
// Rust modules were reorganised.
//
// What the report cannot answer is the vendors it does not list: a disabled
// one, or one with no key. That comes from config.toml and the environment,
// which is why the TOML readers below stay.
//
// Everything in this module is IO-free: file contents, environment values and
// the report text are injected, so it runs under plain Node in
// api-status-logic.test.mjs. The Gio/GLib glue lives in extension.js.

// Vendors the section can show, in menu order. `creds` is a $HOME-relative
// credential file (its existence = logged in); `env` is the API-key variable;
// `login` is the hint shown when an oauth vendor has no credentials.
export const API_VENDORS = [
    {id: 'anthropic', name: 'Anthropic (Claude)', kind: 'oauth', creds: '.claude/.credentials.json', env: '', login: 'claude'},
    {id: 'anthropic_api', name: 'Anthropic (API)', kind: 'apikey', creds: '', env: 'ANTHROPIC_ADMIN_KEY', login: ''},
    {id: 'openai', name: 'OpenAI (Codex)', kind: 'oauth', creds: '.codex/auth.json', env: '', login: 'codex login'},
    {id: 'copilot', name: 'GitHub Copilot', kind: 'oauth', creds: '.config/gh/hosts.yml', env: 'GITHUB_COPILOT_TOKEN', login: 'gh auth login'},
    {id: 'zai', name: 'Z.AI (GLM)', kind: 'apikey', creds: '', env: 'ZAI_API_KEY', login: ''},
    {id: 'openrouter', name: 'OpenRouter', kind: 'apikey', creds: '', env: 'OPENROUTER_API_KEY', login: ''},
    {id: 'deepseek', name: 'DeepSeek', kind: 'apikey', creds: '', env: 'DEEPSEEK_API_KEY', login: ''},
    {id: 'kimi', name: 'Kimi', kind: 'apikey', creds: '', env: 'KIMI_API_KEY', login: ''},
    {id: 'kilo', name: 'Kilo', kind: 'apikey', creds: '', env: 'KILO_API_KEY', login: ''},
    {id: 'novita', name: 'Novita', kind: 'apikey', creds: '', env: 'NOVITA_API_KEY', login: ''},
    {id: 'moonshot', name: 'Moonshot', kind: 'apikey', creds: '', env: 'MOONSHOT_API_KEY', login: ''},
    {id: 'grok', name: 'Grok (xAI)', kind: 'apikey', creds: '', env: 'XAI_MANAGEMENT_KEY', login: ''},
    {id: 'minimax', name: 'MiniMax', kind: 'apikey', creds: '', env: 'MINIMAX_API_KEY', login: ''},
    {id: 'opencode-go', name: 'OpenCode Go', kind: 'apikey', creds: '', env: 'OPENCODE_GO_API_KEY', login: ''},
    {id: 'commandcode', name: 'Command Code', kind: 'oauth', creds: '.commandcode/auth.json', env: 'COMMANDCODE_API_KEY', login: 'commandcode'},
    {id: 'shvia', name: 'ShvIA', kind: 'apikey', creds: '', env: 'SHVIA_API_KEY', login: ''},
];

// Vendors the binary enables unless told otherwise. Everything else needs an
// explicit `enabled = true` — mirrors `Config::default` in src/config.rs.
const ENABLED_BY_DEFAULT = ['anthropic', 'openai', 'zai', 'openrouter'];

// A TOML section header may carry a trailing inline comment ("[zai] # note")
// and surrounding whitespace — the real TOML parser the binary uses accepts
// those, so compare only the header token, not the whole line.
export function tomlHeaderIs(line, section) {
    if (!line.startsWith('['))
        return false;
    const head = line.split('#', 1)[0] ?? line;
    return head.trim() === `[${section}]`;
}

// The per-vendor `api_key_env = "VAR"` override (resolve_api_key in the
// binary checks that variable first). Returns null when not set.
export function configApiKeyEnv(text, section) {
    if (!text)
        return null;
    let inSection = false;
    for (const raw of text.split('\n')) {
        const line = raw.trim();
        if (line.startsWith('[')) {
            inSection = tomlHeaderIs(line, section);
            continue;
        }
        if (inSection) {
            const m = /^api_key_env\s*=\s*["']([^"']+)["']/.exec(line);
            if (m)
                return m[1];
        }
    }
    return null;
}

// Whether config.toml's `[section]` sets a NON-EMPTY inline api_key — an
// empty `api_key = ""` is not configured, matching the binary.
export function configHasApiKey(text, section) {
    if (!text)
        return false;
    let inSection = false;
    for (const raw of text.split('\n')) {
        const line = raw.trim();
        if (line.startsWith('[')) {
            inSection = tomlHeaderIs(line, section);
        } else if (inSection && !line.startsWith('#') &&
                   /^api_key\s*=\s*["'][^"'\s]/.test(line)) {
            return true;
        }
    }
    return false;
}

// Mirrors the binary's config defaults: four core vendors are on unless the
// config says otherwise, and every key-authenticated vendor is opt-in.
export function configVendorEnabled(text, id) {
    const dflt = ENABLED_BY_DEFAULT.includes(id);
    if (!text)
        return dflt;
    let inSection = false;
    for (const raw of text.split('\n')) {
        const line = raw.trim();
        if (line.startsWith('[')) {
            inSection = tomlHeaderIs(line, id);
            continue;
        }
        // Anchor on `enabled =` exactly: a stale `enabledd`/`enabled_x` key is
        // ignored by the binary's TOML parser and must be ignored here too.
        if (inSection && /^enabled\s*=/.test(line)) {
            const val = line.slice(line.indexOf('=') + 1).trim().toLowerCase();
            if (val.startsWith('true'))
                return true;
            if (val.startsWith('false'))
                return false;
        }
    }
    return dflt;
}

// One bounded line: a provider error can be long and can carry newlines, and
// a menu row is neither.
export function oneLine(text, max = 52) {
    const flat = String(text ?? '').replace(/[\r\n\t]+/g, ' ').trim();
    return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

// Parse `ai-usagebar usage --json` into the fields this section shows, keyed
// by entry id (`<vendor>` or `<vendor>@<account>`). Anything unparseable is
// an empty map, never a throw: the section degrades to "sem dados" rather
// than taking the dropdown down with it.
export function parseUsageReport(text) {
    let parsed;
    try {
        parsed = JSON.parse(text);
    } catch (e) {
        return {};
    }
    const entries = parsed && Array.isArray(parsed.entries) ? parsed.entries : [];
    const out = {};
    for (const entry of entries) {
        if (!entry || typeof entry !== 'object' || typeof entry.id !== 'string')
            continue;
        const metrics = Array.isArray(entry.metrics) ? entry.metrics : [];
        const error = typeof entry.error === 'string' ? entry.error : '';
        if (out[entry.id] === undefined) {
            out[entry.id] = {
                id: entry.id,
                name: typeof entry.display_name === 'string' ? entry.display_name : entry.id,
                failed: error !== '' || entry.status === 'error',
                error,
                stale: entry.stale === true,
                metrics: metrics
                    .filter(m => m && typeof m === 'object')
                    .map(m => ({
                        label: typeof m.label === 'string' ? m.label : '',
                        percent: Number.isFinite(m.percent) ? m.percent : 0,
                        value: typeof m.value === 'string' ? m.value : '',
                    })),
            };
        }
    }
    return out;
}

// The one figure a row shows: the value of the most-consumed metric. It is
// the metric's own string, not a percentage assembled here — that is how a
// balance vendor reads "$12.34" and a quota vendor "78%" without this side
// keeping a table of which is which.
export function reportHeadline(entry) {
    if (!entry || !Array.isArray(entry.metrics) || entry.metrics.length === 0)
        return '';
    let best = entry.metrics[0];
    for (const m of entry.metrics) {
        if (m.percent > best.percent)
            best = m;
    }
    return best.value ?? '';
}

// What a vendor still needs before it can report anything.
export function missingCredentialHint(vendor) {
    if (vendor.kind === 'oauth' && vendor.login)
        return `não logado — ${vendor.login}`;
    if (vendor.env)
        return `sem API key — ${vendor.env}`;
    return 'não configurado';
}

// Derives a vendor's row from injected state — no IO here.
//   ctx = {
//     enabled, configured: booleans (config + env/creds checks, by the caller)
//     entry: the vendor's `usage --json` entry, or null/undefined
//     reportRan: whether a sweep has finished at all
//   }
// Returns {state: 'ok'|'warn'|'error'|'off', value, detail}.
export function rowStatus(vendor, ctx) {
    if (!ctx.enabled)
        return {state: 'off', value: '', detail: 'desativado'};
    if (!ctx.configured)
        return {state: 'warn', value: '', detail: missingCredentialHint(vendor)};
    const entry = ctx.entry;
    if (!entry) {
        return ctx.reportRan
            ? {state: 'warn', value: '', detail: 'sem dados'}
            : {state: 'warn', value: '', detail: '…'};
    }
    if (entry.failed)
        return {state: 'error', value: '', detail: oneLine(entry.error) || 'erro'};
    const value = reportHeadline(entry);
    return entry.stale
        ? {state: 'warn', value, detail: 'cache — a última atualização falhou'}
        : {state: 'ok', value, detail: ''};
}

// The whole section, in one call: the rows to draw, in menu order.
export function apiStatusRows({vendors = API_VENDORS, enabled, configured, report, reportRan}) {
    return vendors.map(v => ({
        id: v.id,
        name: v.name,
        ...rowStatus(v, {
            enabled: enabled(v),
            configured: configured(v),
            entry: report[v.id],
            reportRan,
        }),
    }));
}
