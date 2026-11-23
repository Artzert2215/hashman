use hashman::{App, COMPACT_HEIGHT, WINDOW_WIDTH};

/// Thin eframe wrapper around the library's `App`.
#[derive(Default)]
struct Shell {
    app: App,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ui, |ui| {
            self.app.ui(ui);
        });
    }
}

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png"))
        .unwrap_or_default();

    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([WINDOW_WIDTH, COMPACT_HEIGHT])
            .with_min_inner_size([WINDOW_WIDTH, COMPACT_HEIGHT])
            .with_max_inner_size([WINDOW_WIDTH, 99999.0])
            .with_resizable(true)
            .with_app_id("io.github.Artzert2215.Hashman")
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "Hashman",
        opts,
        Box::new(|_cc| Ok(Box::new(Shell::default()))),
    )
}
