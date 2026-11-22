pub mod manifest;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(all(target_os = "linux", feature = "gui"))]
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Instant;

#[cfg(not(all(target_os = "linux", feature = "gui")))]
use egui_file_dialog::FileDialog;
use manifest::JobResult;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Generate,
    Validate,
}

/// Shared with the worker thread via atomics; the UI polls it every frame.
struct JobState {
    total: AtomicU64,
    done: AtomicU64,
    running: AtomicBool,
    error: Mutex<Option<String>>,
    result: Mutex<Option<JobResult>>,
}

impl JobState {
    fn new(total: u64) -> Self {
        Self {
            total: AtomicU64::new(total),
            done: AtomicU64::new(0),
            running: AtomicBool::new(false),
            error: Mutex::new(None),
            result: Mutex::new(None),
        }
    }
}

/// Human-friendly file-size formatting
fn gb(n: u64) -> String {
    let v = n as f64 / 1e9;
    if n >= 10_000_000_000 {
        format!("{:.1} GB", v)
    } else {
        format!("{:.2} GB", v)
    }
}

/// The Hashman application.
pub struct App {
	#[cfg(not(all(target_os = "linux", feature = "gui")))]
    file_dialog: FileDialog,
    #[cfg(all(target_os = "linux", feature = "gui"))]
    portal_pick: Option<Receiver<Option<std::path::PathBuf>>>,
    folder: Option<String>,
    workers: i32,
    job: Arc<JobState>,
    started: bool,
    t0: Option<Instant>,
    pending: Option<Mode>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
			#[cfg(not(all(target_os = "linux", feature = "gui")))]
            file_dialog: FileDialog::new(),
            #[cfg(all(target_os = "linux", feature = "gui"))]
            portal_pick: None,
            folder: None,
            workers: 4,
            job: Arc::new(JobState::new(0)),
            started: false,
            t0: None,
            pending: None,
        }
    }

    /// Begin a generate/validate job for the selected folder.
    fn start_job(&mut self, ui: &mut egui::Ui, mode: Mode) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        let workers = self.workers as usize;
        let path = Path::new(&folder);

        if !path.is_dir() {
            self.set_error(ui, "That is not a folder.");
            self.started = true;
            return;
        }

        self.job = Arc::new(JobState::new(0));
        // Mark it running so the UI shows progress and repaints each frame.
        self.job.running.store(true, Ordering::Release);
        self.started = true;
        self.t0 = Some(Instant::now());

        let js = Arc::clone(&self.job);
        let folder = folder.clone();
        let ctx = ui.ctx().clone();
        thread::spawn(move || {
            // Shared borrow only: `fetch_add` takes `&self` on the atomic.
            let on_bytes = |n: u64| {
                js.done.fetch_add(n, Ordering::Relaxed);
            };
            let p = Path::new(&folder);

            // Total bytes for the progress bar's denominator.
            let total = manifest::list_files(p).iter().map(|(_, s)| *s).sum::<u64>();
            js.total.store(total, Ordering::Relaxed);

            // A panic in a worker must not leave the UI stuck on "running".
            let res = catch_unwind(AssertUnwindSafe(|| match mode {
                Mode::Generate => manifest::generate(p, workers, &on_bytes),
                Mode::Validate => manifest::validate(p, workers, &on_bytes),
            }));
            match res {
                Ok(Ok(r)) => *js.result.lock().unwrap() = Some(r),
                Ok(Err(e)) => *js.error.lock().unwrap() = Some(e.to_string()),
                Err(_) => *js.error.lock().unwrap() = Some("Internal error while hashing.".into()),
            }
            // Result/error are written first; Release makes them visible to the UI.
            js.running.store(false, Ordering::Release);
            ctx.request_repaint();
        });
        ui.request_repaint();
    }

    fn set_error(&mut self, ui: &mut egui::Ui, msg: &str) {
        *self.job.error.lock().unwrap() = Some(msg.to_string());
        *self.job.result.lock().unwrap() = None;
        self.job.running.store(false, Ordering::Release);
        ui.request_repaint();
    }

    /// Render the whole UI.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        #[cfg(all(target_os = "linux", feature = "gui"))]
        {
            let picked = self.portal_pick.as_ref().and_then(|rx| match rx.try_recv() {
                Ok(path) => Some(path),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
            });
            if let Some(path) = picked {
                self.portal_pick = None;
                if let Some(path) = path {
                    self.folder = Some(path.to_string_lossy().to_string());
                    if let Some(mode) = self.pending.take() {
                        self.start_job(ui, mode);
                    }
                } else {
                    self.pending = None;
                }
            }
        }

        #[cfg(not(all(target_os = "linux", feature = "gui")))]
        {
            self.file_dialog.update(ui);
            if let Some(path) = self.file_dialog.take_picked() {
                self.folder = Some(path.to_string_lossy().to_string());
                if let Some(mode) = self.pending.take() {
                    self.start_job(ui, mode);
                }
            }
        }

        ui.add_space(12.0);

        let running = self.job.running.load(Ordering::Acquire);

        // --- Main actions --------------------------------------------------------
        let btn_size = egui::vec2(272.0, 64.0);
        let btn_font_size = 20.0;
        let mut run = None::<Mode>;
        if !self.started {
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(egui::RichText::new("Generate").size(btn_font_size)).min_size(btn_size)).clicked() {
                    run = Some(Mode::Generate);
                }
                ui.add_space(12.0);
                if ui.add(egui::Button::new(egui::RichText::new("Validate").size(btn_font_size)).min_size(btn_size)).clicked() {
                    run = Some(Mode::Validate);
                }
            });
        }

        if let Some(m) = run {
            // Ask for the folder first; `start_job` runs once it is picked.
            self.pending = Some(m);
            #[cfg(all(target_os = "linux", feature = "gui"))]
            {
                let (tx, rx) = mpsc::channel();
                self.portal_pick = Some(rx);
                // Run the synchronous portal picker off the UI thread.
                thread::spawn(move || {
                    let selected = rfd::FileDialog::new()
                        .set_title("Choose a folder")
                        .pick_folder();
                    let _ = tx.send(selected);
                });
            }
            #[cfg(not(all(target_os = "linux", feature = "gui")))]
            self.file_dialog.pick_directory();
        }

        // --- Folder + parallelism -------------------------------------------------
        //let mut workers = self.workers as f32;
        //if !self.started {
        //    ui.horizontal(|ui| {
        //        ui.label("Worker count:");
        //        ui.add_space(12.0);
        //        ui.add(egui::DragValue::new(&mut workers).range(1..=i32::MAX).speed(1.0));
        //    });
        //    
        //    self.workers = workers.round() as i32;
        //    ui.add_space(6.0);
        //}
        
		if self.started {
            if !running {
                if let Some(f) = &self.folder {
                    ui.horizontal(|ui| {
                        ui.label(format!("Folder: {}", f));
                        ui.add_space(5.0);
                        if ui.add(egui::Button::new("New Operation")).clicked() {
                            //reset state
                            self.started = false;
                        }
                    });
                    ui.add_space(6.0);
                }
            } else {
                if let Some(f) = &self.folder {
                    ui.label(format!("Folder: {}", f));
                    ui.add_space(6.0);
                }
            }
		}

        // --- Progress ------------------------------------------------------------------
        if running {
            let total = self.job.total.load(Ordering::Relaxed);
            let done = self.job.done.load(Ordering::Relaxed);

            let f = if total > 0 {
                done as f32 / total as f32
            } else {
                0.0
            };

            if let Some(t0) = self.t0 {
                let secs = t0.elapsed().as_secs_f64();
                if secs > 1.0 && done > 0 {
                    ui.add(egui::ProgressBar::new(f).text(format!("{} / {} ∙ {:.1} MB/s", gb(done), gb(total), done as f64 / secs / 1e6)));
                } else {
                    ui.add(egui::ProgressBar::new(f).text(format!("{} / {}", gb(done), gb(total))));
                }
            } else {
                ui.add(egui::ProgressBar::new(f).text(format!("{} / {}", gb(done), gb(total))));
            }
            ui.add_space(6.0);
            ui.request_repaint();
        }

        // --- Result ----------------------------------------------------------------------
        if !running && self.started {
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    self.show_result(ui);
                });
        }
    }

    fn show_result(&self, ui: &mut egui::Ui) {
        {
            if let Some(e) = self.job.error.lock().unwrap().as_ref() {
                ui.label(egui::RichText::new(format!("Error: {}", e)).color(egui::Color32::RED));
                ui.add_space(10.0);
            }

            if let Some(r) = self.job.result.lock().unwrap().as_ref() {
                match r {
                    JobResult::Generate { files, unreadable } => {
                        let text_color = if unreadable.len() > 0 {
                            egui::Color32::RED
                        } else {
                            egui::Color32::GREEN
                        };
                        ui.label(egui::RichText::new(format!(
                            "Manifest created: {} file{} hashed ({} unreadable)",
                            files,
                            if *files != 1 { "s" } else { "" },
                            unreadable.len(),
                        )).color(text_color));
                        ui.add_space(6.0);
                        self.show_list(ui, "Could not read", unreadable);
                    }
                    JobResult::Validate {
                        checked,
                        bad,
                        missing,
                        extra,
                        unreadable,
                    } => {
                        let text_color = if bad.len() == 0 && missing.len() == 0 && unreadable.len() == 0 && extra.len() == 0 {
                            egui::Color32::GREEN
                        } else {
                            egui::Color32::RED
                        };
                        ui.label(egui::RichText::new(format!(
                            "Checked {} file{}: {} bad, {} missing, {} unreadable, {} extra",
                            checked,
                            if *checked != 1 { "s" } else { "" },
                            bad.len(),
                            missing.len(),
                            unreadable.len(),
                            extra.len(),
                        )).color(text_color));
                        ui.add_space(6.0);
                        self.show_list(ui, "Changed or corrupt", bad);
                        self.show_list(ui, "Missing", missing);
                        self.show_list(ui, "Could not read", unreadable);
                        self.show_list(ui, "Extra (not in manifest)", extra);
                    }
                }
            }
        }
    }

    fn show_list(&self, ui: &mut egui::Ui, label: &str, items: &[String]) {
        if items.is_empty() {
            return;
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{} ({} entries)", label, items.len())).strong());
            ui.add(egui::Separator::default().horizontal());
        });
        ui.add_space(2.0);
        for item in items.iter().take(200) {
            ui.label(item);
            ui.add_space(1.0);
        }
        if items.len() > 200 {
            ui.label(format!("… and {} more", items.len() - 200));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_constructs() {
        let _ = App::new();
    }

    #[test]
    fn default_state() {
        let app = App::new();
        assert!(app.folder.is_none());
        assert_eq!(app.workers, 4);
        assert!(!app.started);
    }

    #[test]
    fn gb_formats() {
        assert_eq!(gb(0), "0.00 GB");
        assert_eq!(gb(2_000_000_000), "2.00 GB");
        assert_eq!(gb(10_500_000_000), "10.5 GB");
    }

    /// Headless UI validation via egui_kittest
    #[test]
    fn ui_renders_headlessly() {
        use egui_kittest::kittest::Queryable;

        let mut app = App::new();
        let mut harness = egui_kittest::Harness::new_ui(move |ui: &mut egui::Ui| {
            app.ui(ui);
        });
        harness.run();

        assert!(
            harness.query_by_label("Generate").is_some(),
            "Generate button missing"
        );
        assert!(
            harness.query_by_label("Validate").is_some(),
            "Validate button missing"
        );
        assert!(
            harness.query_by_label("Hashman").is_some(),
            "app title missing"
        );
    }
}
