import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const source = readFileSync(new URL('./prefs.js', import.meta.url), 'utf8');

// The first page holds Display/Menu/Colours/Data/Position: it needs its own
// tab label and icon, not a blank one next to "Vendors".
const firstPage = source.match(/new Adw\.PreferencesPage\(\{[\s\S]*?\}\)/);
assert.ok(firstPage, 'the first page must be constructed with properties');
assert.match(firstPage[0], /title:/, 'the first page must carry a tab title');
assert.match(firstPage[0], /icon_name:/, 'the first page must carry a tab icon');

// use-markup defaults FALSE on Adw rows, so escaping a subtitle shows the
// literal &amp;. Subtitles stay plain text.
assert.doesNotMatch(source, /subtitle:\s*GLib\.markup_escape_text/,
    'no subtitle may be wrapped in GLib.markup_escape_text');

// Translatable strings are English; these Portuguese leftovers slipped in.
for (const pt of [
    'o custo extra ($) como terceira barra',
    'Pools no painel',
    'Antigravity detectado',
])
    assert.ok(!source.includes(pt), `Portuguese leftover in prefs.js: ${pt}`);

console.log('GNOME prefs tests passed');
