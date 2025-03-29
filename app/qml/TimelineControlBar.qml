import QtQuick 6.8
import QtQuick.Layouts 6.8

Rectangle {
  color: theme.toolbarBackgroundColor

  RowLayout {
    anchors.fill: parent
    anchors.margins: 5
    spacing: 10

    // Play button
    Rectangle {
      width: 30
      height: 30
      radius: 15
      color: theme.primaryColor

      Text {
        anchors.centerIn: parent
        text: "▶"
        color: theme.textColor
        font.pixelSize: 14
      }

      MouseArea {
        anchors.fill: parent
        onClicked: {
          console.log("Play/Pause")
          trackList.loadTracks()
        }
      }
    }

    // Time display
    Text {
      text: {
        var minutes = Math.floor(timelineContainer.timePosition / 60);
        var seconds = Math.floor(timelineContainer.timePosition % 60);
        var ms = Math.floor((timelineContainer.timePosition % 1) * 100);
        return minutes.toString().padStart(2, '0') + ":" +
          seconds.toString().padStart(2, '0') + "." +
          ms.toString().padStart(2, '0');
      }
      color: theme.textColor
      font.pixelSize: 14
      font.family: "Monospace"
    }

    Item { Layout.fillWidth: true }

    // Zoom information
    Text {
      text: "ズーム: " + timelineContainer.zoomLevel.toFixed(1) + "x / " +
        "横ズーム: " + timelineContainer.horizontalZoom.toFixed(1) + "x"
      color: theme.textColor
      font.pixelSize: 11
    }

    // Zoom reset button
    Rectangle {
      width: 24
      height: 24
      radius: 3
      color: theme.activeColor

      Text {
        anchors.centerIn: parent
        text: "1:1"
        color: theme.textColor
        font.pixelSize: 10
      }

      MouseArea {
        anchors.fill: parent
        onClicked: {
          timelineContainer.zoomLevel = 1.0;
          timelineContainer.horizontalZoom = 1.0;
        }
      }
    }
  }
}
