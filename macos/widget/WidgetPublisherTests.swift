// Hermetic integration checks: the widget uses the menu's existing headline
// and request IDs, without reading configuration, credentials or the network.
import Foundation

@main
struct WidgetPublisherTests {
    static func main() throws {
        var checks = 0
        func expect(_ condition: @autoclosure () -> Bool, _ label: String) {
            precondition(condition(), label)
            checks += 1
        }
        let now = Date(timeIntervalSince1970: 1_700_000_000)
        let app = AppDelegate()
        func catalog(_ id: String, enabled: Bool = true) -> VendorCatalogEntry {
            VendorCatalogEntry(id: id, name: id, shortName: id, kind: "oauth",
                               enabled: enabled, configured: true,
                               needsCredential: true, env: "", login: "")
        }
        func reading(_ id: String, snapshot: Snapshot?, stale: Bool = false) -> MenuReading {
            MenuReading(entry: MenuEntry(id: id, name: id), output: "not exported",
                        snapshot: snapshot, checkedAt: now, stale: stale)
        }
        func snapshot(_ pct: Int?, unlimited: Bool = false) -> Snapshot {
            Snapshot(plan: "Test plan", hasUsageWindows: true, creditBalance: nil,
                     session: pct.map { Window(pct: $0, reset: "2h", elapsed: nil,
                                               unlimited: unlimited) },
                     weekly: nil, sonnet: nil, sonnetLabel: "", extra: nil)
        }
        expect(widgetSlot(for: "anthropic-desktop@b3") == .anthropicB3, "Desktop account maps to b3")
        expect(widgetSlot(for: "anthropic@me") == .anthropicMe, "CLI account maps to me")
        expect(widgetSlot(for: "anthropic") == nil, "Unnamed Claude is not a widget slot")
        expect(widgetSlot(for: "openai@other") == nil, "An unrelated account is not substituted")

        let available = ["anthropic@me", "anthropic-desktop@b3", "openai", "kimi", "shvia", "cursor"]
            .map { MenuEntry(id: $0, name: $0) }
        let requests = widgetCollectionEntries(menu: [available[2]], available: available)
        expect(requests.count == 5, "Single-provider menu shares one five-slot sweep")
        expect(requests.filter { $0.id == "openai" }.count == 1, "Selected provider is fetched once")
        expect(!requests.contains { $0.id == "cursor" }, "Unrequested providers are not added")
        expect(widgetCollectionEntries(menu: available, available: available).count == 6,
               "Overview entries stay intact and are not duplicated")
        let selectedDisabled = widgetCollectionEntries(menu: [available[3]], available: [available[2]])
        expect(!selectedDisabled.contains { $0.id == "kimi" },
               "A disabled stored selection is never restored as an implicit opt-in")
        expect(widgetCollectionEntries(menu: [available[3]], available: []).isEmpty,
               "An unknown startup catalog cannot authorize a stored selection")

        var events: [String] = []
        _ = collectMenuReadings(bin: "/not/a/real/cli", entries: Array(available.prefix(2)),
            onReading: { events.append("display:" + $0.entry.id) },
            readEntry: { _, entry in
                if entry.id == available[1].id {
                    expect(events == ["display:" + available[0].id],
                           "Selected provider is delivered before collecting the next account")
                }
                return reading(entry.id, snapshot: snapshot(1))
            })
        expect(events.count == 2, "One progress callback per collected account")
        var continueSweep = true
        var requestsStarted = 0
        let cancelled = collectMenuReadings(bin: "/not/a/real/cli", entries: available,
            shouldContinue: { continueSweep },
            onReading: { _ in continueSweep = false },
            readEntry: { _, entry in
                requestsStarted += 1
                return reading(entry.id, snapshot: snapshot(1))
            })
        expect(cancelled.count == 1 && requestsStarted == 1,
               "A selection change stops remaining widget-only requests between accounts")

        let catalogs = ["anthropic", "openai", "kimi", "shvia"].map { catalog($0) }
        var kimi = snapshot(4)
        kimi.secondaryWeekly = Window(pct: 97, reset: "5d", elapsed: nil)
        let readings = [reading("anthropic@me", snapshot: snapshot(0)),
                        reading("anthropic-desktop@b3", snapshot: snapshot(76)),
                        reading("openai", snapshot: snapshot(nil)),
                        reading("kimi", snapshot: kimi, stale: true),
                        reading("shvia", snapshot: snapshot(0, unlimited: true))]
        let rows = widgetRows(readings: readings, catalog: catalogs, headline: app.overviewHeadline)
        expect(rows.count == 5, "All five widget slots are retained")
        expect(rows[0].pct == 0, "A real zero remains a real zero")
        expect(rows[1].pct == 76, "Named b3 reading is preserved")
        expect(rows[2].pct == nil && rows[2].value == "—", "Unknown usage never becomes zero")
        expect(rows[3].pct == 97 && rows[3].stale, "Kimi uses the menu's highest-window headline")
        expect(rows[3].resetAt == now.addingTimeInterval(5 * 86_400), "Existing reset parser is reused")
        expect(rows[4].pct == nil && rows[4].value == "Unlimited", "Unlimited is not zero percent")
        expect(rows.allSatisfy { $0.checkedAt == now }, "Timestamp is the actual local observation")
        let disabled = widgetRows(readings: readings, catalog: [catalog("openai", enabled: false)],
                                  headline: app.overviewHeadline)
        expect(disabled[2].status == .disabled && disabled[2].pct == nil,
               "Disabled providers cannot publish an old reading")
        expect(disabled[0].status == .unavailable, "Missing catalog metadata is unknown, not disabled")
        let failed = widgetRows(readings: [], catalog: catalogs, headline: app.overviewHeadline)
        expect(failed.allSatisfy { $0.pct == nil && $0.status == .unavailable },
               "A failed collection cannot look like fresh zero usage")

        let raw = "{\"text\":\"invalid ⏸\",\"tooltip\":\"sensitive diagnostic\"}"
        let parsed = menuReading(entry: available[0], output: raw, checkedAt: now)
        expect(parsed.stale && parsed.checkedAt == now, "Transport preserves backend stale marker")
        let document = UsageWidgetSnapshot(generatedAt: now, rows: rows)
        expect(widgetReloadRequired(previous: nil, next: document), "First publication requests a reload")
        let unchanged = UsageWidgetSnapshot(generatedAt: now.addingTimeInterval(30), rows: rows)
        expect(!widgetReloadRequired(previous: document, next: unchanged), "Unchanged ticks do not reload")
        let heartbeat = UsageWidgetSnapshot(generatedAt: now.addingTimeInterval(300), rows: rows)
        expect(widgetReloadRequired(previous: document, next: heartbeat), "Freshness heartbeat is bounded")
        let changed = UsageWidgetSnapshot(generatedAt: now.addingTimeInterval(30),
                                         rows: [UsageWidgetRow(slot: .anthropicMe, pct: 1, status: .available)])
        expect(widgetReloadRequired(previous: document, next: changed), "Changed content requests a reload")
        let encoded = String(data: try JSONEncoder().encode(document), encoding: .utf8)!
        expect(!encoded.contains("sensitive diagnostic") && !encoded.contains("not exported"),
               "Raw CLI responses and diagnostics never enter the shared document")
        print("\(checks) widget publisher checks passed")
    }
}
