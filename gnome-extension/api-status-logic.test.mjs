import assert from 'node:assert/strict';
import {API_VENDORS, balanceDisplay, configApiKeyEnv, configHasApiKey,
    configMonthlyLimit, configVendorEnabled, extractSnapshot, fmtAge,
    parseLastError, quotaDisplay, rowStatus, shortHttpError,
    tomlHeaderIs} from './api-status-logic.js';

// ── tomlHeaderIs ──────────────────────────────────────────────────────────
assert.equal(tomlHeaderIs('[zai]', 'zai'), true);
assert.equal(tomlHeaderIs('  [zai]  # note', 'zai'), false); // caller trims; raw line with leading space is not a header
assert.equal(tomlHeaderIs('[zai] # note', 'zai'), true);
assert.equal(tomlHeaderIs('[zai2]', 'zai'), false);
assert.equal(tomlHeaderIs('zai', 'zai'), false);

// ── configHasApiKey ──────────────────────────────────────────────────────
const CFG = `
[zai]
api_key = "sk-real-key"

[kilo]
api_key = ""

[novita]
# api_key = "commented"
enabled = true
`;
assert.equal(configHasApiKey(CFG, 'zai'), true);
assert.equal(configHasApiKey(CFG, 'kilo'), false);   // empty key ≠ configured
assert.equal(configHasApiKey(CFG, 'novita'), false); // commented ≠ configured
assert.equal(configHasApiKey(null, 'zai'), false);

// ── configVendorEnabled: defaults mirror the binary ──────────────────────
for (const id of ['anthropic', 'openai', 'zai', 'openrouter', 'shvia'])
    assert.equal(configVendorEnabled('', id), true, `${id} default-on`);
for (const id of ['anthropic_api', 'deepseek', 'kilo', 'novita', 'moonshot', 'grok'])
    assert.equal(configVendorEnabled('', id), false, `${id} opt-in`);
assert.equal(configVendorEnabled('[kilo]\nenabled = true', 'kilo'), true);
assert.equal(configVendorEnabled('[zai]\nenabled = false', 'zai'), false);
assert.equal(configVendorEnabled('[zai] # inline\nenabled = false # off', 'zai'), false);

// ── configMonthlyLimit ───────────────────────────────────────────────────
assert.equal(configMonthlyLimit('[anthropic_api]\nmonthly_limit = 1000', 'anthropic_api'), 1000);
assert.equal(configMonthlyLimit('[anthropic_api]\nmonthly_limit = 1000 # USD', 'anthropic_api'), 1000);
assert.equal(configMonthlyLimit('[anthropic_api]\n# monthly_limit = 1000', 'anthropic_api'), null);
assert.equal(configMonthlyLimit('[openai]\nmonthly_limit = 5', 'anthropic_api'), null);
assert.equal(configMonthlyLimit(null, 'anthropic_api'), null);

// ── fmtAge / shortHttpError / parseLastError ─────────────────────────────
assert.equal(fmtAge(14), '14s');
assert.equal(fmtAge(90), '1m');
assert.equal(fmtAge(7200), '2h');
assert.equal(fmtAge(200000), '2d');
assert.equal(shortHttpError(401), 'HTTP 401 · chave inválida');
assert.equal(shortHttpError(429), 'HTTP 429 · limite atingido');
assert.equal(shortHttpError(503), 'HTTP 503 · erro do servidor');
assert.equal(shortHttpError(0), 'sem conexão');
assert.equal(shortHttpError(418), 'HTTP 418');
assert.deepEqual(parseLastError('401\ninvalid x-api-key'), {code: 401, msg: 'invalid x-api-key'});
assert.deepEqual(parseLastError('0'), {code: 0, msg: ''});
assert.equal(parseLastError('garbage'), null);
assert.equal(parseLastError(null), null);

// ── balanceDisplay: shapes mirror src/<vendor>/fetch.rs cache reprs ──────
assert.equal(balanceDisplay('kilo', {balance: 3.9}, ''), '$3.90');
assert.equal(balanceDisplay('grok', {balance: 25}, ''), '$25.00');
assert.equal(balanceDisplay('novita', {available: 25}, ''), '$25.00');
assert.equal(balanceDisplay('moonshot', {available: 27, currency: 'CNY'}, ''), '¥27.00');
assert.equal(balanceDisplay('moonshot', {available: 27}, ''), '$27.00');
assert.equal(balanceDisplay('deepseek', {balance: 1.5, currency: 'CNY'}, ''), '¥1.50');
assert.equal(balanceDisplay('openrouter', {total_credits: 25, total_usage: 5}, ''), '$20.00');
assert.equal(balanceDisplay('openrouter', {total_credits: 1, total_usage: 9}, ''), '$0.00'); // clamped
// anthropic_api: config limit wins over the cached one; % rounds like pct().
assert.equal(balanceDisplay('anthropic_api', {spent: 3.08, limit: 500},
    '[anthropic_api]\nmonthly_limit = 1000'), '$3.08 / $1000 · 0%');
assert.equal(balanceDisplay('anthropic_api', {spent: 3.08, limit: 500}, ''), '$3.08 / $500 · 1%');
assert.equal(balanceDisplay('anthropic_api', {spent: 3.08, limit: null}, ''), '$3.08/mo');
assert.equal(balanceDisplay('anthropic_api', {}, ''), null);
// quota vendors have no balance headline
for (const id of ['anthropic', 'openai', 'zai', 'shvia'])
    assert.equal(balanceDisplay(id, {spent: 1}, ''), null, id);
// malformed snapshots never throw
assert.equal(balanceDisplay('kilo', null, ''), null);
assert.equal(balanceDisplay('kilo', {balance: 'NaN?'}, ''), null);

// ── rowStatus: the decision ladder mirrors the macOS apiRowStatus ────────
const zai = API_VENDORS.find(v => v.id === 'zai');
const anthropic = API_VENDORS.find(v => v.id === 'anthropic');
const kilo = API_VENDORS.find(v => v.id === 'kilo');
const base = {enabled: true, configured: true, lastError: null, ageSecs: null,
    snap: null, configText: '', activePcts: null};

assert.deepEqual(rowStatus(zai, {...base, enabled: false}),
    {state: 'off', detail: 'desativado', age: ''});
assert.deepEqual(rowStatus(zai, {...base, configured: false}),
    {state: 'warn', detail: 'sem API key — ZAI_API_KEY', age: ''});
assert.deepEqual(rowStatus(anthropic, {...base, configured: false}),
    {state: 'warn', detail: 'não logado — claude', age: ''});
assert.deepEqual(rowStatus(zai, {...base, lastError: {code: 401, msg: ''}, ageSecs: 120}),
    {state: 'error', detail: 'HTTP 401 · chave inválida', age: '2m'});
assert.deepEqual(rowStatus(kilo, {...base, ageSecs: 14, snap: {balance: 3.9}}),
    {state: 'ok', detail: '$3.90', age: '14s'});
assert.deepEqual(rowStatus(anthropic, {...base, ageSecs: 14, activePcts: {session: 49, weekly: 29}}),
    {state: 'ok', detail: '5h 49% · 7d 29%', age: '14s'});
assert.deepEqual(rowStatus(anthropic, {...base, ageSecs: 14}),
    {state: 'ok', detail: 'OK', age: '14s'});
assert.deepEqual(rowStatus(zai, base),
    {state: 'warn', detail: 'sem dados — use “Verificar todas”', age: ''});

// shvia: quota vendor, enabled by default, shows OK/err like the others
const shvia = API_VENDORS.find(v => v.id === 'shvia');
assert.equal(configVendorEnabled('', 'shvia'), true);
assert.deepEqual(rowStatus(shvia, {...base, ageSecs: 30}),
    {state: 'ok', detail: 'OK', age: '30s'});
assert.deepEqual(rowStatus(shvia, {...base, configured: false}),
    {state: 'warn', detail: 'sem API key — SHVIA_API_KEY', age: ''});

// ── review fixes (adversarial review of the port) ────────────────────────

// DeepSeek's cache is FLAT (src/deepseek/fetch.rs snap_to_json) — every other
// vendor wraps in {"snapshot": …}. extractSnapshot bridges the difference;
// the macOS panel misses this and never renders the DeepSeek balance.
assert.deepEqual(
    extractSnapshot('deepseek', {is_available: true, balance: 1.5, currency: 'USD'}),
    {is_available: true, balance: 1.5, currency: 'USD'});
assert.equal(
    balanceDisplay('deepseek',
        extractSnapshot('deepseek', {is_available: true, balance: 5, currency: 'USD'}), ''),
    '$5.00');
assert.deepEqual(extractSnapshot('kilo', {snapshot: {balance: 3.9}}), {balance: 3.9});
assert.equal(extractSnapshot('kilo', {balance: 3.9}), null); // unwrapped non-deepseek → no snap
assert.equal(extractSnapshot('kilo', null), null);
assert.equal(extractSnapshot('kilo', [1]), null);
assert.equal(extractSnapshot('kilo', {snapshot: [1]}), null);
assert.equal(extractSnapshot('deepseek', 'garbage'), null);

// Stale/typo'd keys the binary's TOML parser ignores must be ignored here too.
assert.equal(configVendorEnabled('[zai]\nenabledd = false\napi_key = "k"', 'zai'), true);
assert.equal(configVendorEnabled('[zai]\nenabled_x = false', 'zai'), true);
assert.equal(configVendorEnabled('[zai]\nenabled=false', 'zai'), false); // no-space form still counts
assert.equal(configMonthlyLimit('[anthropic_api]\nmonthly_limit_x = 7\nmonthly_limit = 100', 'anthropic_api'), 100);

// TOML digit-group underscores (1_000) are valid for the binary's parser.
assert.equal(configMonthlyLimit('[anthropic_api]\nmonthly_limit = 1_000', 'anthropic_api'), 1000);

// Per-vendor api_key_env override (resolve_api_key checks it first).
assert.equal(configApiKeyEnv('[deepseek]\napi_key_env = "MY_DS_KEY"', 'deepseek'), 'MY_DS_KEY');
assert.equal(configApiKeyEnv('[deepseek]\nenabled = true', 'deepseek'), null);
assert.equal(configApiKeyEnv(null, 'deepseek'), null);

// lastError outranks a balance-bearing snapshot (the ladder's order).
assert.deepEqual(
    rowStatus(kilo, {...base, lastError: {code: 500, msg: ''}, ageSecs: 60, snap: {balance: 9}}),
    {state: 'error', detail: 'HTTP 500 · erro do servidor', age: '1m'});

// ── quotaDisplay (MiniMax) ────────────────────────────────────────────────
// The window length is read from the payload: MiniMax's short window has been
// observed at both 4h and 5h, so a hardcoded "5h" label would eventually lie.
const mmSnap = (sSecs, wSecs) => ({
    plan: 'MiniMax Token Plan',
    session: {pct: 12, resets_at: null, window_secs: sSecs},
    weekly: {pct: 34, resets_at: null, window_secs: wSecs},
});
assert.equal(quotaDisplay('minimax', mmSnap(18000, 604800)), '5h 12% · 7d 34%');
assert.equal(quotaDisplay('minimax', mmSnap(14400, 604800)), '4h 12% · 7d 34%');
// Degenerate/absent lengths fall back to the conventional labels.
assert.equal(quotaDisplay('minimax', mmSnap(0, 0)), '5h 12% · 7d 34%');
// Only vendors whose cache we write ourselves are parsed.
assert.equal(quotaDisplay('anthropic', mmSnap(18000, 604800)), null);
assert.equal(quotaDisplay('minimax', null), null);
assert.equal(quotaDisplay('minimax', {session: {pct: 1}}), null, 'needs both windows');

// A configured MiniMax that is NOT the active vendor shows its windows rather
// than the bare "OK" the generic branch would produce.
const minimax = API_VENDORS.find(v => v.id === 'minimax');
assert.deepEqual(
    rowStatus(minimax, {...base, ageSecs: 20, snap: mmSnap(14400, 604800), activePcts: null}),
    {state: 'ok', detail: '4h 12% · 7d 34%', age: '20s'});

console.log('api-status-logic: all assertions passed');
