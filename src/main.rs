// Hashman — eframe desktop binary (behind the `gui` feature).
//
// Creates a window and drives the headless `hashman_egui::App` inside a
// central panel. On a headless machine (no X11/Wayland) the `gui` feature is
// off, and only this empty `main` is produced, so the lib still builds.

#[cfg(feature = "gui")]
fn main() -> eframe::Result<()> {
    // `ViewportBuilder` lives in `egui` (re-exported as `eframe::egui`), and
    // `NativeOptions` is a plain struct with a public `viewport` field.
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([580.0, 560.0])
            .with_min_inner_size([460.0, 420.0]),
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
    // In eframe 0.37 `App::ui` already receives the root `Ui` (there is no
    // `Frame::context()` any more), and panels are shown directly inside it.
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ui, |ui| {
            self.app.ui(ui);
        });
    }
}

#[cfg(not(feature = "gui"))]
fn main() {
    println!(
        "Hashman core built without the GUI. Build with `--features gui` (requires X11/Wayland) to get the window."
    );
}
