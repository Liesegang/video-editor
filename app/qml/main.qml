import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

ApplicationWindow {
  id: window
  visible: true
  width: 1920
  height: 1080
  title: "Motion Graphics Editor"

  Theme {
    id: theme
  }

  Component.onCompleted: {
    if (typeof style !== "undefined") {
      style = "Material"
    }
    theme.initialize()
  }

  // Import the menu bar component
  menuBar: MainMenuBar {}

  // Main vertical split view
  SplitView {
    id: verticalSplit
    orientation: Qt.Vertical
    anchors.top: parent.top
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: statusBar.top

    // Main area (horizontally split)
    MainSplitView {
      SplitView.preferredHeight: parent.height * 0.7
    }

    // Timeline area
    TimelineView {
      SplitView.preferredHeight: parent.height * 0.3
    }
  }

  // Status bar at the bottom
  Rectangle {
    id: statusBar
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    height: 30
    color: theme.statusBarColor
    border.color: theme.borderColor
    border.width: 1

    Text {
      anchors.centerIn: parent
      text: "Status Bar"
      color: theme.textColor
    }
  }
}
