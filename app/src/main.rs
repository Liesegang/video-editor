use eframe::egui;

#[cfg(feature = "logic-editor")]
mod logic_graph_ui;
mod timeline_app;

fn main() -> eframe::Result<()> {
    env_logger::init();

    eframe::run_native(
        "RuViE - Rust Video Editor",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1920.0, 1080.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(timeline_app::TimelineApp::new(cc)?))),
    )
}
