import assert from 'node:assert/strict';
import {API_VENDORS, apiStatusRows, configApiKeyEnv, configHasApiKey, configVendorEnabled,
    missingCredentialHint, oneLine, parseUsageReport, reportHeadline, rowStatus,
    tomlHeaderIs} from './api-status-logic.js';

// ─── TOML reading ────────────────────────────────────────────────────────
assert.equal(tomlHeaderIs('[zai]', 'zai'), true);
assert.equal(tomlHeaderIs('[zai] # a note', 'zai'), true);
// The caller trims before asking, so a leading space is not this function's job.
assert.equal(tomlHeaderIs('  [zai]', 'zai'), false);
assert.equal(tomlHeaderIs('[zai]  ', 'zai'), true);
assert.equal(tomlHeaderIs('[zai.accounts]', 'zai'), false);
assert.equal(tomlHeaderIs('enabled = true', 'zai'), false);

// The four core vendors are on unless the config turns them off; everything
// key-authenticated is opt-in — the same table as Config::default.
assert.equal(configVendorEnabled('', 'anthropic'), true);
assert.equal(configVendorEnabled('', 'zai'), true);
assert.equal(configVendorEnabled('', 'shvia'), false);
assert.equal(configVendorEnabled('', 'grok'), false);
assert.equal(configVendorEnabled('[shvia]\nenabled = true\n', 'shvia'), true);
assert.equal(configVendorEnabled('[anthropic]\nenabled = false\n', 'anthropic'), false);
// A key that merely starts with "enabled" is not `enabled`.
assert.equal(configVendorEnabled('[anthropic]\nenabled_extra = false\n', 'anthropic'), true);
// Only the vendor's own section counts.
assert.equal(configVendorEnabled('[zai]\nenabled = false\n[shvia]\nenabled = true\n', 'shvia'), true);

assert.equal(configHasApiKey('[shvia]\napi_key = "shvia_x"\n', 'shvia'), true);
assert.equal(configHasApiKey('[shvia]\napi_key = ""\n', 'shvia'), false);
assert.equal(configHasApiKey('[shvia]\n# api_key = "shvia_x"\n', 'shvia'), false);
assert.equal(configHasApiKey('[zai]\napi_key = "z"\n', 'shvia'), false);
assert.equal(configApiKeyEnv('[shvia]\napi_key_env = "MY_KEY"\n', 'shvia'), 'MY_KEY');
assert.equal(configApiKeyEnv('[shvia]\n', 'shvia'), null);

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

// Garbage never throws — the section degrades, it does not take the menu down.
assert.deepEqual(parseUsageReport('not json'), {});
assert.deepEqual(parseUsageReport(''), {});
assert.deepEqual(parseUsageReport('{"entries":"nope"}'), {});
assert.deepEqual(parseUsageReport('{"entries":[{"no_id":1}]}'), {});

assert.equal(oneLine('line one\nline two'), 'line one line two');
assert.equal(oneLine('x'.repeat(60), 10), `${'x'.repeat(9)}…`);

// ─── The row decision table ──────────────────────────────────────────────
const shvia = API_VENDORS.find(v => v.id === 'shvia');
const anthropic = API_VENDORS.find(v => v.id === 'anthropic');

assert.deepEqual(rowStatus(shvia, {enabled: false, configured: true, reportRan: true}),
    {state: 'off', value: '', detail: 'desativado'});
assert.deepEqual(rowStatus(shvia, {enabled: true, configured: false, reportRan: true}),
    {state: 'warn', value: '', detail: 'sem API key — SHVIA_API_KEY'});
assert.deepEqual(rowStatus(anthropic, {enabled: true, configured: false, reportRan: true}),
    {state: 'warn', value: '', detail: 'não logado — claude'});
assert.deepEqual(
    rowStatus(shvia, {enabled: true, configured: true, entry: report.shvia, reportRan: true}),
    {state: 'ok', value: '63%', detail: ''});
assert.deepEqual(
    rowStatus(shvia, {enabled: true, configured: true, entry: report.zai, reportRan: true}),
    {state: 'error', value: '', detail: 'HTTP 401 Authentication failed'});
// A stale entry still shows the last figure it had — that is the point of a cache.
const stale = rowStatus(shvia, {enabled: true, configured: true, entry: report.openrouter, reportRan: true});
assert.equal(stale.state, 'warn');
assert.equal(stale.value, '$12.34');
// Pending is not "sem dados": one says no sweep has run, the other that a
// sweep ran and skipped this vendor.
assert.equal(rowStatus(shvia, {enabled: true, configured: true, reportRan: false}).detail, '…');
assert.equal(rowStatus(shvia, {enabled: true, configured: true, reportRan: true}).detail, 'sem dados');

// Every vendor gets a row, on or off — that is what the section is for.
const rows = apiStatusRows({
    enabled: v => v.id !== 'anthropic',
    configured: v => ['shvia', 'zai', 'openrouter'].includes(v.id),
    report,
    reportRan: true,
});
assert.equal(rows.length, API_VENDORS.length);
assert.equal(rows.find(r => r.id === 'anthropic').state, 'off');
assert.equal(rows.find(r => r.id === 'shvia').value, '63%');
assert.equal(rows.find(r => r.id === 'zai').state, 'error');
assert.equal(rows.find(r => r.id === 'kimi').state, 'warn');   // enabled, no key

// Every vendor in the table can say what it is missing.
for (const v of API_VENDORS)
    assert.ok(missingCredentialHint(v).length > 0, `${v.id} has no hint`);

console.log('api-status-logic: all assertions passed');
