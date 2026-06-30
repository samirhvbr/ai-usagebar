using System.Text.Json;
using System.Text.RegularExpressions;

namespace AiUsagebarTray;

/// <summary>
/// Severity class emitted by ai-usagebar's JSON ("class" field), used to color
/// the tray icon. Mirrors the backend's waybar::Class enum.
/// </summary>
public enum Severity
{
    Low,
    Mid,
    High,
    Critical,
}

/// <summary>
/// One displayable usage window: a label, a percentage (0..100), an optional
/// "resets in …" string, and a Nerd Font glyph. Vendors that don't expose a
/// percentage (OpenRouter/DeepSeek credit balances) use <see cref="Detail"/>
/// rows instead.
/// </summary>
public sealed record UsageRow(string Glyph, string Label, double Percent, string Reset);

/// <summary>A label/value line for vendors whose data isn't a percentage.</summary>
public sealed record DetailRow(string Glyph, string Label, string Value);

/// <summary>
/// A parsed snapshot of one vendor's usage, ready for the tray to render.
/// Built from `ai-usagebar --vendor X --format '...' --json`. The set of rows
/// is vendor-specific (e.g. Anthropic has Sonnet; OpenAI doesn't; OpenRouter
/// shows a credit balance), which is why rows are a list rather than fixed
/// Session/Weekly/Sonnet fields.
/// </summary>
public sealed record UsageSnapshot
{
    public required string Vendor { get; init; }
    public Severity Severity { get; init; } = Severity.Low;

    /// <summary>Plan / account label shown as the panel title.</summary>
    public string Title { get; init; } = "";

    /// <summary>Percentage windows (Session/Weekly/…); may be empty.</summary>
    public IReadOnlyList<UsageRow> Rows { get; init; } = Array.Empty<UsageRow>();

    /// <summary>Non-percentage detail lines (credit balances); may be empty.</summary>
    public IReadOnlyList<DetailRow> Details { get; init; } = Array.Empty<DetailRow>();

    public bool IsError { get; init; }
    public string ErrorMessage { get; init; } = "";

    public DateTime FetchedAt { get; init; } = DateTime.Now;

    /// <summary>
    /// The percentage shown on the tray icon: the first percentage row (the
    /// vendor's primary window, e.g. Session). Null for vendors that report no
    /// percentage (OpenRouter/DeepSeek credit balances) — those fall back to a
    /// colored dot.
    /// </summary>
    public double? IconPercent => Rows.Count > 0 ? Rows[0].Percent : null;

    /// <summary>
    /// Build a human-readable tooltip (plain text — the Windows NotifyIcon
    /// tooltip does not render Pango markup and is capped at ~127 chars).
    /// </summary>
    public string ToTooltip()
    {
        if (IsError)
            return Truncate($"{Vendor}: {ErrorMessage}", 127);

        var parts = new List<string>();
        if (!string.IsNullOrWhiteSpace(Title)) parts.Add(Title);
        foreach (var r in Rows)
            parts.Add($"{r.Label} {FormatPct(r.Percent)}%" +
                      (string.IsNullOrWhiteSpace(r.Reset) ? "" : $" ({r.Reset})"));
        foreach (var d in Details)
            parts.Add($"{d.Label} {d.Value}");

        var s = string.Join("  ·  ", parts);
        // Fall back to the vendor name (never a raw payload) if nothing parsed.
        return Truncate(string.IsNullOrWhiteSpace(s) ? Vendor : s, 127);
    }

    internal static string FormatPct(double p) =>
        p == Math.Floor(p) ? ((int)p).ToString() : p.ToString("0.#");

    private static string Truncate(string s, int max) =>
        s.Length <= max ? s : s[..(max - 1)] + "…";
}
