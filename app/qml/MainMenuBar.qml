import QtQuick 2.15
import QtQuick.Controls 2.15

MenuBar {
  id: menuBar

  // メニューバー全体のスタイル
  background: Rectangle {
    color: "#2c3e50"
    radius: 2
  }

  // メニュー項目のスタイル（共通）
  delegate: MenuBarItem {
    id: menuBarItem

    contentItem: Text {
      text: menuBarItem.text
      font.pixelSize: 14
      font.family: "Helvetica"
      font.weight: Font.Medium
      opacity: enabled ? 1.0 : 0.3
      color: menuBarItem.highlighted ? "#ecf0f1" : "#bdc3c7"
      horizontalAlignment: Text.AlignLeft
      verticalAlignment: Text.AlignVCenter
      elide: Text.ElideRight
    }

    background: Rectangle {
      color: menuBarItem.highlighted ? "#34495e" : "transparent"
      radius: 2
    }
  }

  Menu {
    id: fileMenu
    title: "File"

    // メニュードロップダウン部分のスタイル（共通）
    background: Rectangle {
      implicitWidth: 160  // 幅を少し狭く
      color: "#2c3e50"
      border.color: "#34495e"
      radius: 3
      border.width: 1
    }

    // マージンを小さくしたデリゲート
    delegate: MenuItem {
      id: menuItem
      implicitHeight: 28  // 高さを小さく

      contentItem: Row {
        spacing: 4  // スペースを小さく
        anchors.fill: parent
        anchors.leftMargin: 6  // 左マージンを小さく
        anchors.rightMargin: 6  // 右マージンを小さく

        Text {
          id: menuText
          text: menuItem.text
          font.pixelSize: 13
          font.family: "Helvetica"
          color: menuItem.highlighted ? "#ecf0f1" : "#bdc3c7"
          anchors.verticalCenter: parent.verticalCenter
          elide: Text.ElideRight
          width: parent.width - shortcutText.width - parent.spacing
        }

        Text {
          id: shortcutText
          text: menuItem.action && menuItem.action.shortcut ? menuItem.action.shortcut : ""
          font.pixelSize: 12
          font.family: "Helvetica"
          color: "#7f8c8d"
          anchors.verticalCenter: parent.verticalCenter
          horizontalAlignment: Text.AlignRight
        }
      }

      background: Rectangle {
        color: menuItem.highlighted ? "#34495e" : "transparent"
        radius: 2
      }
    }

    // 項目間のパディングを調整
    padding: 2

    Action { text: "Open"; shortcut: "Ctrl+O" }
    MenuSeparator {
      contentItem: Rectangle {
        implicitHeight: 1
        color: "#34495e"
      }
      padding: 0  // セパレーターのパディングを削除
    }
    Action { text: "Close"; shortcut: "Ctrl+W" }
    Action { text: "Save"; shortcut: "Ctrl+S" }
    Action { text: "Save as"; shortcut: "Shift+Ctrl+S" }
    MenuSeparator {
      contentItem: Rectangle {
        implicitHeight: 1
        color: "#34495e"
      }
      padding: 0  // セパレーターのパディングを削除
    }
    Action { text: "Export"; shortcut: "Ctrl+E" }
    MenuSeparator {
      contentItem: Rectangle {
        implicitHeight: 1
        color: "#34495e"
      }
      padding: 0  // セパレーターのパディングを削除
    }
    Action { text: "Exit"; shortcut: "Ctrl+Q" }
  }

  Menu {
    id: editMenu
    title: "Edit"

    // fileMenuと同じバックグラウンドを使用
    background: Rectangle {
      implicitWidth: 160  // 幅を少し狭く
      color: "#2c3e50"
      border.color: "#34495e"
      radius: 3
      border.width: 1
    }

    // マージンを小さくしたデリゲート (fileMenuと同じ)
    delegate: MenuItem {
      id: menuItem
      implicitHeight: 28  // 高さを小さく

      contentItem: Row {
        spacing: 4  // スペースを小さく
        anchors.fill: parent
        anchors.leftMargin: 6  // 左マージンを小さく
        anchors.rightMargin: 6  // 右マージンを小さく

        Text {
          id: menuText
          text: menuItem.text
          font.pixelSize: 13
          font.family: "Helvetica"
          color: menuItem.highlighted ? "#ecf0f1" : "#bdc3c7"
          anchors.verticalCenter: parent.verticalCenter
          elide: Text.ElideRight
          width: parent.width - shortcutText.width - parent.spacing
        }

        Text {
          id: shortcutText
          text: menuItem.action && menuItem.action.shortcut ? menuItem.action.shortcut : ""
          font.pixelSize: 12
          font.family: "Helvetica"
          color: "#7f8c8d"
          anchors.verticalCenter: parent.verticalCenter
          horizontalAlignment: Text.AlignRight
        }
      }

      background: Rectangle {
        color: menuItem.highlighted ? "#34495e" : "transparent"
        radius: 2
      }
    }

    // 項目間のパディングを調整
    padding: 2

    Action { text: "Copy"; shortcut: "Ctrl+C" }
    Action { text: "Cut"; shortcut: "Ctrl+X" }
    Action { text: "Paste"; shortcut: "Ctrl+V" }
    MenuSeparator {
      contentItem: Rectangle {
        implicitHeight: 1
        color: "#34495e"
      }
      padding: 0  // セパレーターのパディングを削除
    }
    Action { text: "Settings"; shortcut: "Ctrl+," }
  }
}
