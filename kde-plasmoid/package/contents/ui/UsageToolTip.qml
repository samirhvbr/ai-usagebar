import QtQuick
import org.kde.kirigami as Kirigami

// A custom toolTipItem rather than toolTipMainText/toolTipSubText: the default
// tooltip hardcodes textFormat: Text.PlainText on the main text and caps the
// sub text at 8 lines in a single wrapped Label, which cannot express one row
// per quota window with a bar.
Item {
    id: tip

    required property var applet

    // Tooltips use the Window colour set. Copying what DefaultToolTip.qml does
    // matters: without `inherit: false` the set is overwritten by the parent and
    // the text picks up panel colours, which on some themes is invisible.
    Kirigami.Theme.colorSet: Kirigami.Theme.Window
    Kirigami.Theme.inherit: false

    // The tooltip does not size a custom item for us, so the floor has to be
    // applied HERE. It used to sit on `content` as Layout.minimumWidth, which
    // did nothing: Layout attached properties only take effect on the direct
    // child of a layout, and this parent is a plain Item.
    implicitWidth: Math.max(content.implicitWidth, Kirigami.Units.gridUnit * 20)
    implicitHeight: content.implicitHeight

    UsageRows {
        id: content
        applet: tip.applet
        anchors.fill: parent
    }
}
