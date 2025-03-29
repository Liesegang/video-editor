use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
  CxxQtBuilder::new()
    // Link Qt's Network library
    // - Qt Core is always linked
    // - Qt Gui is linked by enabling the qt_gui Cargo feature of cxx-qt-lib.
    // - Qt Qml is linked by enabling the qt_qml Cargo feature of cxx-qt-lib.
    // - Qt Qml requires linking Qt Network on macOS
    .qt_module("Network")
    .qml_module(QmlModule {
      uri: "com.kdab.cxx_qt.demo",
      rust_files: &["src/track_list.rs"],
      qml_files: &[
        "qml/main.qml",
        "qml/CanvasPanel.qml",
        "qml/MainMenuBar.qml",
        "qml/MainSplitView.qml",
        "qml/TimelineContent.qml",
        "qml/TimelineControlBar.qml",
        "qml/TimelineRuler.qml",
        "qml/TimelineView.qml",
        "qml/FileListPanel.qml",
        "qml/Theme.qml",
        "qml/ThemeSettingsPage.qml",
        "qml/Settings.qml",
      ],
      qrc_files: &["qml/mock.png"],
      ..Default::default()
    })
    .build();
}
