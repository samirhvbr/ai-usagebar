// The row list the tooltip shows. Same rows as the popup, without the popup's
// chrome: no tab strip and no action buttons, because a tooltip cannot be
// clicked.
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents
import "../code/plasmoid-logic.mjs" as Logic

ColumnLayout {
    id: rows

    required property var applet

    readonly property var entry: rows.applet.entry
    readonly property string status: rows.applet.statusMessage()
    readonly property var list: Logic.detailRows(rows.entry)

    spacing: Kirigami.Units.smallSpacing

    Kirigami.Heading {
        Layout.fillWidth: true
        level: 4
        elide: Text.ElideRight
        text: rows.entry ? rows.entry.label : i18n("AI Usage Bar")
        textFormat: Text.PlainText
    }

    PlasmaComponents.Label {
        Layout.fillWidth: true
        visible: text !== ""
        elide: Text.ElideRight
        font: Kirigami.Theme.smallFont
        opacity: 0.7
        text: rows.entry ? rows.entry.plan : ""
        textFormat: Text.PlainText
    }

    PlasmaComponents.Label {
        Layout.fillWidth: true
        visible: text !== ""
        wrapMode: Text.WordWrap
        font: Kirigami.Theme.smallFont
        color: rows.applet.statusIsUrgent()
            ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
        text: rows.status
        textFormat: Text.PlainText
    }

    Repeater {
        model: rows.list

        UsageRow {
            required property var modelData

            Layout.fillWidth: true
            row: modelData
            colors: rows.applet.colors
            resetText: rows.applet.resetText(modelData.resetAt)
            showBar: rows.applet.showBars
        }
    }

    PlasmaComponents.Label {
        Layout.fillWidth: true
        visible: text !== ""
        horizontalAlignment: Text.AlignHCenter
        font: Kirigami.Theme.smallFont
        opacity: 0.6
        text: rows.applet.updatedText()
        textFormat: Text.PlainText
    }
}
