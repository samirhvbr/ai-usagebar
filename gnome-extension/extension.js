// AI Usage Bar — GNOME Shell indicator that renders ai-usagebar's
// 5-hour (session) and weekly bars in the top panel, next to the
// clock/network, with the full bordered tooltip in a dropdown.
//
// It shells out to the `ai-usagebar` binary (always exits 0, emits Waybar
// JSON `{text, tooltip, class}`) and draws the bars natively. Colors mirror
// the binary's default One Dark theme + severity_for() thresholds, so the
// panel looks identical to the Waybar widget.

import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

const ROLE = 'ai-usagebar';

// One Dark palette — matches ai-usagebar's default theme (src/theme.rs).
const C = {
    green: '#98c379',
    yellow: '#e5c07b',
    orange: '#d19a66',
    red: '#e06c75',
    empty: '#3e4451',
    fg: '#abb2bf',
    dim: '#5c6370',
};

// severity_for(pct) from src/pango.rs: >=90 red, >=75 orange, >=50 yellow, else green.
function colorForPct(pct) {
    if (pct >= 90)
        return C.red;
    if (pct >= 75)
        return C.orange;
    if (pct >= 50)
        return C.yellow;
    return C.green;
}

function esc(s) {
    return String(s)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
}

// Two-segment block bar as Pango markup, `width` cells wide.
function barMarkup(pct, width) {
    const p = Math.max(0, Math.min(100, Math.round(pct)));
    const filled = Math.round((p * width) / 100);
    const empty = width - filled;
    return `<span foreground="${colorForPct(p)}">${'█'.repeat(filled)}</span>` +
        `<span foreground="${C.empty}">${'░'.repeat(empty)}</span>`;
}

function resolveBinary(settings) {
    const configured = settings.get_string('binary-path');
    if (configured && GLib.file_test(configured, GLib.FileTest.IS_EXECUTABLE))
        return configured;
    const onPath = GLib.find_program_in_path('ai-usagebar');
    if (onPath)
        return onPath;
    const cargo = `${GLib.get_home_dir()}/.cargo/bin/ai-usagebar`;
    if (GLib.file_test(cargo, GLib.FileTest.IS_EXECUTABLE))
        return cargo;
    return 'ai-usagebar';
}

const Indicator = GObject.registerClass(
class AiUsageBarIndicator extends PanelMenu.Button {
    _init(settings, openPrefs) {
        super._init(0.0, 'AI Usage Bar', false);

        this._settings = settings;
        this._openPrefs = openPrefs;
        this._last = null;          // {sess, week} cache for redraws
        this._busy = false;
        this._timer = 0;
        this._cancellable = new Gio.Cancellable();

        // Panel: one markup label holds tags + percentages + bars.
        this._label = new St.Label({
            text: '5h …',
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'aiub-label',
        });
        this.add_child(this._label);

        // Dropdown: the binary's full bordered tooltip (monospace) + actions.
        this._tipItem = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        this._tipLabel = new St.Label({style_class: 'aiub-tip'});
        this._tipLabel.clutter_text.line_wrap = false;
        this._tipItem.add_child(this._tipLabel);
        this.menu.addMenuItem(this._tipItem);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        const refreshItem = new PopupMenu.PopupMenuItem('Atualizar agora');
        refreshItem.connect('activate', () => this._refresh());
        this.menu.addMenuItem(refreshItem);

        const tuiItem = new PopupMenu.PopupMenuItem('Abrir TUI');
        tuiItem.connect('activate', () => this._openTui());
        this.menu.addMenuItem(tuiItem);

        const prefsItem = new PopupMenu.PopupMenuItem('Configurações');
        prefsItem.connect('activate', () => this._openPrefs());
        this.menu.addMenuItem(prefsItem);

        // Re-render cached data when display settings change.
        this._redrawIds = [
            'changed::bar-width',
            'changed::show-percent',
            'changed::show-session',
            'changed::show-weekly',
        ].map(sig => this._settings.connect(sig, () => this._redraw()));

        // Re-arm the timer when the interval changes; refetch when the
        // source (vendor / binary) changes.
        this._intervalId = this._settings.connect('changed::refresh-interval',
            () => this._restartTimer());
        this._sourceIds = [
            this._settings.connect('changed::vendor', () => this._refresh()),
            this._settings.connect('changed::binary-path', () => this._refresh()),
        ];

        // Refresh on click-to-open, so the dropdown is never stale.
        this.menu.connect('open-state-changed', (_m, open) => {
            if (open)
                this._refresh();
        });

        this._refresh();
        this._restartTimer();
    }

    _restartTimer() {
        if (this._timer) {
            GLib.source_remove(this._timer);
            this._timer = 0;
        }
        const secs = Math.max(5, this._settings.get_int('refresh-interval'));
        this._timer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, secs, () => {
            this._refresh();
            return GLib.SOURCE_CONTINUE;
        });
    }

    _refresh() {
        if (this._busy)
            return;
        this._busy = true;

        const bin = resolveBinary(this._settings);
        const vendor = this._settings.get_string('vendor') || 'anthropic';
        const argv = [bin, '--vendor', vendor, '--format', '{session_pct};;{weekly_pct}'];

        let proc;
        try {
            proc = new Gio.Subprocess({
                argv,
                flags: Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE,
            });
            proc.init(this._cancellable);
        } catch (e) {
            this._busy = false;
            this._setError(`não consegui executar "${bin}"`, String(e));
            return;
        }

        proc.communicate_utf8_async(null, this._cancellable, (p, res) => {
            this._busy = false;
            try {
                const [, out, err] = p.communicate_utf8_finish(res);
                if ((!out || !out.trim()) && !p.get_successful()) {
                    this._setError('ai-usagebar falhou', err || '');
                    return;
                }
                this._consume(out || '');
            } catch (e) {
                if (!(e instanceof GLib.Error &&
                      e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED)))
                    this._setError('erro ao ler a saída', String(e));
            }
        });
    }

    _consume(stdout) {
        let data;
        try {
            data = JSON.parse(stdout);
        } catch (e) {
            this._setError('saída inválida', stdout);
            return;
        }
        const text = (data.text ?? '').toString();
        const tip = (data.tooltip ?? '').toString();
        const m = text.match(/(\d+);;(\d+)/);
        if (m) {
            this._last = {sess: parseInt(m[1], 10), week: parseInt(m[2], 10)};
            this._redraw();
        } else {
            // Loading… / ⚠ — show the binary's own text, stripped of markup.
            this._last = null;
            this._label.clutter_text.set_markup(
                `<span foreground="${C.fg}">${esc(text.replace(/<[^>]+>/g, ''))}</span>`);
        }
        this._setTip(tip);
    }

    _redraw() {
        if (!this._last)
            return;
        const {sess, week} = this._last;
        const w = Math.max(4, Math.min(20, this._settings.get_int('bar-width')));
        const showPct = this._settings.get_boolean('show-percent');
        const parts = [];
        if (this._settings.get_boolean('show-session')) {
            let s = `<span foreground="${C.dim}">5h </span>`;
            if (showPct)
                s += `<span foreground="${colorForPct(sess)}">${sess}% </span>`;
            parts.push(s + barMarkup(sess, w));
        }
        if (this._settings.get_boolean('show-weekly')) {
            let s = `<span foreground="${C.dim}">7d </span>`;
            if (showPct)
                s += `<span foreground="${colorForPct(week)}">${week}% </span>`;
            parts.push(s + barMarkup(week, w));
        }
        const gap = `<span foreground="${C.dim}">   </span>`;
        this._label.clutter_text.set_markup(parts.join(gap) || ' ');
    }

    _setTip(tipMarkup) {
        const ok = tipMarkup && tipMarkup.trim();
        if (ok)
            this._tipLabel.clutter_text.set_markup(tipMarkup);
        this._tipItem.visible = !!ok;
    }

    _setError(short, detail) {
        this._last = null;
        this._label.clutter_text.set_markup(`<span foreground="${C.red}">⚠ ai</span>`);
        const msg = detail ? `${short}\n\n${esc(detail).slice(0, 400)}` : short;
        this._tipLabel.clutter_text.set_markup(`<span foreground="${C.fg}">${esc(msg)}</span>`);
        this._tipItem.visible = true;
    }

    _openTui() {
        const tui = GLib.find_program_in_path('ai-usagebar-tui') ||
            `${GLib.get_home_dir()}/.cargo/bin/ai-usagebar-tui`;
        const candidates = [
            ['kgx', '--', tui],
            ['gnome-terminal', '--', tui],
            ['xterm', '-e', tui],
        ];
        for (const argv of candidates) {
            if (!GLib.find_program_in_path(argv[0]))
                continue;
            try {
                GLib.spawn_async(null, argv, null,
                    GLib.SpawnFlags.SEARCH_PATH | GLib.SpawnFlags.DO_NOT_REAP_CHILD, null);
                return;
            } catch (e) {
                // try the next terminal
            }
        }
        Main.notify('AI Usage Bar', 'Nenhum terminal encontrado (kgx / gnome-terminal / xterm).');
    }

    destroy() {
        if (this._timer) {
            GLib.source_remove(this._timer);
            this._timer = 0;
        }
        this._cancellable.cancel();
        for (const id of this._redrawIds ?? [])
            this._settings.disconnect(id);
        for (const id of this._sourceIds ?? [])
            this._settings.disconnect(id);
        if (this._intervalId)
            this._settings.disconnect(this._intervalId);
        this._redrawIds = this._sourceIds = null;
        this._intervalId = 0;
        super.destroy();
    }
});

export default class AiUsageBarExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._place();
        this._placeIds = [
            this._settings.connect('changed::panel-box', () => this._place()),
            this._settings.connect('changed::panel-index', () => this._place()),
        ];
    }

    _place() {
        // Defensive: free the role if a previous indicator is still registered.
        const existing = Main.panel.statusArea[ROLE];
        if (existing) {
            existing.destroy();
            delete Main.panel.statusArea[ROLE];
        }
        this._indicator = new Indicator(this._settings, () => this.openPreferences());
        const box = this._settings.get_string('panel-box') || 'right';
        const index = Math.max(0, this._settings.get_int('panel-index'));
        Main.panel.addToStatusArea(ROLE, this._indicator, index, box);
    }

    disable() {
        for (const id of this._placeIds ?? [])
            this._settings.disconnect(id);
        this._placeIds = null;
        if (this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }
        delete Main.panel.statusArea[ROLE];
        this._settings = null;
    }
}
