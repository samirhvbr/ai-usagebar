using System.Drawing.Drawing2D;
using System.Drawing.Text;

namespace AiUsagebarTray;

/// <summary>
/// A small borderless popup that renders the detailed usage panel (progress
/// bars), shown on left-click of the tray icon near the cursor. Mirrors the
/// bordered tooltip the Waybar/TUI build shows, with rounded corners, a soft
/// drop shadow, and Nerd Font glyphs when available.
/// </summary>
public sealed class PanelForm : Form
{
    private UsageSnapshot _snapshot;

    // One Dark palette (matches the Waybar/TUI default theme).
    private static readonly Color Bg = Color.FromArgb(40, 44, 52);       // #282c34
    private static readonly Color Accent = Color.FromArgb(97, 175, 239); // #61afef
    private static readonly Color Fg = Color.FromArgb(171, 178, 191);    // #abb2bf
    private static readonly Color Dim = Color.FromArgb(92, 99, 112);     // #5c6370
    private static readonly Color Track = Color.FromArgb(62, 68, 81);    // #3e4451

    // Layout constants.
    private const int Pad = 18;          // outer padding
    private const int CornerRadius = 12;
    private const int BarHeight = 8;
    private const int PctColumn = 52;    // reserved width for the "NN%" text
    private const int SectionGap = 16;

    // Resolved once: a Nerd Font family if installed, else null (text fallback).
    private static readonly string? GlyphFont = ResolveGlyphFont();

    public PanelForm(UsageSnapshot snapshot)
    {
        _snapshot = snapshot;
        FormBorderStyle = FormBorderStyle.None;
        ShowInTaskbar = false;
        TopMost = true;
        StartPosition = FormStartPosition.Manual;
        BackColor = Bg;
        DoubleBuffered = true;
        Width = 340;
        Height = 240; // recomputed per snapshot in Relayout()
        // Close when it loses focus, like a tooltip/flyout.
        Deactivate += (_, _) => Hide();
        Relayout();
    }

    /// <summary>Drop the focus rectangle and give the window a soft shadow.</summary>
    protected override CreateParams CreateParams
    {
        get
        {
            const int CS_DROPSHADOW = 0x00020000;
            var cp = base.CreateParams;
            cp.ClassStyle |= CS_DROPSHADOW;
            return cp;
        }
    }

    public void Update(UsageSnapshot snapshot)
    {
        _snapshot = snapshot;
        Relayout();
        Invalidate();
    }

    /// <summary>Compute height from the visible sections and round the corners.</summary>
    private void Relayout()
    {
        // title + N bar rows + M detail lines + footer, consistent spacing.
        int barRows = _snapshot.Rows.Count;
        int detailRows = _snapshot.Details.Count;
        int contentHeight = _snapshot.IsError
            ? 110
            : 34 /*title*/
              + barRows * (48 + SectionGap)
              + detailRows * 24
              + (detailRows > 0 ? 6 : 0)
              + 26 /*footer*/;
        Height = Pad * 2 + contentHeight;

        // Form.Region's setter does not dispose the previous region, and
        // Relayout runs on every refresh — so free the old one to avoid leaking
        // one GDI region per poll.
        var previous = Region;
        using var path = RoundedRect(new Rectangle(0, 0, Width, Height), CornerRadius);
        Region = new Region(path);
        previous?.Dispose();
    }

    /// <summary>Show near the cursor, kept on-screen.</summary>
    public void ShowNearCursor()
    {
        var cur = Cursor.Position;
        var screen = Screen.FromPoint(cur).WorkingArea;
        var x = Math.Min(cur.X, screen.Right - Width - 8);
        var y = Math.Min(cur.Y, screen.Bottom - Height - 8);
        x = Math.Max(screen.Left + 8, x);
        y = Math.Max(screen.Top + 8, y);
        Location = new Point(x, y);
        Show();
        Activate();
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        var g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        g.TextRenderingHint = TextRenderingHint.ClearTypeGridFit;

        // Filled rounded background + 1px accent border.
        var outer = new Rectangle(0, 0, Width - 1, Height - 1);
        using (var path = RoundedRect(outer, CornerRadius))
        using (var bg = new SolidBrush(Bg))
        using (var border = new Pen(Color.FromArgb(90, Accent), 1f))
        {
            g.FillPath(bg, path);
            g.DrawPath(border, path);
        }

        using var titleFont = new Font("Segoe UI Semibold", 12f, FontStyle.Bold);
        using var labelFont = new Font("Segoe UI", 10f, FontStyle.Regular);
        using var dimFont = new Font("Segoe UI", 8.5f, FontStyle.Regular);
        using var pctFont = new Font("Segoe UI Semibold", 10f, FontStyle.Bold);
        using var titleBrush = new SolidBrush(Accent);
        using var fgBrush = new SolidBrush(Fg);
        using var dimBrush = new SolidBrush(Dim);

        int x = Pad;
        int y = Pad;

        if (_snapshot.IsError)
        {
            using var errBrush = new SolidBrush(IconFactory.ColorFor(Severity.Critical));
            DrawGlyphText(g, "\uf071", "  " + _snapshot.Vendor, titleFont, errBrush, x, y); // nf-fa-warning
            var rect = new RectangleF(x, y + 32, Width - 2 * Pad, Height - 2 * Pad - 32);
            g.DrawString(_snapshot.ErrorMessage, labelFont, fgBrush, rect);
            return;
        }

        // Title (plan/account label or vendor).
        var title = string.IsNullOrWhiteSpace(_snapshot.Title) ? _snapshot.Vendor : _snapshot.Title;
        g.DrawString(title, titleFont, titleBrush, x, y);
        y += 34;

        // Percentage rows (Session/Weekly/…).
        foreach (var row in _snapshot.Rows)
            y = DrawBarRow(g, x, y, row, labelFont, dimFont, pctFont, fgBrush, dimBrush);

        // Non-percentage detail lines (credit balances, etc.).
        foreach (var d in _snapshot.Details)
            y = DrawDetailRow(g, x, y, d, labelFont, fgBrush);

        // Footer: subtle divider + timestamp.
        int footY = Height - Pad - 16;
        using (var divider = new Pen(Color.FromArgb(40, Fg)))
            g.DrawLine(divider, x, footY - 6, Width - Pad, footY - 6);
        DrawGlyphText(g, "\uf021", $"  Updated {_snapshot.FetchedAt:HH:mm}", dimFont, dimBrush, x, footY); // nf-fa-refresh
    }

    private int DrawBarRow(Graphics g, int x, int y, UsageRow row,
        Font labelFont, Font dimFont, Font pctFont, Brush fgBrush, Brush dimBrush)
    {
        // Label row (glyph + name).
        DrawGlyphText(g, row.Glyph, "  " + row.Label, labelFont, fgBrush, x, y);

        var barColor = IconFactory.ColorFor(SeverityForPct(row.Percent));

        int barY = y + 24;
        int barW = Width - 2 * Pad - PctColumn;

        // Rounded track + fill.
        var trackRect = new Rectangle(x, barY, barW, BarHeight);
        using (var tpath = RoundedRect(trackRect, BarHeight / 2))
        using (var tbrush = new SolidBrush(Track))
            g.FillPath(tbrush, tpath);

        if (row.Percent > 0)
        {
            int fillW = Math.Max(BarHeight, (int)(barW * row.Percent / 100.0));
            var fillRect = new Rectangle(x, barY, fillW, BarHeight);
            using var fpath = RoundedRect(fillRect, BarHeight / 2);
            using var fbrush = new SolidBrush(barColor);
            g.FillPath(fbrush, fpath);
        }

        // % text, vertically centered on the bar, right-aligned in its column.
        using var pctBrush = new SolidBrush(barColor);
        var pctText = $"{UsageSnapshot.FormatPct(row.Percent)}%";
        var sz = g.MeasureString(pctText, pctFont);
        g.DrawString(pctText, pctFont, pctBrush,
            Width - Pad - sz.Width, barY + BarHeight / 2f - sz.Height / 2f);

        int next = barY + BarHeight + 6;
        if (!string.IsNullOrWhiteSpace(row.Reset))
        {
            g.DrawString($"Resets in {row.Reset}", dimFont, dimBrush, x, next);
            next += 16;
        }
        return next + SectionGap;
    }

    private int DrawDetailRow(Graphics g, int x, int y, DetailRow d, Font labelFont, Brush fgBrush)
    {
        DrawGlyphText(g, d.Glyph, "  " + d.Label, labelFont, fgBrush, x, y);
        using var valBrush = new SolidBrush(Accent);
        var sz = g.MeasureString(d.Value, labelFont);
        g.DrawString(d.Value, labelFont, valBrush, Width - Pad - sz.Width, y);
        return y + 24;
    }

    /// <summary>
    /// Draw a glyph (Nerd Font if available) followed by text. When no Nerd Font
    /// is installed, the glyph is omitted so we never render tofu boxes.
    /// </summary>
    private static void DrawGlyphText(Graphics g, string glyph, string text, Font textFont, Brush brush, float x, float y)
    {
        if (GlyphFont is not null)
        {
            using var gf = new Font(GlyphFont, textFont.Size, textFont.Style);
            g.DrawString(glyph, gf, brush, x, y);
            x += g.MeasureString(glyph, gf).Width;
            g.DrawString(text, textFont, brush, x, y);
        }
        else
        {
            // Trim the leading spacing that normally separates glyph and label.
            g.DrawString(text.TrimStart(), textFont, brush, x, y);
        }
    }

    private static Severity SeverityForPct(double pct) => pct switch
    {
        >= 90 => Severity.Critical,
        >= 75 => Severity.High,
        >= 50 => Severity.Mid,
        _ => Severity.Low,
    };

    /// <summary>Build a rounded-rectangle path.</summary>
    private static GraphicsPath RoundedRect(Rectangle r, int radius)
    {
        int d = radius * 2;
        var path = new GraphicsPath();
        if (d <= 0)
        {
            path.AddRectangle(r);
            return path;
        }
        path.AddArc(r.X, r.Y, d, d, 180, 90);
        path.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        path.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90);
        path.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
        path.CloseFigure();
        return path;
    }

    /// <summary>
    /// Find an installed glyph-capable font. Prefers a dedicated symbols font,
    /// then any "Nerd Font" family (these carry the icon glyphs we use), then a
    /// few common patched families by substring so name variants still match.
    /// Returns null if none is present, so callers fall back to text.
    /// </summary>
    private static string? ResolveGlyphFont()
    {
        using var installed = new InstalledFontCollection();
        var names = installed.Families.Select(f => f.Name).ToList();

        bool Has(string name) =>
            names.Any(n => n.Equals(name, StringComparison.OrdinalIgnoreCase));
        string? FirstContaining(params string[] needles) =>
            names.FirstOrDefault(n =>
                needles.Any(x => n.Contains(x, StringComparison.OrdinalIgnoreCase)));

        // 1. A dedicated symbols font renders glyphs at any base typeface.
        foreach (var exact in new[] { "Symbols Nerd Font", "Symbols Nerd Font Mono" })
            if (Has(exact)) return exact;

        // 2. Any installed "Nerd Font" family (covers "JetBrainsMono Nerd Font",
        //    "MesloLGS NF", "… NFM", etc.).
        var nerd = FirstContaining("Nerd Font", " NF", " NFM", "NerdFont");
        if (nerd is not null) return nerd;

        // 3. Common patched families by substring as a last resort.
        return FirstContaining("CaskaydiaCove", "FiraCode", "JetBrainsMono", "Hack", "Meslo");
    }
}
