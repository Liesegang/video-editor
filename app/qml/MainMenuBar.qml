import QtQuick 2.15
import QtQuick.Controls 2.15

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

    contentWidth: 160
    contentHeight: implicitContentHeight

    background: Rectangle {
      implicitWidth: 240
      implicitHeight: 40
      color: theme.surfaceColor
      border.color: theme.borderColor
      radius: 2
    }

    delegate: MenuItem {
      id: menuItem
      implicitHeight: 28
      height: 28

      background: Rectangle {
        implicitWidth: 240
        implicitHeight: 40
        opacity: enabled ? 1 : 0.3
        color: menuItem.highlighted ? theme.highlightColor : "transparent"
      }

      palette.highlight: theme.highlightColor
      palette.highlightedText: theme.textColor
      palette.windowText: Qt.darker(theme.textColor, 1.2)

      contentItem: Row {
        anchors.fill: parent

        Text {
          id: menuText
          text: menuItem.text
          font.pixelSize: 13
          font.family: "Helvetica"
          color: menuItem.highlighted ? theme.textColor : Qt.darker(theme.textColor, 1.2)
          anchors.verticalCenter: parent.verticalCenter
          elide: Text.ElideRight
          width: parent.width - shortcutText.width - parent.spacing
        }

        Text {
          id: shortcutText
          text: menuItem.action && menuItem.action.shortcut ? menuItem.action.shortcut : ""
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

    contentWidth: 160
    contentHeight: implicitContentHeight

    background: Rectangle {
      implicitWidth: 240
      implicitHeight: 40
      color: theme.surfaceColor
      border.color: theme.borderColor
      radius: 2
    }

    delegate: MenuItem {
      id: menuItem
      implicitHeight: 28
      height: 28

      palette.highlight: theme.highlightColor
      palette.highlightedText: theme.textColor
      palette.windowText: Qt.darker(theme.textColor, 1.2)

      contentItem: Row {
        spacing: 2
        anchors.fill: parent
        anchors.leftMargin: 2
        anchors.rightMargin: 2

        Text {
          id: menuText
          text: menuItem.text
          font.pixelSize: 13
          font.family: "Helvetica"
          color: menuItem.highlighted ? theme.textColor : Qt.darker(theme.textColor, 1.2)
          anchors.verticalCenter: parent.verticalCenter
          elide: Text.ElideRight
          width: parent.width - shortcutText.width - parent.spacing
        }

        Text {
          id: shortcutText
          text: menuItem.action && menuItem.action.shortcut ? menuItem.action.shortcut : ""
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
