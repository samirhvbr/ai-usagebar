// Preferences (libadwaita) for AI Usage Bar.

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';
import Gio from 'gi://Gio';

import {ExtensionPreferences, gettext as _} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs/prefs.js';

// Bind an Adw.ComboRow (index-based) to a string GSetting via a value table.
function bindCombo(settings, key, comboRow, values) {
    const sync = () => {
        const idx = values.indexOf(settings.get_string(key));
        comboRow.selected = idx < 0 ? 0 : idx;
    };
    sync();
    comboRow.connect('notify::selected', () => {
        const v = values[comboRow.selected];
        if (v !== undefined && v !== settings.get_string(key))
            settings.set_string(key, v);
    });
    const id = settings.connect(`changed::${key}`, sync);
    comboRow.connect('destroy', () => settings.disconnect(id));
}

export default class AiUsageBarPrefs extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();
        const page = new Adw.PreferencesPage();
        window.add(page);

        // ── Display ──────────────────────────────────────────────────────
        const display = new Adw.PreferencesGroup({title: _('Exibição')});
        page.add(display);

        const showSession = new Adw.SwitchRow({title: _('Mostrar barra de 5h (sessão)')});
        settings.bind('show-session', showSession, 'active', Gio.SettingsBindFlags.DEFAULT);
        display.add(showSession);

        const showWeekly = new Adw.SwitchRow({title: _('Mostrar barra semanal')});
        settings.bind('show-weekly', showWeekly, 'active', Gio.SettingsBindFlags.DEFAULT);
        display.add(showWeekly);

        const showPercent = new Adw.SwitchRow({title: _('Mostrar porcentagem')});
        settings.bind('show-percent', showPercent, 'active', Gio.SettingsBindFlags.DEFAULT);
        display.add(showPercent);

        const barWidth = new Adw.SpinRow({
            title: _('Largura de cada barra (células)'),
            adjustment: new Gtk.Adjustment({lower: 4, upper: 20, step_increment: 1, page_increment: 2}),
        });
        settings.bind('bar-width', barWidth, 'value', Gio.SettingsBindFlags.DEFAULT);
        display.add(barWidth);

        // ── Data ─────────────────────────────────────────────────────────
        const data = new Adw.PreferencesGroup({title: _('Dados')});
        page.add(data);

        const interval = new Adw.SpinRow({
            title: _('Intervalo de atualização (s)'),
            adjustment: new Gtk.Adjustment({lower: 5, upper: 3600, step_increment: 5, page_increment: 30}),
        });
        settings.bind('refresh-interval', interval, 'value', Gio.SettingsBindFlags.DEFAULT);
        data.add(interval);

        const vendor = new Adw.ComboRow({
            title: _('Vendor'),
            subtitle: _('anthropic expõe as janelas de 5h + semanal'),
            model: Gtk.StringList.new(['anthropic', 'openai', 'zai', 'openrouter', 'deepseek']),
        });
        bindCombo(settings, 'vendor', vendor, ['anthropic', 'openai', 'zai', 'openrouter', 'deepseek']);
        data.add(vendor);

        const binPath = new Adw.EntryRow({title: _('Caminho do binário (vazio = auto)')});
        settings.bind('binary-path', binPath, 'text', Gio.SettingsBindFlags.DEFAULT);
        data.add(binPath);

        // ── Position ─────────────────────────────────────────────────────
        const pos = new Adw.PreferencesGroup({
            title: _('Posição no painel'),
            description: _('Mudanças aplicam na hora.'),
        });
        page.add(pos);

        const box = new Adw.ComboRow({
            title: _('Área'),
            subtitle: _('right = ao lado da rede/relógio'),
            model: Gtk.StringList.new(['left', 'center', 'right']),
        });
        bindCombo(settings, 'panel-box', box, ['left', 'center', 'right']);
        pos.add(box);

        const index = new Adw.SpinRow({
            title: _('Índice na área'),
            subtitle: _('0 = mais à esquerda da área escolhida'),
            adjustment: new Gtk.Adjustment({lower: 0, upper: 20, step_increment: 1, page_increment: 1}),
        });
        settings.bind('panel-index', index, 'value', Gio.SettingsBindFlags.DEFAULT);
        pos.add(index);
    }
}
