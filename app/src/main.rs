pub mod track_list;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};
use log::{debug, info};

fn main() {
  // Initialize env_logger only once
  let _ = env_logger::builder().format_timestamp_millis().try_init();
  info!("Starting Qt application");

  // Create the application and engine
  let mut app = QGuiApplication::new();
  let mut engine = QQmlApplicationEngine::new();

  // Load the QML path into the engine
  if let Some(engine) = engine.as_mut() {
    debug!("Loading QML entry point");
    engine.load(&QUrl::from("qrc:/qt/qml/com/kdab/cxx_qt/demo/qml/main.qml"));
  }

  if let Some(engine) = engine.as_mut() {
    // Listen to a signal from the QML Engine
    engine
      .as_qqmlengine()
      .on_quit(|_| {
        info!("QML requested quit");
      })
      .release();
  }

  // Start the app
  if let Some(app) = app.as_mut() {
    info!("Entering Qt event loop");
    app.exec();
  }
}
