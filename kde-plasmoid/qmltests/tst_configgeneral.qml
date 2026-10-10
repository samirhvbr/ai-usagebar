// Instantiates the real settings page and walks the "Current vendor" drop-down
// delegates. configGeneral.qml is a lower-case file name, so it cannot be used
// as a QML type name; a Loader takes it by URL instead.
import QtQuick
import QtTest

TestCase {
    id: root
    name: "ConfigGeneral"
    when: windowShown

    Loader {
        id: loader
        width: 600
        height: 600
        source: "../package/contents/ui/configGeneral.qml"
    }

    // i18n() is injected by the Plasma runtime and is absent here, so those
    // ReferenceErrors are expected and ignored; only `index` is asserted on.
    //
    // Plasma 6 does not inject `index` into a delegate that declares
    // `required property`, so using it unqualified raised
    // "ReferenceError: index is not defined" every time the page was opened.
    function test_01_vendor_delegates_raise_no_errors() {
        compare(loader.status, Loader.Ready);
        var page = loader.item;
        page.cfg_vendorRing = ["anthropic", "nous"];
        page.cfg_vendor = "nous";
        failOnWarning(/index is not defined/);
        var combo = findCombo(page);
        verify(combo, "the Current vendor combo box exists");
        combo.popup.open();
        wait(300);
        combo.popup.close();
    }

    function findCombo(item) {
        var kids = item.children;
        for (var i = 0; i < kids.length; ++i) {
            var k = kids[i];
            if (k.hasOwnProperty("highlightedIndex") && k.hasOwnProperty("textRole")
                    && k.model && k.model.length === 2)
                return k;
            var found = findCombo(k);
            if (found)
                return found;
        }
        return null;
    }
}
