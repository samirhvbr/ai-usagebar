import Foundation

@main
struct WidgetSnapshotTests {
    static var failures = 0
    static let now = Date(timeIntervalSince1970: 1_795_000_000)

    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        if !condition() {
            failures += 1
            print("FAIL: \(message)")
        }
    }

    static func expectFailure(_ message: String, _ body: () throws -> Void) {
        do {
            try body()
            check(false, message)
        } catch { }
    }

    static func reading(_ slot: UsageWidgetSlot = .openai, checkedAt: Date = now,
                        percent: Int? = 42, resetAt: Date? = nil) -> UsageWidgetRow {
        UsageWidgetRow(slot: slot, pct: percent, value: percent.map { "\($0)%" },
                       reset: "2h 10m", resetAt: resetAt, headline: "Session",
                       checkedAt: checkedAt, status: .available)
    }

    static func testSelectionAndMissingData() {
        let empty = UsageWidgetSnapshot(generatedAt: now)
        check(empty.rows.map(\.id) == ["anthropic@me", "anthropic@b3", "openai", "kimi", "shvia"],
              "all five fixed slots remain visible and ordered")
        check(empty.rows.allSatisfy { $0.pct == nil && $0.value == nil && $0.status == .unavailable },
              "an unobserved quota is never represented as zero")
        check(empty.lastCheckedAt == nil, "writing an empty snapshot is not a provider observation")

        let zero = reading(percent: 0)
        check(zero.pct == 0 && zero.value == "0%", "a reported zero remains a real reading")
        let unlimited = UsageWidgetRow(slot: .shvia, value: "Unlimited", checkedAt: now, status: .available)
        check(unlimited.pct == nil && unlimited.value == "Unlimited", "unlimited does not fabricate a percentage")
        check(reading(percent: -1).pct == 0 && reading(percent: 140).pct == 100,
              "present percentages clamp to the meter bounds")

        let newest = reading(checkedAt: now, percent: 67)
        let older = reading(checkedAt: now.addingTimeInterval(-60), percent: 12)
        for rows in [[newest, older], [older, newest]] {
            let snapshot = UsageWidgetSnapshot(generatedAt: now, rows: rows)
            check(snapshot.rows[2].pct == 67, "duplicate IDs keep the newest observation independent of order")
        }
    }

    static func testSanitization() {
        let row = UsageWidgetRow(slot: .kimi, title: "Kimi\n\u{202e} test", value: "  42%\t ",
                                 reset: "\n", headline: String(repeating: "A", count: 300))
        check(row.title == "Kimi test", "control and directional override characters are not displayed")
        check(row.value == "42%" && row.reset == nil, "blank fields stay absent after cleaning")
        check(row.headline?.count == 64, "untrusted strings have a bounded display length")
        let unicode = UsageWidgetRow(slot: .shvia, title: "ShvIA · ação 🧑‍💻")
        check(unicode.title == "ShvIA · ação 🧑‍💻", "printable Unicode and joined emoji are preserved")
    }

    static func testFreshnessAndResetBoundaries() {
        let fresh = reading(resetAt: now.addingTimeInterval(120))
        check(!fresh.isStale(at: now, staleAfter: 300), "a recent observation remains fresh")
        check(fresh.isStale(at: now.addingTimeInterval(120), staleAfter: 300),
              "the known reset boundary makes the old figure stale, including equality")
        check(reading().isStale(at: now.addingTimeInterval(300), staleAfter: 300),
              "an idle producer's snapshot ages even with no new writes")
        check(UsageWidgetRow(slot: .openai, pct: 10, status: .available)
            .isStale(at: now, staleAfter: 300), "a reading with no observation time is not claimed fresh")
        let marked = UsageWidgetRow(slot: .openai, pct: 10, stale: true,
                                    checkedAt: now, status: .available)
        check(marked.isStale(at: now, staleAfter: 300), "the host's stale marker is preserved")
        check(reading(checkedAt: now.addingTimeInterval(120)).isStale(at: now, staleAfter: 300),
              "a future clock anomaly cannot claim indefinite freshness")
        let disabled = UsageWidgetRow(slot: .shvia, status: .disabled)
        check(!disabled.isStale(at: now, staleAfter: 300), "disabled and stale are distinct states")

        let snapshot = UsageWidgetSnapshot(generatedAt: now, rows: [fresh, reading(.kimi)])
        check(snapshot.timelineDates(after: now) == [now.addingTimeInterval(120), now.addingTimeInterval(300)],
              "the timeline contains reset and age boundaries without another network fetch")
        check(snapshot.timelineDates(after: now.addingTimeInterval(300)).isEmpty,
              "past boundaries are not scheduled repeatedly")
    }

    static func testStore(in directory: URL) throws {
        let store = UsageWidgetStore(fileURL: directory.appendingPathComponent("shared/snapshot.json"))
        let first = UsageWidgetSnapshot(generatedAt: now, rows: [reading(), reading(.shvia)])
        try store.write(first)
        let firstRead = try store.read()
        check(firstRead == first, "atomic snapshot round-trips all readings and nil fields")
        let attributes = try FileManager.default.attributesOfItem(atPath: store.fileURL.path)
        check((attributes[.posixPermissions] as? NSNumber)?.intValue == 0o600,
              "snapshot permissions are private")

        let second = UsageWidgetSnapshot(generatedAt: now, rows: [reading(percent: 68)])
        try store.write(second)
        let secondRead = try store.read()
        check(secondRead == second, "replacement publishes the new complete snapshot")
        let leftovers = try FileManager.default.contentsOfDirectory(atPath: store.fileURL.deletingLastPathComponent().path)
        check(leftovers == [UsageWidgetStore.fileName], "successful replacement leaves no staging files")

        let data = try Data(contentsOf: store.fileURL)
        var object = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        check(object["schemaVersion"] as? Int == 1, "the persisted contract is explicitly versioned")
        let row = (object["rows"] as! [[String: Any]])[2]
        check(row["checkedAt"] != nil && row["fetchedAt"] == nil,
              "local observation is not mislabeled as an API fetch time")
        object["schemaVersion"] = 999
        try JSONSerialization.data(withJSONObject: object).write(to: store.fileURL)
        expectFailure("unsupported versions must not be interpreted as current readings") { _ = try store.read() }
        try Data("incomplete{".utf8).write(to: store.fileURL)
        expectFailure("partial JSON does not become a zero snapshot") { _ = try store.read() }
        try Data(repeating: 0x20, count: UsageWidgetStore.maximumBytes + 1).write(to: store.fileURL)
        expectFailure("oversized snapshots are refused") { _ = try store.read() }
        try store.write(first)

        // A denied write must leave the previous reading in place. The fixture
        // directory is owned by this test and no real App Group is resolved.
        let parent = store.fileURL.deletingLastPathComponent()
        try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: parent.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: parent.path) }
        expectFailure("an unwritable snapshot directory must report failure") { try store.write(second) }
        let afterFailure = try store.read()
        check(afterFailure == first, "a failed publish preserves the previous complete reading")
    }

    static func main() throws {
        testSelectionAndMissingData()
        testSanitization()
        testFreshnessAndResetBoundaries()
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("widget-model-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        try testStore(in: directory)
        guard failures == 0 else { exit(1) }
        print("Widget snapshot tests passed")
    }
}
