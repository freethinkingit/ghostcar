use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
    status: String,
    percent: u8, // 0-100 per-file encoding progress
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

/// Get duration in seconds via ffprobe
fn get_duration(path: &Path, state: &AppState) -> Option<f64> {
    let output = Command::new(ffprobe_path(state))
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout);
    s.trim().parse::<f64>().ok()
}

/// Parse "time=HH:MM:SS.xx" from ffmpeg stderr line into seconds
fn parse_time(line: &str) -> Option<f64> {
    let time_start = line.find("time=")?;
    let after = &line[time_start + 5..];
    let end = after.find(|c: char| c == ' ' || c == '\r' || c == '\n').unwrap_or(after.len());
    let ts = &after[..end];
    let parts: Vec<&str> = ts.split(':').collect();
    if parts.len() == 3 {
        let h = parts[0].parse::<f64>().ok()?;
        let m = parts[1].parse::<f64>().ok()?;
        let s = parts[2].parse::<f64>().ok()?;
        Some(h * 3600.0 + m * 60.0 + s)
    } else {
        None
    }
}

fn encode_file(
    source: &Path,
    dest: &Path,
    state: &AppState,
    duration: Option<f64>,
    app: &tauri::AppHandle,
    current: usize,
    total: usize,
    rel: &str,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut child = Command::new(ffmpeg_path(state))
        .args([
            "-hide_banner", "-progress", "pipe:2", "-y",
            "-hwaccel", "videotoolbox",
            "-i", &source.to_string_lossy(),
            "-vf", "scale=720:-2",
            "-c:v", "h264_videotoolbox", "-b:v", "5M",
            "-c:a", "copy",
            &dest.to_string_lossy(),
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;

    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Some(elapsed) = parse_time(&line) {
                let pct = duration
                    .map(|d| ((elapsed / d) * 100.0).min(99.0) as u8)
                    .unwrap_or(0);
                let _ = app.emit("progress", ProgressEvent {
                    current, total, file: rel.to_string(), status: "encoding".into(), percent: pct,
                });
            }
        }
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(dest);
        Err("ffmpeg exited with error".into())
    }
}

fn validate_file(path: &Path, state: &AppState) -> bool {
    Command::new(ffprobe_path(state))
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
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

        if dest_path.exists() && validate_file(&dest_path, &state) {
            skipped += 1;
            let _ = app.emit("progress", ProgressEvent {
                current: i + 1, total, file: rel, status: "done".into(), percent: 100,
            });
            continue;
        }

        let duration = get_duration(file, &state);

        let _ = app.emit("progress", ProgressEvent {
            current: i + 1, total, file: rel.clone(), status: "encoding".into(), percent: 0,
        });

        match encode_file(file, &dest_path, &state, duration, &app, i + 1, total, &rel) {
            Ok(()) => {
                if validate_file(&dest_path, &state) {
                    done += 1;
                    let _ = app.emit("progress", ProgressEvent {
                        current: i + 1, total, file: rel, status: "done".into(), percent: 100,
                    });
                } else {
                    let _ = std::fs::remove_file(&dest_path);
                    failed += 1;
                    let _ = app.emit("progress", ProgressEvent {
                        current: i + 1, total, file: rel, status: "invalid".into(), percent: 0,
                    });
                }
            }
            Err(_) => {
                failed += 1;
                let _ = app.emit("progress", ProgressEvent {
                    current: i + 1, total, file: rel, status: "failed".into(), percent: 0,
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
