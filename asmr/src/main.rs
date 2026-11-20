mod controller;
mod model;
mod pyrsutils;
mod view;

use eframe::egui;
use view::DeltaAsmrView;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 740.0])
            .with_min_inner_size([820.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Delta Waves, Schumann 7.83 Hz, 432 Hz, Lo-Fi & ASMR Synth (MVC)",
        options,
        Box::new(|cc| Ok(Box::new(DeltaAsmrView::new(cc)))),
    )
}
