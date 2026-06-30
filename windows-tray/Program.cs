namespace AiUsagebarTray;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        // Single-instance guard: a second launch just exits quietly.
        using var mutex = new Mutex(initiallyOwned: true, "AiUsagebarTray.SingleInstance", out var isNew);
        if (!isNew) return;

        ApplicationConfiguration.Initialize();
        Application.SetHighDpiMode(HighDpiMode.PerMonitorV2);
        Application.Run(new TrayApp());
    }
}
