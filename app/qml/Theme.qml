import QtQuick 2.15
import Qt.labs.settings 1.0

QtObject {
  // Theme types
  readonly property int light_theme: 0
  readonly property int dark_theme: 1
  readonly property int custom_theme: 2

  // Current theme
  property int currentTheme: dark_theme

  // Base colors - ライトテーマのベースカラー
  readonly property var lightBase: ({
    primary: "#4a86e8",       // メインカラー (青系の色)
    secondary: "#a84f4f",     // アクセントカラー (赤系の色)
    background: "#ffffff",    // 背景色
    surface: "#f5f5f5",       // 表面色
    text: "#333333",          // テキスト色
    border: "#cccccc"         // 境界線
  })

  // Base colors - ダークテーマのベースカラー
  readonly property var darkBase: ({
    primary: "#4285F4",       // メインカラー (青系の色)
    secondary: "#dd2d27",     // アクセントカラー (赤系の色)
    background: "#2d2d30",    // 背景色
    surface: "#1e1e1e",       // 表面色
    text: "#e0e0e0",          // テキスト色
    border: "#555555"         // 境界線
  })

  // Custom colors
  property var customBase: ({
    primary: "#4a86e8",
    secondary: "#6aa84f",
    background: "#ffffff",
    surface: "#f5f5f5",
    text: "#333333",
    border: "#cccccc"
  })

  // Get base color based on current theme
  function getBaseColor(colorKey) {
    var color;
    switch(currentTheme) {
      case light_theme:
        color = lightBase[colorKey];
        break;
      case dark_theme:
        color = darkBase[colorKey];
        break;
      case custom_theme:
        color = customBase[colorKey];
        break;
      default:
        color = lightBase[colorKey];
    }
    return color || lightBase[colorKey];
  }

  // セマンティックカラー - ベースカラーから派生
  readonly property color primaryColor: getBaseColor("primary")
  readonly property color secondaryColor: getBaseColor("secondary")
  readonly property color backgroundColor: getBaseColor("background")
  readonly property color surfaceColor: getBaseColor("surface") 
  readonly property color textColor: getBaseColor("text")
  readonly property color borderColor: getBaseColor("border")
  
  // UI要素のセマンティックカラー
  readonly property color statusBarColor: getBaseColor("surface")
  readonly property color toolbarBackgroundColor: getBaseColor("surface")
  readonly property color panelBackgroundColor: Qt.darker(getBaseColor("surface"), 1.05)
  readonly property color timelineBackgroundColor: getBaseColor("surface")
  
  // 状態を表すセマンティックカラー
  readonly property color highlightColor: Qt.alpha(getBaseColor("primary"), 0.3)
  readonly property color hoverColor: Qt.lighter(getBaseColor("surface"), 1.1)
  readonly property color activeColor: Qt.darker(getBaseColor("primary"), 1.1)
  readonly property color inactiveColor: Qt.lighter(getBaseColor("surface"), 1.05)

  // Initialize theme
  function initialize() {
    console.log("Theme initialized with theme:", currentTheme);
    themeChanged();
  }

  // Change theme
  function setTheme(themeId) {
    if (themeId >= 0 && themeId <= 2) {
      currentTheme = themeId;
      themeChanged();
    }
  }

  // Theme change signal
  signal themeChanged()

  // テーマ変更時に自動的に色が更新されるように
  onCurrentThemeChanged: themeChanged()
}
