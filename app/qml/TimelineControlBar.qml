import QtQuick 2.15
import QtQuick.Layouts 1.15

Rectangle {
  color: "#1e1e1e"

  RowLayout {
    anchors.fill: parent
    anchors.margins: 8
    spacing: 10

    // Play button
    Rectangle {
      width: 30
      height: 30
      radius: 15
      color: "#4285F4"

      Text {
        anchors.centerIn: parent
        text: "▶"
        color: "#ffffff"
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
      color: "#ffffff"
      font.pixelSize: 14
      font.family: "Monospace"
    }

    Item { Layout.fillWidth: true }

    // Zoom information
    Text {
      text: "ズーム: " + timelineContainer.zoomLevel.toFixed(1) + "x / " +
        "横ズーム: " + timelineContainer.horizontalZoom.toFixed(1) + "x"
      color: "#bbbbbb"
      font.pixelSize: 11
    }

    // Zoom reset button
    Rectangle {
      width: 24
      height: 24
      radius: 3
      color: "#3e3e42"

      Text {
        anchors.centerIn: parent
        text: "1:1"
        color: "#ffffff"
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
