mod api;
mod models;
mod plugins;
mod storage;
mod ui;

use eframe::egui;
use ui::OllaFreeGuiApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 760.0])
            .with_title("OllaFree GUI"),
        ..Default::default()
    };

    eframe::run_native(
        "OllaFree GUI",
        options,
        Box::new(|_cc| Ok(Box::new(OllaFreeGuiApp::new()))),
    )
}
