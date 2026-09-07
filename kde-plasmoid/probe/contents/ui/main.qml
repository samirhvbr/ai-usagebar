// Engine-contract probe. Proves plasmoid-logic.mjs loads and behaves inside a
// real KPackage under a real Plasma applet host — the checks Node CANNOT catch,
// because QML's V4 engine differs from V8:
//   - V4 rejects the ES2019 optional catch binding (`catch {`) outright, and
//   - V4 silently evaluates Unicode property escapes (\p{L}) to false.
// The second is the dangerous one: no error, just wrong answers.
//
// Run: make mjs-probe
import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.plasma5support as Plasma5Support
import "../../../package/contents/code/plasmoid-logic.mjs" as Logic

PlasmoidItem {
    id: probe

    // buildCommand() single-quotes every argument, including the timeout(1)
    // wrapper. Node can assert the STRING; only a real applet host can prove
    // KShell::splitArgs(AbortOnMeta) parses it and KProcess runs it. /bin/echo
    // stands in for the binary so this touches no network and no credential.
    Plasma5Support.DataSource {
        id: runner
        engine: "executable"
        connectedSources: []
        onNewData: (sourceName, data) => {
            disconnectSource(sourceName);
            const code = data["exit code"];
            const out = String(data["stdout"] || "").trim();
            const want = "usage --json";
            if (code === 0 && out === want)
                console.log("ok   timeout wrapper runs through the executable engine");
            else
                console.log("FAIL timeout wrapper: exit=" + code + " stdout=" + JSON.stringify(out)
                            + " want " + JSON.stringify(want));
            console.log("MJS PROBE EXEC DONE");
        }
    }

    Component.onCompleted: {
        let failures = 0;
        function check(label, actual, expected) {
            if (JSON.stringify(actual) === JSON.stringify(expected)) {
                console.log("ok   " + label + " = " + JSON.stringify(actual));
            } else {
                failures++;
                console.log("FAIL " + label + ": got " + JSON.stringify(actual)
                            + " want " + JSON.stringify(expected));
            }
        }

        // A report shaped like the real one, inline so the probe needs no
        // network, no credential and no configured vendor.
        const REPORT = JSON.stringify({
            primary: "anthropic",
            entries: [{
                id: "anthropic", display_name: "Claude", plan: "Max 20x",
                status: "ready", stale: false, error: null,
                fetched_at: "2026-01-01T00:00:00Z",
                sections: [
                    {type: "spacer"},
                    {type: "metric", label: "Session (5h)", value: "62%", percent: 62,
                     severity: "mid", reset_at: "2026-01-01T02:00:00Z",
                     detail: "Resets in 2h · 40% elapsed · 22pts over"},
                ],
            }, {
                id: "zai", display_name: "Z.AI", plan: "", status: "error",
                stale: false, error: "no API key", sections: [],
            }],
        });

        const report = Logic.parseReport(REPORT);
        check("parseReport", report.ok, true);
        check("entries", report.entries.length, 2);
        // display_name is what the tab strip and the header show.
        check("entryFor picks the asked-for vendor", Logic.entryFor(report, "zai").id, "zai");
        check("entry label comes from display_name", Logic.entryFor(report, "anthropic").label, "Claude");
        check("tabs mark the failing vendor",
              Logic.vendorTabs(report, "anthropic").map(t => t.failing), [false, true]);

        const entry = Logic.entryFor(report, "anthropic");
        check("headline", Logic.headline(entry).text, "62%");
        check("panel cells", Logic.panelCells(entry, {max: 2}).length, 1);
        check("short label", Logic.shortLabel("Session (5h)"), "5h");
        // The regression this file exists for: a Unicode property escape here
        // would evaluate to false under V4 and silently return the wrong text.
        check("severity bands still resolve", Logic.severityOf(95, ""), "critical");
        // metricDetail strips the CLI's own reset sentence, leaving the pace text.
        check("metricDetail drops the reset clause",
              Logic.metricDetail(entry.sections[1]), "40% elapsed · 22pts over");
        check("spacers are dropped from the popup rows", Logic.detailRows(entry).length, 1);

        const errored = Logic.entryFor(report, "zai");
        check("an errored vendor shows a warning cell",
              Logic.panelCells(errored)[0].text, "⚠");

        check("formatDuration", Logic.formatDuration(3720000), "1h 2m");
        check("resetRemainingMs on a bad timestamp", Logic.resetRemainingMs("nope", 0), null);
        check("buildArgv wraps in timeout when given one",
              Logic.buildArgv("b", 60).slice(0, 4), ["timeout", "-k", "5", "60"]);
        check("buildArgv asks for the whole report",
              Logic.buildArgv("b", 0).slice(-3), ["b", "usage", "--json"]);
        check("buildArgv never disables its process bound",
              Logic.buildArgv("b", 0).slice(0, 4), ["timeout", "-k", "5", "60"]);

        console.log(failures === 0 ? "MJS PROBE OK" : "MJS PROBE FAILED (" + failures + ")");

        // Fire the real thing last, so the synchronous checks are already
        // reported even if the engine never calls back.
        runner.connectSource(Logic.buildCommand("/bin/echo", 60));
    }
}
