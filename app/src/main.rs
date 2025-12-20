use eframe::egui;

mod action;
mod app;
mod command;
mod config;
mod model;
mod shortcut;
mod state;
mod ui;
pub mod utils;

fn main() -> eframe::Result<()> {
    // Initialize the logger
    env_logger::init();

    // Set Python Environment Variables for PyO3
    // This is a temporary fix for local development to ensure the embedded Python finds its standard library.
    std::env::set_var("PYTHONHOME", r"C:\Users\y-yam\miniconda3");
    std::env::set_var("PYTHONPATH", r"C:\Users\y-yam\miniconda3\Lib;C:\Users\y-yam\miniconda3\DLLs");
    eframe::run_native(
        "Video Editor with Canvas",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::MyApp::new(cc)))),
    )
}
