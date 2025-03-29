import QtQuick 2.15
import Qt.labs.settings 1.0

QtObject {
  // Theme types
  readonly property int light_theme: 0
  readonly property int dark_theme: 1
  readonly property int custom_theme: 2

  // Current theme
  property int currentTheme: dark_theme

  // // Settings for persistent storage
  // property Settings themeSettings: Settings {
  //   id: settings
  //   category: "theme"
  //   property int currentTheme: light_theme
  //   property var customPrimary: "#4a86e8"
  //   property var customSecondary: "#6aa84f"
  //   property var customBackground: "#ffffff"
  //   property var customSurface: "#f5f5f5"
  //   property var customText: "#333333"
  //   property var customBorder: "#cccccc"
  //   property var customHighlight: "#e0e0ff"
  //   property var customHover: "#f0f0f0"
  // }

  // Custom theme colors
  property var customColors: ({
    primary: "#4a86e8",
    secondary: "#6aa84f",
    background: "#ffffff",
    surface: "#f5f5f5",
    text: "#333333",
    border: "#cccccc",
    highlight: "#e0e0ff",
    hover: "#f0f0f0"
  })

  // Light theme colors
  readonly property var lightColors: ({
    primary: "#4a86e8",
    secondary: "#6aa84f",
    background: "#ffffff",
    surface: "#f5f5f5",
    text: "#333333",
    border: "#cccccc",
    highlight: "#e0e0ff",
    hover: "#f0f0f0"
  })

  // Dark theme colors
  readonly property var darkColors: ({
    primary: "#4285F4",
    secondary: "#EA4335",
    background: "#2d2d30",
    surface: "#1e1e1e",
    text: "#e0e0e0",
    border: "#555555",
    highlight: "#293e61",
    hover: "#434347"
  })

  // Change theme
  // function setTheme(themeId) {
  //   if (themeId >= 0 && themeId <= 2) {
  //     currentTheme = themeId;
  //     themeSettings.currentTheme = themeId;
  //     themeChanged();
  //   }
  // }

  // Update custom color
  function setCustomColor(colorKey, colorValue) {
    if (customColors.hasOwnProperty(colorKey)) {
      customColors[colorKey] = colorValue;
      themeSettings["custom" + colorKey.charAt(0).toUpperCase() + colorKey.slice(1)] = colorValue;
      themeChanged();
    }
  }

  // Get color based on current theme
  function getColor(colorKey) {
    var color;
    switch(currentTheme) {
      case light_theme:
        color = lightColors[colorKey];
        break;
      case dark_theme:
        color = darkColors[colorKey];
        break;
      case custom_theme:
        color = customColors[colorKey];
        break;
      default:
        color = lightColors[colorKey];
    }

    return color || lightColors[colorKey];
  }

  // Initialize theme (load from settings)
  function initialize() {
    currentTheme = themeSettings.currentTheme;

    if (currentTheme === custom_theme) {
      customColors.primary = themeSettings.customPrimary;
      customColors.secondary = themeSettings.customSecondary;
      customColors.background = themeSettings.customBackground;
      customColors.surface = themeSettings.customSurface;
      customColors.text = themeSettings.customText;
      customColors.border = themeSettings.customBorder;
      customColors.highlight = themeSettings.customHighlight;
      customColors.hover = themeSettings.customHover;
    }

    console.log("Theme initialized with theme:", currentTheme);
    themeChanged();
  }

  // Direct color getters
  readonly property color primaryColor: getColor("primary")
  readonly property color secondaryColor: getColor("secondary")
  readonly property color backgroundColor: getColor("background")
  readonly property color surfaceColor: getColor("surface")
  readonly property color textColor: getColor("text")
  readonly property color borderColor: getColor("border")
  readonly property color highlightColor: getColor("highlight")
  readonly property color hoverColor: getColor("hover")

  // Theme change signal
  signal themeChanged()

  // テーマ変更時に自動的に色が更新されるように
  onCurrentThemeChanged: themeChanged()
}
