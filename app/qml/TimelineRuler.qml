import QtQuick 2.15

Item {
  clip: true
  height: parent.height

  Rectangle {
    id: rulerSpacer
    width: 100
    height: parent.height
    color: "#252526"
    z: 10
  }

  Rectangle {
    id: timeRuler
    color: "#252526"
    clip: true
    anchors.left: rulerSpacer.right
    anchors.right: parent.right
    anchors.top: parent.top
    height: parent.height

    // Ruler content
    Item {
      width: timelineContainer.timelineDuration * timelineContainer.pixelsPerSecond
      height: parent.height
      x: -timelineContainer.timelineStart * timelineContainer.pixelsPerSecond

      // Second markers
      Repeater {
        model: Math.ceil(timelineContainer.timelineDuration)

        Item {
          x: index * timelineContainer.pixelsPerSecond
          height: parent.height
          width: timelineContainer.pixelsPerSecond

          // Vertical line
          Rectangle {
            x: 0
            width: 1
            height: parent.height
            color: "#3f3f3f"
          }

          // Second number text
          Text {
            x: 5
            anchors.verticalCenter: parent.verticalCenter
            text: index + "s"
            font.pixelSize: 10
            color: "#bbbbbb"
          }

          // Small scale (0.5 second interval)
          Rectangle {
            x: timelineContainer.pixelsPerSecond * 0.5
            width: 1
            height: parent.height / 2
            anchors.bottom: parent.bottom
            color: "#3f3f3f"
            visible: timelineContainer.zoomLevel > 0.5
          }
        }
      }
    }
  }
}
