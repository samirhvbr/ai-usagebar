import Foundation

/// The fixed selection for the first widget. Account IDs stay identical to
/// the menu's IDs; credentials and provider responses never enter this model.
enum UsageWidgetSlot: String, Codable, CaseIterable, Identifiable {
    case anthropicMe = "anthropic@me"
    case anthropicB3 = "anthropic@b3"
    case openai
    case kimi
    case shvia

    var id: String { rawValue }

    var defaultTitle: String {
        switch self {
        case .anthropicMe: return "Claude · me"
        case .anthropicB3: return "Claude · b3"
        case .openai: return "Codex"
        case .kimi: return "Kimi"
        case .shvia: return "ShvIA"
        }
    }
}

enum UsageWidgetStatus: String, Codable {
    case available
    case unavailable
    case disabled
}

/// A presentation-only reading copied from the menu's existing refresh.
/// `checkedAt` is the local observation time, not an API fetch timestamp.
struct UsageWidgetRow: Codable, Equatable, Identifiable {
    let slot: UsageWidgetSlot
    let title: String
    let pct: Int?
    let value: String?
    let reset: String?
    let resetAt: Date?
    let headline: String?
    let stale: Bool
    let checkedAt: Date?
    let status: UsageWidgetStatus

    var id: String { slot.rawValue }

    init(slot: UsageWidgetSlot, title: String? = nil, pct: Int? = nil,
         value: String? = nil, reset: String? = nil, resetAt: Date? = nil,
         headline: String? = nil, stale: Bool = false, checkedAt: Date? = nil,
         status: UsageWidgetStatus = .unavailable) {
        self.slot = slot
        self.title = widgetText(title, limit: 48) ?? slot.defaultTitle
        // A missing reading stays missing. Only a present percentage is
        // clamped, matching the meter's bounded geometry.
        self.pct = pct.map { min(100, max(0, $0)) }
        self.value = widgetText(value, limit: 64)
        self.reset = widgetText(reset, limit: 48)
        self.resetAt = resetAt
        self.headline = widgetText(headline, limit: 64)
        self.stale = stale
        self.checkedAt = checkedAt
        self.status = status
    }

    private enum CodingKeys: String, CodingKey {
        case slot, title, pct, value, reset, resetAt, headline, stale, checkedAt, status
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            slot: try c.decode(UsageWidgetSlot.self, forKey: .slot),
            title: try c.decodeIfPresent(String.self, forKey: .title),
            pct: try c.decodeIfPresent(Int.self, forKey: .pct),
            value: try c.decodeIfPresent(String.self, forKey: .value),
            reset: try c.decodeIfPresent(String.self, forKey: .reset),
            resetAt: try c.decodeIfPresent(Date.self, forKey: .resetAt),
            headline: try c.decodeIfPresent(String.self, forKey: .headline),
            stale: try c.decodeIfPresent(Bool.self, forKey: .stale) ?? false,
            checkedAt: try c.decodeIfPresent(Date.self, forKey: .checkedAt),
            status: try c.decode(UsageWidgetStatus.self, forKey: .status)
        )
    }

    func isStale(at now: Date, staleAfter: TimeInterval) -> Bool {
        guard status == .available else { return false }
        guard let checkedAt else { return true }
        return stale || now.timeIntervalSince(checkedAt) >= staleAfter
            || checkedAt.timeIntervalSince(now) > 60
            || resetAt.map { $0 <= now } == true
    }

    /// Timeline boundaries let an idle host's last reading age visibly, even
    /// when WidgetKit does not grant another timeline reload immediately.
    func staleDate(staleAfter: TimeInterval) -> Date? {
        guard status == .available, !stale, let checkedAt else { return nil }
        let ageDate = checkedAt.addingTimeInterval(staleAfter)
        return resetAt.map { min($0, ageDate) } ?? ageDate
    }
}

/// A versioned export, not a second provider cache. Missing slots are kept as
/// unavailable rows so a failed collection cannot silently remove an account.
struct UsageWidgetSnapshot: Codable, Equatable {
    static let currentSchemaVersion = 1
    static let widgetKind = "AIUsageMediumWidget"
    static let defaultStaleAfter: TimeInterval = 300

    let schemaVersion: Int
    let generatedAt: Date
    let rows: [UsageWidgetRow]
    let staleAfter: TimeInterval

    init(generatedAt: Date = Date(), rows: [UsageWidgetRow] = [],
         staleAfter: TimeInterval = Self.defaultStaleAfter) {
        self.schemaVersion = Self.currentSchemaVersion
        self.generatedAt = generatedAt
        self.staleAfter = staleAfter.isFinite && staleAfter > 0
            ? min(86_400, max(30, staleAfter)) : Self.defaultStaleAfter
        self.rows = UsageWidgetSlot.allCases.map { slot in
            // A duplicate can arrive while default/desktop IDs converge.
            // Keep the newest observation rather than whichever was appended.
            rows.filter { $0.slot == slot }.max {
                ($0.checkedAt ?? .distantPast) < ($1.checkedAt ?? .distantPast)
            } ?? UsageWidgetRow(slot: slot)
        }
    }

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, generatedAt, rows, staleAfter
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let version = try c.decode(Int.self, forKey: .schemaVersion)
        guard version == Self.currentSchemaVersion else {
            throw DecodingError.dataCorruptedError(forKey: .schemaVersion, in: c,
                debugDescription: "Unsupported widget snapshot version")
        }
        self.init(generatedAt: try c.decode(Date.self, forKey: .generatedAt),
                  rows: try c.decode([UsageWidgetRow].self, forKey: .rows),
                  staleAfter: try c.decodeIfPresent(Double.self, forKey: .staleAfter)
                    ?? Self.defaultStaleAfter)
    }

    var lastCheckedAt: Date? { rows.compactMap(\.checkedAt).max() }

    func hasStaleReadings(at now: Date) -> Bool {
        rows.contains { $0.isStale(at: now, staleAfter: staleAfter) }
    }

    func timelineDates(after now: Date) -> [Date] {
        Array(Set(rows.compactMap { $0.staleDate(staleAfter: staleAfter) }
            .filter { $0 > now })).sorted()
    }
}

/// Text is rendered as plain text, limited in length, and stripped of control
/// characters and bidi overrides. Keep printable Unicode, including emoji.
private func widgetText(_ text: String?, limit: Int) -> String? {
    guard let text else { return nil }
    let cleaned = String(String.UnicodeScalarView(text.unicodeScalars.map { scalar in
        let n = scalar.value
        let joiner = n == 0x200c || n == 0x200d
        if (CharacterSet.controlCharacters.contains(scalar) && !joiner)
            || CharacterSet.newlines.contains(scalar)
            || n == 0x061c || n == 0x200e || n == 0x200f
            || (0x202a...0x202e).contains(n) || (0x2066...0x2069).contains(n) {
            return Unicode.Scalar(0x20)!
        }
        return scalar
    })).split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
    let bounded = String(cleaned.prefix(limit))
    return bounded.isEmpty ? nil : bounded
}

/// Only the host calls `write`. The extension calls `read` and never starts a
/// process, loads a credential, or contacts a provider. Tests inject a temp URL.
struct UsageWidgetStore {
    static let fileName = "snapshot.json"
    static let appGroupInfoKey = "AIUsageWidgetAppGroup"
    static let maximumBytes = 128 * 1024
    let fileURL: URL

    static func shared(bundle: Bundle = .main) -> UsageWidgetStore? {
        guard let identifier = bundle.object(forInfoDictionaryKey: appGroupInfoKey) as? String,
              !identifier.isEmpty,
              let container = FileManager.default.containerURL(
                forSecurityApplicationGroupIdentifier: identifier) else { return nil }
        return UsageWidgetStore(fileURL: container.appendingPathComponent(fileName))
    }

    func read() throws -> UsageWidgetSnapshot {
        let handle = try FileHandle(forReadingFrom: fileURL)
        defer { try? handle.close() }
        let data = try handle.read(upToCount: Self.maximumBytes + 1) ?? Data()
        guard data.count <= Self.maximumBytes else { throw StoreError.snapshotTooLarge }
        return try Self.decoder().decode(UsageWidgetSnapshot.self, from: data)
    }

    func write(_ snapshot: UsageWidgetSnapshot) throws {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(snapshot)
        guard data.count <= Self.maximumBytes else { throw StoreError.snapshotTooLarge }
        let files = FileManager.default
        let directory = fileURL.deletingLastPathComponent()
        try files.createDirectory(at: directory, withIntermediateDirectories: true,
                                  attributes: [.posixPermissions: 0o700])
        let temporary = directory.appendingPathComponent(".widget-\(UUID().uuidString).tmp")
        guard files.createFile(atPath: temporary.path, contents: nil,
                               attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteUnknown)
        }
        defer { try? files.removeItem(at: temporary) }
        let handle = try FileHandle(forWritingTo: temporary)
        do {
            try handle.write(contentsOf: data)
            try handle.synchronize()
            try handle.close()
        } catch {
            try? handle.close()
            throw error
        }
        // Same-directory replacement is atomic. The new file is already
        // private before it becomes visible under the shared snapshot name.
        if files.fileExists(atPath: fileURL.path) {
            _ = try files.replaceItemAt(fileURL, withItemAt: temporary,
                                       options: .usingNewMetadataOnly)
        } else {
            try files.moveItem(at: temporary, to: fileURL)
        }
    }

    private static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }

    enum StoreError: Error { case snapshotTooLarge }
}
