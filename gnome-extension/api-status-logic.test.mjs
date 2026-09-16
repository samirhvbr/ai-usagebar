import assert from 'node:assert/strict';
import {apiStatusRows, missingCredentialHint, oneLine, parseUsageReport,
    parseVendorCatalog, reportHeadline, rowStatus} from './api-status-logic.js';

// ─── vendors --json ──────────────────────────────────────────────────────
//
// The catalog replaced a table that lived in this file. It listed sixteen of
// the binary's twenty-one providers, so the five below could never appear in a
// section whose whole purpose is to be complete. They are in the fixture for
// that reason: nothing here may filter a provider the binary reported.
const CATALOG = JSON.stringify({
    vendors: [
        {id: 'anthropic', name: 'Claude', short_name: 'cld', kind: 'oauth',
         enabled: true, configured: false, needs_credential: true, env: '', login: 'claude'},
        {id: 'shvia', name: 'ShvIA', short_name: 'shv', kind: 'apikey',
         enabled: true, configured: true, needs_credential: true, env: 'SHVIA_API_KEY', login: ''},
        {id: 'zai', name: 'Z.AI', short_name: 'zai', kind: 'apikey',
         enabled: true, configured: true, needs_credential: true, env: 'ZAI_API_KEY', login: ''},
        {id: 'openrouter', name: 'OpenRouter', short_name: 'opr', kind: 'apikey',
         enabled: true, configured: true, needs_credential: true, env: 'OPENROUTER_API_KEY', login: ''},
        {id: 'kimi', name: 'Kimi', short_name: 'kmi', kind: 'apikey',
         enabled: true, configured: false, needs_credential: true, env: 'KIMI_API_KEY', login: 'kimi'},
        {id: 'grok', name: 'Grok', short_name: 'grk', kind: 'apikey',
         enabled: false, configured: false, needs_credential: true, env: 'XAI_MANAGEMENT_KEY', login: ''},
        {id: 'antigravity', name: 'Antigravity', short_name: 'agy', kind: 'local',
         enabled: true, configured: true, needs_credential: false, env: '', login: ''},
        {id: 'cursor', name: 'Cursor', short_name: 'cur', kind: 'local',
         enabled: false, configured: false, needs_credential: true, env: '', login: ''},
        {id: 'kiro', name: 'Kiro', short_name: 'kir', kind: 'local',
         enabled: false, configured: false, needs_credential: true, env: '', login: 'kiro-cli login'},
        {id: 'nous', name: 'Nous Research', short_name: 'nrs', kind: 'oauth',
         enabled: false, configured: false, needs_credential: true, env: '',
         login: 'ai-usagebar auth nous login'},
        {id: 'supergrok', name: 'SuperGrok', short_name: 'sgk', kind: 'local',
         enabled: false, configured: false, needs_credential: true, env: '', login: ''},
    ],
});

const catalog = parseVendorCatalog(CATALOG);
assert.equal(catalog.length, 11);
assert.equal(catalog[0].id, 'anthropic', 'the binary decides the order');
assert.equal(catalog[0].login, 'claude');
assert.equal(catalog[1].env, 'SHVIA_API_KEY');
assert.equal(catalog.find(v => v.id === 'antigravity').needsCredential, false);
// The five the old hard-coded table never had.
for (const id of ['antigravity', 'cursor', 'kiro', 'nous', 'supergrok'])
    assert.ok(catalog.some(v => v.id === id), `${id} must survive parsing`);

// Absent `needs_credential` means "has one" — an older binary must not make a
// frontend start claiming providers need nothing.
assert.equal(parseVendorCatalog('{"vendors":[{"id":"zai"}]}')[0].needsCredential, true);
assert.equal(parseVendorCatalog('{"vendors":[{"id":"zai"}]}')[0].name, 'zai',
    'a nameless row falls back to its id rather than rendering blank');
assert.equal(parseVendorCatalog('{"vendors":[{"id":"zai"}]}')[0].enabled, false);

// Garbage never throws — the section degrades, it does not take the menu down.
assert.deepEqual(parseVendorCatalog('not json'), []);
assert.deepEqual(parseVendorCatalog(''), []);
assert.deepEqual(parseVendorCatalog('{"vendors":"nope"}'), []);
assert.deepEqual(parseVendorCatalog('{"vendors":[{"no_id":1},null,7]}'), []);

// ─── usage --json ────────────────────────────────────────────────────────
const REPORT = JSON.stringify({
    primary: 'shvia',
    entries: [
        {
            id: 'shvia', name: 'shvia', display_name: 'ShvIA', short_name: 'shv',
            plan: 'ShvIA', status: 'ready', error: null, stale: false,
            metrics: [
                {label: 'Today', percent: 12, value: '12%'},
                {label: 'Week', percent: 63, value: '63%'},
            ],
            sections: [],
        },
        {
            id: 'openrouter', name: 'openrouter', display_name: 'OpenRouter',
            status: 'ready', error: null, stale: true,
            metrics: [{label: 'Credit balance', percent: 4, value: '$12.34'}],
        },
        {
            id: 'zai', name: 'zai', display_name: 'Z.AI', status: 'error',
            error: 'HTTP 401 Authentication failed', stale: false, metrics: [],
        },
        {
            id: 'anthropic@work', name: 'anthropic · work', display_name: 'Claude · work',
            status: 'ready', error: null, stale: false, metrics: [],
        },
    ],
});

const report = parseUsageReport(REPORT);
assert.equal(Object.keys(report).length, 4);
assert.equal(report.shvia.name, 'ShvIA');
assert.equal(report.shvia.failed, false);
assert.equal(reportHeadline(report.shvia), '63%');       // the most-consumed metric
assert.equal(reportHeadline(report['anthropic@work']), ''); // no metrics, no claim
assert.equal(report.zai.failed, true);
assert.equal(report.openrouter.stale, true);
// Account ids keep the report's own `<vendor>@<label>` shape.
assert.ok(Object.prototype.hasOwnProperty.call(report, 'anthropic@work'));

const shvia = catalog.find(v => v.id === 'shvia');

// A vendor with no gauge at all still has a headline: ShvIA's windows are
// uncapped on some plans, so they report a used count and no ratio.
const uncapped = parseUsageReport(JSON.stringify({
    entries: [{
        id: 'shvia', display_name: 'ShvIA', status: 'ready', error: null, stale: false,
        metrics: [],
        sections: [
            {type: 'title', left: 'ShvIA'},
            {type: 'spacer'},
            {type: 'text', label: 'Today', value: '0 used · unlimited'},
            {type: 'text', label: '', value: 'resets in 11h'},
            {type: 'text', label: 'Week', value: '169.2k used · unlimited'},
        ],
    }],
}));
assert.equal(uncapped.shvia.texts.length, 2, 'unlabelled continuation rows are not headlines');
assert.equal(reportHeadline(uncapped.shvia), '0 used · unlimited');
assert.equal(rowStatus(shvia, {entry: uncapped.shvia, reportRan: true}).state, 'ok');

// Garbage never throws — the section degrades, it does not take the menu down.
assert.deepEqual(parseUsageReport('not json'), {});
assert.deepEqual(parseUsageReport(''), {});
assert.deepEqual(parseUsageReport('{"entries":"nope"}'), {});
assert.deepEqual(parseUsageReport('{"entries":[{"no_id":1}]}'), {});

assert.equal(oneLine('line one\nline two'), 'line one line two');
assert.equal(oneLine('x'.repeat(60), 10), `${'x'.repeat(9)}…`);

// ─── The row decision table ──────────────────────────────────────────────
//
// `enabled` and `configured` now ride on the vendor row: the binary resolved
// them, so this side only decides how to draw the answer.
const off = catalog.find(v => v.id === 'grok');
const anthropic = catalog.find(v => v.id === 'anthropic');
const noKey = catalog.find(v => v.id === 'kimi');
const agy = catalog.find(v => v.id === 'antigravity');
const app = catalog.find(v => v.id === 'cursor');

assert.deepEqual(rowStatus(off, {reportRan: true}),
    {state: 'off', value: '', detail: 'desativado'});
assert.deepEqual(rowStatus(noKey, {reportRan: true}),
    {state: 'warn', value: '', detail: 'não logado — kimi'});
assert.deepEqual(rowStatus(anthropic, {reportRan: true}),
    {state: 'warn', value: '', detail: 'não logado — claude'});
// A provider with no login command and no variable is signed in somewhere this
// cannot name: its own app.
assert.deepEqual(rowStatus({...app, enabled: true}, {reportRan: true}),
    {state: 'warn', value: '', detail: 'não logado no app'});
assert.deepEqual(
    rowStatus(shvia, {entry: report.shvia, reportRan: true}),
    {state: 'ok', value: '63%', detail: ''});
assert.deepEqual(
    rowStatus(shvia, {entry: report.zai, reportRan: true}),
    {state: 'error', value: '', detail: 'HTTP 401 Authentication failed'});
// A stale entry still shows the last figure it had — that is the point of a cache.
const stale = rowStatus(shvia, {entry: report.openrouter, reportRan: true});
assert.equal(stale.state, 'warn');
assert.equal(stale.value, '$12.34');
// Pending is not "sem dados": one says no sweep has run, the other that a
// sweep ran and skipped this vendor.
assert.equal(rowStatus(shvia, {reportRan: false}).detail, '…');
assert.equal(rowStatus(shvia, {reportRan: true}).detail, 'sem dados');

// Antigravity has no credential at all, so it is never "missing" one: with the
// section open and no sweep yet it is pending, not a warning about a key.
assert.equal(missingCredentialHint(agy), '');
assert.equal(rowStatus(agy, {reportRan: true}).detail, 'sem dados');

// ─── The section ─────────────────────────────────────────────────────────
//
// Every provider the binary reported gets a row, on or off. This is the guard
// the old table failed: the count comes from the catalog, so a provider added
// in Rust appears here with no change to this file.
const rows = apiStatusRows({vendors: catalog, report, reportRan: true});
assert.equal(rows.length, catalog.length);
assert.deepEqual(rows.map(r => r.id.split('@')[0]), catalog.map(v => v.id), 'the catalog order is kept');
assert.equal(rows.find(r => r.id === 'grok').state, 'off');
assert.equal(rows.find(r => r.id === 'shvia').value, '63%');
assert.equal(rows.find(r => r.id === 'zai').state, 'error');
assert.equal(rows.find(r => r.id === 'kimi').state, 'warn');   // enabled, no key
assert.equal(rows.find(r => r.id === 'supergrok').state, 'off');
assert.equal(rows.find(r => r.id === 'anthropic@work').state, 'ok');

// No catalog yet: no rows, and nothing thrown.
assert.deepEqual(apiStatusRows({vendors: [], report, reportRan: true}), []);
assert.deepEqual(apiStatusRows({report, reportRan: false}), []);

// Every provider that needs a credential can say what it is missing.
for (const v of catalog) {
    if (v.needsCredential)
        assert.ok(missingCredentialHint(v).length > 0, `${v.id} has no hint`);
}

// Named entries have independent credentials and health, even when the
// default account has no credential. Desktop uses the same report id format.
const accounts = parseUsageReport(JSON.stringify({entries: [
    {id: 'anthropic@work', display_name: 'Claude · work', error: 'HTTP 401'},
    {id: 'anthropic@personal', display_name: 'Claude · personal', metrics: [{label: 'Weekly', percent: 21, value: '21%'}]},
    {id: 'openrouter@team', display_name: 'OpenRouter · team', stale: true, metrics: [{label: 'Balance', percent: 0, value: '$12'}]},
    {id: 'openai@work', display_name: 'Codex · work', metrics: [{label: 'Weekly', percent: 30, value: '30%'}]},
    {id: 'anthropic-other@ignored', display_name: 'Wrong vendor'},
]}));
const accountVendors = [anthropic,
    {...anthropic, id: 'openrouter', name: 'OpenRouter'},
    {...anthropic, id: 'openai', name: 'Codex'}];
const accountRows = apiStatusRows({vendors: accountVendors, report: accounts, reportRan: true});
assert.deepEqual(accountRows.map(r => r.id), ['anthropic@personal', 'anthropic@work', 'openrouter@team', 'openai@work']);
assert.equal(accountRows[0].name, 'Claude · personal');
assert.equal(accountRows[0].value, '21%');
assert.equal(accountRows[0].state, 'ok');
assert.equal(accountRows[1].state, 'error');
assert.equal(accountRows[1].detail, 'HTTP 401');
assert.equal(accountRows[2].state, 'warn');
assert.equal(accountRows[2].value, '$12');
assert.equal(accountRows[3].value, '30%');
assert.deepEqual(apiStatusRows({vendors: [{...anthropic, enabled: false}], report: accounts, reportRan: true})
    .map(r => [r.id, r.state]), [['anthropic', 'off']]);
const withDefault = {...accounts, ...parseUsageReport(JSON.stringify({entries: [{id: 'anthropic', error: 'Default login expired'}]}))};
assert.deepEqual(apiStatusRows({vendors: [anthropic], report: withDefault, reportRan: true}).map(r => r.id),
    ['anthropic', 'anthropic@personal', 'anthropic@work']);
assert.equal(apiStatusRows({vendors: [anthropic], report: withDefault, reportRan: true})[0].detail, 'Default login expired');

console.log('api-status-logic: all assertions passed');
