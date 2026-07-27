// Pure logic for the dropdown's "Status das APIs" section — the GNOME port of
// the macOS menu bar panel of the same name. A row per configured vendor with
// a health state derived ENTIRELY from local state (config.toml, credential
// files, and the binary's per-vendor on-disk cache) — no network calls; the
// section mirrors the last fetch the binary wrote.
//
// Everything in this module is IO-free: file contents, env values and ages are
// injected, so it runs under plain Node in api-status-logic.test.mjs. The
// Gio/GLib glue (reading the actual files) lives in extension.js.

// Vendors the section can show. `creds` is a $HOME-relative OAuth credential
// file (its existence = logged in); `env` is the API-key variable; `login` is
// the hint shown when an oauth vendor has no credentials. Mirrors the macOS
// panel's VENDOR_AUTH, plus ShvIA (quota vendor; enabled by default in the
// Rust config, so a configured gateway shows its health here too).
export const API_VENDORS = [
    {id: 'anthropic', name: 'Anthropic (Claude)', kind: 'oauth', creds: '.claude/.credentials.json', env: '', login: 'claude'},
    {id: 'anthropic_api', name: 'Anthropic (API)', kind: 'apikey', creds: '', env: 'ANTHROPIC_ADMIN_KEY', login: ''},
    {id: 'openai', name: 'OpenAI (Codex)', kind: 'oauth', creds: '.codex/auth.json', env: '', login: 'codex login'},
    {id: 'zai', name: 'Z.AI (GLM)', kind: 'apikey', creds: '', env: 'ZAI_API_KEY', login: ''},
    {id: 'openrouter', name: 'OpenRouter', kind: 'apikey', creds: '', env: 'OPENROUTER_API_KEY', login: ''},
    {id: 'deepseek', name: 'DeepSeek', kind: 'apikey', creds: '', env: 'DEEPSEEK_API_KEY', login: ''},
    {id: 'kilo', name: 'Kilo', kind: 'apikey', creds: '', env: 'KILO_API_KEY', login: ''},
    {id: 'novita', name: 'Novita', kind: 'apikey', creds: '', env: 'NOVITA_API_KEY', login: ''},
    {id: 'moonshot', name: 'Kimi (Moonshot)', kind: 'apikey', creds: '', env: 'MOONSHOT_API_KEY', login: ''},
    {id: 'grok', name: 'Grok (xAI)', kind: 'apikey', creds: '', env: 'XAI_MANAGEMENT_KEY', login: ''},
    {id: 'shvia', name: 'ShvIA', kind: 'apikey', creds: '', env: 'SHVIA_API_KEY', login: ''},
    {id: 'minimax', name: 'MiniMax', kind: 'apikey', creds: '', env: 'MINIMAX_API_KEY', login: ''},
];

// A TOML section header may carry a trailing inline comment ("[zai] # note")
// and surrounding whitespace — the real TOML parser the binary uses accepts
// those, so compare only the header token, not the whole line.
export function tomlHeaderIs(line, section) {
    if (!line.startsWith('['))
        return false;
    const head = line.split('#', 1)[0] ?? line;
    return head.trim() === `[${section}]`;
}

// Extracts the snapshot object from a parsed usage.json. Every vendor wraps
// its cache repr in `{"snapshot": …}` — except DeepSeek, whose cache is
// written FLAT (src/deepseek/fetch.rs snap_to_json; its own parse_cache reads
// top-level keys). The macOS panel misses this and silently never shows the
// DeepSeek balance; the port fixes it.
export function extractSnapshot(vendorId, parsed) {
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed))
        return null;
    if (vendorId === 'deepseek')
        return parsed;
    const snap = parsed.snapshot;
    return snap && typeof snap === 'object' && !Array.isArray(snap) ? snap : null;
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

// Mirrors the binary's config defaults: every vendor is enabled unless the
// config's `[vendor] enabled = false`, except the opt-in vendors (explicit
// key or Admin key required), which default to disabled.
export function configVendorEnabled(text, id) {
    const dflt = !['anthropic_api', 'deepseek', 'kilo', 'novita', 'moonshot', 'grok', 'minimax']
        .includes(id);
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

// The monthly spend limit is config-authoritative (the API never exposes it),
// so it is re-read on every render — mirroring how the binary re-applies it —
// instead of trusting the value frozen in the cache payload.
export function configMonthlyLimit(text, id) {
    if (!text)
        return null;
    let inSection = false;
    for (const raw of text.split('\n')) {
        const line = raw.trim();
        if (line.startsWith('[')) {
            inSection = tomlHeaderIs(line, id);
            continue;
        }
        if (inSection && /^monthly_limit\s*=/.test(line)) {
            let v = line.slice(line.indexOf('=') + 1);
            const hash = v.indexOf('#');
            if (hash >= 0)
                v = v.slice(0, hash); // strip inline comment
            // TOML allows digit-group underscores (1_000) — the binary's
            // parser accepts them, so strip before converting.
            const n = Number(v.trim().replace(/_/g, ''));
            return Number.isFinite(n) ? n : null;
        }
    }
    return null;
}

export function fmtAge(seconds) {
    const sec = Math.max(0, Math.trunc(seconds));
    if (sec < 60)
        return `${sec}s`;
    if (sec < 3600)
        return `${Math.trunc(sec / 60)}m`;
    if (sec < 86400)
        return `${Math.trunc(sec / 3600)}h`;
    return `${Math.trunc(sec / 86400)}d`;
}

// A compact, referenced error for a menu row — never the raw response body
// (which can be long JSON). The full text stays in the vendor's `.last_error`.
export function shortHttpError(code) {
    if (code === 401 || code === 403)
        return `HTTP ${code} · chave inválida`;
    if (code === 404)
        return 'HTTP 404 · não encontrado';
    if (code === 429)
        return 'HTTP 429 · limite atingido';
    if (code >= 500 && code <= 599)
        return `HTTP ${code} · erro do servidor`;
    if (code === 0)
        return 'sem conexão';
    return `HTTP ${code}`;
}

// `.last_error` is `code\nmessage`; present only when the last fetch failed
// (a successful cache write removes it).
export function parseLastError(raw) {
    if (raw == null)
        return null;
    const lines = String(raw).split('\n');
    const code = Number.parseInt((lines[0] ?? '').trim(), 10);
    if (!Number.isFinite(code))
        return null;
    return {code, msg: lines.length > 1 ? lines[1] : ''};
}

function money(v, cur) {
    return cur === 'CNY' ? `¥${v.toFixed(2)}` : `$${v.toFixed(2)}`;
}

function num(v) {
    return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

// Formatted headline VALUE from the cached `usage.json` snapshot for the
// balance-style vendors (remaining credit, in the right currency) and for
// anthropic_api (month-to-date spend, optionally vs the config limit).
// Returns null for the quota vendors (anthropic/openai/zai/shvia), whose
// cache is the raw API response — those show usage% from the live snapshot
// instead. Field names mirror each vendor's cache repr in src/<vendor>/fetch.rs.
export function balanceDisplay(vendorId, snap, configText) {
    if (!snap || typeof snap !== 'object')
        return null;
    switch (vendorId) {
    case 'anthropic_api': {
        const spent = num(snap.spent);
        if (spent == null)
            return null;
        const limit = configMonthlyLimit(configText, 'anthropic_api') ?? num(snap.limit);
        if (limit != null && limit > 0) {
            const pct = Math.round((spent / limit) * 100);
            return `$${spent.toFixed(2)} / $${limit.toFixed(0)} · ${pct}%`;
        }
        return `$${spent.toFixed(2)}/mo`;
    }
    case 'kilo':
    case 'grok': {
        const b = num(snap.balance);
        return b == null ? null : money(b, 'USD');
    }
    case 'novita': {
        const a = num(snap.available);
        return a == null ? null : money(a, 'USD');
    }
    case 'moonshot': {
        const a = num(snap.available);
        return a == null ? null : money(a, typeof snap.currency === 'string' ? snap.currency : 'USD');
    }
    case 'deepseek': {
        const b = num(snap.balance);
        return b == null ? null : money(b, typeof snap.currency === 'string' ? snap.currency : 'USD');
    }
    case 'openrouter': {
        const tc = num(snap.total_credits);
        const tu = num(snap.total_usage);
        if (tc == null || tu == null)
            return null;
        return money(Math.max(0, tc - tu), 'USD');
    }
    default:
        return null;
    }
}

// Window label from its length in seconds — `4h`, `5h`, `7d`. MiniMax's short
// window is not a fixed 5h (it has been observed at both 4h and 5h, and the
// video pool rolls daily), so the label is derived rather than assumed.
function windowLabel(secs, fallback) {
    if (!Number.isFinite(secs) || secs <= 0)
        return fallback;
    return secs < 86400
        ? `${Math.round(secs / 3600)}h`
        : `${Math.round(secs / 86400)}d`;
}

// Headline for a QUOTA vendor whose cache we write ourselves and can therefore
// read back: the two windows' consumed percentages. Without this a configured
// quota vendor that isn't the active one falls through to a bare "OK", hiding
// the number the panel exists to show. Returns null for vendors whose cache is
// the raw upstream response (anthropic/openai/zai), which this does not parse.
export function quotaDisplay(vendorId, snap) {
    if (vendorId !== 'minimax' || !snap || typeof snap !== 'object')
        return null;
    const win = (w) => (w && typeof w === 'object' && Number.isFinite(w.pct) ? w : null);
    const session = win(snap.session);
    const weekly = win(snap.weekly);
    if (!session || !weekly)
        return null;
    const s = windowLabel(session.window_secs, '5h');
    const w = windowLabel(weekly.window_secs, '7d');
    return `${s} ${session.pct}% · ${w} ${weekly.pct}%`;
}

// Derives a vendor's row purely from injected local state — no IO here.
//   ctx = {
//     enabled, configured: booleans (config + env/creds checks, done by caller)
//     lastError: {code, msg} | null        (parsed .last_error)
//     ageSecs: number | null               (usage.json mtime age; null = never fetched)
//     snap: object | null                  (usage.json's `snapshot`, if parseable)
//     configText: string | null            (config.toml contents)
//     activePcts: {session, weekly} | null (live pcts when this vendor is the selected one)
//   }
// Returns {state: 'ok'|'warn'|'error'|'off', detail, age}.
export function rowStatus(vendor, ctx) {
    if (!ctx.enabled)
        return {state: 'off', detail: 'desativado', age: ''};
    if (!ctx.configured) {
        const hint = vendor.kind === 'oauth'
            ? `não logado — ${vendor.login}`
            : `sem API key — ${vendor.env}`;
        return {state: 'warn', detail: hint, age: ''};
    }
    if (ctx.lastError) {
        return {
            state: 'error',
            detail: shortHttpError(ctx.lastError.code),
            age: ctx.ageSecs != null ? fmtAge(ctx.ageSecs) : '',
        };
    }
    if (ctx.ageSecs != null) {
        const bal = balanceDisplay(vendor.id, ctx.snap, ctx.configText);
        if (bal != null)
            return {state: 'ok', detail: bal, age: fmtAge(ctx.ageSecs)};
        // A quota vendor we can read back from cache shows its windows even
        // when it is not the selected vendor.
        const quota = quotaDisplay(vendor.id, ctx.snap);
        if (quota != null)
            return {state: 'ok', detail: quota, age: fmtAge(ctx.ageSecs)};
        if (ctx.activePcts)
            return {state: 'ok', detail: `5h ${ctx.activePcts.session}% · 7d ${ctx.activePcts.weekly}%`, age: fmtAge(ctx.ageSecs)};
        return {state: 'ok', detail: 'OK', age: fmtAge(ctx.ageSecs)};
    }
    return {state: 'warn', detail: 'sem dados — use “Verificar todas”', age: ''};
}
