// ai-usagebar-menubar — macOS menu bar app for ai-usagebar.
//
// Shows ai-usagebar's 5-hour (session), weekly, and optional extra-usage
// bars in the macOS menu bar, next to the clock, with a native dropdown.
// Mirrors the GNOME Shell extension: same binary, same One Dark colors and
// severity thresholds. Runs as a menu-bar agent (no Dock icon).
//
// Build:  swiftc -O ai-usagebar-menubar.swift -o ai-usagebar-menubar
//         (needs the Xcode command-line tools: `xcode-select --install`)
// Run:    ./ai-usagebar-menubar &      (or ./install-agent.sh for login start)
//
// First, on the Mac: run `claude` once so the OAuth creds land in the login
// Keychain — ai-usagebar reads them there (src/anthropic/keychain.rs).

import Cocoa

// ─── Config (edit, then rebuild with ./build.sh) ─────────────────────────
let VENDOR       = "anthropic"
let INTERVAL     = 30.0      // seconds between refreshes
let BAR_WIDTH    = 8         // cells per menu-bar bar
let MENU_BAR_W   = 14        // cells per dropdown bar
let SHOW_SESSION = true
let SHOW_WEEKLY  = true
let SHOW_EXTRA   = false     // extra-usage (cost) as a third bar
let SHOW_PERCENT = true
let SHOW_BARS    = true      // false = numbers only, no bars

// One Dark bar colors (>=90 critical, >=75 high, >=50 mid, else low).
// Tags/labels use system colors so they adapt to a light/dark menu bar.
let COLOR_LOW      = "#98c379"
let COLOR_MID      = "#e5c07b"
let COLOR_HIGH     = "#d19a66"
let COLOR_CRITICAL = "#e06c75"
let COLOR_EMPTY    = "#3e4451"

let FORMAT = "{plan};;{session_pct};;{session_reset};;{weekly_pct};;{weekly_reset};;" +
             "{sonnet_pct};;{sonnet_reset};;{extra_pct};;{extra_spent};;{extra_limit}"

// ─── Color / text helpers ────────────────────────────────────────────────
func hexColor(_ hex: String) -> NSColor {
    var s = hex
    if s.hasPrefix("#") { s.removeFirst() }
    guard s.count == 6, let v = UInt32(s, radix: 16) else { return .labelColor }
    return NSColor(srgbRed: CGFloat((v >> 16) & 0xff) / 255.0,
                   green: CGFloat((v >> 8) & 0xff) / 255.0,
                   blue: CGFloat(v & 0xff) / 255.0,
                   alpha: 1.0)
}

func colorForPct(_ pct: Int) -> NSColor {
    if pct >= 90 { return hexColor(COLOR_CRITICAL) }
    if pct >= 75 { return hexColor(COLOR_HIGH) }
    if pct >= 50 { return hexColor(COLOR_MID) }
    return hexColor(COLOR_LOW)
}

let barFont = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)

func run(_ s: String, _ color: NSColor, _ font: NSFont = barFont) -> NSAttributedString {
    NSAttributedString(string: s, attributes: [.foregroundColor: color, .font: font])
}

func barAttr(pct: Int, width: Int) -> NSAttributedString {
    let p = max(0, min(100, pct))
    let filled = Int((Double(p) * Double(width) / 100.0).rounded())
    let out = NSMutableAttributedString()
    out.append(run(String(repeating: "█", count: filled), colorForPct(p)))
    out.append(run(String(repeating: "░", count: max(0, width - filled)), hexColor(COLOR_EMPTY)))
    return out
}

func resolveBinary(_ name: String) -> String? {
    let home = NSHomeDirectory()
    let fm = FileManager.default
    for c in ["\(home)/.cargo/bin/\(name)", "/opt/homebrew/bin/\(name)", "/usr/local/bin/\(name)"]
    where fm.isExecutableFile(atPath: c) {
        return c
    }
    // Fall back to `which` (login shells put cargo/brew on PATH).
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/which")
    p.arguments = [name]
    let pipe = Pipe()
    p.standardOutput = pipe
    p.standardError = FileHandle.nullDevice
    do {
        try p.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        let path = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !path.isEmpty && fm.isExecutableFile(atPath: path) { return path }
    } catch {}
    return nil
}

// ─── Data model ──────────────────────────────────────────────────────────
struct Window { let pct: Int; let reset: String }
struct Snapshot {
    let plan: String
    let session: Window
    let weekly: Window
    let sonnet: Window?
    let extra: (pct: Int, spent: String, limit: String)?
}

func stripMarkup(_ s: String) -> String {
    s.replacingOccurrences(of: "<[^>]*>", with: "", options: .regularExpression)
}

func parse(_ text: String) -> Snapshot? {
    let f = stripMarkup(text).components(separatedBy: ";;")
    guard f.count >= 10 else { return nil }
    func t(_ i: Int) -> String { f[i].trimmingCharacters(in: .whitespaces) }
    func n(_ i: Int) -> Int? { Int(t(i)) }
    let sonnet = n(5).map { Window(pct: $0, reset: t(6)) }
    let spent = t(8)
    let extra: (pct: Int, spent: String, limit: String)? =
        spent.isEmpty ? nil : (n(7) ?? 0, spent, t(9))
    return Snapshot(plan: t(0),
                    session: Window(pct: n(1) ?? 0, reset: t(2)),
                    weekly: Window(pct: n(3) ?? 0, reset: t(4)),
                    sonnet: sonnet,
                    extra: extra)
}

// ─── App ─────────────────────────────────────────────────────────────────
class AppDelegate: NSObject, NSApplicationDelegate {
    var statusItem: NSStatusItem!
    var timer: Timer?
    let binary = resolveBinary("ai-usagebar")
    let headerItem = NSMenuItem()
    var rows: [String: NSMenuItem] = [:]

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.title = "5h …"
        buildMenu()
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: INTERVAL, repeats: true) { [weak self] _ in
            self?.refresh()
        }
    }

    func buildMenu() {
        let menu = NSMenu()
        menu.autoenablesItems = false

        menu.addItem(headerItem)   // plan name (enabled so colors aren't dimmed)
        for (key, _) in [("session", ""), ("weekly", ""), ("sonnet", ""), ("extra", "")] {
            let it = NSMenuItem()
            rows[key] = it
            menu.addItem(it)
        }

        menu.addItem(.separator())
        let refreshIt = NSMenuItem(title: "Atualizar agora", action: #selector(refreshAction), keyEquivalent: "r")
        refreshIt.target = self
        menu.addItem(refreshIt)
        let tuiIt = NSMenuItem(title: "Abrir TUI", action: #selector(openTui), keyEquivalent: "t")
        tuiIt.target = self
        menu.addItem(tuiIt)
        menu.addItem(.separator())
        let quitIt = NSMenuItem(title: "Sair", action: #selector(quit), keyEquivalent: "q")
        quitIt.target = self
        menu.addItem(quitIt)

        statusItem.menu = menu
    }

    @objc func refreshAction() { refresh() }
    @objc func quit() { NSApp.terminate(nil) }

    @objc func openTui() {
        guard let tui = resolveBinary("ai-usagebar-tui") else { return }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        p.arguments = ["-e", "tell application \"Terminal\" to do script \"\(tui)\""]
        try? p.run()
    }

    func refresh() {
        guard let bin = binary else {
            setError("ai-usagebar não encontrado (PATH / ~/.cargo/bin / homebrew)")
            return
        }
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let p = Process()
            p.executableURL = URL(fileURLWithPath: bin)
            p.arguments = ["--vendor", VENDOR, "--format", FORMAT]
            let pipe = Pipe()
            p.standardOutput = pipe
            p.standardError = FileHandle.nullDevice
            var out = ""
            do {
                try p.run()
                let data = pipe.fileHandleForReading.readDataToEndOfFile()  // read before wait
                p.waitUntilExit()
                out = String(data: data, encoding: .utf8) ?? ""
            } catch {
                DispatchQueue.main.async { self?.setError("falha ao executar ai-usagebar") }
                return
            }
            DispatchQueue.main.async { self?.consume(out) }
        }
    }

    func consume(_ output: String) {
        guard let data = output.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let text = obj["text"] as? String else {
            setError("saída inválida")
            return
        }
        guard let snap = parse(text) else {
            statusItem.button?.attributedTitle = run(stripMarkup(text), .labelColor)  // Loading… / ⚠
            return
        }
        renderPanel(snap)
        renderMenu(snap)
    }

    func renderPanel(_ s: Snapshot) {
        let title = NSMutableAttributedString()
        func seg(_ tag: String, _ pct: Int, _ value: String) {
            if title.length > 0 { title.append(run("   ", .secondaryLabelColor)) }
            title.append(run("\(tag) ", .secondaryLabelColor))
            if SHOW_PERCENT { title.append(run(value + (SHOW_BARS ? " " : ""), colorForPct(pct))) }
            if SHOW_BARS { title.append(barAttr(pct: pct, width: BAR_WIDTH)) }
            if !SHOW_PERCENT && !SHOW_BARS { title.append(run(value, colorForPct(pct))) }
        }
        if SHOW_SESSION { seg("5h", s.session.pct, "\(s.session.pct)%") }
        if SHOW_WEEKLY { seg("7d", s.weekly.pct, "\(s.weekly.pct)%") }
        if SHOW_EXTRA, let e = s.extra { seg("ex", e.pct, e.spent) }
        statusItem.button?.attributedTitle = title
    }

    func renderMenu(_ s: Snapshot) {
        headerItem.attributedTitle = run(s.plan.isEmpty ? "AI Usage" : s.plan,
                                         .labelColor, NSFont.boldSystemFont(ofSize: 13))

        func row(_ key: String, _ name: String, _ pct: Int, _ value: String, _ reset: String?) {
            guard let item = rows[key] else { return }
            item.isHidden = false
            let a = NSMutableAttributedString()
            a.append(run(name.padding(toLength: 12, withPad: " ", startingAt: 0), .labelColor))
            a.append(barAttr(pct: pct, width: MENU_BAR_W))
            a.append(run("  \(value)", colorForPct(pct)))
            if let r = reset, !r.isEmpty { a.append(run("   ↺ \(r)", .secondaryLabelColor)) }
            item.attributedTitle = a
        }
        row("session", "Session", s.session.pct, "\(s.session.pct)%", s.session.reset)
        row("weekly", "Weekly", s.weekly.pct, "\(s.weekly.pct)%", s.weekly.reset)
        if let sn = s.sonnet { row("sonnet", "Sonnet only", sn.pct, "\(sn.pct)%", sn.reset) }
        else { rows["sonnet"]?.isHidden = true }
        if let e = s.extra { row("extra", "Extra usage", e.pct, "\(e.spent) / \(e.limit)", nil) }
        else { rows["extra"]?.isHidden = true }
    }

    func setError(_ msg: String) {
        statusItem.button?.attributedTitle = run("⚠ ai", hexColor(COLOR_CRITICAL))
        headerItem.attributedTitle = run(msg, .labelColor)
        for (_, it) in rows { it.isHidden = true }
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)   // menu-bar agent, no Dock icon
app.run()
