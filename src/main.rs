// Hashman — eframe desktop binary (behind the `gui` feature).
//
// The icon is embedded at compile time so the native window/task switcher
// uses the same artwork as the platform installers.

#[cfg(feature = "gui")]
fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png"))
        .unwrap_or_default();

    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([580.0, 560.0])
            .with_min_inner_size([460.0, 420.0])
            .with_app_id("org.hashman.Hashman")
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native("Hashman", opts, Box::new(|_cc| Ok(Box::new(Shell::new()))))
}

#[cfg(feature = "gui")]
struct Shell {
    app: hashman_egui::App,
}

#[cfg(feature = "gui")]
impl Shell {
    fn new() -> Self {
        Self {
            app: hashman_egui::App::new(),
        }
    }
}

#[cfg(feature = "gui")]
impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ui, |ui| {
            self.app.ui(ui);
        });
    }
}

#[cfg(not(feature = "gui"))]
fn main() {
    println!(
        "Hashman core built without the GUI. Build with `--features gui` to get the window."
    );
}
