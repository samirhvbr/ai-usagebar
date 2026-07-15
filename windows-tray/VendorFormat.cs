using System.Globalization;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace AiUsagebarTray;

/// <summary>
/// Per-vendor format strings and parsing. Each vendor exposes its OWN family of
/// `--format` placeholders (`{session_pct}` for Anthropic, `{oai_*}` for OpenAI,
/// `{zai_*}`, `{or_*}`, `{ds_*}`), and unknown placeholders are left untouched
/// by the backend — so a single Anthropic-only format string produces literal
/// `{sonnet_pct}` text (and a bogus Sonnet row) on other vendors. This maps each
/// vendor to the right placeholders and builds vendor-appropriate rows.
/// </summary>
public static class VendorFormat
{
    // ASCII Unit Separator delimits fields; it won't appear in human text.
    private const char Sep = '\u001f';

    // Nerd Font glyphs (fall back to text when no glyph font is installed).
    private const string GClock = "\uf017";    // session (5h)
    private const string GCalendar = "\uf073"; // weekly (7d)
    private const string GLayers = "\uf0c9";   // sonnet / sub-window
    private const string GReview = "\uf06e";   // code review
    private const string GWallet = "\uf555";   // credit balance
    private const string GTools = "\uf0ad";    // mcp tools

    private static readonly Regex PangoTag = new("<[^>]*>", RegexOptions.Compiled);

    /// <summary>The `--format` string to pass for a given vendor.</summary>
    // The `*_elapsed` meta positions are appended AFTER each vendor's existing
    // fields so the leading indices stay stable. An empty/non-numeric value
    // (old binary, or a window with no reset) means "no meta marker".
    public static string FormatFor(string vendor) => vendor.ToLowerInvariant() switch
    {
        "openai" => Join(
            "{oai_plan}", "{oai_session_pct}", "{oai_session_reset}",
            "{oai_weekly_pct}", "{oai_weekly_reset}",
            "{oai_code_review_pct}", "{oai_credit_balance}",
            "{oai_session_elapsed}", "{oai_weekly_elapsed}"),
        "zai" => Join(
            "{zai_plan}", "{zai_session_pct}", "{zai_session_reset}",
            "{zai_weekly_pct}", "{zai_weekly_reset}",
            "{zai_mcp_pct}", "{zai_mcp_reset}",
            "{zai_session_elapsed}", "{zai_weekly_elapsed}", "{zai_mcp_elapsed}"),
        "openrouter" => Join(
            "{or_label}", "{or_balance}", "{or_consumed_pct}",
            "{or_used_today}", "{or_used_week}", "{or_used_month}"),
        "deepseek" => Join(
            "{ds_balance}", "{ds_available}", "{ds_granted}"),
        // anthropic (default)
        _ => Join(
            "{plan}", "{session_pct}", "{session_reset}",
            "{weekly_pct}", "{weekly_reset}", "{sonnet_pct}",
            "{session_elapsed}", "{weekly_elapsed}", "{sonnet_elapsed}"),
    };

    private static string Join(params string[] parts) => string.Join(Sep, parts);

    /// <summary>
    /// Parse the JSON line ai-usagebar emits into a vendor-appropriate snapshot.
    /// </summary>
    public static UsageSnapshot Parse(string vendor, string jsonLine)
    {
        using var doc = JsonDocument.Parse(jsonLine);
        var root = doc.RootElement;

        var rawText = root.TryGetProperty("text", out var t) ? t.GetString() ?? "" : "";
        var rawTooltip = root.TryGetProperty("tooltip", out var tt) ? tt.GetString() ?? "" : "";
        var cls = root.TryGetProperty("class", out var c) ? c.GetString() ?? "low" : "low";

        var severity = cls.ToLowerInvariant() switch
        {
            "critical" => Severity.Critical,
            "high" => Severity.High,
            "mid" => Severity.Mid,
            _ => Severity.Low,
        };

        var cleanText = StripPango(rawText);

        // Backend error fallback: text is "⚠" and the tooltip carries the msg.
        if (cleanText.Trim() == "⚠"
            || (severity == Severity.Critical && cleanText.Contains('⚠', StringComparison.Ordinal)))
        {
            return new UsageSnapshot
            {
                Vendor = vendor,
                Severity = Severity.Critical,
                IsError = true,
                ErrorMessage = StripPango(rawTooltip).Replace("\n", " ", StringComparison.Ordinal).Trim(),
            };
        }

        var f = cleanText.Split(Sep);
        string Field(int i) => i < f.Length ? f[i].Trim() : "";

        return vendor.ToLowerInvariant() switch
        {
            "openai" => ParseOpenAi(vendor, severity, Field),
            "zai" => ParseZai(vendor, severity, Field),
            "openrouter" => ParseOpenRouter(vendor, severity, Field),
            "deepseek" => ParseDeepSeek(vendor, severity, Field),
            _ => ParseAnthropic(vendor, severity, Field),
        };
    }

    private static UsageSnapshot ParseAnthropic(string vendor, Severity sev, Func<int, string> f)
    {
        var rows = new List<UsageRow>();
        AddPct(rows, GClock, "Session", f(1), f(2), f(6));
        AddPct(rows, GCalendar, "Weekly", f(3), f(4), f(7));
        // Sonnet only shows when there's actual usage (it's an Anthropic-only,
        // often-zero window).
        if (ParsePct(f(5)) > 0)
            AddPct(rows, GLayers, "Sonnet", f(5), "", f(8));
        return new UsageSnapshot { Vendor = vendor, Severity = sev, Title = f(0), Rows = rows };
    }

    private static UsageSnapshot ParseOpenAi(string vendor, Severity sev, Func<int, string> f)
    {
        var rows = new List<UsageRow>();
        AddPct(rows, GClock, "Session", f(1), f(2), f(7));
        AddPct(rows, GCalendar, "Weekly", f(3), f(4), f(8));
        if (ParsePct(f(5)) > 0)
            AddPct(rows, GReview, "Code review", f(5), "");

        var details = new List<DetailRow>();
        if (!string.IsNullOrWhiteSpace(f(6)))
            details.Add(new DetailRow(GWallet, "Credits", f(6)));
        return new UsageSnapshot
        {
            Vendor = vendor,
            Severity = sev,
            Title = f(0),
            Rows = rows,
            Details = details,
        };
    }

    private static UsageSnapshot ParseZai(string vendor, Severity sev, Func<int, string> f)
    {
        var rows = new List<UsageRow>();
        AddPct(rows, GClock, "Session", f(1), f(2), f(7));
        AddPct(rows, GCalendar, "Weekly", f(3), f(4), f(8));
        AddPct(rows, GTools, "MCP tools", f(5), f(6), f(9));
        return new UsageSnapshot { Vendor = vendor, Severity = sev, Title = f(0), Rows = rows };
    }

    private static UsageSnapshot ParseOpenRouter(string vendor, Severity sev, Func<int, string> f)
    {
        // {or_label}{or_balance}{or_consumed_pct}{or_used_today}{week}{month}
        var rows = new List<UsageRow>();
        var consumed = ParsePct(f(2));
        if (!string.IsNullOrWhiteSpace(f(2)))
            rows.Add(new UsageRow(GWallet, "Balance used", consumed, ""));

        var details = new List<DetailRow>();
        if (!string.IsNullOrWhiteSpace(f(1))) details.Add(new DetailRow(GWallet, "Balance", f(1)));
        if (!string.IsNullOrWhiteSpace(f(3))) details.Add(new DetailRow(GClock, "Today", f(3)));
        if (!string.IsNullOrWhiteSpace(f(4))) details.Add(new DetailRow(GCalendar, "Week", f(4)));
        if (!string.IsNullOrWhiteSpace(f(5))) details.Add(new DetailRow(GCalendar, "Month", f(5)));

        var title = string.IsNullOrWhiteSpace(f(0)) ? vendor : f(0);
        return new UsageSnapshot
        {
            Vendor = vendor,
            Severity = sev,
            Title = title,
            Rows = rows,
            Details = details,
        };
    }

    private static UsageSnapshot ParseDeepSeek(string vendor, Severity sev, Func<int, string> f)
    {
        // {ds_balance}{ds_available}{ds_granted}
        var details = new List<DetailRow>();
        if (!string.IsNullOrWhiteSpace(f(1))) details.Add(new DetailRow(GWallet, "Available", f(1)));
        if (!string.IsNullOrWhiteSpace(f(0))) details.Add(new DetailRow(GWallet, "Balance", f(0)));
        if (!string.IsNullOrWhiteSpace(f(2))) details.Add(new DetailRow(GWallet, "Granted", f(2)));
        return new UsageSnapshot { Vendor = vendor, Severity = sev, Title = "DeepSeek", Details = details };
    }

    private static void AddPct(
        List<UsageRow> rows, string glyph, string label, string pct, string reset, string elapsed = "")
    {
        if (string.IsNullOrWhiteSpace(pct)) return;
        rows.Add(new UsageRow(glyph, label, ParsePct(pct), reset, ParseElapsed(elapsed)));
    }

    private static double ParsePct(string s) =>
        double.TryParse(s, NumberStyles.Float, CultureInfo.InvariantCulture, out var v)
            ? Math.Clamp(v, 0, 100)
            : 0;

    /// <summary>
    /// The meta (elapsed-time) position 0..100, or null when the field is
    /// empty/non-numeric — i.e. the window has no time-based meta, so no marker
    /// is drawn.
    /// </summary>
    private static double? ParseElapsed(string s) =>
        double.TryParse(s, NumberStyles.Float, CultureInfo.InvariantCulture, out var v)
            ? Math.Clamp(v, 0, 100)
            : null;

    public static string StripPango(string s) =>
        System.Net.WebUtility.HtmlDecode(PangoTag.Replace(s, "")).Trim();
}
