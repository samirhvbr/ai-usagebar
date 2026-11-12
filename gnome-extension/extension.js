// AI Usage Bar — GNOME Shell indicator that renders ai-usagebar's
// 5-hour (session), weekly, and (optionally) extra-usage bars in the top
// panel next to the clock/network, with a native, aligned dropdown.
//
// The top bar reads the widget's Waybar JSON; provider submenus read
// `ai-usagebar usage --json`. Both commands run asynchronously, and the UI
// uses native St widgets. Bar colors are user-configurable.

import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Pango from 'gi://Pango';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {barMarkup, colorForDelta, colorForPct, disambiguateTags, field, FIELD, FORMAT, hasUsageWindows, integer,
    isGrouped, MARKER, markerElapsed, paceGlyph, panelSegments, plainTextFromPango, selectPools,
    splitFormatOutput} from './marker-logic.js';
import {commandFailure, errorLine, parseReport} from './report-model.js';
import {terminalCandidates} from './terminal.js';
import {apiVendorRows, configApiKeyEnv, configHasApiKey, configVendorEnabled,
    extractSnapshot, parseLastError, rowStatus,
    splitVendorSetting} from './api-status-logic.js';

const ROLE = 'ai-usagebar';

// Newer shells gave St.BoxLayout an `orientation` property and deprecated
// `vertical`. Shell 46 has only `vertical`, and GJS throws on a property the
// class does not have, so a hardcoded `orientation` kept the extension from
// loading there. Every vertical box goes through here; a contract test keeps
// it that way.
const HAS_ORIENTATION = !!GObject.Object.find_property.call(St.BoxLayout, 'orientation');

function verticalBox(props) {
    return new St.BoxLayout(HAS_ORIENTATION
        ? {orientation: Clutter.Orientation.VERTICAL, ...props}
        : {vertical: true, ...props});
}

// Report text (errors, block lines, details) has no length the menu can
// predict. Unwrapped, one long line sets the whole menu's width, past the edge
// of the screen; wrapped, it stays inside the menu's bounded width.
function wrappedLabel(text, styleClass) {
    const label = new St.Label({text, x_expand: true, style_class: styleClass});
    label.clutter_text.line_wrap = true;
    label.clutter_text.line_wrap_mode = Pango.WrapMode.WORD_CHAR;
    label.clutter_text.ellipsize = Pango.EllipsizeMode.NONE;
    return label;
}

// The detail bar fits inside the menu's bounded content width.
const PANEL_BAR_W = 320;

// A bar built from St widgets rather than █ cells: a track, a fill in the
// severity color, and an optional pace marker at the elapsed share of the
// window.
function barWidget(percent, width, height, color, elapsed) {
    const track = new St.Widget({
        style_class: 'aiub-track',
        width,
        height,
        y_align: Clutter.ActorAlign.CENTER,
    });
    if (percent > 0) {
        track.add_child(new St.Widget({
            width: Math.max(height, Math.round(width * percent / 100)),
            height,
            style: `background-color: ${color}; border-radius: ${height / 2}px;`,
        }));
    }
    if (Number.isFinite(elapsed)) {
        track.add_child(new St.Widget({
            x: Math.min(width - 2, Math.max(0, Math.round(width * elapsed / 100) - 1)),
            y: -3,
            width: 2,
            height: height + 6,
            style: `background-color: ${MARKER}; border-radius: 1px;`,
        }));
    }
    return track;
}

// Fixed accent colors (tags / dim text). Bar colors are user-configurable.
const DIM = '#5c6370';
const FG = '#abb2bf';
const RED = '#e06c75';
// Muted secondary text (the API rows' status line): between DIM and FG.
const MUTED = '#8b93a1';
// FORMAT's final ignored literal sentinel receives a stale suffix, keeping the
// preceding elapsed fields numeric. It and its field indexes live in marker-logic.
const REFRESH_TIMEOUT_SECS = 60;

function esc(s) {
    return String(s)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
}

// One subprocess slot: at most one run in flight, a request made meanwhile
// remembered and run once it settles, and a superseded or timed-out run never
// painted. See AiUsageBarIndicator._run.
function newJob() {
    return {busy: false, pending: false, token: 0, timeoutId: 0, cancellable: null, proc: null};
}

// The report as a plain document, or null. The multi-entry panel reads the
// raw `usage --json` entries (ids, metrics, window lengths); the menu reads
// the projection `parseReport` makes of the same output.
function parseJson(text) {
    try {
        return JSON.parse(text);
    } catch (e) {
        return null;
    }
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
    _init(settings, openPrefs, iconDir, vendor, backgroundApis) {
        super._init(0.0, 'AI Usage Bar', false);

        this._settings = settings;
        this._openPrefs = openPrefs;
        this._vendor = vendor;
        this._backgroundApis = backgroundApis;
        this._iconDir = iconDir;
        this._marks = new Map();
        this._data = null;          // parsed snapshot for redraws
        this._report = null;        // parsed `usage --json` for the menu
        this._panelDoc = null;      // the same report, raw, for the multi-entry panel
        this._panelError = '';      // why the top bar shows ⚠, shown in the menu
        this._tab = null;           // selected provider tab (entry id)
        this._reportAt = 0;
        this._destroyed = false;
        this._timer = 0;
        this._apiTimer = 0;
        this._apiCheckToken = 0;
        // The top bar (`--vendor --format`) and the menu (`usage --json`) are
        // separate commands on separate schedules; each gets its own slot.
        this._panelJob = newJob();
        this._reportJob = newJob();

        // Panel: one markup label holds tags + percentages + bars. The icon
        // stands in when the settings leave nothing for the label to draw,
        // so the indicator never collapses into an empty click target.
        this._label = new St.Label({
            text: '5h …',
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'aiub-label',
        });
        this._icon = new St.Icon({
            style_class: 'system-status-icon',
            y_align: Clutter.ActorAlign.CENTER,
            visible: false,
        });
        const panelBox = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        // With several indicators side by side, the brand mark ahead of the
        // bars tells them apart.
        const mark = this._panelEntryIds().length
            ? null : this._markIcon(splitVendorSetting(vendor).vendor);
        this._hasPanelMark = !!mark;
        if (mark) {
            panelBox.add_child(new St.Icon({gicon: mark, style_class: 'system-status-icon',
                y_align: Clutter.ActorAlign.CENTER}));
        }
        panelBox.add_child(this._icon);
        panelBox.add_child(this._label);
        this.add_child(panelBox);

        // The shell's default popup grey sits close to the bars' empty track,
        // which washes the tracks out. Tag the menu so the stylesheet can pin
        // it to the One Dark base the bars are themed against.
        this.menu.actor.add_style_class_name('aiub-menu');

        this._buildMenu();

        // Re-render cached data when any display setting changes (no refetch).
        const viewKeys = [
            'bar-width', 'show-percent', 'show-bars', 'show-pace', 'show-session',
            'show-weekly', 'show-extra', 'color-low', 'color-mid',
            'color-high', 'color-critical', 'color-empty',
            'panel-pools', 'panel-auto-threshold', 'panel-entries',
            'menu-summary-style', 'menu-show-icons', 'menu-compact',
        ];
        this._viewIds = viewKeys.map(k =>
            this._settings.connect(`changed::${k}`, () => this._render()));

        this._intervalId = this._settings.connect('changed::refresh-interval',
            () => this._restartTimer());
        this._apiIntervalId = this._settings.connect('changed::api-refresh-interval',
            () => this._restartApiTimer());
        this._sourceIds = [
            this._settings.connect('changed::binary-path', () => this._refreshAll()),
        ];

        this.menu.connect('open-state-changed', (_m, open) => {
            if (open)
                this._refreshAll();
        });

        this._refresh();
        this._restartTimer();
        this._restartApiTimer();
    }

    _buildMenu() {
        // One non-reactive row hosts the whole tabbed panel: header, provider
        // tabs, the selected provider's metrics, and a footer.
        this._panel = verticalBox({style_class: 'aiub-panel', x_expand: true});
        const host = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        host.add_child(this._panel);
        this.menu.addMenuItem(host);
        this._buildApiSection(apiVendorRows(this._readConfigText(), GLib.get_home_dir()));
        this.menu.addAction('Refresh now', () => this._refreshAll());
        this.menu.addAction('Open TUI', () => this._openTui());
        this.menu.addAction('Settings', () => this._openPrefs());
        this._paintReport(null);
    }

    // ── "Status das APIs" (port of the macOS menu bar section) ──────────
    // A native collapsible submenu; each visible row reads the vendor's
    // on-disk cache (usage.json age + .last_error) and the config — pure
    // local disk, no network. The only network path is "Verificar todas".
    _buildApiSection(rows, position) {
        this._apiSection = new PopupMenu.PopupSubMenuMenuItem('Status das APIs', false);

        const subhead = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        this._apiSubhead = new St.Label({x_expand: true, style_class: 'aiub-row-reset'});
        subhead.add_child(this._apiSubhead);
        this._apiSection.menu.addMenuItem(subhead);

        this._apiRows = [];
        this._apiRowKeys = rows.map(vendor => vendor.key);
        for (const vendor of rows) {
            const item = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
            // One line: dot, name, then value and age in right-aligned columns.
            // The age label carries a min-width so the values line up across
            // rows whose ages differ in length ("há 1m" vs "há 19h").
            const box = new St.BoxLayout({x_expand: true, style_class: 'aiub-api-row'});
            const dot = new St.Widget({style_class: 'aiub-api-dot',
                y_align: Clutter.ActorAlign.CENTER});
            const nameL = new St.Label({text: vendor.name, x_expand: true,
                style_class: 'aiub-api-name', y_align: Clutter.ActorAlign.CENTER});
            const detailL = new St.Label({style_class: 'aiub-api-detail',
                y_align: Clutter.ActorAlign.CENTER});
            const ageL = new St.Label({style_class: 'aiub-api-age',
                x_align: Clutter.ActorAlign.END, y_align: Clutter.ActorAlign.CENTER});
            box.add_child(dot);
            box.add_child(nameL);
            box.add_child(detailL);
            box.add_child(ageL);
            item.add_child(box);
            this._apiSection.menu.addMenuItem(item);
            this._apiRows.push({vendor, item, dot, detailL, ageL});
        }

        this._apiCheckItem = new PopupMenu.PopupMenuItem('Verificar todas agora');
        this._apiCheckItem.connect('activate', () => this._checkAllApis());
        this._apiSection.menu.addMenuItem(this._apiCheckItem);

        // A rebuild re-inserts where the old section sat, or the section
        // would jump below "Abrir TUI"/"Settings".
        this.menu.addMenuItem(this._apiSection, position);
        // Populate lazily: reading ~11 tiny local files is cheap, but there is
        // no reason to do it before the section is first expanded.
        this._apiSection.menu.connect('open-state-changed', (_m, open) => {
            if (open)
                this._refreshApiSection();
        });
    }

    _readFileText(path) {
        try {
            const [ok, bytes] = GLib.file_get_contents(path);
            if (!ok)
                return null;
            return new TextDecoder().decode(bytes);
        } catch (e) {
            return null; // missing file — the common case, not an error
        }
    }

    _fileAgeSecs(path) {
        try {
            const info = Gio.File.new_for_path(path)
                .query_info('time::modified', Gio.FileQueryInfoFlags.NONE, null);
            const mtime = info.get_attribute_uint64('time::modified');
            if (!mtime)
                return null;
            return Math.max(0, Math.trunc(GLib.get_real_time() / 1e6) - mtime);
        } catch (e) {
            return null; // no cache yet
        }
    }

    _readConfigText() {
        return this._readFileText(
            `${GLib.get_user_config_dir()}/ai-usagebar/config.toml`);
    }

    // Rebuild the rows when the config's account list changed under us —
    // adding `[[anthropic.accounts]]` should show up on the next open, not
    // after a shell restart. Returns the rows when they still match, and
    // `null` once a rebuild is queued: the caller is usually inside the
    // section's own `open-state-changed`, and destroying the section during
    // its emission is not worth the risk, so the swap happens on the next
    // idle and renders itself.
    _syncApiRows(configText) {
        if (!this._apiSection)
            return null;
        const rows = apiVendorRows(configText, GLib.get_home_dir());
        const keys = rows.map(row => row.key);
        if (this._apiRowKeys?.length === keys.length &&
            this._apiRowKeys.every((key, i) => key === keys[i]))
            return rows;
        if (this._apiRebuildId)
            return null;
        this._apiRebuildId = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this._apiRebuildId = 0;
            if (!this._apiSection)
                return GLib.SOURCE_REMOVE;
            const position = this.menu._getMenuItems().indexOf(this._apiSection);
            const wasOpen = this._apiSection.menu.isOpen;
            this._apiSection.destroy();
            this._apiSection = null;
            this._buildApiSection(rows, position >= 0 ? position : undefined);
            if (wasOpen)
                this._apiSection.menu.open(false);
            this._refreshApiSection();
            return GLib.SOURCE_REMOVE;
        });
        return null;
    }

    // The effective API-key env var: the config's per-vendor `api_key_env`
    // override wins (resolve_api_key in the binary checks it first), else the
    // vendor's default. Used for both the configured check and the row hint.
    _vendorEnvName(vendor, configText) {
        return configApiKeyEnv(configText, vendor.id) ?? vendor.env;
    }

    _vendorConfigured(vendor, configText) {
        if (vendor.kind === 'oauth') {
            const creds = vendor.credsPath ?? `${GLib.get_home_dir()}/${vendor.creds}`;
            return GLib.file_test(creds, GLib.FileTest.EXISTS);
        }
        const envName = this._vendorEnvName(vendor, configText);
        const env = envName ? GLib.getenv(envName) : null;
        if (env && env.trim())
            return true;
        return configHasApiKey(configText, vendor.id);
    }

    _refreshApiSection() {
        if (!this._apiRows)
            return;
        const configText = this._readConfigText();
        if (!this._syncApiRows(configText))
            return;
        const cacheBase = `${GLib.get_user_cache_dir()}/ai-usagebar`;
        const colors = this._colors();
        const stateColor = {low: colors.low, ok: colors.low, warn: colors.mid,
            error: colors.critical, off: DIM};
        const activeVendor = this._vendor;

        let shownAny = false;
        for (const row of this._apiRows) {
            const v = row.vendor;
            const enabled = configVendorEnabled(configText, v.id);
            const configured = this._vendorConfigured(v, configText);
            // Only vendors that are enabled AND have credentials get a row —
            // the panel is a health view, not a catalog (macOS parity).
            const show = enabled && configured;
            row.item.visible = show;
            if (!show)
                continue;
            shownAny = true;

            const dir = `${cacheBase}/${v.cacheDir ?? v.id}`;
            const lastError = parseLastError(this._readFileText(`${dir}/.last_error`));
            const ageSecs = this._fileAgeSecs(`${dir}/usage.json`);
            let snap = null;
            if (ageSecs != null) {
                try {
                    snap = extractSnapshot(v.id,
                        JSON.parse(this._readFileText(`${dir}/usage.json`) ?? ''));
                } catch (e) {
                    // corrupt cache → no headline; the state ladder still works
                }
            }
            const activePcts = v.key === activeVendor && this._data?.hasUsageWindows
                ? {session: this._data.session.pct, weekly: this._data.weekly.pct}
                : null;

            // Row hints must name the EFFECTIVE env var (api_key_env override).
            const vEff = v.kind === 'oauth' ? v : {...v, env: this._vendorEnvName(v, configText)};
            const st = rowStatus(vEff, {enabled, configured, lastError, ageSecs,
                snap, configText, activePcts});
            row.dot.style = `background-color: ${stateColor[st.state]};`;
            row.detailL.clutter_text.set_markup(`<span foreground="${
                st.state === 'error' ? colors.critical : MUTED}">${esc(st.detail)}</span>`);
            row.ageL.clutter_text.set_markup(st.age
                ? `<span foreground="${DIM}">há ${esc(st.age)}</span>` : '');
        }
        this._apiCheckItem.visible = shownAny;
        this._apiSubhead.clutter_text.set_markup(`<span foreground="${DIM}">${
            shownAny ? 'status do cache local — sem novas chamadas' : 'nenhuma API configurada'}</span>`);
    }

    // The only path that touches the network: run the binary once per enabled
    // + configured vendor (each run still honors the binary's own cache TTL,
    // so this is bounded), then re-read the caches into the rows.
    // `silent` is the background timer's path: same work, but it leaves the
    // subhead alone so a closed menu never flashes "verificando…" and an open
    // one does not shift under the pointer.
    _checkAllApis(silent = false) {
        if (!this._apiRows)
            return;
        const configText = this._readConfigText();
        const bin = resolveBinary(this._settings);
        const targets = this._apiRows
            .map(r => r.vendor)
            .filter(v => configVendorEnabled(configText, v.id) &&
                this._vendorConfigured(v, configText));
        if (!targets.length)
            return;
        const token = ++this._apiCheckToken;
        if (!silent) {
            this._apiSubhead.clutter_text.set_markup(
                `<span foreground="${DIM}">verificando…</span>`);
        }
        let pending = targets.length;
        for (const v of targets) {
            let proc;
            try {
                const argv = v.account
                    ? [bin, '--vendor', v.id, '--account', v.account, '--json']
                    : [bin, '--vendor', v.id, '--json'];
                proc = Gio.Subprocess.new(argv,
                    Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
            } catch (e) {
                pending -= 1;
                continue;
            }
            proc.wait_async(null, () => {
                pending -= 1;
                if (pending === 0 && this._apiCheckToken === token && this._apiRows)
                    this._refreshApiSection();
            });
        }
        if (pending === 0)
            this._refreshApiSection();
    }

    _markIcon(brand) {
        if (!brand)
            return null;
        if (!this._marks.has(brand)) {
            const file = this._iconDir.get_child(`${brand}-symbolic.svg`);
            this._marks.set(brand, file.query_exists(null) ? new Gio.FileIcon({file}) : null);
        }
        return this._marks.get(brand);
    }

    _iconButton(iconName, label, onClick) {
        const button = new St.Button({
            style_class: 'aiub-icon-button',
            can_focus: true,
            accessible_name: label,
            child: new St.Icon({icon_name: iconName, icon_size: 16}),
        });
        button.connect('clicked', onClick);
        return button;
    }

    _header(entry) {
        const header = new St.BoxLayout({x_expand: true, style_class: 'aiub-header'});
        const mark = this._settings.get_boolean('menu-show-icons')
            ? this._markIcon(entry.brand)
            : null;
        if (mark)
            header.add_child(new St.Icon({gicon: mark, icon_size: 28, style_class: 'aiub-header-icon',
                y_align: Clutter.ActorAlign.CENTER}));
        const titles = verticalBox({x_expand: true, y_align: Clutter.ActorAlign.CENTER});
        titles.add_child(new St.Label({text: entry.title, style_class: 'aiub-title'}));
        if (entry.plan)
            titles.add_child(new St.Label({text: entry.plan.toUpperCase(), style_class: 'aiub-plan'}));
        header.add_child(titles);
        const pcts = entry.rows.filter(r => r.type === 'metric' && Number.isFinite(r.percent)).map(r => r.percent);
        if (pcts.length) {
            header.add_child(new St.Label({text: `${Math.round(Math.max(...pcts))}%`, style_class: 'aiub-pill',
                y_align: Clutter.ActorAlign.CENTER}));
        }
        header.add_child(this._iconButton('view-refresh-symbolic', 'Refresh now', () => this._refreshAll()));
        header.add_child(this._iconButton('emblem-system-symbolic', 'Settings', () => {
            this.menu.close();
            this._openPrefs();
        }));
        return header;
    }

    _tabs(entries, selectedId) {
        const tabs = new St.BoxLayout({x_expand: true, style_class: 'aiub-tabs'});
        for (const e of entries) {
            const tab = new St.Button({
                label: e.title,
                can_focus: true,
                style_class: 'aiub-tab' + (e.id === selectedId ? ' aiub-tab-active' : ''),
            });
            tab.connect('clicked', () => {
                this._tab = e.id;
                this._paintReport(this._report);
            });
            tabs.add_child(tab);
        }
        return new St.ScrollView({
            hscrollbar_policy: St.PolicyType.AUTOMATIC,
            vscrollbar_policy: St.PolicyType.NEVER,
            overlay_scrollbars: true,
            child: tabs,
        });
    }

    _entryBody(entry, colors) {
        const body = verticalBox({x_expand: true, style_class: 'aiub-body'});
        body.add_child(new St.Label({text: 'USAGE & BALANCE', style_class: 'aiub-section'}));
        if (entry.error)
            body.add_child(wrappedLabel(entry.error, 'aiub-detail'));
        for (const row of entry.rows) {
            if (row.type === 'metric') {
                const box = verticalBox({x_expand: true, style_class: 'aiub-metric'});
                const top = new St.BoxLayout({x_expand: true});
                top.add_child(new St.Label({text: row.label, x_expand: true, style_class: 'aiub-heading'}));
                top.add_child(new St.Label({text: row.valueText, style_class: 'aiub-value'}));
                box.add_child(top);
                if (this._settings.get_string('menu-summary-style') === 'bars') {
                    box.add_child(barWidget(row.percent, PANEL_BAR_W, 5,
                        colors[row.severity] || colors.low, row.elapsed));
                }
                const reset = row.reset === 'now'
                    ? 'Resets now'
                    : row.reset
                        ? (row.regen ? `Regenerates ${row.regen} in ${row.reset}` : `Resets in ${row.reset}`)
                        : '';
                const detail = [reset, row.detail].filter(Boolean).join(' · ');
                if (detail)
                    box.add_child(wrappedLabel(detail, 'aiub-detail'));
                body.add_child(box);
            } else if (row.type === 'block') {
                if (row.label)
                    body.add_child(wrappedLabel(row.label, 'aiub-heading'));
                for (const line of row.body)
                    body.add_child(wrappedLabel(line, 'aiub-detail'));
            } else {
                const text = row.value ? `${row.label ? row.label + ': ' : ''}${row.value}` : row.label;
                body.add_child(wrappedLabel(text, row.value ? 'aiub-detail' : 'aiub-heading'));
            }
        }
        if (!entry.error && entry.rows.length === 0)
            body.add_child(wrappedLabel('No usage data reported', 'aiub-detail'));
        return body;
    }

    _updatedAgo() {
        if (!this._reportAt)
            return '';
        const mins = Math.floor((Date.now() - this._reportAt) / 60000);
        return `Updated ${mins < 1 ? 'just now' : `${mins}m ago`}`;
    }

    _paintReport(report) {
        const panel = this._panel;
        panel.set_style_class_name('aiub-panel' +
            (this._settings.get_boolean('menu-compact') ? ' aiub-compact' : ''));
        panel.destroy_all_children();
        if (this._panelError)
            panel.add_child(wrappedLabel(`Top bar: ${this._panelError}`, 'aiub-detail'));
        if (!report?.ok || report.entries.length === 0) {
            panel.add_child(wrappedLabel(!report ? 'Loading…' : report.ok ? 'No providers enabled' : report.error,
                'aiub-detail'));
            return;
        }
        const entries = report.entries;
        const entry = entries.find(e => e.id === this._tab) || entries[0];
        this._tab = entry.id;
        panel.add_child(this._header(entry));
        if (entries.length > 1)
            panel.add_child(this._tabs(entries, entry.id));
        panel.add_child(this._entryBody(entry, this._colors()));
        const ago = this._updatedAgo();
        if (ago)
            panel.add_child(new St.Label({text: ago, style_class: 'aiub-footer',
                x_align: Clutter.ActorAlign.CENTER}));
    }

    _colors() {
        const g = k => this._settings.get_string(k);
        return {
            low: g('color-low'),
            mid: g('color-mid'),
            high: g('color-high'),
            critical: g('color-critical'),
            empty: g('color-empty'),
        };
    }

    _restartTimer() {
        if (this._timer) {
            GLib.source_remove(this._timer);
            this._timer = 0;
        }
        const secs = Math.max(5, this._settings.get_int('refresh-interval'));
        this._timer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, secs, () => {
            if (this.menu.isOpen)
                this._refreshAll();
            else
                this._refresh();
            return GLib.SOURCE_CONTINUE;
        });
    }

    // The main timer only fetches the vendor on the panel, so every other
    // vendor's cache — and the row that reads it — ages indefinitely. This
    // refreshes all of them in the background at a much slower cadence
    // (LOW priority: it is never what the user is waiting for). 0 = off.
    _restartApiTimer() {
        if (this._apiTimer) {
            GLib.source_remove(this._apiTimer);
            this._apiTimer = 0;
        }
        // Only the first indicator refreshes all providers in the background.
        if (!this._backgroundApis)
            return;
        const secs = this._settings.get_int('api-refresh-interval');
        if (secs <= 0)
            return;
        this._apiTimer = GLib.timeout_add_seconds(GLib.PRIORITY_LOW, secs, () => {
            this._checkAllApis(true);
            return GLib.SOURCE_CONTINUE;
        });
    }

    // Spawn `argv` in `job`'s slot (see newJob). `handlers.done(out, err, ok)`
    // receives a finished, current run's output; `handlers.failed(short,
    // detail)` a spawn failure, a timeout, or output that could not be read;
    // `handlers.again()` re-requests a run that was asked for while busy.
    _run(job, argv, handlers) {
        if (this._destroyed)
            return;
        if (job.busy) {
            job.pending = true;
            return;
        }
        job.busy = true;
        const token = ++job.token;
        const cancellable = new Gio.Cancellable();
        job.cancellable = cancellable;
        // Run whatever was requested while we were busy — never after
        // destroy, where it would spawn into a torn-down indicator.
        const again = () => {
            if (job.pending && !this._destroyed) {
                job.pending = false;
                handlers.again();
            }
        };

        let proc;
        try {
            proc = new Gio.Subprocess({
                argv,
                flags: Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE,
            });
            proc.init(cancellable);
        } catch (e) {
            job.busy = false;
            job.cancellable = null;
            job.pending = false;
            handlers.failed(`could not run "${argv[0]}"`, String(e));
            return;
        }
        job.proc = proc;

        let timedOut = false;
        const timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, REFRESH_TIMEOUT_SECS, () => {
            timedOut = true;
            if (job.timeoutId === timeoutId)
                job.timeoutId = 0;
            try {
                proc.force_exit();
            } catch (e) {}
            cancellable.cancel();
            if (job.token === token) {
                job.busy = false;
                handlers.failed('ai-usagebar took too long', `timed out after ${REFRESH_TIMEOUT_SECS}s`);
                // Do not strand a request that arrived while this one hung.
                again();
            }
            return GLib.SOURCE_REMOVE;
        });
        job.timeoutId = timeoutId;

        const cleanup = () => {
            if (job.timeoutId === timeoutId) {
                GLib.source_remove(timeoutId);
                job.timeoutId = 0;
            }
            if (job.cancellable === cancellable)
                job.cancellable = null;
            if (job.proc === proc)
                job.proc = null;
        };

        proc.communicate_utf8_async(null, cancellable, (p, res) => {
            // A superseded attempt must not paint: its output belongs to
            // whatever was selected when it started.
            const current = job.token === token && !this._destroyed;
            if (current)
                job.busy = false;
            try {
                const [, out, err] = p.communicate_utf8_finish(res);
                cleanup();
                if (timedOut || !current)
                    return;
                handlers.done(out || '', err || '', p.get_successful());
            } catch (e) {
                cleanup();
                if (current && !(e instanceof GLib.Error &&
                      e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED)) && !timedOut)
                    handlers.failed('could not read the output', String(e));
            } finally {
                if (current)
                    again();
            }
        });
    }

    _stop(job) {
        job.pending = false;
        if (job.timeoutId) {
            GLib.source_remove(job.timeoutId);
            job.timeoutId = 0;
        }
        if (job.cancellable)
            job.cancellable.cancel();
        if (job.proc) {
            try {
                job.proc.force_exit();
            } catch (e) {}
            job.proc = null;
        }
    }

    // The top bar and the menu together: what opening the menu, "Refresh now"
    // and a new binary path ask for. With entries selected `_refresh` already
    // runs the report, so it is not asked for twice.
    _refreshAll() {
        this._refresh();
        if (!this._panelEntryIds().length)
            this._refreshReport();
    }

    _refresh() {
        // A saved aggregate selection keeps the fork's single report-driven
        // strip; otherwise each indicator owns one fixed vendor/account.
        if (this._panelEntryIds().length) {
            this._refreshReport();
            return;
        }
        const {vendor, account} = splitVendorSetting(this._vendor);
        const bin = resolveBinary(this._settings);
        const argv = account
            ? [bin, '--vendor', vendor, '--account', account, '--format', FORMAT]
            : [bin, '--vendor', vendor, '--format', FORMAT];
        this._run(this._panelJob, argv, {
            again: () => this._refresh(),
            // Entries took the top bar over while this ran: it is not theirs
            // to paint, and its error does not describe what the bar shows.
            failed: (short, detail) => {
                if (!this._panelEntryIds().length)
                    this._setError(short, detail);
            },
            done: (out, err, ok) => {
                if (!out.trim() && !ok) {
                    this._setError('ai-usagebar failed', err);
                    return;
                }
                this._consume(out);
            },
        });
    }

    // The aggregate report — one entry per provider AND per named account.
    // The menu lists it, and it is the only source that can put two Claude
    // accounts and a Codex on the panel at once: one process per refresh
    // regardless of how many entries are selected, and each entry still
    // honors the binary's own cache TTL.
    _refreshReport() {
        const argv = [resolveBinary(this._settings), 'usage', '--json'];
        this._run(this._reportJob, argv, {
            again: () => this._refreshReport(),
            failed: (short, detail) =>
                this._showReport({ok: false, error: errorLine(short, detail), entries: []}, null),
            done: (out, err, ok) => {
                if (!out.trim() && !ok) {
                    this._showReport(commandFailure(err), null);
                    return;
                }
                // `usage` exits 0 with per-entry errors inside the document,
                // so a parse is the only real verdict for the panel.
                this._showReport(parseReport(out), parseJson(out));
            },
        });
    }

    // `doc` is null when the command failed: the panel then keeps what it
    // last drew, and the menu says why.
    _showReport(report, doc) {
        this._report = report;
        this._panelDoc = doc;
        const ids = this._panelEntryIds();
        if (ids.length && doc)
            this._renderPanelEntries(ids, this._colors());
        if (report?.ok)
            this._reportAt = Date.now();
        this._paintReport(report);
    }

    // The ids the panel draws side by side. Empty means the classic
    // single-provider panel, driven by the `vendor` key.
    _panelEntryIds() {
        return this._settings.get_strv('panel-entries').filter(id => id);
    }

    _consume(stdout) {
        let data;
        try {
            data = JSON.parse(stdout);
        } catch (e) {
            this._setError('invalid output', stdout);
            return;
        }
        // The command answered, so a reason left over from a failed run no
        // longer describes the top bar.
        if (this._panelError) {
            this._panelError = '';
            this._paintReport(this._report);
        }
        const raw = plainTextFromPango(data.text);
        const f = splitFormatOutput(raw);
        if (f.length <= FIELD.extraLimit) {
            // Loading… / ⚠ — show the binary's own text.
            this._data = null;
            this._setPanelMarkup(`<span foreground="${FG}">${esc(raw) || '…'}</span>`);
            return;
        }
        // Only what the top bar draws. The click menu reads `usage --json`.
        this._data = {
            hasUsageWindows: hasUsageWindows(f[FIELD.vendorShort]),
            grouped: isGrouped(f[FIELD.sessionModel]),
            session: {pct: integer(f[FIELD.sessionPct]),
                model: field(f[FIELD.sessionModel]),
                elapsed: markerElapsed(field(f[FIELD.sessionReset]), integer(f[FIELD.sessionElapsed]))},
            weekly: {pct: integer(f[FIELD.weeklyPct]),
                model: field(f[FIELD.weeklyModel]),
                elapsed: markerElapsed(field(f[FIELD.weeklyReset]), integer(f[FIELD.weeklyElapsed]))},
            // Per-model weekly bar: a non-empty scoped model is the presence
            // signal. A reset may be unavailable, which must not make us show
            // the unrelated legacy Sonnet window instead.
            sonnet: (() => {
                const scopedModel = field(f[FIELD.scopedModel]);
                if (scopedModel) {
                    const scopedPct = integer(f[FIELD.scopedPct]);
                    if (scopedPct != null && scopedPct >= 0 && scopedPct <= 100)
                        return {pct: scopedPct, model: scopedModel,
                            elapsed: markerElapsed(field(f[FIELD.scopedReset]), integer(f[FIELD.scopedElapsed]))};
                    // A scoped model with malformed data is unavailable; do
                    // not fall back to a potentially unrelated Sonnet window.
                    return {pct: null, model: scopedModel, elapsed: null};
                }
                return {pct: integer(f[FIELD.sonnetPct]), model: '', elapsed: null};
            })(),
            // A named extra window (model + reset) renders as a percentage bar;
            // without a name the slot stays a spent/limit money budget.
            extra: {pct: integer(f[FIELD.extraPct]), spent: field(f[FIELD.extraSpent]),
                limit: field(f[FIELD.extraLimit]), model: field(f[FIELD.extraModel]),
                elapsed: markerElapsed(field(f[FIELD.extraReset]), integer(f[FIELD.extraElapsed]))},
        };
        this._render();
    }

    // Redraw both the panel and the dropdown from cached data + settings.
    // With entries selected the panel draws the report rather than the
    // `vendor` fetch, so a panel full of accounts does not depend on it.
    _render() {
        const colors = this._colors();
        const ids = this._panelEntryIds();
        if (ids.length)
            this._renderPanelEntries(ids, colors);
        else if (this._data)
            this._renderPanel(this._data, colors);
        this._paintReport(this._report);
    }

    // One segment per selected entry per selected window, in selection order.
    // Everything shown comes from the report: the label, the window length,
    // the percentage and the error text. A selected entry that is switched off
    // in config.toml still gets a muted segment, because a silently missing
    // one reads as a bug in the extension.
    _renderPanelEntries(ids, colors) {
        if (!this._panelDoc)
            return;
        const w = Math.max(4, Math.min(20, this._settings.get_int('bar-width')));
        const showPct = this._settings.get_boolean('show-percent');
        const showBars = this._settings.get_boolean('show-bars');
        const windows = {
            session: this._settings.get_boolean('show-session'),
            weekly: this._settings.get_boolean('show-weekly'),
        };
        const parts = [];
        for (const s of panelSegments(this._panelDoc, ids, windows)) {
            const tag = `<span foreground="${DIM}">${esc(s.tag)}${
                s.window ? ` ${esc(s.window)}` : ''}</span>`;
            if (s.status === 'error') {
                parts.push(`${tag} <span foreground="${colors.critical}">⚠</span>`);
                continue;
            }
            if (s.status === 'absent' || s.pct == null) {
                parts.push(`${tag} <span foreground="${DIM}">—</span>`);
                continue;
            }
            const toks = [tag];
            // Pacing markers are absent on purpose: the report states elapsed
            // only inside prose, and deriving it here is Rust's job.
            if (showPct || !showBars)
                toks.push(`<span foreground="${colorForPct(s.pct, colors)}">${esc(s.value) ||
                    `${s.pct}%`}</span>`);
            if (showBars)
                toks.push(barMarkup(s.pct, w, colors, null));
            if (s.stale)
                toks.push(`<span foreground="${DIM}">⏸</span>`);
            parts.push(toks.join(' '));
        }
        const gap = `<span foreground="${DIM}">   </span>`;
        this._setPanelMarkup(parts.join(gap));
    }

    _renderPanel(d, colors) {
        const w = Math.max(4, Math.min(20, this._settings.get_int('bar-width')));
        const showPct = this._settings.get_boolean('show-percent');
        const showBars = this._settings.get_boolean('show-bars');
        const showPace = !showBars && this._settings.get_boolean('show-pace');

        const seg = (tag, pct, valueText, elapsed) => {
            // Pace mode: without bars there is no pace marker, so the value
            // carries the pace instead, colored by usage minus elapsed, with
            // its arrow.
            const paced = showPace && Number.isFinite(elapsed);
            const color = paced ? colorForDelta(pct - elapsed, colors) : colorForPct(pct, colors);
            const text = paced ? `${valueText}${paceGlyph(pct - elapsed)}` : valueText;
            const toks = [`<span foreground="${DIM}">${tag}</span>`];
            if (showPct || !showBars) // never render an empty segment
                toks.push(`<span foreground="${color}">${esc(text)}</span>`);
            if (showBars)
                toks.push(barMarkup(pct, w, colors, elapsed));
            return toks.join(' ');
        };

        const showSession = this._settings.get_boolean('show-session');
        const showWeekly = this._settings.get_boolean('show-weekly');
        const parts = [];

        if (d.grouped) {
            // Two independent pools. panel-pools picks the pools, show-session /
            // show-weekly still pick the windows, so segments are pools ×
            // windows and "just the 5h of both" needs no mode of its own.
            for (const pool of this._selectedPools(d, showSession, showWeekly)) {
                if (showSession && pool.session.pct != null) {
                    parts.push(seg(`${pool.tag} 5h`, pool.session.pct,
                        `${pool.session.pct}%`, pool.session.elapsed));
                }
                if (showWeekly && pool.weekly.pct != null) {
                    parts.push(seg(`${pool.tag} 7d`, pool.weekly.pct,
                        `${pool.weekly.pct}%`, pool.weekly.elapsed));
                }
            }
        } else {
            if (d.hasUsageWindows && showSession && d.session.pct != null)
                parts.push(seg('5h', d.session.pct, `${d.session.pct}%`, d.session.elapsed));
            if (d.hasUsageWindows && showWeekly && d.weekly.pct != null)
                parts.push(seg('7d', d.weekly.pct, `${d.weekly.pct}%`, d.weekly.elapsed));
            if (this._settings.get_boolean('show-extra') &&
                d.extra.pct != null && d.extra.spent && d.extra.limit)
                parts.push(seg('ex', d.extra.pct, d.extra.spent, null)); // $ budget → no meta
        }

        const gap = `<span foreground="${DIM}">   </span>`;
        this._setPanelMarkup(parts.join(gap));
    }

    // Every top-bar write goes through here. Empty markup, e.g. both windows
    // switched off, hides the label; what stays visible is decided below.
    _setPanelMarkup(markup) {
        const empty = !markup;
        // The aggregate strip has no prepended mark. Its empty state gets
        // the legacy icon; separate indicators already own a brand mark.
        const standIn = empty && !this._hasPanelMark;
        if (standIn) {
            const mark = this._markIcon(splitVendorSetting(this._vendor).vendor);
            if (mark)
                this._icon.gicon = mark;
            else
                this._icon.icon_name = 'application-x-executable-symbolic';
        }
        this._icon.visible = standIn;
        this._label.visible = !empty;
        this._label.clutter_text.set_markup(markup);
    }

    // The pools the panel should draw, tagged and in display order. Primary is
    // the generic session/weekly pair; secondary reuses the scoped and extra
    // slots, which for a grouped vendor hold the second pool's two windows.
    _selectedPools(d, showSession, showWeekly) {
        // Either secondary window may be absent. Derive its tag from whichever
        // model-bearing slot exists instead of assuming the weekly one does.
        const secondaryModel = d.sonnet.model || d.extra.model;
        const [primaryTag, secondaryTag] = disambiguateTags(d.session.model, secondaryModel);
        const primary = {tag: primaryTag, session: d.session, weekly: d.weekly};
        const secondary = {tag: secondaryTag, session: d.sonnet, weekly: d.extra};
        const pct = pool => ({
            session: pool.session.pct,
            weekly: pool.weekly.pct,
        });
        const pools = {primary, secondary};
        return selectPools(pct(primary), pct(secondary),
            this._settings.get_string('panel-pools'),
            this._settings.get_int('panel-auto-threshold'),
            {session: showSession, weekly: showWeekly})
            .map(name => pools[name]);
    }

    _setError(short, detail) {
        this._data = null;
        // The top bar stays a compact ⚠; the reason is the menu's first line.
        this._setPanelMarkup(`<span foreground="${RED}">⚠ ai</span>`);
        this._panelError = errorLine(short, detail);
        this._paintReport(this._report);
    }

    _openTui() {
        const tui = GLib.find_program_in_path('ai-usagebar-tui') ||
            `${GLib.get_home_dir()}/.cargo/bin/ai-usagebar-tui`;
        const line = `export PATH="$HOME/.local/bin:$PATH" && ${GLib.shell_quote(tui)}`;
        for (const argv of terminalCandidates(line)) {
            if (!GLib.find_program_in_path(argv[0]))
                continue;
            try {
                Gio.Subprocess.new(argv, Gio.SubprocessFlags.NONE);
                return;
            } catch (e) {
                // try the next terminal
            }
        }
        Main.notify('AI Usage Bar', 'No terminal found (ptyxis / kgx / gnome-terminal / xterm).');
    }

    destroy() {
        this._destroyed = true;
        if (this._timer) {
            GLib.source_remove(this._timer);
            this._timer = 0;
        }
        if (this._apiTimer) {
            GLib.source_remove(this._apiTimer);
            this._apiTimer = 0;
        }
        if (this._apiRebuildId) {
            GLib.source_remove(this._apiRebuildId);
            this._apiRebuildId = 0;
        }
        this._stop(this._panelJob);
        this._stop(this._reportJob);
        for (const id of this._viewIds ?? [])
            this._settings.disconnect(id);
        for (const id of this._sourceIds ?? [])
            this._settings.disconnect(id);
        if (this._intervalId)
            this._settings.disconnect(this._intervalId);
        if (this._apiIntervalId)
            this._settings.disconnect(this._apiIntervalId);
        this._viewIds = this._sourceIds = null;
        this._intervalId = 0;
        this._apiIntervalId = 0;
        // Orphan any in-flight "Verificar todas" callbacks: they check
        // _apiRows before touching destroyed actors.
        this._apiRows = null;
        this._apiCheckToken += 1;
        super.destroy();
    }
});

export default class AiUsageBarExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._indicators = [];
        this._place();
        this._placeIds = [
            this._settings.connect('changed::panel-box', () => this._place()),
            this._settings.connect('changed::panel-index', () => this._place()),
            this._settings.connect('changed::vendor', () => this._place()),
            this._settings.connect('changed::panel-entries', () => this._place()),
        ];
    }

    // `vendor` takes a comma-separated list ("anthropic,openai"): one
    // indicator per provider, side by side in the order given. An empty list
    // (every vendor switch off in prefs) means NO indicator — the panel must
    // agree with prefs rather than silently defaulting to one provider.
    _vendors() {
        const list = this._settings.get_string('vendor').split(',')
            .map(v => v.trim()).filter(Boolean);
        // Legacy combined mode draws the selected report entries only once,
        // even when the new per-provider setting lists several indicators.
        if (this._settings.get_strv('panel-entries').some(Boolean))
            return [list[0] || 'anthropic'];
        return [...new Set(list)];
    }

    _place() {
        this._clear();
        const box = this._settings.get_string('panel-box') || 'right';
        const index = Math.max(0, this._settings.get_int('panel-index'));
        this._vendors().forEach((vendor, i) => {
            const indicator = new Indicator(this._settings, () => this.openPreferences(),
                this.dir.get_child('icons'), vendor, i === 0);
            Main.panel.addToStatusArea(`${ROLE}:${vendor}`, indicator, index + i, box);
            this._indicators.push({role: `${ROLE}:${vendor}`, indicator});
        });
    }

    _clear() {
        for (const {role, indicator} of this._indicators) {
            indicator.destroy();
            delete Main.panel.statusArea[role];
        }
        this._indicators = [];
    }

    disable() {
        for (const id of this._placeIds ?? [])
            this._settings.disconnect(id);
        this._placeIds = null;
        this._clear();
        this._settings = null;
    }
}
