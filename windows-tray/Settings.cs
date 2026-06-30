using System.Text.Json;
using System.Text.Json.Serialization;

namespace AiUsagebarTray;

/// <summary>
/// User settings, persisted to %APPDATA%\ai-usagebar-tray\settings.json.
/// </summary>
public sealed class Settings
{
    /// <summary>Explicit path to ai-usagebar.exe. Empty = auto-detect.</summary>
    public string BackendPath { get; set; } = "";

    /// <summary>Vendor to display (anthropic | openai | zai | openrouter | deepseek).</summary>
    public string Vendor { get; set; } = "anthropic";

    /// <summary>
    /// Poll interval in seconds. The Anthropic/OpenAI endpoints rate-limit below
    /// ~300s, so we default to that and keep it as the floor.
    /// </summary>
    public int IntervalSeconds { get; set; } = 300;

    // Note: "start with Windows" is stored in the HKCU\...\Run registry key
    // (the OS's own source of truth), not here — see TrayApp.SetAutoStart.

    [JsonIgnore]
    public int EffectiveIntervalSeconds => Math.Max(60, IntervalSeconds);

    // ---- persistence ----

    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static string ConfigDir =>
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "ai-usagebar-tray");

    public static string ConfigPath => Path.Combine(ConfigDir, "settings.json");

    public static Settings Load()
    {
        try
        {
            if (File.Exists(ConfigPath))
            {
                var json = File.ReadAllText(ConfigPath);
                var s = JsonSerializer.Deserialize<Settings>(json, JsonOpts);
                if (s is not null) return s;
            }
        }
        catch
        {
            // Corrupt/unreadable config — fall back to defaults rather than crash.
        }
        return new Settings();
    }

    public void Save()
    {
        try
        {
            Directory.CreateDirectory(ConfigDir);
            File.WriteAllText(ConfigPath, JsonSerializer.Serialize(this, JsonOpts));
        }
        catch
        {
            // Best-effort; not fatal if we can't persist.
        }
    }
}
