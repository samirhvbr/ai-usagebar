import assert from 'node:assert/strict';
import {barMarkup, colorForDelta, commonLabelPrefix, disambiguateTags, entryLabel, entryMetrics,
    entryTag, field, FIELD, FORMAT, hasUsageWindows, integer, isGrouped, markerElapsed,
    panelSegments, pickPool, plainTextFromPango, poolAvailable, poolTag, selectPools,
    SESSION_SECS, splitFormatOutput, WEEKLY_SECS, windowTag} from './marker-logic.js';

const colors = {low: 'low', mid: 'mid', high: 'high', critical: 'critical', empty: 'empty'};
const visibleCells = markup => markup.replace(/<[^>]+>/g, '');

assert.equal(markerElapsed('1h', 0), 0);
assert.equal(markerElapsed('1h', 100), 100);
assert.equal(markerElapsed('', 0), null);
assert.equal(markerElapsed('—', 0), null);
assert.equal(markerElapsed('1h', null), null);

assert.equal(integer('27'), 27);
assert.equal(integer(''), null);
assert.equal(integer('27 ⏸'), null);
assert.equal(integer('{scoped_elapsed}'), null);
assert.equal(field('{scoped_model}'), '');
assert.equal(plainTextFromPango('<span>A &amp; B &lt;b&gt;</span>'), 'A & B <b>');
assert.equal(plainTextFromPango('&amp;lt;literal&amp;gt;'), '&lt;literal&gt;');
assert.equal(hasUsageWindows('dsk'), false);
assert.equal(hasUsageWindows('gpt'), true);
assert.equal(hasUsageWindows('agy'), true);
assert.equal(hasUsageWindows('{vendor_short}'), true); // older binary compatibility
const formatFields = FORMAT.split(';;');
assert.deepEqual(formatFields, [
    '{plan}', '{session_pct}', '{session_reset}', '{weekly_pct}', '{weekly_reset}',
    '{sonnet_pct}', '{sonnet_reset}', '{extra_pct}', '{extra_spent}', '{extra_limit}',
    '{scoped_model}', '{scoped_pct}', '{scoped_reset}', '{session_elapsed}',
    '{weekly_elapsed}', '{scoped_elapsed}', '{vendor_short}',
    '{extra_model}', '{extra_reset}', '{extra_elapsed}',
    '{session_model}', '{weekly_model}', '__aiub_end__',
]);
assert.deepEqual(FIELD, {
    plan: 0, sessionPct: 1, sessionReset: 2, weeklyPct: 3, weeklyReset: 4,
    sonnetPct: 5, sonnetReset: 6, extraPct: 7, extraSpent: 8, extraLimit: 9,
    scopedModel: 10, scopedPct: 11, scopedReset: 12,
    sessionElapsed: 13, weeklyElapsed: 14, scopedElapsed: 15, vendorShort: 16,
    extraModel: 17, extraReset: 18, extraElapsed: 19,
    sessionModel: 20, weeklyModel: 21,
    sentinel: 22,
});
// Appended fields must not disturb the indices an older binary already fills.
assert.equal(FIELD.vendorShort, 16);
// An older binary echoes unknown placeholders back; field() must discard them
// so the extra row falls back to its spent/limit money-budget shape.
assert.equal(field('{extra_model}'), '');
assert.equal(field('{extra_reset}'), '');
// Grouped layout is opted into by data, never by vendor id. An older binary
// echoes the placeholder back, which must read as "not grouped".
assert.equal(isGrouped('Gemini'), true);
assert.equal(isGrouped('{session_model}'), false);
assert.equal(isGrouped(''), false);
assert.equal(isGrouped(undefined), false);

// Panel pool tags.
assert.equal(poolTag('Gemini'), 'G');
assert.equal(poolTag('Claude & GPT OSS'), 'C');
assert.equal(poolTag(''), '');
assert.equal(poolTag('{session_model}'), ''); // older binary echoes it back
assert.equal(poolTag(undefined), '');
// Leading punctuation must not become the tag.
assert.equal(poolTag('  &claude'), 'C');
const astralTag = poolTag('𐐨elta');
assert.equal(Array.from(astralTag).length, 1); // never split a non-BMP letter

assert.deepEqual(disambiguateTags('Gemini', 'Claude & GPT OSS'), ['G', 'C']);
// Shared initial widens both tags until they differ.
assert.deepEqual(disambiguateTags('Gemini', 'GPT OSS'), ['GE', 'GP']);
assert.deepEqual(disambiguateTags('Gemini Pro', 'Gemini Flash'), ['GEMINIP', 'GEMINIF']);
// Nothing to disambiguate against.
assert.deepEqual(disambiguateTags('Gemini', ''), ['G', '']);

assert.equal(poolAvailable({session: 0, weekly: null}), true);
assert.equal(poolAvailable({session: null, weekly: 66}), true);
assert.equal(poolAvailable({session: null, weekly: null}), false);
assert.equal(poolAvailable(undefined), false);

// Auto pool selection. A pool is spent when either window crosses the threshold.
const free = {session: 10, weekly: 10};
const spent5h = {session: 99, weekly: 10};
const spentWeekly = {session: 10, weekly: 99};
assert.equal(pickPool(free, free, 95), 'primary');
assert.equal(pickPool(spent5h, free, 95), 'secondary');
// The weekly running out must switch too, not just the 5h.
assert.equal(pickPool(spentWeekly, free, 95), 'secondary');
// Both spent → stay on the preferred pool instead of flapping.
assert.equal(pickPool(spent5h, spent5h, 95), 'primary');
// Never switch away from a healthy primary.
assert.equal(pickPool(free, spent5h, 95), 'primary');
// The threshold itself counts as spent.
assert.equal(pickPool({session: 95, weekly: 0}, free, 95), 'secondary');
assert.equal(pickPool({session: 94, weekly: 0}, free, 95), 'primary');
// A missing secondary pool is unavailable, not a pristine 0%-used fallback.
assert.equal(pickPool(free, undefined, 95), 'primary');
assert.equal(pickPool(spent5h, undefined, 95), 'primary');
assert.equal(pickPool(undefined, free, 95), 'secondary');
assert.deepEqual(selectPools(free, free, 'both', 95), ['primary', 'secondary']);
assert.deepEqual(selectPools(free, undefined, 'both', 95), ['primary']);
assert.deepEqual(selectPools(free, undefined, 'secondary', 95), ['primary']);
assert.deepEqual(selectPools(undefined, free, 'primary', 95), ['secondary']);
assert.deepEqual(selectPools(spent5h, free, 'auto', 95), ['secondary']);
assert.deepEqual(selectPools(spent5h, undefined, 'auto', 95), ['primary']);
assert.deepEqual(selectPools(undefined, undefined, 'auto', 95), []);
const onlySession = {session: 10, weekly: null};
const onlyWeekly = {session: null, weekly: 10};
assert.deepEqual(selectPools(onlySession, onlyWeekly, 'both', 95,
    {session: true, weekly: false}), ['primary']);
assert.deepEqual(selectPools(onlySession, onlyWeekly, 'secondary', 95,
    {session: true, weekly: false}), ['primary']);
assert.deepEqual(selectPools(spent5h, onlyWeekly, 'auto', 95,
    {session: true, weekly: false}), ['primary']);
const values = {'{scoped_elapsed}': '27'};
const framed = splitFormatOutput(formatFields.map(value => values[value] ?? value).join(';;') + ' ⏸');
assert.equal(integer(framed[FIELD.scopedElapsed]), 27);
assert.equal(framed[FIELD.sentinel], '__aiub_end__ ⏸');

assert.equal(colorForDelta(-11, colors), 'low');
assert.equal(colorForDelta(-10, colors), 'mid');
assert.equal(colorForDelta(0, colors), 'mid');
assert.equal(colorForDelta(1, colors), 'high');
assert.equal(colorForDelta(9, colors), 'high');
assert.equal(colorForDelta(10, colors), 'critical');

for (const [pct, elapsed, expected] of [[25, 50, 'low'], [50, 50, 'mid'], [75, 50, 'critical']]) {
    const markup = barMarkup(pct, 8, colors, elapsed);
    assert.equal(visibleCells(markup).length, 8);
    assert.ok(markup.includes('│'));
    assert.ok(markup.includes(`foreground="${expected}"`));
}
assert.equal(visibleCells(barMarkup(50, 8, colors, 0)).length, 8);
assert.equal(visibleCells(barMarkup(50, 8, colors, 100)).length, 8);
assert.ok(!barMarkup(50, 8, colors, markerElapsed('—', 0)).includes('│'));

// ── multi-entry panel ─────────────────────────────────────────────────────
assert.equal(windowTag(SESSION_SECS), '5h');
assert.equal(windowTag(WEEKLY_SECS), '7d');
assert.equal(windowTag(86400), '1d');
assert.equal(windowTag(2678400), '31d');
assert.equal(windowTag(14400), '4h');
assert.equal(windowTag(90), '2m');       // rounds to the nearest minute
assert.equal(windowTag(0), '');
assert.equal(windowTag(null), '');

assert.equal(entryLabel('anthropic@claude-b3'), 'claude-b3');
assert.equal(entryLabel('openai'), '');
assert.equal(entryLabel('anthropic@'), '');
assert.equal(entryLabel('@x'), '');
assert.equal(entryLabel(null), '');

// ── shared label prefix ───────────────────────────────────────────────────
// Labels kept on one machine repeat a word that says nothing once they sit
// side by side, and the panel is where that costs the most room.
assert.equal(commonLabelPrefix(['anthropic@claude-me', 'anthropic@claude-b3', 'openai']),
    'claude-');
// Nothing shared: both names stay whole.
assert.equal(commonLabelPrefix(['anthropic@work', 'anthropic@personal']), '');
// One label has no shared prefix to discover, so it keeps its owner's name.
assert.equal(commonLabelPrefix(['anthropic@claude-me', 'openai']), '');
assert.equal(commonLabelPrefix([]), '');
assert.equal(commonLabelPrefix(null), '');
// Trimmed to a separator: never `e`/`x` out of `claude-me`/`claude-mx`.
assert.equal(commonLabelPrefix(['a@claude-me', 'a@claude-mx']), 'claude-');
assert.equal(commonLabelPrefix(['a@dev1', 'a@dev2']), '');
// Every label must survive with something left.
assert.equal(commonLabelPrefix(['a@claude', 'a@claude-me']), '');
assert.equal(commonLabelPrefix(['a@x-', 'a@x-me']), '');
// Other separators are boundaries too.
assert.equal(commonLabelPrefix(['a@claude_me', 'a@claude_b3']), 'claude_');
assert.equal(commonLabelPrefix(['a@claude me', 'a@claude b3']), 'claude ');
// Parallel labels under different providers share nothing, so two entries
// cannot collapse onto the same tag.
assert.equal(commonLabelPrefix(['anthropic@claude-me', 'openai@codex-me']), '');

// The account label names the segment; a plain vendor falls back to the
// report's own short name, never to a table kept here.
assert.equal(entryTag({id: 'anthropic@claude-b3', short_name: 'cld'}), 'claude-b3');
assert.equal(entryTag({id: 'anthropic@claude-b3', short_name: 'cld'}, 'claude-'), 'b3');
// A prefix the label does not carry is not applied blindly.
assert.equal(entryTag({id: 'anthropic@work', short_name: 'cld'}, 'claude-'), 'work');
// A provider's short name is never trimmed — it is not a label.
assert.equal(entryTag({id: 'openai', short_name: 'gpt'}, 'gp'), 'gpt');
assert.equal(entryTag({id: 'openai', short_name: 'gpt'}), 'gpt');
assert.equal(entryTag({id: 'openai'}), 'openai');          // no short name in the report
assert.equal(entryTag({id: 'anthropic@', short_name: 'cld'}), 'cld'); // empty label
assert.equal(entryTag({id: '@x', short_name: 'cld'}), 'cld');         // no vendor half

const CLAUDE = {
    id: 'anthropic@claude-me', short_name: 'cld', status: 'ready', stale: false,
    metrics: [
        {label: 'Session (5h)', percent: 4, value: '4%', window_secs: SESSION_SECS},
        {label: 'Weekly (7d)', percent: 95, value: '95%', window_secs: WEEKLY_SECS},
        {label: 'Fable (7d)', percent: 1, value: '1%', window_secs: WEEKLY_SECS},
    ],
};
const CODEX = {
    id: 'openai', short_name: 'gpt', status: 'ready', stale: true,
    metrics: [{label: 'Codex weekly', percent: 5, value: '5%', window_secs: WEEKLY_SECS}],
};
const CURSOR = {
    id: 'cursor', short_name: 'cur', status: 'ready',
    metrics: [{label: 'Cursor Models', percent: 0, value: '0%', window_secs: 2678400}],
};

// First match per window: the account's own weekly, not its Fable weekly too.
assert.deepEqual(entryMetrics(CLAUDE, {weekly: true}).map(m => m.label), ['Weekly (7d)']);
assert.deepEqual(entryMetrics(CLAUDE, {session: true, weekly: true}).map(m => m.label),
    ['Session (5h)', 'Weekly (7d)']);
// Windows are asked for in session-then-weekly order regardless of report order.
assert.deepEqual(entryMetrics({metrics: [CLAUDE.metrics[1], CLAUDE.metrics[0]]},
    {session: true, weekly: true}).map(m => m.label), ['Session (5h)', 'Weekly (7d)']);
// Codex has no 5h window: asking for both yields only what it has.
assert.deepEqual(entryMetrics(CODEX, {session: true, weekly: true}).map(m => m.label),
    ['Codex weekly']);
// Neither window: the first metric stands in, so the entry is never invisible.
assert.deepEqual(entryMetrics(CURSOR, {weekly: true}).map(m => m.label), ['Cursor Models']);
// No window enabled at all is not a fallback case — it is "draw nothing".
assert.deepEqual(entryMetrics(CLAUDE, {}), []);
assert.deepEqual(entryMetrics({metrics: []}, {weekly: true}), []);
assert.deepEqual(entryMetrics(undefined, {weekly: true}), []);

const REPORT = {entries: [CLAUDE, {...CLAUDE, id: 'anthropic@claude-b3',
    metrics: [{label: 'Weekly (7d)', percent: 16, value: '16%', window_secs: WEEKLY_SECS}]},
    CODEX,
    {id: 'zai', short_name: 'zai', status: 'error', error: 'no API key', metrics: []}]};

const weekly = panelSegments(REPORT,
    ['anthropic@claude-me', 'anthropic@claude-b3', 'openai'], {weekly: true});
assert.deepEqual(weekly.map(s => [s.tag, s.window, s.pct, s.status]), [
    ['me', '7d', 95, 'ready'],
    ['b3', '7d', 16, 'ready'],
    ['gpt', '7d', 5, 'ready'],
]);
// The prefix comes from the selection, so a tag does not grow back when the
// account next to it fails or is switched off.
assert.deepEqual(panelSegments({entries: [REPORT.entries[0]]},
    ['anthropic@claude-me', 'anthropic@claude-b3'], {weekly: true})
    .map(s => [s.tag, s.status]), [['me', 'ready'], ['b3', 'absent']]);
// One account selected keeps its full name: nothing to compare it against.
assert.equal(panelSegments(REPORT, ['anthropic@claude-me'], {weekly: true})[0].tag,
    'claude-me');
// Selection order is display order, not report order.
assert.deepEqual(panelSegments(REPORT, ['openai', 'anthropic@claude-me'], {weekly: true})
    .map(s => s.tag), ['gpt', 'claude-me']);
// Staleness rides along for the renderer to mark.
assert.equal(weekly[2].stale, true);
assert.equal(weekly[0].stale, false);

// An entry that failed keeps its own error text; one not in the report at all
// is reported as absent rather than silently dropped.
assert.deepEqual(panelSegments(REPORT, ['zai'], {weekly: true}),
    [{id: 'zai', tag: 'zai', status: 'error', error: 'no API key'}]);
assert.deepEqual(panelSegments(REPORT, ['minimax'], {weekly: true}),
    [{id: 'minimax', tag: 'minimax', status: 'absent'}]);
assert.deepEqual(panelSegments(REPORT, ['anthropic@gone'], {weekly: true}),
    [{id: 'anthropic@gone', tag: 'gone', status: 'absent'}]);
// Selection order is display order for the tags too.
assert.deepEqual(panelSegments(REPORT, ['anthropic@claude-b3', 'anthropic@claude-me'],
    {weekly: true}).map(s => s.tag), ['b3', 'me']);

// Percentages are clamped and rounded the way the bar expects.
assert.equal(panelSegments({entries: [{id: 'x', metrics: [
    {percent: 142.6, window_secs: WEEKLY_SECS}]}]}, ['x'], {weekly: true})[0].pct, 100);
assert.equal(panelSegments({entries: [{id: 'x', metrics: [
    {percent: -3, window_secs: WEEKLY_SECS}]}]}, ['x'], {weekly: true})[0].pct, 0);
// A missing or non-numeric percentage stays null: a 0% bar would claim the
// window is untouched.
for (const percent of [null, undefined, '95', NaN, {}])
    assert.equal(panelSegments({entries: [{id: 'x', metrics: [
        {percent, window_secs: WEEKLY_SECS}]}]}, ['x'], {weekly: true})[0].pct, null,
    `percent ${JSON.stringify(percent)} must not read as a figure`);

// Garbage in: an empty selection, or no report at all, draws nothing.
assert.deepEqual(panelSegments(REPORT, [], {weekly: true}), []);
assert.deepEqual(panelSegments(null, ['openai'], {weekly: true}),
    [{id: 'openai', tag: 'openai', status: 'absent'}]);
assert.deepEqual(panelSegments(REPORT, null, {weekly: true}), []);

console.log('marker logic tests passed');
