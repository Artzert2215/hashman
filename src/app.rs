//! The egui front end.

use crate::manifest::{self, JobResult};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(target_os = "linux")]
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(not(target_os = "linux"))]
use egui_file_dialog::FileDialog;

/// Window width, and the height of the window before any results are shown.
pub const WINDOW_WIDTH: f32 = 580.0;
pub const COMPACT_HEIGHT: f32 = 100.0;
/// Height the window grows to when there is something to list.
const RESULT_HEIGHT: f32 = 380.0;
/// Longest list shown in the results before it is cut off with "… and N more".
const MAX_LISTED: usize = 200;

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
    fn new() -> Self {
        Self {
            total: AtomicU64::new(0),
            done: AtomicU64::new(0),
            running: AtomicBool::new(false),
            error: Mutex::new(None),
            result: Mutex::new(None),
        }
    }

    fn failed(msg: &str) -> Self {
        let s = Self::new();
        *s.error.lock().unwrap() = Some(msg.to_string());
        s
    }

    /// Is there anything worth a taller window (an error or a non-empty list)?
    fn has_details(&self) -> bool {
        if self.error.lock().unwrap().is_some() {
            return true;
        }
        match self.result.lock().unwrap().as_ref() {
            Some(JobResult::Generate { unreadable, .. }) => !unreadable.is_empty(),
            Some(JobResult::Validate {
                bad,
                missing,
                extra,
                unreadable,
                ..
            }) => [bad, missing, extra, unreadable]
                .iter()
                .any(|l| !l.is_empty()),
            None => false,
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

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The Hashman application.
pub struct App {
    #[cfg(not(target_os = "linux"))]
    file_dialog: FileDialog,
    #[cfg(target_os = "linux")]
    portal_pick: Option<Receiver<Option<PathBuf>>>,
    folder: Option<PathBuf>,
    workers: usize,
    job: Arc<JobState>,
    started: bool,
    /// Whether the window has already been resized for the current results.
    resized_for_results: bool,
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
            #[cfg(not(target_os = "linux"))]
            file_dialog: FileDialog::new(),
            #[cfg(target_os = "linux")]
            portal_pick: None,
            folder: None,
            workers: manifest::default_workers(),
            job: Arc::new(JobState::new()),
            started: false,
            resized_for_results: false,
            t0: None,
            pending: None,
        }
    }

    /// Begin a generate/validate job for the selected folder.
    fn start_job(&mut self, ctx: &egui::Context, mode: Mode) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        self.started = true;
        self.resized_for_results = false;

        if !folder.is_dir() {
            self.job = Arc::new(JobState::failed("That is not a folder."));
            ctx.request_repaint();
            return;
        }

        self.job = Arc::new(JobState::new());
        // Mark it running so the UI shows progress and keeps repainting.
        self.job.running.store(true, Ordering::Release);
        self.t0 = Some(Instant::now());

        let js = Arc::clone(&self.job);
        let workers = self.workers;
        let repaint_ctx = ctx.clone();
        thread::spawn(move || {
            let on_total = |n: u64| js.total.store(n, Ordering::Relaxed);
            let on_bytes = |n: u64| {
                js.done.fetch_add(n, Ordering::Relaxed);
            };

            // A panic in a worker must not leave the UI stuck on "running".
            let res = catch_unwind(AssertUnwindSafe(|| match mode {
                Mode::Generate => manifest::generate_with(&folder, workers, &on_total, &on_bytes),
                Mode::Validate => manifest::validate_with(&folder, workers, &on_total, &on_bytes),
            }));
            match res {
                Ok(Ok(r)) => *js.result.lock().unwrap() = Some(r),
                Ok(Err(e)) => *js.error.lock().unwrap() = Some(e.to_string()),
                Err(_) => *js.error.lock().unwrap() = Some("Internal error while hashing.".into()),
            }
            // Result/error are written first; Release makes them visible to the UI.
            js.running.store(false, Ordering::Release);
            repaint_ctx.request_repaint();
        });
        ctx.request_repaint();
    }

    /// Pick up the folder chosen in the file dialog and start the pending job.
    fn poll_folder_picker(&mut self, ui: &mut egui::Ui) {
        #[cfg(target_os = "linux")]
        let picked: Option<Option<PathBuf>> =
            self.portal_pick
                .as_ref()
                .and_then(|rx| match rx.try_recv() {
                    Ok(path) => Some(path),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => Some(None),
                });
        #[cfg(target_os = "linux")]
        if picked.is_some() {
            self.portal_pick = None;
        }

        #[cfg(not(target_os = "linux"))]
        let picked: Option<Option<PathBuf>> = {
            self.file_dialog.update(ui);
            self.file_dialog
                .take_picked()
                .map(|p| Some(p.to_path_buf()))
        };

        match picked {
            Some(Some(path)) => {
                self.folder = Some(path);
                if let Some(mode) = self.pending.take() {
                    self.start_job(ui.ctx(), mode);
                }
            }
            Some(None) => self.pending = None, // dialog cancelled
            None => {}
        }
    }

    /// Ask the user for a folder; `start_job` runs once it is picked.
    fn ask_for_folder(&mut self, mode: Mode) {
        self.pending = Some(mode);
        #[cfg(target_os = "linux")]
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
        #[cfg(not(target_os = "linux"))]
        self.file_dialog.pick_directory();
    }

    fn resize_window(ctx: &egui::Context, height: f32) {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            WINDOW_WIDTH,
            height,
        )));
    }

    /// Render the whole UI.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll_folder_picker(ui);
        ui.add_space(12.0);

        let running = self.job.running.load(Ordering::Acquire);

        // --- Main actions --------------------------------------------------------
        if !self.started {
            let btn_size = egui::vec2(272.0, 64.0);
            let mut run = None;
            ui.horizontal(|ui| {
                for (label, mode) in [("Generate", Mode::Generate), ("Validate", Mode::Validate)] {
                    let text = egui::RichText::new(label).size(20.0);
                    if ui.add(egui::Button::new(text).min_size(btn_size)).clicked() {
                        run = Some(mode);
                    }
                    ui.add_space(12.0);
                }
            });
            if let Some(mode) = run {
                self.ask_for_folder(mode);
            }
        }

        // --- Folder + "New Operation" ---------------------------------------------
        if self.started {
            if let Some(f) = &self.folder {
                let mut reset = false;
                ui.horizontal(|ui| {
                    ui.label(format!("Folder: {}", f.display()));
                    if !running {
                        ui.add_space(5.0);
                        reset = ui.button("New Operation").clicked();
                    }
                });
                ui.add_space(6.0);
                if reset {
                    self.started = false;
                    self.resized_for_results = false;
                    Self::resize_window(ui.ctx(), COMPACT_HEIGHT);
                }
            }
        }

        // --- Progress ------------------------------------------------------------------
        if running {
            self.show_progress(ui);
            // Progress is polled, so a modest repaint rate is plenty.
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        // --- Result ----------------------------------------------------------------------
        if self.started && !running {
            if !self.resized_for_results {
                self.resized_for_results = true;
                if self.job.has_details() {
                    Self::resize_window(ui.ctx(), RESULT_HEIGHT);
                }
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| show_result(&self.job, ui));
        }
    }

    fn show_progress(&self, ui: &mut egui::Ui) {
        let total = self.job.total.load(Ordering::Relaxed);
        let done = self.job.done.load(Ordering::Relaxed);
        let fraction = if total > 0 {
            (done as f32 / total as f32).min(1.0)
        } else {
            0.0
        };

        let mut text = format!("{} / {}", gb(done), gb(total));
        let secs = self.t0.map_or(0.0, |t| t.elapsed().as_secs_f64());
        if secs > 1.0 && done > 0 {
            text.push_str(&format!(" ∙ {:.1} MB/s", done as f64 / secs / 1e6));
        }
        ui.add(egui::ProgressBar::new(fraction).text(text));
        ui.add_space(6.0);
    }
}

fn show_result(job: &JobState, ui: &mut egui::Ui) {
    if let Some(e) = job.error.lock().unwrap().as_ref() {
        ui.label(egui::RichText::new(format!("Error: {}", e)).color(egui::Color32::RED));
        ui.add_space(10.0);
    }

    let result = job.result.lock().unwrap();
    let Some(result) = result.as_ref() else {
        return;
    };

    let (summary, ok, symlinks) = match result {
        JobResult::Generate {
            files,
            unreadable,
            symlinks,
        } => (
            format!(
                "Manifest created: {} file{} hashed ({} unreadable)",
                files,
                plural(*files),
                unreadable.len(),
            ),
            unreadable.is_empty(),
            *symlinks,
        ),
        JobResult::Validate {
            checked,
            bad,
            missing,
            extra,
            unreadable,
            symlinks,
        } => (
            format!(
                "Checked {} file{}: {} bad, {} missing, {} unreadable, {} extra",
                checked,
                plural(*checked),
                bad.len(),
                missing.len(),
                unreadable.len(),
                extra.len(),
            ),
            bad.is_empty() && missing.is_empty() && unreadable.is_empty() && extra.is_empty(),
            *symlinks,
        ),
    };
    let color = if ok {
        egui::Color32::GREEN
    } else {
        egui::Color32::RED
    };
    ui.label(egui::RichText::new(summary).color(color));
    if symlinks > 0 {
        ui.label(
            egui::RichText::new(format!(
                "{} symbolic link{} skipped (links are not followed or recorded)",
                symlinks,
                plural(symlinks)
            ))
            .weak(),
        );
    }
    ui.add_space(6.0);

    match result {
        JobResult::Generate { unreadable, .. } => show_list(ui, "Could not read", unreadable),
        JobResult::Validate {
            bad,
            missing,
            extra,
            unreadable,
            ..
        } => {
            show_list(ui, "Changed or corrupt", bad);
            show_list(ui, "Missing", missing);
            show_list(ui, "Could not read", unreadable);
            show_list(ui, "Extra (not in manifest)", extra);
        }
    }
}

fn show_list(ui: &mut egui::Ui, label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{} ({} entries)", label, items.len())).strong());
        ui.add(egui::Separator::default().horizontal());
    });
    ui.add_space(2.0);
    for item in items.iter().take(MAX_LISTED) {
        ui.label(item);
        ui.add_space(1.0);
    }
    if items.len() > MAX_LISTED {
        ui.label(format!("… and {} more", items.len() - MAX_LISTED));
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
        assert_eq!(app.workers, manifest::default_workers());
        assert!(app.workers >= 1);
        assert!(!app.started);
    }

    #[test]
    fn gb_formats() {
        assert_eq!(gb(0), "0.00 GB");
        assert_eq!(gb(2_000_000_000), "2.00 GB");
        assert_eq!(gb(10_500_000_000), "10.5 GB");
    }

    #[test]
    fn plural_forms() {
        assert_eq!(plural(0), "s");
        assert_eq!(plural(1), "");
        assert_eq!(plural(2), "s");
    }

    #[test]
    fn details_only_when_there_is_something_to_list() {
        let clean = JobState::new();
        assert!(!clean.has_details());
        *clean.result.lock().unwrap() = Some(JobResult::Generate {
            files: 3,
            unreadable: vec![],
            symlinks: 0,
        });
        assert!(!clean.has_details());
        *clean.result.lock().unwrap() = Some(JobResult::Generate {
            files: 3,
            unreadable: vec!["x".into()],
            symlinks: 0,
        });
        assert!(clean.has_details());
        assert!(JobState::failed("boom").has_details());
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
    }
}
