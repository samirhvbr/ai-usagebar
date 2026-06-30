using Microsoft.Win32;

namespace AiUsagebarTray;

/// <summary>
/// The application context: owns the tray icon, the poll timer, the context
/// menu, and the detail panel. Lives for the whole process.
/// </summary>
public sealed class TrayApp : ApplicationContext
{
    private const string AutoStartKey = @"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
    private const string AutoStartValue = "AiUsagebarTray";

    private readonly Settings _settings;
    private readonly Backend _backend;
    private readonly NotifyIcon _tray;
    private readonly System.Windows.Forms.Timer _timer;
    private PanelForm? _panel;
    private Icon? _currentIcon;
    private UsageSnapshot? _last;
    private bool _fetching;

    public TrayApp()
    {
        _settings = Settings.Load();
        _backend = new Backend(_settings);

        _tray = new NotifyIcon
        {
            Visible = true,
            Text = "ai-usagebar (loading…)",
            Icon = SwapIcon(Severity.Low),
            ContextMenuStrip = BuildMenu(),
        };
        _tray.MouseClick += OnTrayClick;

        _timer = new System.Windows.Forms.Timer
        {
            Interval = _settings.EffectiveIntervalSeconds * 1000,
        };
        _timer.Tick += async (_, _) => await RefreshAsync();
        _timer.Start();

        // Kick off the first fetch immediately.
        _ = RefreshAsync();
    }

    // ---- icon handling (avoid GDI leaks) ----

    private Icon SwapIcon(Severity severity, double? percent = null)
    {
        // Show the percentage when the active vendor reports one; otherwise a
        // colored dot (e.g. OpenRouter/DeepSeek credit balances, or load/error).
        var newIcon = percent is double p
            ? IconFactory.CreatePercent(severity, p)
            : IconFactory.CreateDot(severity);
        var old = _currentIcon;
        _currentIcon = newIcon;
        old?.Dispose();
        return newIcon;
    }

    // ---- menu ----

    private ContextMenuStrip BuildMenu()
    {
        var menu = new ContextMenuStrip();

        var open = new ToolStripMenuItem("Open panel", null, (_, _) => ShowPanel());
        var refresh = new ToolStripMenuItem("Refresh now", null, async (_, _) => await RefreshAsync(force: true));

        var vendor = new ToolStripMenuItem("Vendor");
        foreach (var v in new[] { "anthropic", "openai", "zai", "openrouter", "deepseek" })
        {
            var item = new ToolStripMenuItem(v)
            {
                Checked = string.Equals(v, _settings.Vendor, StringComparison.OrdinalIgnoreCase),
                CheckOnClick = false,
            };
            item.Click += async (_, _) =>
            {
                _settings.Vendor = v;
                _settings.Save();
                foreach (ToolStripMenuItem mi in vendor.DropDownItems)
                    mi.Checked = mi == item;
                await RefreshAsync(force: true);
            };
            vendor.DropDownItems.Add(item);
        }

        var autostart = new ToolStripMenuItem("Start with Windows")
        {
            Checked = IsAutoStartEnabled(),
            CheckOnClick = true,
        };
        autostart.CheckedChanged += (_, _) => SetAutoStart(autostart.Checked);

        var exit = new ToolStripMenuItem("Exit", null, (_, _) => ExitThread());

        menu.Items.Add(open);
        menu.Items.Add(refresh);
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(vendor);
        menu.Items.Add(autostart);
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(exit);
        return menu;
    }

    private void OnTrayClick(object? sender, MouseEventArgs e)
    {
        if (e.Button == MouseButtons.Left)
            ShowPanel();
    }

    private void ShowPanel()
    {
        if (_last is null) return;
        _panel ??= new PanelForm(_last);
        _panel.Update(_last);
        _panel.ShowNearCursor();
    }

    // ---- polling ----

    private async Task RefreshAsync(bool force = false)
    {
        // A forced refresh while another backend process is still running would
        // contend with the same per-vendor cache lock and surface a misleading
        // "cache lock timeout" error. Let the in-flight fetch finish instead.
        if (_fetching) return;
        _fetching = true;
        try
        {
            var snap = await _backend.FetchAsync(_settings.Vendor);
            _last = snap;

            _tray.Icon = SwapIcon(snap.Severity, snap.IsError ? null : snap.IconPercent);
            _tray.Text = TrayTextFor(snap);
            _panel?.Update(snap);
        }
        catch (Exception ex)
        {
            // RefreshAsync is invoked from async-void Timer/menu handlers, so an
            // escaping exception (GDI, paint, etc.) would reach the message loop
            // and could crash the process. Surface it as a red icon instead.
            _tray.Icon = SwapIcon(Severity.Critical);
            _tray.Text = $"{_settings.Vendor}: {ex.Message}";
        }
        finally
        {
            _fetching = false;
        }
    }

    private static string TrayTextFor(UsageSnapshot s)
    {
        // NotifyIcon.Text is capped at 127 chars (modern Windows); ToTooltip
        // already truncates.
        var head = s.IsError ? $"{s.Vendor}: error" : $"ai-usagebar — {s.Vendor}";
        var body = s.ToTooltip();
        var full = $"{head}\n{body}";
        return full.Length <= 127 ? full : body;
    }

    // ---- auto-start (HKCU Run key) ----

    private static bool IsAutoStartEnabled()
    {
        using var key = Registry.CurrentUser.OpenSubKey(AutoStartKey, writable: false);
        return key?.GetValue(AutoStartValue) is not null;
    }

    private static void SetAutoStart(bool enabled)
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(AutoStartKey, writable: true)
                            ?? Registry.CurrentUser.CreateSubKey(AutoStartKey);
            if (key is null) return;
            if (enabled)
            {
                var exe = Environment.ProcessPath ?? Application.ExecutablePath;
                key.SetValue(AutoStartValue, $"\"{exe}\"");
            }
            else
            {
                key.DeleteValue(AutoStartValue, throwOnMissingValue: false);
            }
        }
        catch
        {
            // Non-fatal.
        }
    }

    // ---- cleanup ----

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _timer.Stop();
            _timer.Dispose();
            _tray.Visible = false;
            _tray.Dispose();
            _currentIcon?.Dispose();
            _panel?.Dispose();
        }
        base.Dispose(disposing);
    }
}
