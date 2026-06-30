using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;

namespace AiUsagebarTray;

/// <summary>
/// Builds tray icons at runtime: a filled rounded dot colored by severity.
/// Colors mirror the backend's One Dark severity palette so the tray matches
/// the Waybar/TUI look.
/// </summary>
public static class IconFactory
{
    // One Dark-ish severity colors (low=green, mid=yellow, high=orange, crit=red).
    public static Color ColorFor(Severity s) => s switch
    {
        Severity.Critical => Color.FromArgb(224, 108, 117), // #e06c75
        Severity.High => Color.FromArgb(209, 154, 102),     // #d19a66
        Severity.Mid => Color.FromArgb(229, 192, 123),      // #e5c07b
        _ => Color.FromArgb(152, 195, 121),                 // #98c379
    };

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyIcon(IntPtr handle);

    /// <summary>
    /// Create a tray icon (filled dot). The caller is responsible for disposing
    /// the returned Icon AND must not leak GDI handles — see TrayApp which
    /// destroys the previous icon before swapping.
    /// </summary>
    public static Icon CreateDot(Severity severity, int size = 32)
    {
        using var bmp = new Bitmap(size, size);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.Clear(Color.Transparent);

            var color = ColorFor(severity);
            var pad = size / 8f;
            var rect = new RectangleF(pad, pad, size - 2 * pad, size - 2 * pad);

            using var fill = new SolidBrush(color);
            g.FillEllipse(fill, rect);

            // Subtle darker ring for contrast on light/dark trays alike.
            using var pen = new Pen(Color.FromArgb(120, 0, 0, 0), Math.Max(1f, size / 16f));
            g.DrawEllipse(pen, rect);
        }

        return ToIcon(bmp);
    }

    /// <summary>
    /// Create a tray icon showing a percentage number (0..100) colored by
    /// severity. The number fills the icon; a dark outline keeps it legible on
    /// both light and dark taskbars. "100" is drawn as "99+" since three digits
    /// don't fit a 16px tray slot legibly. Same GDI-handle discipline as
    /// <see cref="CreateDot"/>.
    /// </summary>
    public static Icon CreatePercent(Severity severity, double percent, int size = 32)
    {
        var value = (int)Math.Round(Math.Clamp(percent, 0, 100));
        var text = value >= 100 ? "99+" : value.ToString();

        using var bmp = new Bitmap(size, size);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.TextRenderingHint = System.Drawing.Text.TextRenderingHint.AntiAlias;
            g.Clear(Color.Transparent);

            var color = ColorFor(severity);

            // Fit font to the digit count so 1–3 chars all fill the slot.
            float fontSize = text.Length switch
            {
                >= 3 => size * 0.42f,
                2 => size * 0.62f,
                _ => size * 0.80f,
            };
            using var font = new Font("Segoe UI", fontSize, FontStyle.Bold, GraphicsUnit.Pixel);

            // Center the text using a path so we can stroke an outline.
            using var fmt = new StringFormat
            {
                Alignment = StringAlignment.Center,
                LineAlignment = StringAlignment.Center,
            };
            var rect = new RectangleF(0, 0, size, size);

            using var path = new GraphicsPath();
            path.AddString(text, font.FontFamily, (int)font.Style, font.Size, rect, fmt);

            using var outline = new Pen(Color.FromArgb(200, 0, 0, 0), Math.Max(1.5f, size / 12f))
            {
                LineJoin = LineJoin.Round,
            };
            using var fill = new SolidBrush(color);
            g.DrawPath(outline, path);
            g.FillPath(fill, path);
        }

        return ToIcon(bmp);
    }

    /// <summary>
    /// Convert a bitmap to an HICON-backed Icon, then clone to a managed copy so
    /// we can free the native handle immediately (avoids GDI handle leaks on
    /// swap).
    /// </summary>
    private static Icon ToIcon(Bitmap bmp)
    {
        var hicon = bmp.GetHicon();
        try
        {
            using var tmp = Icon.FromHandle(hicon);
            return (Icon)tmp.Clone();
        }
        finally
        {
            DestroyIcon(hicon);
        }
    }
}
