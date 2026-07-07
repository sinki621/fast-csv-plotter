mod app;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([800.0, 550.0])
            .with_title("Fast CSV Plotter"),
        ..Default::default()
    };

    eframe::run_native(
        "Fast CSV Plotter",
        options,
        Box::new(|cc| {
            // Customize styles, visual settings, etc.
            Ok(Box::new(app::CsvPlotterApp::new(cc)))
        }),
    )
}
