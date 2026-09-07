// Pure logic for the dropdown's "Status das APIs" section — a row per vendor
// with a health state, including the vendors that are switched off or have no
// credential yet. The GNOME counterpart of the macOS menu bar panel of the
// same name.
//
// Two binary calls feed it, and this module knows no provider of its own:
//
//   ai-usagebar vendors --json   every provider, how each authenticates, and
//                                whether it is enabled and credentialed
//   ai-usagebar usage --json     the figures, for the enabled ones
//
// The catalog call is why there is no vendor table here any more. There was
// one, and it drifted: it listed sixteen of the binary's twenty-one providers,
// so Antigravity, Cursor, Kiro, Nous Research and SuperGrok could never appear
// in a section whose entire purpose is to be complete. It also re-implemented
// `Config::default`'s enabled-by-default set and a TOML reader for
// `api_key`/`api_key_env`, which is provider knowledge a frontend has no way
// to keep correct. All of that now has one home, in Rust.
//
// Everything here is IO-free: both payloads are injected as text, so it runs
// under plain Node in api-status-logic.test.mjs. The Gio/GLib glue lives in
// extension.js.

// Parse `ai-usagebar vendors --json`. Anything unparseable is an empty list,
// never a throw — with no catalog the section draws nothing rather than
// taking the dropdown down with it.
export function parseVendorCatalog(text) {
    let parsed;
    try {
        parsed = JSON.parse(text);
    } catch (e) {
        return [];
    }
    const vendors = parsed && Array.isArray(parsed.vendors) ? parsed.vendors : [];
    const out = [];
    for (const v of vendors) {
        if (!v || typeof v !== 'object' || typeof v.id !== 'string' || v.id === '')
            continue;
        out.push({
            id: v.id,
            name: typeof v.name === 'string' && v.name !== '' ? v.name : v.id,
            kind: typeof v.kind === 'string' ? v.kind : '',
            enabled: v.enabled === true,
            configured: v.configured === true,
            // Absent means "has a credential", the case for every provider but
            // Antigravity: a frontend reading an older binary must not start
            // claiming providers need nothing.
            needsCredential: v.needs_credential !== false,
            env: typeof v.env === 'string' ? v.env : '',
            login: typeof v.login === 'string' ? v.login : '',
        });
    }
    return out;
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
        // `sections` is the lossless list; `metrics` is a view over its gauge
        // rows only. The labelled text rows are what is left, and are the only
        // headline a vendor with no ratio has — every ShvIA window on an
        // uncapped plan reports a used count and no percentage.
        const sections = Array.isArray(entry.sections) ? entry.sections : [];
        const texts = sections
            .filter(sec => sec && sec.type === 'text' && sec.label && sec.value)
            .map(sec => ({label: String(sec.label), value: String(sec.value)}));
        const error = typeof entry.error === 'string' ? entry.error : '';
        if (out[entry.id] === undefined) {
            out[entry.id] = {
                id: entry.id,
                name: typeof entry.display_name === 'string' ? entry.display_name : entry.id,
                failed: error !== '' || entry.status === 'error',
                error,
                stale: entry.stale === true,
                texts,
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
// With no gauge at all, the first labelled text row stands in: a blank cell
// beside a green dot says less than the count does.
export function reportHeadline(entry) {
    if (!entry)
        return '';
    const metrics = Array.isArray(entry.metrics) ? entry.metrics : [];
    if (metrics.length === 0) {
        const texts = Array.isArray(entry.texts) ? entry.texts : [];
        return texts.length > 0 ? texts[0].value ?? '' : '';
    }
    let best = metrics[0];
    for (const m of metrics) {
        if (m.percent > best.percent)
            best = m;
    }
    return best.value ?? '';
}

// What a vendor still needs before it can report anything. The command and the
// variable name both come from the catalog, so this says the same thing the
// binary's own credential errors do.
export function missingCredentialHint(vendor) {
    if (!vendor.needsCredential)
        return '';
    if (vendor.login)
        return `não logado — ${vendor.login}`;
    if (vendor.env)
        return `sem API key — ${vendor.env}`;
    if (vendor.kind === 'local')
        return 'não logado no app';
    return 'não configurado';
}

// Derives a vendor's row from injected state — no IO here.
//   vendor = one `parseVendorCatalog` row; it carries `enabled` and
//            `configured`, which the binary decided
//   ctx = {
//     entry: the vendor's `usage --json` entry, or null/undefined
//     reportRan: whether a sweep has finished at all
//   }
// Returns {state: 'ok'|'warn'|'error'|'off', value, detail}.
export function rowStatus(vendor, ctx) {
    if (!vendor.enabled)
        return {state: 'off', value: '', detail: 'desativado'};
    if (!vendor.configured)
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

// The whole section, in one call: the rows to draw, in the catalog's own order
// — which is the binary's canonical vendor order, so a provider added in Rust
// appears here with no change to this file.
export function apiStatusRows({vendors, report, reportRan}) {
    return (vendors ?? []).map(v => ({
        id: v.id,
        name: v.name,
        ...rowStatus(v, {entry: (report ?? {})[v.id], reportRan}),
    }));
}
