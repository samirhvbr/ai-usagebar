// Pure marker helpers shared by the GNOME indicator and its Node table tests.
export const MARKER = '#61afef';
export const POINT_MID_MIN = -10;
export const POINT_CRITICAL_MIN = 10;
// Keep the stale suffix after this ignored literal field, never on elapsed.
// New fields are appended before the sentinel so existing indices never shift:
// an older binary simply echoes the unknown placeholders back and field()
// discards them.
export const FORMAT = '{plan};;{session_pct};;{session_reset};;{weekly_pct};;{weekly_reset};;' +
    '{sonnet_pct};;{sonnet_reset};;{extra_pct};;{extra_spent};;{extra_limit};;' +
    '{scoped_model};;{scoped_pct};;{scoped_reset};;' +
    '{session_elapsed};;{weekly_elapsed};;{scoped_elapsed};;{vendor_short};;' +
    '{extra_model};;{extra_reset};;{extra_elapsed};;' +
    '{session_model};;{weekly_model};;__aiub_end__';
export const FIELD = Object.freeze({
    plan: 0, sessionPct: 1, sessionReset: 2, weeklyPct: 3, weeklyReset: 4,
    sonnetPct: 5, sonnetReset: 6, extraPct: 7, extraSpent: 8, extraLimit: 9,
    scopedModel: 10, scopedPct: 11, scopedReset: 12,
    sessionElapsed: 13, weeklyElapsed: 14, scopedElapsed: 15, vendorShort: 16,
    extraModel: 17, extraReset: 18, extraElapsed: 19,
    sessionModel: 20, weeklyModel: 21,
    sentinel: 22,
});

// A vendor that names its primary rows is telling us its windows come in two
// independent pools, so the dropdown groups them under Session/Weekly headings
// instead of listing four flat rows. Data-driven on purpose: no vendor ids here.
export function isGrouped(sessionModel) {
    return field(sessionModel) !== '';
}

export function splitFormatOutput(text) {
    return String(text).split(';;');
}

// The CLI output is Pango markup. Strip only tags first, then decode one layer
// of XML entities so API labels escaped by Rust display as literal text rather
// than "&amp;". Decoding after tag removal cannot reintroduce active markup.
export function plainTextFromPango(value) {
    return String(value ?? '')
        .replace(/<[^>]*>/g, '')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&apos;/g, "'")
        .replace(/&amp;/g, '&');
}

export function field(value) {
    const text = String(value ?? '').trim();
    return text && !/^\{[^}]+\}$/.test(text) ? text : '';
}

// Do not accept a numeric prefix: a stale suffix such as "27 ⏸" is not elapsed.
export function integer(value) {
    const text = field(value);
    return /^-?\d+$/.test(text) ? Number(text) : null;
}

export function markerElapsed(reset, elapsed) {
    return reset && reset !== '—' && Number.isFinite(elapsed) ? elapsed : null;
}

// Balance-only vendors do not expose generic rolling quota windows. Keep this
// vendor-aware at the native surface so their compatibility aliases cannot
// turn into confident 0% bars.
export function hasUsageWindows(vendorShort) {
    return field(vendorShort) !== 'dsk';
}

function poolChars(model) {
    return Array.from(field(model)).filter(ch => /[\p{L}\p{N}]/u.test(ch));
}

// Short panel tag for a quota pool: the model group's initial. Work in Unicode
// code points rather than UTF-16 code units so a non-BMP letter is never split
// into an invalid surrogate. The panel is width-constrained, so the full name
// only lives in the dropdown.
export function poolTag(model) {
    const chars = poolChars(model);
    return chars.length ? chars[0].toUpperCase() : '';
}

// Two pools whose names share an initial would produce identical tags, so widen
// both until they differ. The names come from the binary and can change, which
// is exactly when a silent collision would be hardest to notice.
export function disambiguateTags(a, b) {
    const [ca, cb] = [poolChars(a), poolChars(b)];
    if (!ca.length || !cb.length)
        return [poolTag(a), poolTag(b)];
    for (let n = 1; n <= Math.max(ca.length, cb.length); n++) {
        const [ta, tb] = [ca.slice(0, n).join('').toUpperCase(), cb.slice(0, n).join('').toUpperCase()];
        if (ta !== tb)
            return [ta, tb];
    }
    // Identical names: nothing distinguishes them, so keep the plain initials.
    return [poolTag(a), poolTag(b)];
}

export function poolAvailable(pool) {
    return Number.isFinite(pool?.session) || Number.isFinite(pool?.weekly);
}

// Which pool the panel shows in "auto" mode. A pool counts as spent when *any*
// of its windows crosses the threshold — switching on the 5h alone would strand
// the user on a pool whose weekly is the one that ran out. Only switch if the
// other pool still has room; with both spent, stay put rather than flapping.
export function pickPool(primary, secondary, threshold) {
    // An unavailable secondary is not a pristine 0%-used pool. Treating it as
    // one would switch the panel to an empty label exactly when the primary
    // reaches the warning threshold.
    if (!poolAvailable(primary))
        return poolAvailable(secondary) ? 'secondary' : 'primary';
    if (!poolAvailable(secondary))
        return 'primary';
    const spent = w => Math.max(
        Number.isFinite(w.session) ? w.session : 0,
        Number.isFinite(w.weekly) ? w.weekly : 0) >= threshold;
    return spent(primary) && !spent(secondary) ? 'secondary' : 'primary';
}

// Resolve every panel-pools mode while filtering unavailable pools. Keeping
// this policy pure makes the partial-payload cases testable outside GNOME.
export function selectPools(primary, secondary, mode, threshold,
    windows = {session: true, weekly: true}) {
    const visible = pool => ({
        session: windows.session ? pool?.session : null,
        weekly: windows.weekly ? pool?.weekly : null,
    });
    const available = {
        primary: poolAvailable(visible(primary)),
        secondary: poolAvailable(visible(secondary)),
    };
    if (mode === 'primary')
        return available.primary ? ['primary'] : (available.secondary ? ['secondary'] : []);
    if (mode === 'secondary')
        return available.secondary ? ['secondary'] : (available.primary ? ['primary'] : []);
    if (mode === 'auto') {
        const selected = pickPool(primary, secondary, threshold);
        if (available[selected])
            return [selected];
        const fallback = selected === 'primary' ? 'secondary' : 'primary';
        return available[fallback] ? [fallback] : [];
    }
    return ['primary', 'secondary'].filter(name => available[name]);
}

// Matches pacing::pace_severity: < -10 low, -10..=0 mid, 1..=9 high, >= 10 critical.
export function colorForDelta(delta, colors) {
    if (delta >= POINT_CRITICAL_MIN)
        return colors.critical;
    if (delta > 0)
        return colors.high;
    if (delta >= POINT_MID_MIN)
        return colors.mid;
    return colors.low;
}

export function colorForPct(pct, colors) {
    if (pct >= 90)
        return colors.critical;
    if (pct >= 75)
        return colors.high;
    if (pct >= 50)
        return colors.mid;
    return colors.low;
}

export function barMarkup(pct, width, colors, elapsed) {
    const p = Math.max(0, Math.min(100, Math.round(pct)));
    const filled = Math.round((p * width) / 100);

    if (!Number.isFinite(elapsed)) {
        return `<span foreground="${colorForPct(p, colors)}">${'█'.repeat(filled)}</span>` +
            `<span foreground="${colors.empty}">${'░'.repeat(width - filled)}</span>`;
    }

    const e = Math.max(0, Math.min(100, Math.round(elapsed)));
    const base = colorForPct(p, colors);
    const over = colorForDelta(p - e, colors);
    let marker = Math.floor((e * width) / 100);
    if (marker > width - 1)
        marker = width - 1;
    const preFilled = Math.min(filled, marker);
    const postFilled = filled > marker + 1 ? filled - marker - 1 : 0;
    const preEmpty = marker - preFilled;
    const postEmpty = width - marker - 1 - postFilled;
    return `<span foreground="${base}">${'█'.repeat(preFilled)}</span>` +
        `<span foreground="${colors.empty}">${'░'.repeat(preEmpty)}</span>` +
        `<span foreground="${MARKER}">│</span>` +
        `<span foreground="${over}">${'█'.repeat(postFilled)}</span>` +
        `<span foreground="${colors.empty}">${'░'.repeat(postEmpty)}</span>`;
}

// ── Multi-entry panel ─────────────────────────────────────────────────────
// The panel above shows one vendor at a time, driven by `--format`. A machine
// with two Claude subscriptions and a Codex wants all three weeklies side by
// side instead, which is what the aggregate `usage --json` report already
// carries — one entry per vendor *and per named account*. These helpers turn
// that report into panel segments; the report stays the authority on labels,
// windows, order and severity, exactly as it is for the KDE and Omarchy
// frontends.

// The two windows the panel's show-session / show-weekly settings name. They
// are the window *lengths* Rust reports, so no vendor id appears here: any
// provider with a 5h or 7d window is matched by the same rule.
export const SESSION_SECS = 18000;
export const WEEKLY_SECS = 604800;

// A short tag for a window length, for the provider whose windows are neither
// of the two above (a calendar-month quota, say) and would otherwise have no
// segment at all. Whole days and hours only — the report has no window that is
// not a round number of either.
export function windowTag(secs) {
    const n = Number(secs);
    if (!Number.isFinite(n) || n <= 0)
        return '';
    if (n % 86400 === 0)
        return `${n / 86400}d`;
    if (n % 3600 === 0)
        return `${n / 3600}h`;
    return `${Math.round(n / 60)}m`;
}

// How a segment names its entry: the account label when the id carries one
// (`anthropic@claude-b3` → `claude-b3`, the name the user chose in
// config.toml), otherwise the report's own short name (`gpt`). Never a table
// of vendor ids — that is what `short_name` is in the report for.
export function entryTag(entry) {
    const id = String(entry?.id ?? '');
    const at = id.indexOf('@');
    if (at > 0 && at < id.length - 1)
        return id.slice(at + 1);
    return entry?.short_name || id;
}

// One metric per enabled window, in `windows` order, first match wins: a
// Claude account reports "Weekly (7d)" before its "Fable (7d)", and the panel
// wants the account's weekly, not both. An entry with no metric in any enabled
// window falls back to its first metric, tagged with that metric's own window,
// so selecting a monthly-quota provider shows a figure instead of nothing.
export function entryMetrics(entry, windows) {
    const metrics = Array.isArray(entry?.metrics) ? entry.metrics : [];
    const wanted = [];
    if (windows?.session)
        wanted.push(SESSION_SECS);
    if (windows?.weekly)
        wanted.push(WEEKLY_SECS);
    const picked = [];
    for (const secs of wanted) {
        const metric = metrics.find(m => Number(m?.window_secs) === secs);
        if (metric)
            picked.push(metric);
    }
    if (picked.length)
        return picked;
    return metrics.length && wanted.length ? [metrics[0]] : [];
}

// The panel's segments for the selected entry ids, in the order the user
// selected them. Every selected id yields exactly one status:
//
//   'ready'   a figure to draw (`pct`, `value`, `tag`, `window`)
//   'error'   the entry reported a failure — its own text, not a guess
//   'absent'  selected but not in the report: switched off in config.toml, or
//             a label that no longer exists. Shown, muted, rather than
//             silently dropped, which is indistinguishable from a bug.
//
// `stale` rides along so the renderer can mark a figure the binary served from
// cache. Pacing markers do not: the report states elapsed only inside prose
// (`detail`), and deriving a number from prose in a frontend is exactly what
// this project keeps in Rust.
export function panelSegments(report, ids, windows) {
    const entries = Array.isArray(report?.entries) ? report.entries : [];
    const out = [];
    for (const id of ids ?? []) {
        const entry = entries.find(e => e?.id === id);
        if (!entry) {
            out.push({id, tag: entryTag({id}), status: 'absent'});
            continue;
        }
        const tag = entryTag(entry);
        if (entry.status === 'error' || (!entry.metrics?.length && entry.error)) {
            out.push({id, tag, status: 'error', error: entry.error || ''});
            continue;
        }
        for (const metric of entryMetrics(entry, windows)) {
            // A report number or nothing. `Number(null)` is 0, and a missing
            // percentage drawn as a confident 0% bar is the one reading this
            // project refuses everywhere else.
            const raw = metric?.percent;
            const pct = typeof raw === 'number' && Number.isFinite(raw)
                ? Math.max(0, Math.min(100, Math.round(raw)))
                : null;
            out.push({
                id, tag, status: 'ready',
                window: windowTag(metric?.window_secs),
                pct,
                value: String(metric?.value ?? ''),
                label: String(metric?.label ?? ''),
                stale: !!entry.stale,
            });
        }
    }
    return out;
}
