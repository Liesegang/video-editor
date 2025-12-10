use eframe::egui;

mod action;
mod app;
mod command;
mod config;
mod model;
mod shortcut;
mod state;
mod ui;
mod utils;

fn main() -> eframe::Result<()> {
    env_logger::init();
    eframe::run_native(
        "Video Editor with Canvas",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::MyApp::new(cc)))),
    )
}
