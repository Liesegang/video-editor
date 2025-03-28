import QtQuick 2.15
import QtQuick.Controls 2.15

SplitView {
  id: mainArea
  orientation: Qt.Horizontal
  clip: true

  // Left panel - now using the detailed FileListPanel component
  FileListPanel {
    id: fileListPanel
    SplitView.preferredWidth: parent.width * 0.2

    // Connect the fileSelected signal to handle selected files
    onFileSelected: function(filePath) {
      console.log("Selected file: " + filePath);
      // Here you can update the canvas or property panel based on the selected file
    }
  }

  // Center panel with canvas
  CanvasPanel {
    SplitView.preferredWidth: parent.width * 0.6
  }

  // Right panel
  Rectangle {
    color: "#f0f0f0"
    SplitView.preferredWidth: parent.width * 0.2
    clip: true

    Text {
      anchors.centerIn: parent
      text: "Property Panel"
      font.pixelSize: 24
    }
  }
}