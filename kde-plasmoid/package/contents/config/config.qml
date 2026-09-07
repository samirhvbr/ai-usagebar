import QtQuick
import org.kde.plasma.configuration

// `source` resolves relative to contents/ui/, NOT contents/config/.
//
// One category. The GNOME prefs window has a second "Vendors" page listing per
// vendor login status, which this used to port; the report now carries `status`
// and `error` for every configured vendor, so the popup's own tab strip shows
// that in place, where the user is already looking.
ConfigModel {
    ConfigCategory {
        name: i18n("General")
        icon: "configure"
        source: "configGeneral.qml"
    }
}
