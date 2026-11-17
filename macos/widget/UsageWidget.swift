import SwiftUI
import WidgetKit

struct UsageWidgetEntry: TimelineEntry {
    let date: Date
    let snapshot: UsageWidgetSnapshot
    var isPreview = false
}

struct UsageWidgetProvider: TimelineProvider {
    func placeholder(in context: Context) -> UsageWidgetEntry {
        UsageWidgetEntry(date: UsageWidgetPreview.date,
                         snapshot: UsageWidgetPreview.snapshot(), isPreview: true)
    }

    func getSnapshot(in context: Context, completion: @escaping (UsageWidgetEntry) -> Void) {
        if context.isPreview {
            completion(placeholder(in: context))
        } else {
            let now = Date()
            completion(UsageWidgetEntry(date: now, snapshot: readSnapshot(at: now)))
        }
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<UsageWidgetEntry>) -> Void) {
        let now = Date()
        let snapshot = readSnapshot(at: now)
        let dates = [now] + snapshot.timelineDates(after: now)
        let entries = dates.map { UsageWidgetEntry(date: $0, snapshot: snapshot) }
        // The app requests reloads after publishing. This inexpensive fallback
        // reads the shared file only; it never polls providers or runs the CLI.
        completion(Timeline(entries: entries, policy: .after(now.addingTimeInterval(900))))
    }

    private func readSnapshot(at now: Date) -> UsageWidgetSnapshot {
        (try? UsageWidgetStore.shared()?.read()) ?? UsageWidgetSnapshot(generatedAt: now)
    }
}

#if WIDGET_EXTENSION
@main
#endif
struct AIUsageMediumWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: UsageWidgetSnapshot.widgetKind, provider: UsageWidgetProvider()) { entry in
            UsageWidgetView(snapshot: entry.snapshot, now: entry.date, isPreview: entry.isPreview)
                .containerBackground(for: .widget) { Color.clear }
        }
        .configurationDisplayName("AI Usage")
        .description("Acompanhe as cinco leituras do menu, sem novas conexões às suas contas.")
        .supportedFamilies([.systemMedium])
        .contentMarginsDisabled()
    }
}
