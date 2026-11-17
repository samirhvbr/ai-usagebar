// The optional WidgetKit host shares the existing menu's single refresh worker.
// Only this allowlisted display projection crosses into the shared container.
import Foundation
import AppKit
import WidgetKit

private var lastRequestedWidgetSnapshot: UsageWidgetSnapshot?

func widgetSlot(for id: String) -> UsageWidgetSlot? {
    let canonical = isDesktopAccountId(id)
        ? CLAUDE_ACCOUNT_ID_PREFIX + (accountLabel(of: id) ?? "") : id
    return UsageWidgetSlot(rawValue: canonical)
}

/// Union the visible menu requests with the configured widget accounts. The
/// catalog and account discovery remain authoritative; a widget never enables
/// a provider, invents a login, or fetches an unconfigured account.
func widgetCollectionEntries(menu: [MenuEntry], available: [MenuEntry]) -> [MenuEntry] {
    var seen = Set<String>()
    let allowed = Set(available.map { $0.id })
    return (menu.filter { allowed.contains($0.id) }
            + available.filter { widgetSlot(for: $0.id) != nil }).filter {
        seen.insert($0.id).inserted
    }
}

func widgetRows(readings: [MenuReading], catalog: [VendorCatalogEntry],
                headline: (Snapshot) -> (pct: Int, value: String, reset: String?, elapsed: Int?))
    -> [UsageWidgetRow] {
    UsageWidgetSlot.allCases.map { slot in
        let base = String(slot.rawValue.split(separator: "@")[0])
        guard let vendor = catalog.first(where: { $0.id == base }) else {
            return UsageWidgetRow(slot: slot, status: .unavailable)
        }
        guard vendor.enabled else {
            return UsageWidgetRow(slot: slot, status: .disabled)
        }
        guard let reading = readings.first(where: { widgetSlot(for: $0.entry.id) == slot }),
              let snapshot = reading.snapshot else {
            return UsageWidgetRow(slot: slot, status: .unavailable)
        }
        let result = headline(snapshot)
        // A missing or unlimited quota has no numerical percentage. In
        // particular, overviewHeadline's internal 0 must not become a fake 0%.
        let numeric = result.value.hasSuffix("%") ? result.pct : nil
        return UsageWidgetRow(slot: slot, title: reading.entry.name, pct: numeric, value: result.value,
                              reset: result.reset,
                              resetAt: result.reset.flatMap { resetDate($0, now: reading.checkedAt) },
                              headline: snapshot.plan, stale: reading.stale,
                              checkedAt: reading.checkedAt, status: .available)
    }
}

/// Reload for a changed reading, or a bounded freshness heartbeat. Merely
/// observing an unchanged value on every menu tick does not spend a reload.
func widgetReloadRequired(previous: UsageWidgetSnapshot?, next: UsageWidgetSnapshot) -> Bool {
    guard let previous else { return true }
    if next.generatedAt.timeIntervalSince(previous.generatedAt) >= 300 { return true }
    return zip(previous.rows, next.rows).contains { old, new in
        if old.slot != new.slot || old.title != new.title || old.pct != new.pct
            || old.value != new.value || old.status != new.status || old.stale != new.stale {
            return true
        }
        switch (old.resetAt, new.resetAt) {
        case let (a?, b?): return abs(a.timeIntervalSince(b)) > 120
        case (nil, nil): return old.reset != new.reset
        default: return true
        }
    }
}

extension AppDelegate {
    @objc func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        statusItem.button?.performClick(nil)
        return false
    }

    func refreshWidgetHost(bin: String, generation: Int, vendor: String) {
        let catalogGeneration = vendorCatalogGeneration
        let accountGeneration = accountStatusGeneration
        let catalog = vendorCatalog
        // Catalog discovery owns enablement and account availability. Waiting
        // for it avoids treating the stored selection as an implicit opt-in.
        guard !catalog.isEmpty else {
            finishRefresh(generation) { _ in }
            return
        }
        let available = vendorEntries(active: vendor == "overview" ? "" : vendor,
                                      usageAccounts: lastAccountStatus?.usageAccounts)
        let menu = vendor == "overview"
            ? filterOverviewEntries(available, requested: configuredOverviewVendorIds())
            : available.filter { $0.id == vendor }
        if vendor != "overview", menu.isEmpty {
            setError("Selected provider is disabled or unavailable")
        }
        let entries = widgetCollectionEntries(menu: menu, available: available)
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let readings = collectMenuReadings(bin: bin, entries: entries,
                shouldContinue: { vendor == VENDOR }, onReading: { reading in
                    guard vendor != "overview", reading.entry.id == vendor else { return }
                    // The menu's selected provider is first. Display it now,
                    // without waiting for slower widget-only accounts. The
                    // in-flight slot remains held until the serial sweep ends.
                    DispatchQueue.main.async {
                        guard let app = self, generation == app.refreshGeneration,
                              vendor == VENDOR,
                              catalogGeneration == app.vendorCatalogGeneration,
                              accountGeneration == app.accountStatusGeneration else { return }
                        if reading.output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            app.setError("ai-usagebar did not return a reading")
                        } else {
                            app.consume(reading.output)
                        }
                    }
                })
            DispatchQueue.main.async {
                self?.finishRefresh(generation) { app in
                    guard vendor == VENDOR else { return }
                    guard catalogGeneration == app.vendorCatalogGeneration,
                          accountGeneration == app.accountStatusGeneration else {
                        app.refreshQueued = true
                        return
                    }
                    if vendor == "overview" {
                        let ids = Set(menu.map { $0.id })
                        app.renderOverview(readings.filter { ids.contains($0.entry.id) }.map {
                            (name: $0.entry.name, id: $0.entry.id, snap: $0.snapshot)
                        })
                    } else if !readings.contains(where: { $0.entry.id == vendor }) {
                        app.setError("Selected provider is disabled or unavailable")
                    }
                    // Discovery starts asynchronously. Do not overwrite a good
                    // persisted snapshot with an empty startup catalog.
                    guard !catalog.isEmpty, let store = UsageWidgetStore.shared() else { return }
                    let snapshot = UsageWidgetSnapshot(
                        generatedAt: Date(),
                        rows: widgetRows(readings: readings, catalog: catalog,
                                         headline: app.overviewHeadline),
                        staleAfter: max(300, INTERVAL * 3))
                    do {
                        try store.write(snapshot)
                        if widgetReloadRequired(previous: lastRequestedWidgetSnapshot, next: snapshot) {
                            WidgetCenter.shared.reloadTimelines(ofKind: UsageWidgetSnapshot.widgetKind)
                            lastRequestedWidgetSnapshot = snapshot
                        }
                    } catch {
                        // Failure to publish the optional widget must not take
                        // the working menu or its credential owner offline.
                        NSLog("AI Usage widget snapshot could not be published")
                    }
                }
            }
        }
    }
}
