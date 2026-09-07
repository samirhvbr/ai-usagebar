// Renders the real UsageBar.qml offscreen and asserts what it actually paints.
//
// Possible because UsageBar.qml never touches the Plasmoid attached property —
// unlike main.qml, which the applet host injects it into and which therefore
// genuinely cannot be instantiated here.
//
// `visible` is deliberately NOT asserted: nothing is inside a shown window, so
// every item reports visible=false regardless of its binding. Width is the
// honest proxy — a zero-width fill paints nothing either way.
import QtQuick
import QtTest
import "../package/contents/ui" as Ui
import "../package/contents/code/plasmoid-logic.mjs" as Logic

TestCase {
    id: root
    name: "UsageBar"
    when: windowShown

    // One Dark, the five the other frontends default to (src/theme.rs).
    readonly property var palette: ({
        low: "#98c379", mid: "#e5c07b", high: "#d19a66",
        critical: "#e06c75", empty: "#5c6370",
    })

    Ui.UsageBar {
        id: bar
        width: 200
        height: 10
        colors: root.palette
        pct: 0
        severity: "low"
    }

    function parts() {
        var track = bar.children[0];
        return {track: track, fill: track.children[0]};
    }

    function test_01_structure() {
        var p = parts();
        compare(bar.children.length, 1, "root holds the track");
        compare(p.track.children.length, 1, "the track holds one fill");
        compare(p.track.width, 200, "the track spans the widget");
    }

    // tryCompare, not compare: the fill animates over 160ms, so reading width
    // in the same tick catches it mid-interpolation. Waiting for the settled
    // value is the honest assertion — the animation is deliberate.
    function test_02_fill_is_percent_of_track() {
        var fill = parts().fill;
        for (var pct = 0; pct <= 100; pct += 5) {
            bar.pct = pct;
            tryCompare(fill, "width", Math.round(200 * pct / 100), 1000,
                       "fill must settle at pct% of the track at " + pct);
        }
    }

    function test_03_clamps_out_of_range() {
        var fill = parts().fill;
        bar.pct = 140;
        tryCompare(fill, "width", 200, 1000, "over 100% cannot overflow the track");
        bar.pct = -40;
        tryCompare(fill, "width", 0, 1000, "negative cannot paint backwards");
        bar.pct = 0;
        tryCompare(fill, "width", 0, 1000);
    }

    // Colour comes from the severity the Rust core computed, not from a
    // threshold re-implemented here — that is what keeps the four frontends
    // agreeing about what counts as critical.
    function test_04_colour_follows_severity() {
        bar.pct = 50;
        var seen = {};
        var expected = {low: "#98c379", mid: "#e5c07b",
                        high: "#d19a66", critical: "#e06c75"};
        for (var key in expected) {
            bar.severity = key;
            var got = String(parts().fill.color);
            compare(got, expected[key], "severity " + key + " must take its own colour");
            verify(!seen[got], "severity " + key + " must not reuse another band's colour");
            seen[got] = true;
        }
        bar.severity = "low";
    }

    // A theme switch must recolour in place: the panel has to follow Breeze
    // Light/Dark live, and no One Dark value may survive it.
    function test_05_repaints_on_a_palette_change() {
        bar.pct = 50; bar.severity = "critical";
        compare(String(parts().fill.color), "#e06c75");
        bar.colors = {low: "#007700", mid: "#0000ff", high: "#884400",
                      critical: "#ff0000", empty: "#eeeeee"};
        compare(String(parts().fill.color), "#ff0000", "the fill must follow the new palette");
        compare(String(parts().track.color), "#eeeeee", "and so must the track");
        bar.colors = root.palette;
        bar.severity = "low";
    }

    // An unknown severity must still paint something rather than going blank.
    function test_06_unknown_severity_is_not_invisible() {
        bar.pct = 50; bar.severity = "nonsense";
        compare(String(parts().fill.color), "#98c379", "falls back to the low band");
        bar.severity = "low";
    }

    // Exercise the pure module under QML's V4 engine, not just Node's V8.
    // These are the security-sensitive paths that consume provider text and
    // build the executable-engine command.
    function test_07_logic_security_contract() {
        compare(Logic.safeText("<img src='https://example.invalid/x'>\u202eevil"),
                "‹img src='https://example.invalid/x'›evil");
        compare(Logic.timeoutSeconds(0), 60, "the process bound cannot be disabled");
        compare(Logic.timeoutSeconds(null), 600, "missing config uses the safe default");

        var command = Logic.buildCommand("/opt/my apps/ai-usagebar", 600);
        verify(command.indexOf("'timeout' '-k' '5' '600'") === 0,
               "the timeout wrapper leads the quoted command");
        verify(command.indexOf("'/opt/my apps/ai-usagebar'") !== -1,
               "a binary path with spaces remains one argument");

        var report = Logic.parseReport(JSON.stringify({entries: [{
            id: "hostile", display_name: "<b>provider</b>", status: "ready",
            sections: [],
        }]}));
        compare(report.entries[0].label, "‹b›provider‹/b›",
                "provider markup remains visible but inert");
    }
}
