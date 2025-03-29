import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import com.kdab.cxx_qt.demo 1.0

Rectangle {
  id: timelineContainer
  color: theme.timelineBackgroundColor
  border.color: theme.borderColor
  border.width: 1
  clip: true

  // Timeline state
  property real zoomLevel: 1.0         // Zoom level
  property real horizontalZoom: 1.0    // Horizontal zoom
  property real timePosition: 0.0      // Current playback position (seconds)
  property real timelineStart: 0.0     // Timeline display start position (seconds)
  property real timelineDuration: 300.0  // Total timeline length (seconds)
  property real pixelsPerSecond: 50 * zoomLevel * horizontalZoom  // Pixels per second

  // Sample clips
  property var videoClips: [
    { start: 0.0, duration: 10.0, name: "イントロ", color: "#4285F4", track: 0 },
    { start: 10.0, duration: 15.0, name: "シーン1", color: "#EA4335", track: 0 },
    { start: 25.0, duration: 20.0, name: "シーン2", color: "#FBBC05", track: 0 },
    { start: 5.0, duration: 8.0, name: "オーバーレイ", color: "#34A853", track: 1 },
    { start: 18.0, duration: 12.0, name: "テキスト", color: "#8E44AD", track: 1 },
    { start: 45.0, duration: 7.0, name: "エフェクト", color: "#1ABC9C", track: 2 }
  ]

  // Track information
  property var tracks: [
    { name: "ビデオ", height: 50 },
    { name: "オーバーレイ", height: 40 },
    { name: "エフェクト", height: 35 }
  ]

  TrackList {
    id: trackList
  }

  ColumnLayout {
    anchors.fill: parent
    spacing: 0

    // Second ruler
    TimelineRuler {
      Layout.fillWidth: true
      height: 30
    }

    // Timeline content area
    TimelineContent {
      id: timelineContentContainer
      Layout.fillWidth: true
      Layout.fillHeight: true
    }

    // Control bar
    TimelineControlBar {
      Layout.fillWidth: true
      height: 40
    }
  }
}
