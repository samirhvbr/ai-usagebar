using System.Diagnostics;
using System.Text;

namespace AiUsagebarTray;

/// <summary>
/// Locates and invokes the native ai-usagebar.exe backend, returning parsed
/// snapshots. The tray never reimplements vendor logic; it shells out.
/// </summary>
public sealed class Backend
{
    private static readonly TimeSpan BackendTimeout = TimeSpan.FromSeconds(90);

    private readonly Settings _settings;

    public Backend(Settings settings) => _settings = settings;

    /// <summary>
    /// Resolve the path to ai-usagebar.exe:
    ///   1. explicit Settings.BackendPath, if set and exists
    ///   2. next to this tray exe
    ///   3. common dev/build locations
    ///   4. PATH (let the OS resolve "ai-usagebar.exe")
    /// Returns null only if nothing plausible is found (we still try "ai-usagebar"
    /// via PATH as a last resort in RunAsync).
    /// </summary>
    public string? ResolveBackendPath()
    {
        // 1. Configured path.
        if (!string.IsNullOrWhiteSpace(_settings.BackendPath) && File.Exists(_settings.BackendPath))
            return _settings.BackendPath;

        const string exe = "ai-usagebar.exe";

        // 2. Beside the tray executable.
        var appDir = AppContext.BaseDirectory;
        var beside = Path.Combine(appDir, exe);
        if (File.Exists(beside)) return beside;

        // 3. Common dev/build locations (relative to a typical clone layout and
        //    the user profile).
        var userProfile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var candidates = new[]
        {
            // when running from windows-tray/bin/... inside the repo
            Path.GetFullPath(Path.Combine(appDir, "..", "..", "..", "..", "target", "release", exe)),
            Path.GetFullPath(Path.Combine(appDir, "..", "..", "..", "..", "target", "debug", exe)),
            Path.Combine(userProfile, "dev", "projects", "ai-usagebar", "target", "release", exe),
            Path.Combine(userProfile, ".cargo", "bin", exe),
        };
        foreach (var c in candidates)
            if (File.Exists(c)) return c;

        // 4. Not found on disk — RunAsync will fall back to "ai-usagebar" on PATH.
        return null;
    }

    /// <summary>
    /// Run the backend for one vendor and parse the result. Never throws for
    /// "normal" failures (missing exe, non-zero exit): returns an error
    /// snapshot instead, so the tray can show a red icon with the reason.
    /// </summary>
    public async Task<UsageSnapshot> FetchAsync(string vendor, CancellationToken ct = default)
    {
        var path = ResolveBackendPath();
        var fileName = path ?? "ai-usagebar"; // PATH fallback

        var psi = new ProcessStartInfo
        {
            FileName = fileName,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            CreateNoWindow = true,
            StandardOutputEncoding = Encoding.UTF8,
        };
        psi.ArgumentList.Add("--vendor");
        psi.ArgumentList.Add(vendor);
        psi.ArgumentList.Add("--format");
        psi.ArgumentList.Add(VendorFormat.FormatFor(vendor));
        psi.ArgumentList.Add("--json");

        try
        {
            using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
            timeoutCts.CancelAfter(BackendTimeout);

            using var proc = new Process { StartInfo = psi };
            proc.Start();

            var stdoutTask = proc.StandardOutput.ReadToEndAsync(timeoutCts.Token);
            var stderrTask = proc.StandardError.ReadToEndAsync(timeoutCts.Token);

            try
            {
                await proc.WaitForExitAsync(timeoutCts.Token);
            }
            catch (OperationCanceledException) when (timeoutCts.IsCancellationRequested && !ct.IsCancellationRequested)
            {
                try
                {
                    if (!proc.HasExited)
                        proc.Kill(entireProcessTree: true);
                }
                catch
                {
                    // Best-effort cleanup; the user-facing error is the timeout.
                }

                return ErrorSnapshot(vendor, $"ai-usagebar timed out after {BackendTimeout.TotalSeconds:0}s");
            }

            var stdout = (await stdoutTask).Trim();
            var stderr = (await stderrTask).Trim();

            if (string.IsNullOrWhiteSpace(stdout))
            {
                return ErrorSnapshot(vendor,
                    string.IsNullOrWhiteSpace(stderr)
                        ? "no output from ai-usagebar"
                        : stderr);
            }

            // The backend may emit multiple lines defensively; take the last
            // non-empty one (the JSON document).
            var line = stdout
                .Split('\n', StringSplitOptions.RemoveEmptyEntries)
                .LastOrDefault(l => l.TrimStart().StartsWith('{'))
                ?? stdout;

            return VendorFormat.Parse(vendor, line.Trim());
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            var hint = path is null
                ? "ai-usagebar.exe not found (set its path in settings)"
                : $"failed to run {path}";
            return ErrorSnapshot(vendor, $"{hint}: {ex.Message}");
        }
    }

    private static UsageSnapshot ErrorSnapshot(string vendor, string msg) => new()
    {
        Vendor = vendor,
        Severity = Severity.Critical,
        IsError = true,
        ErrorMessage = msg,
    };
}
