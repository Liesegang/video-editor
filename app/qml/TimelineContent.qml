import QtQuick 2.15
import QtQuick.Controls 2.15

Item {
  clip: true

  // Track name sidebar
  Rectangle {
    id: trackSidebar
    width: 100
    height: parent.height
    color: "#252526"
    z: 10

    Column {
      anchors.fill: parent
      anchors.margins: 0
      spacing: 0

      Repeater {
        model: timelineContainer.tracks

        Rectangle {
          width: parent.width
          height: modelData.height
          color: index % 2 === 0 ? "#252526" : "#2a2a2a"
          border.width: 1
          border.color: "#1e1e1e"

          Text {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: 10
            text: modelData.name
            color: "#ffffff"
            font.pixelSize: 12
          }
        }
      }
    }
  }

  // Scrollable area
  Flickable {
    id: timelineFlickable
    anchors.left: trackSidebar.right
    anchors.right: parent.right
    anchors.top: parent.top
    anchors.bottom: horizontalScrollBar.top
    contentWidth: timelineContainer.timelineDuration * timelineContainer.pixelsPerSecond
    contentHeight: timelineClipsArea.height
    flickableDirection: Flickable.HorizontalAndVerticalFlick

    onContentXChanged: {
      timelineContainer.timelineStart = contentX / timelineContainer.pixelsPerSecond;
    }

    // Clips display area
    Item {
      id: timelineClipsArea
      width: parent.contentWidth
      height: {
        var totalHeight = 0;
        for (var i = 0; i < timelineContainer.tracks.length; i++) {
          totalHeight += timelineContainer.tracks[i].height;
        }
        return totalHeight;
      }

      // Time grid
      Repeater {
        model: Math.ceil(timelineContainer.timelineDuration)

        Rectangle {
          x: index * timelineContainer.pixelsPerSecond
          width: 1
          height: parent.height
          color: "#3f3f3f"
        }
      }

      // Track background
      Column {
        anchors.fill: parent
        spacing: 0

        Repeater {
          model: timelineContainer.tracks

          Rectangle {
            width: parent.width
            height: modelData.height
            color: index % 2 === 0 ? "#2d2d30" : "#333336"
            border.width: 1
            border.color: "#1e1e1e"
          }
        }
      }

      // Video clips
      Repeater {
        model: timelineContainer.videoClips

        Rectangle {
          id: clipItem
          property var trackY: {
            var y = 0;
            for (var i = 0; i < modelData.track; i++) {
              y += timelineContainer.tracks[i].height;
            }
            return y;
          }

          x: modelData.start * timelineContainer.pixelsPerSecond
          y: trackY
          width: modelData.duration * timelineContainer.pixelsPerSecond
          height: timelineContainer.tracks[modelData.track].height - 2
          color: Qt.alpha(modelData.color, 0.7)
          border.color: modelData.color
          border.width: 1
          radius: 3

          // Clip content
          Rectangle {
            anchors.left: parent.left
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            width: 5
            color: modelData.color
          }

          Text {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: 10
            text: modelData.name
            color: "#ffffff"
            font.pixelSize: 11
            elide: Text.ElideRight
            width: parent.width - 15
          }

          // Interaction
          MouseArea {
            anchors.fill: parent
            hoverEnabled: true

            onEntered: {
              parent.opacity = 1
              parent.border.width = 2
            }

            onExited: {
              parent.opacity = 0.8
              parent.border.width = 1
            }

            onClicked: {
              console.log("Selected clip:", modelData.name)
            }

            // Clip drag operations would be implemented here
          }

          // Animation
          Behavior on opacity {
            NumberAnimation { duration: 100 }
          }
        }
      }

      // Current position bar (within content)
      Rectangle {
        id: contentPositionBar
        x: timelineContainer.timePosition * timelineContainer.pixelsPerSecond
        height: parent.height
        width: 2
        color: "#ff5500"
        z: 10
      }
    }

    // Wheel event handling
    MouseArea {
      anchors.fill: parent
      acceptedButtons: Qt.NoButton
      propagateComposedEvents: true

      onWheel: {
        if (wheel.modifiers & Qt.ControlModifier) {
          if (wheel.modifiers & Qt.ShiftModifier) {
            // Ctrl+Shift+Wheel for horizontal zoom
            var hZoomDelta = wheel.angleDelta.y > 0 ? 0.1 : -0.1;
            timelineContainer.horizontalZoom = Math.max(0.1, Math.min(5.0, timelineContainer.horizontalZoom + hZoomDelta));
          } else {
            // Ctrl+Wheel for overall zoom
            var zoomDelta = wheel.angleDelta.y > 0 ? 0.1 : -0.1;
            timelineContainer.zoomLevel = Math.max(0.1, Math.min(5.0, timelineContainer.zoomLevel + zoomDelta));
          }

          // Adjust content position during zoom
          var cursorX = timelineFlickable.contentX + wheel.x;
          var timeAtCursor = cursorX / (timelineContainer.pixelsPerSecond / (timelineContainer.zoomLevel * timelineContainer.horizontalZoom));

          timelineFlickable.contentX = timeAtCursor * timelineContainer.pixelsPerSecond - wheel.x;
          wheel.accepted = true;
        } else {
          // Normal wheel scrolling
          timelineFlickable.contentX -= wheel.angleDelta.y;
          wheel.accepted = true;
        }
      }
    }
  }

  // Horizontal scroll bar (fixed to work with default style)
  ScrollBar {
    id: horizontalScrollBar
    anchors.left: trackSidebar.right
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    height: 12
    orientation: Qt.Horizontal
    policy: ScrollBar.AlwaysOn

    // Don't use custom contentItem or background, use the style's defaults
    position: timelineFlickable.contentX / timelineFlickable.contentWidth
    size: timelineFlickable.width / timelineFlickable.contentWidth
  }

  // Current position bar (screen fixed)
  Rectangle {
    id: currentPositionBar
    anchors.top: parent.top
    anchors.bottom: horizontalScrollBar.top
    anchors.left: trackSidebar.right
    anchors.leftMargin: (timelineContainer.timePosition - timelineContainer.timelineStart) * timelineContainer.pixelsPerSecond
    width: 2
    color: "#ff5500"
    visible: anchors.leftMargin >= 0 && anchors.leftMargin <= timelineFlickable.width
    z: 100

    // Position bar top handle
    Rectangle {
      width: 14
      height: 14
      radius: 7
      color: "#ff5500"
      anchors.horizontalCenter: parent.horizontalCenter
      anchors.top: parent.top
      anchors.topMargin: -7

      MouseArea {
        anchors.fill: parent
        drag.target: parent
        drag.axis: Drag.XAxis

        onPositionChanged: {
          if (drag.active) {
            var newTimePosition = timelineContainer.timelineStart +
              (parent.x + 7 - timelineFlickable.x) / timelineContainer.pixelsPerSecond;
            timelineContainer.timePosition = Math.max(0, newTimePosition);
          }
        }
      }
    }
  }
}