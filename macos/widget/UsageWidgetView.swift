import SwiftUI

/// Shared by WidgetKit and the screenshot harness. This view has no I/O and
/// receives its clock explicitly so previews and regression fixtures are stable.
struct UsageWidgetView: View {
    let snapshot: UsageWidgetSnapshot
    let now: Date
    var isPreview = false

    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        GeometryReader { geometry in
            let compact = geometry.size.height < 165
            VStack(spacing: compact ? 5 : 7) {
                HStack(spacing: 6) {
                    Image(systemName: "chart.bar.fill")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(.orange)
                    Text("AI Usage")
                        .font(.system(size: 12, weight: .semibold))
                    Spacer(minLength: 4)
                    if isPreview {
                        Text("PRÉVIA · DADOS FICTÍCIOS")
                            .font(.system(size: 8, weight: .medium))
                            .foregroundStyle(.secondary)
                    } else if snapshot.hasStaleReadings(at: now) {
                        Label("Dados antigos", systemImage: "clock.badge.exclamationmark")
                            .font(.system(size: 9, weight: .medium))
                            .foregroundStyle(.orange)
                    }
                }

                VStack(spacing: compact ? 3 : 5) {
                    ForEach(snapshot.rows) { row in
                        readingRow(row, compact: compact)
                    }
                }

                HStack(spacing: 4) {
                    Image(systemName: "clock")
                    if let checked = snapshot.lastCheckedAt {
                        Text("Verificado")
                        Text(checked, style: .time)
                            .monospacedDigit()
                    } else {
                        Text("Abra o app para carregar as leituras")
                    }
                    Spacer(minLength: 0)
                    Image(systemName: "arrow.up.forward")
                }
                .font(.system(size: 9))
                .foregroundStyle(.secondary)
            }
            .padding(compact ? 10 : 12)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
        }
        .background(background)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(isPreview ? "Prévia do AI Usage com dados fictícios" : "AI Usage")
    }

    private var background: Color {
        colorScheme == .dark
            ? Color(red: 0.10, green: 0.115, blue: 0.14)
            : Color(red: 0.97, green: 0.975, blue: 0.985)
    }

    private func readingRow(_ row: UsageWidgetRow, compact: Bool) -> some View {
        let stale = row.isStale(at: now, staleAfter: snapshot.staleAfter)
        let available = row.status == .available
        return HStack(spacing: 7) {
            Text(row.title)
                .font(.system(size: 11, weight: .medium))
                .lineLimit(1)
                .minimumScaleFactor(0.8)
                .frame(width: 83, alignment: .leading)

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.primary.opacity(0.08))
                    if available, let percent = row.pct {
                        Capsule()
                            .fill(meterColor(percent).opacity(stale ? 0.45 : 0.9))
                            .frame(width: geometry.size.width * CGFloat(percent) / 100)
                    }
                }
            }
            .frame(height: 5)
            .accessibilityHidden(true)

            Text(readingValue(row))
                .font(.system(size: 11, weight: .semibold, design: .monospaced))
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .foregroundStyle(available ? Color.primary : Color.secondary)
                .frame(width: 40, alignment: .trailing)

            HStack(spacing: 2) {
                if stale {
                    Image(systemName: "clock")
                        .font(.system(size: 8))
                }
                Text(resetLabel(row))
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
            }
            .font(.system(size: 9, design: .monospaced))
            .foregroundStyle(stale ? Color.orange : Color.secondary)
            .frame(width: 52, alignment: .trailing)
        }
        .frame(height: compact ? 17 : 19)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(accessibilityDescription(row, stale: stale))
    }

    private func readingValue(_ row: UsageWidgetRow) -> String {
        guard row.status == .available else { return "—" }
        if let value = row.value { return value }
        return row.pct.map { "\($0)%" } ?? "—"
    }

    private func resetLabel(_ row: UsageWidgetRow) -> String {
        switch row.status {
        case .disabled: return "Desligado"
        case .unavailable: return "Sem dados"
        case .available:
            if let resetAt = row.resetAt {
                let remaining = resetAt.timeIntervalSince(now)
                if remaining <= 0 { return "Rever" }
                let minutes = max(1, Int(ceil(remaining / 60)))
                if minutes < 60 { return "\(minutes)m" }
                let hours = minutes / 60
                if hours < 24 { return "\(hours)h \(minutes % 60)m" }
                return "\(hours / 24)d \(hours % 24)h"
            }
            return row.reset ?? "—"
        }
    }

    private func meterColor(_ percent: Int) -> Color {
        if percent >= 90 { return .red }
        if percent >= 75 { return .orange }
        return Color(red: 0.25, green: 0.64, blue: 0.61)
    }

    private func accessibilityDescription(_ row: UsageWidgetRow, stale: Bool) -> String {
        var parts = [row.title, readingValue(row)]
        if let headline = row.headline { parts.append(headline) }
        parts.append(resetLabel(row))
        if stale { parts.append("Leitura desatualizada") }
        return parts.joined(separator: ", ")
    }
}

/// Explicitly synthetic input for the gallery and screenshot harness. The
/// production provider never substitutes this fixture for a missing snapshot.
enum UsageWidgetPreview {
    static let date = Date(timeIntervalSince1970: 1_795_000_000)

    static func snapshot(at date: Date = UsageWidgetPreview.date) -> UsageWidgetSnapshot {
        let percentages = [38, 67, 24, 82, 46]
        let resets: [TimeInterval] = [7_800, 13_200, 15_300, 691_200, 39_600]
        let rows = UsageWidgetSlot.allCases.enumerated().map { index, slot in
            UsageWidgetRow(slot: slot, pct: percentages[index], value: "\(percentages[index])%",
                           resetAt: date.addingTimeInterval(resets[index]), headline: "Synthetic example",
                           checkedAt: date, status: .available)
        }
        return UsageWidgetSnapshot(generatedAt: date, rows: rows)
    }
}
