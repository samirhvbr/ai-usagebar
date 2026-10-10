import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

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

console.log('GNOME layout compatibility tests passed');
