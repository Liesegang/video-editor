import QtQuick 6.8
import QtQuick.Controls.Basic 6.8

MenuBar {
  id: menuBar

  background: Rectangle {
    color: theme.surfaceColor
    radius: 2
  }

  delegate: MenuBarItem {
    id: menuBarItem

    contentItem: Text {
      text: menuBarItem.text
      font.pixelSize: 14
      font.family: "Helvetica"
      font.weight: Font.Medium
      opacity: enabled ? 1.0 : 0.3
      color: menuBarItem.highlighted ? theme.textColor : Qt.darker(theme.textColor, 1.2)
      horizontalAlignment: Text.AlignLeft
      verticalAlignment: Text.AlignVCenter
      elide: Text.ElideRight
    }

    background: Rectangle {
      color: menuBarItem.highlighted ? theme.highlightColor : "transparent"
      radius: 2
    }
  }

  Menu {
    id: fileMenu
    title: "File"

    palette.base: theme.surfaceColor
    palette.text: theme.textColor
    palette.highlightedText: theme.textColor
    palette.highlight: theme.highlightColor

    background: Rectangle {
      color: theme.surfaceColor
      radius: 2
      implicitWidth: 160
    }

    delegate: MenuItem {
      id: fileMenuItem
      implicitHeight: 28
      height: 28

      palette.highlight: theme.highlightColor
      palette.highlightedText: theme.textColor
      palette.windowText: Qt.darker(theme.textColor, 1.2)

      background: Rectangle {
        implicitWidth: 240
        implicitHeight: 40
        opacity: enabled ? 1 : 0.3
        color: fileMenuItem.highlighted ? theme.highlightColor : "transparent"
      }

      contentItem: Row {
        anchors.fill: parent

        Text {
          id: fileMenuText
          text: fileMenuItem.text
          font.pixelSize: 13
          font.family: "Helvetica"
          color: fileMenuItem.highlighted ? theme.textColor : Qt.darker(theme.textColor, 1.2)
          anchors.verticalCenter: parent.verticalCenter
          elide: Text.ElideRight
          width: parent.width - fileShortcutText.width - parent.spacing
        }

        Text {
          id: fileShortcutText
          text: fileMenuItem.action && fileMenuItem.action.shortcut ? fileMenuItem.action.shortcut : ""
          font.pixelSize: 12
          font.family: "Helvetica"
          color: Qt.darker(theme.textColor, 1.5)
          anchors.verticalCenter: parent.verticalCenter
          horizontalAlignment: Text.AlignRight
        }
      }
    }

    Action { text: "Open"; shortcut: "Ctrl+O" }
    MenuSeparator {
      height: 1
      palette.light: theme.borderColor
    }
    Action { text: "Close"; shortcut: "Ctrl+W" }
    Action { text: "Save"; shortcut: "Ctrl+S" }
    Action { text: "Save as"; shortcut: "Shift+Ctrl+S" }
    MenuSeparator {
      height: 1
      palette.light: theme.borderColor
    }
    Action { text: "Export"; shortcut: "Ctrl+E" }
    MenuSeparator {
      height: 1
      palette.light: theme.borderColor
    }
    Action { text: "Exit"; shortcut: "Ctrl+Q" }
  }

  Menu {
    id: editMenu
    title: "Edit"

    palette.base: theme.surfaceColor
    palette.text: theme.textColor
    palette.highlightedText: theme.textColor
    palette.highlight: theme.highlightColor

    background: Rectangle {
      color: theme.surfaceColor
      radius: 2
      implicitWidth: 160
    }

    delegate: MenuItem {
      id: editMenuItem
      implicitHeight: 28
      height: 28

      palette.highlight: theme.highlightColor
      palette.highlightedText: theme.textColor
      palette.windowText: Qt.darker(theme.textColor, 1.2)

      anchors.leftMargin: 50
      anchors.rightMargin: 100

      background: Rectangle {
        implicitWidth: 240
        implicitHeight: 40
        opacity: enabled ? 1 : 0.3
        color: fileMenuItem.highlighted ? theme.highlightColor : "transparent"
      }

      contentItem: Row {
        spacing: 4
        anchors.fill: parent

        Text {
          id: editMenuText
          text: editMenuItem.text
          font.pixelSize: 13
          font.family: "Helvetica"
          color: editMenuItem.highlighted ? theme.textColor : Qt.darker(theme.textColor, 1.2)
          anchors.verticalCenter: parent.verticalCenter
          elide: Text.ElideRight
          width: parent.width - editShortcutText.width - parent.spacing
        }

        Text {
          id: editShortcutText
          text: editMenuItem.action && editMenuItem.action.shortcut ? editMenuItem.action.shortcut : ""
          font.pixelSize: 12
          font.family: "Helvetica"
          color: Qt.darker(theme.textColor, 1.5)
          anchors.verticalCenter: parent.verticalCenter
          horizontalAlignment: Text.AlignRight
        }
      }
    }

    Action { text: "Copy"; shortcut: "Ctrl+C" }
    Action { text: "Cut"; shortcut: "Ctrl+X" }
    Action { text: "Paste"; shortcut: "Ctrl+V" }
    MenuSeparator {
      height: 1
      palette.light: theme.borderColor
    }
    Action { text: "Settings"; shortcut: "Ctrl+," }
  }
}
