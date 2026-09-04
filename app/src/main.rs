use eframe::egui;

mod app;
mod command;
mod config;
mod model;
mod qa;
mod state;
mod ui;

fn main() -> eframe::Result<()> {
    env_logger::init();

    eframe::run_native(
        "RuViE - Rust Video Editor",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1920.0, 1080.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::RuViEApp::new(cc)?))),
    )
}
