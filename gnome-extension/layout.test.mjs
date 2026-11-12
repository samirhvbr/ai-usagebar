import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import {splitVendorSetting} from './api-status-logic.js';

const source = readFileSync(new URL('./extension.js', import.meta.url), 'utf8');
const helper = source.match(/\r?\nfunction verticalBox\([^)]*\) \{\r?\n[\s\S]*?\r?\n\}\r?\n/);
assert.ok(helper, 'vertical layouts must handle both Shell property names');
assert.doesNotMatch(source.replace(helper[0], ''), /\b(?:orientation|vertical)\s*:/);

// Execute the actual helper against each API shape. Unsupported properties
// throw, as GJS does, and the caller's layout properties must survive.
for (const modern of [false, true]) {
    const result = runInNewContext(`${helper[0]}\nverticalBox({x_expand: true});`, {
        HAS_ORIENTATION: modern,
        Clutter: {Orientation: {VERTICAL: 1}},
        St: {BoxLayout: class {
            constructor(props) {
                assert.ok(!((modern ? 'vertical' : 'orientation') in props));
                Object.assign(this, props);
            }
        }},
    });
    assert.equal(result.x_expand, true);
    assert.equal(modern ? result.orientation : result.vertical, modern ? 1 : true);
}

// The top bar swaps its label for an icon when there is nothing to draw. That
// only holds if every panel write goes through the helper that makes the swap.
const panelWrites = source.match(/\b_label\.clutter_text\.set_markup\(/g) ?? [];
assert.equal(panelWrites.length, 1, 'top-bar markup must go through _setPanelMarkup');
assert.match(source, /\n {4}_setPanelMarkup\(markup\) \{\n[\s\S]*?this\._label\.clutter_text\.set_markup\(markup\);/);

// Every vendor switch off in prefs stores vendor='', and the panel must agree:
// no indicators at all, not a silent Anthropic fallback. The gschema default
// stays 'anthropic', so only an explicit choice produces the empty list.
const vendorsSrc = source.match(/\n {4}_vendors\(\) \{\n[\s\S]*?\n {4}\}/);
assert.ok(vendorsSrc, 'extension.js must keep a _vendors()');
const Extension = runInNewContext(`(class { ${vendorsSrc[0]} })`, {});
const ext = new Extension();
// Cross-realm arrays fail deepStrictEqual prototype checks; re-home them.
const vendorsOf = (s, entries = []) => {
    ext._settings = {get_string: () => s, get_strv: () => entries};
    return [...ext._vendors()];
};
assert.deepEqual(vendorsOf(''), []);
assert.deepEqual(vendorsOf('anthropic, openai ,anthropic'), ['anthropic', 'openai']);
assert.deepEqual(vendorsOf('anthropic@work'), ['anthropic@work']);
// Existing panel-entries must stay one aggregate strip, even with a newer
// comma-separated vendor setting or all per-provider switches disabled.
assert.deepEqual(vendorsOf('anthropic,openai', ['anthropic@work', 'openai']), ['anthropic']);
assert.deepEqual(vendorsOf('', ['anthropic@work']), ['anthropic']);
assert.deepEqual(vendorsOf('', ['']), []);

// Separate indicators already prepend their mark; an empty aggregate strip
// retains the fork's icon fallback without drawing duplicate marks.
const markupSrc = source.match(/\n {4}_setPanelMarkup\(markup\) \{\n[\s\S]*?\n {4}\}/);
assert.ok(markupSrc);
const Markup = runInNewContext(`(class { ${markupSrc[0]} })`, {splitVendorSetting});
for (const hasPanelMark of [false, true]) {
    const indicator = new Markup();
    Object.assign(indicator, {
        _hasPanelMark: hasPanelMark, _vendor: 'anthropic@work', _icon: {},
        _markIcon: brand => { assert.equal(brand, 'anthropic'); return 'brand-icon'; },
        _label: {clutter_text: {set_markup: () => {}}},
    });
    indicator._setPanelMarkup('');
    assert.equal(indicator._icon.visible, !hasPanelMark);
    assert.equal(indicator._icon.gicon, hasPanelMark ? undefined : 'brand-icon');
    indicator._setPanelMarkup('usage');
    assert.equal(indicator._icon.visible, false);
    assert.equal(indicator._label.visible, true);
}

// The actual worker arguments retain the named-account selector; aggregate
// mode fetches one report instead of also polling an unrelated single vendor.
const refreshSrc = source.match(/\n {4}_refresh\(\) \{\n[\s\S]*?\n {4}\}/);
assert.ok(refreshSrc);
const Refresh = runInNewContext(`(class { ${refreshSrc[0]} })`, {
    splitVendorSetting, resolveBinary: () => '/fixture/ai-usagebar', FORMAT: 'fixture',
});
const refresh = new Refresh();
let argv = null;
let reports = 0;
Object.assign(refresh, {
    _vendor: 'anthropic@work', _panelEntryIds: () => [],
    _run: (_job, args) => { argv = [...args]; },
    _refreshReport: () => { reports++; },
});
refresh._refresh();
assert.deepEqual(argv, ['/fixture/ai-usagebar', '--vendor', 'anthropic',
    '--account', 'work', '--format', 'fixture']);
refresh._panelEntryIds = () => ['anthropic@work', 'openai'];
argv = null;
refresh._refresh();
assert.equal(reports, 1);
assert.equal(argv, null);

// Multiple indicators must not multiply the background API refresh timer.
const apiTimerSrc = source.match(/\n {4}_restartApiTimer\(\) \{\n[\s\S]*?\n {4}\}/);
assert.ok(apiTimerSrc);
let timers = 0;
const ApiTimer = runInNewContext(`(class { ${apiTimerSrc[0]} })`, {
    GLib: {PRIORITY_LOW: 1, timeout_add_seconds: () => ++timers},
});
for (const backgroundApis of [true, false, false]) {
    const indicator = new ApiTimer();
    Object.assign(indicator, {_backgroundApis: backgroundApis, _apiTimer: 0,
        _settings: {get_int: () => 900}});
    indicator._restartApiTimer();
}
assert.equal(timers, 1);
assert.match(source, /this\.dir\.get_child\('icons'\), vendor, i === 0/);

// The tabbed menu supersedes provider submenus, but the three existing menu
// appearance settings remain live upgrade-compatible controls.
for (const key of ['menu-summary-style', 'menu-show-icons', 'menu-compact']) {
    assert.match(source, new RegExp(`get_(?:string|boolean)\\('${key}'\\)`),
        `${key} must still affect the tabbed menu`);
    assert.match(source, new RegExp(`'${key}'`),
        `${key} must trigger an immediate redraw`);
}
assert.match(source, /entries\.find\(e => e\.id === this\._tab\) \|\| entries\[0\]/,
    'the selected provider tab must survive report redraws and fall back safely');

const prefsSource = readFileSync(new URL('./prefs.js', import.meta.url), 'utf8');
assert.match(prefsSource, /vendorList = \[[^\]]*'supergrok'/s,
    'SuperGrok must be available in the top-bar provider switches');

console.log('GNOME layout compatibility tests passed');
