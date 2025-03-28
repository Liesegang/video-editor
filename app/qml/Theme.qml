// Theme.qml
pragma Singleton
import QtQuick 2.15
import Qt.labs.settings 1.0

QtObject {
  id: theme

  // Theme types
  readonly property int light_theme: 0
  readonly property int dark_theme: 1
  readonly property int custom_theme: 2

  // Current theme
  property int currentTheme: dark_theme

  // Settings for persistent storage
  property Settings themeSettings: Settings {
    id: settings
    category: "theme"
    property int currentTheme: light_theme
    property var customPrimary: "#4a86e8"
    property var customSecondary: "#6aa84f"
    property var customBackground: "#ffffff"
    property var customSurface: "#f5f5f5"
    property var customText: "#333333"
    property var customBorder: "#cccccc"
    property var customHighlight: "#e0e0ff"
    property var customHover: "#f0f0f0"
  }

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
    primary: "#2979ff",
    secondary: "#4caf50",
    background: "#121212",
    surface: "#1e1e1e",
    text: "#e0e0e0",
    border: "#555555",
    highlight: "#2d2d60",
    hover: "#2a2a2a"
  })

  // Change theme
  function setTheme(themeId) {
    if (themeId >= 0 && themeId <= 2) {
      currentTheme = themeId;
      themeSettings.currentTheme = themeId;
      themeChanged();
    }
  }

  // Update custom color
  function setCustomColor(colorKey, colorValue) {
    if (customColors.hasOwnProperty(colorKey)) {
      customColors[colorKey] = colorValue;
      themeSettings["custom" + colorKey.charAt(0).toUpperCase() + colorKey.slice(1)] = colorValue;
      themeChanged();
    }
  }

  // Get color based on current theme
  function _getCurrentColor(colorKey) {
    switch(currentTheme) {
      case light_theme:
        return lightColors[colorKey];
      case dark_theme:
        return darkColors[colorKey];
      case custom_theme:
        return customColors[colorKey];
      default:
        return lightColors[colorKey];
    }
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

  // Theme colors - properties for components to reference
  readonly property color primaryColor: _getCurrentColor("primary")
  readonly property color secondaryColor: _getCurrentColor("secondary")
  readonly property color backgroundColor: _getCurrentColor("background")
  readonly property color surfaceColor: _getCurrentColor("surface")
  readonly property color textColor: _getCurrentColor("text")
  readonly property color borderColor: _getCurrentColor("border")
  readonly property color highlightColor: _getCurrentColor("highlight")
  readonly property color hoverColor: _getCurrentColor("hover")

  // Theme change signal
  signal themeChanged()
}