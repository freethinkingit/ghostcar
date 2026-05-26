use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};
use walkdir::WalkDir;

const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mxf"];

#[derive(Default)]
pub struct AppState {
    source_dir: Mutex<Option<String>>,
    dest_dir: Mutex<Option<String>>,
    ffmpeg_path: Mutex<Option<PathBuf>>,
    ffprobe_path: Mutex<Option<PathBuf>>,
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    current: usize,
    total: usize,
    file: String,
    status: String, // "encoding", "done", "failed", "validating", "invalid"
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn find_videos(dir: &str) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_video(e.path()))
        .filter(|e| !e.path().to_string_lossy().contains("/."))
        .map(|e| e.into_path())
        .collect()
}

fn proxy_path(source_dir: &str, dest_dir: &str, file: &Path) -> PathBuf {
    let rel = file.strip_prefix(source_dir).unwrap();
    let mut dest = PathBuf::from(dest_dir).join(rel);
    dest.set_extension("mp4");
    dest
}

fn ffmpeg_path(state: &AppState) -> PathBuf {
    state.ffmpeg_path.lock().unwrap().clone().unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn ffprobe_path(state: &AppState) -> PathBuf {
    state.ffprobe_path.lock().unwrap().clone().unwrap_or_else(|| PathBuf::from("ffprobe"))
}

fn encode_file(source: &Path, dest: &Path, state: &AppState) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let output = Command::new(ffmpeg_path(state))
        .args([
            "-hide_banner", "-loglevel", "warning", "-y",
            "-hwaccel", "videotoolbox",
            "-i", &source.to_string_lossy(),
            "-vf", "scale=720:-2",
            "-c:v", "h264_videotoolbox", "-b:v", "5M",
            "-c:a", "copy",
            &dest.to_string_lossy(),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(dest);
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn validate_file(path: &Path, state: &AppState) -> bool {
    Command::new(ffprobe_path(state))
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "csv=p=0",
            &path.to_string_lossy(),
        ])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

#[tauri::command]
fn set_source(dir: String, state: State<AppState>) {
    *state.source_dir.lock().unwrap() = Some(dir);
}

#[tauri::command]
fn set_dest(dir: String, state: State<AppState>) {
    *state.dest_dir.lock().unwrap() = Some(dir);
}

#[tauri::command]
fn scan(state: State<AppState>) -> Result<usize, String> {
    let source = state.source_dir.lock().unwrap().clone()
        .ok_or("No source directory set")?;
    Ok(find_videos(&source).len())
}

#[tauri::command]
async fn start(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let source = state.source_dir.lock().unwrap().clone()
        .ok_or("No source directory set")?;
    let dest = state.dest_dir.lock().unwrap().clone()
        .ok_or("No destination directory set")?;

    let files = find_videos(&source);
    let total = files.len();
    let mut done = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for (i, file) in files.iter().enumerate() {
        let dest_path = proxy_path(&source, &dest, file);
        let rel = file.strip_prefix(&source).unwrap().to_string_lossy().to_string();

        // Skip if proxy exists and is valid
        if dest_path.exists() && validate_file(&dest_path, &state) {
            skipped += 1;
            let _ = app.emit("progress", ProgressEvent {
                current: i + 1, total, file: rel, status: "done".into(),
            });
            continue;
        }

        let _ = app.emit("progress", ProgressEvent {
            current: i + 1, total, file: rel.clone(), status: "encoding".into(),
        });

        match encode_file(file, &dest_path, &state) {
            Ok(()) => {
                // Validate after encode
                if validate_file(&dest_path, &state) {
                    done += 1;
                    let _ = app.emit("progress", ProgressEvent {
                        current: i + 1, total, file: rel, status: "done".into(),
                    });
                } else {
                    let _ = std::fs::remove_file(&dest_path);
                    failed += 1;
                    let _ = app.emit("progress", ProgressEvent {
                        current: i + 1, total, file: rel, status: "invalid".into(),
                    });
                }
            }
            Err(_e) => {
                failed += 1;
                let _ = app.emit("progress", ProgressEvent {
                    current: i + 1, total, file: rel, status: "failed".into(),
                });
            }
        }
    }

    Ok(format!("Done: {done}, Skipped: {skipped}, Failed: {failed}, Total: {total}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let state = app.state::<AppState>();

            // Sidecars are placed next to the main executable
            let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
            let ffmpeg = exe_dir.join("ffmpeg");
            let ffprobe = exe_dir.join("ffprobe");

            *state.ffmpeg_path.lock().unwrap() = Some(if ffmpeg.exists() { ffmpeg } else { PathBuf::from("ffmpeg") });
            *state.ffprobe_path.lock().unwrap() = Some(if ffprobe.exists() { ffprobe } else { PathBuf::from("ffprobe") });

            Ok(())
        })
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![set_source, set_dest, scan, start])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
