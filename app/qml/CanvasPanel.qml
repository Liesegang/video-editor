import QtQuick 2.15
import QtQuick.Controls 2.15

Rectangle {
  color: theme.panelBackgroundColor
  border.color: theme.borderColor
  border.width: 1
  clip: true

  Flickable {
    id: imageFlickable
    anchors.fill: parent
    contentWidth: canvas.width * canvas.scale
    contentHeight: canvas.height * canvas.scale
    clip: true
    interactive: true

    WheelHandler {
      acceptedDevices: PointerDevice.Mouse
      onWheel: (wheel) => {
        const factor = wheel.angleDelta.y > 0 ? 1.1 : 0.9;
        canvas.scale *= factor;
      }
    }

    DragHandler {
      onActiveChanged: {
        if (!active) {
          canvas.x = Math.min(0, Math.max(canvas.x, imageFlickable.width - canvas.width * canvas.scale));
          canvas.y = Math.min(0, Math.max(canvas.y, imageFlickable.height - canvas.height * canvas.scale));
        }
      }
    }

    Rectangle {
      id: canvas
      color: theme.backgroundColor
      border.color: theme.borderColor
      border.width: 1
      scale: 1.0
      width: 800
      height: 600

      Image {
        id: imageItem
        source: "mock.png"
        fillMode: Image.PreserveAspectFit
        anchors.centerIn: parent
      }
    }
  }
}
