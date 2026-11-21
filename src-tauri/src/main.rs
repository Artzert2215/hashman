#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod manifest;

use manifest::JobResult;
use serde::Serialize;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
}

/// mode = "generate" | "validate". Emits "progress" events while running.
#[tauri::command]
async fn run_job(
    app: AppHandle,
    mode: String,
    folder: String,
    workers: usize,
) -> Result<JobResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = PathBuf::from(folder);
        if !root.is_dir() {
            return Err("That is not a folder.".to_string());
        }
        let total: u64 = manifest::list_files(&root).iter().map(|(_, s)| *s).sum();
        let done = Arc::new(AtomicU64::new(0));
        let finished = Arc::new(AtomicBool::new(false));

        let ticker = {
            let (app, done, finished) = (app.clone(), done.clone(), finished.clone());
            std::thread::spawn(move || loop {
                let _ = app.emit(
                    "progress",
                    Progress {
                        done: done.load(Relaxed),
                        total,
                    },
                );
                if finished.load(Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            })
        };

        let counter = done.clone();
        let on_bytes = move |n: u64| {
            counter.fetch_add(n, Relaxed);
        };
        let res = match mode.as_str() {
            "generate" => manifest::generate(&root, workers, &on_bytes),
            "validate" => manifest::validate(&root, workers, &on_bytes),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown mode")),
        };

        finished.store(true, Relaxed);
        let _ = ticker.join();
        res.map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![run_job])
        .run(tauri::generate_context!())
        .expect("error while running application");
}
