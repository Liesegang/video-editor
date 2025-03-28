pragma Singleton
import QtQuick 2.15
import Qt.labs.settings 1.0

QtObject {
  id: settingsManager

  property Settings settings: Settings {
    category: "General"
    fileName: "config.ini"
  }

  function value(key, defaultValue) {
    var val = settings.value(key);
    return val !== undefined ? val : defaultValue;
  }

  function setValue(key, value) {
    settings.setValue(key, value);
  }
}