// A usage bar drawn with real Rectangles rather than the █/░ block characters
// the Waybar and GNOME frontends use — those emit Pango markup, a GTK format
// Qt does not render.
//
// One fill, coloured by the severity the Rust core already computed for this
// metric, exactly as omarchy/Panel.qml does. The width animates so switching
// provider tabs reads as a transition rather than a jump.
import QtQuick
import org.kde.kirigami as Kirigami
import "../code/plasmoid-logic.mjs" as Logic

Item {
    id: root

    required property int pct
    property string severity: "low"
    property var colors: ({})

    readonly property int clampedPct: Math.max(0, Math.min(100, root.pct))

    implicitHeight: Math.max(Kirigami.Units.smallSpacing,
                             Math.round(Kirigami.Units.gridUnit * 0.22))
    implicitWidth: Kirigami.Units.gridUnit * 4

    Rectangle {
        id: track
        anchors.fill: parent
        radius: height / 2
        color: root.colors.empty ?? Kirigami.Theme.disabledTextColor
        opacity: 0.35

        Rectangle {
            width: Math.round(parent.width * root.clampedPct / 100)
            height: parent.height
            radius: parent.radius
            color: Logic.severityColor(root.severity, root.colors)
                ?? Kirigami.Theme.textColor

            Behavior on width {
                NumberAnimation { duration: 160; easing.type: Easing.OutCubic }
            }
        }
    }
}
