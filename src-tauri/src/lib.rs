use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};
use walkdir::WalkDir;

const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mxf"];

// --- Manifest ---

#[derive(Clone, Serialize, Deserialize, Default)]
struct Manifest {
    files: HashMap<String, FileEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
struct FileEntry {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn manifest_path(source_dir: &str) -> PathBuf {
    PathBuf::from(source_dir).join(".proxy_state").join("manifest.json")
}

fn load_manifest(source_dir: &str) -> Manifest {
    let path = manifest_path(source_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_manifest(source_dir: &str, manifest: &Manifest) {
    let path = manifest_path(source_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(manifest).unwrap());
}

fn now_iso() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{d}")
}

// --- App State ---

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
    percent: u8,
}

#[derive(Clone, Serialize)]
struct StatusCounts {
    total: usize,
    done: usize,
    failed: usize,
    pending: usize,
    invalid: usize,
}

// --- Helpers ---

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

fn proxy_path(source_dir: &str, dest_dir: &str, rel: &str) -> PathBuf {
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

fn get_duration(path: &Path, state: &AppState) -> Option<f64> {
    let output = Command::new(ffprobe_path(state))
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse::<f64>().ok()
}

fn parse_time(line: &str) -> Option<f64> {
    let start = line.find("time=")?;
    let after = &line[start + 5..];
    let end = after.find(|c: char| c == ' ' || c == '\r' || c == '\n').unwrap_or(after.len());
    let parts: Vec<&str> = after[..end].split(':').collect();
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
    source: &Path, dest: &Path, state: &AppState, duration: Option<f64>,
    app: &tauri::AppHandle, current: usize, total: usize, rel: &str,
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
        for line in BufReader::new(stderr).lines().flatten() {
            if let Some(elapsed) = parse_time(&line) {
                let pct = duration.map(|d| ((elapsed / d) * 100.0).min(99.0) as u8).unwrap_or(0);
                let _ = app.emit("progress", ProgressEvent {
                    current, total, file: rel.to_string(), status: "encoding".into(), percent: pct,
                });
            }
        }
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else {
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

// --- Commands ---

#[tauri::command]
fn set_source(dir: String, state: State<AppState>) {
    *state.source_dir.lock().unwrap() = Some(dir);
}

#[tauri::command]
fn set_dest(dir: String, state: State<AppState>) {
    *state.dest_dir.lock().unwrap() = Some(dir);
}

#[tauri::command]
fn scan(state: State<AppState>) -> Result<StatusCounts, String> {
    let source = state.source_dir.lock().unwrap().clone().ok_or("No source directory set")?;
    let files = find_videos(&source);
    let mut manifest = load_manifest(&source);

    // Add new files as pending
    for file in &files {
        let rel = file.strip_prefix(&source).unwrap().to_string_lossy().to_string();
        manifest.files.entry(rel).or_insert(FileEntry {
            status: "pending".into(), timestamp: None, error: None,
        });
    }
    save_manifest(&source, &manifest);

    let counts = count_status(&manifest);
    Ok(counts)
}

#[tauri::command]
async fn start(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let source = state.source_dir.lock().unwrap().clone().ok_or("No source directory set")?;
    let dest = state.dest_dir.lock().unwrap().clone().ok_or("No destination directory set")?;
    let mut manifest = load_manifest(&source);

    let pending: Vec<String> = manifest.files.iter()
        .filter(|(_, v)| v.status == "pending")
        .map(|(k, _)| k.clone())
        .collect();

    let total = pending.len();
    let mut done = 0usize;
    let mut failed = 0usize;

    for (i, rel) in pending.iter().enumerate() {
        let source_path = PathBuf::from(&source).join(rel);
        let dest_path = proxy_path(&source, &dest, rel);

        // Skip if proxy already exists and valid
        if dest_path.exists() && validate_file(&dest_path, &state) {
            manifest.files.insert(rel.clone(), FileEntry {
                status: "done".into(), timestamp: Some(now_iso()), error: None,
            });
            save_manifest(&source, &manifest);
            done += 1;
            let _ = app.emit("progress", ProgressEvent {
                current: i + 1, total, file: rel.clone(), status: "done".into(), percent: 100,
            });
            continue;
        }

        let duration = get_duration(&source_path, &state);
        let _ = app.emit("progress", ProgressEvent {
            current: i + 1, total, file: rel.clone(), status: "encoding".into(), percent: 0,
        });

        match encode_file(&source_path, &dest_path, &state, duration, &app, i + 1, total, rel) {
            Ok(()) => {
                manifest.files.insert(rel.clone(), FileEntry {
                    status: "done".into(), timestamp: Some(now_iso()), error: None,
                });
                done += 1;
                let _ = app.emit("progress", ProgressEvent {
                    current: i + 1, total, file: rel.clone(), status: "done".into(), percent: 100,
                });
            }
            Err(e) => {
                manifest.files.insert(rel.clone(), FileEntry {
                    status: "failed".into(), timestamp: Some(now_iso()), error: Some(e),
                });
                failed += 1;
                let _ = app.emit("progress", ProgressEvent {
                    current: i + 1, total, file: rel.clone(), status: "failed".into(), percent: 0,
                });
            }
        }
        save_manifest(&source, &manifest);
    }

    Ok(format!("Done: {done}, Failed: {failed}, Total: {total}"))
}

#[tauri::command]
async fn validate(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let source = state.source_dir.lock().unwrap().clone().ok_or("No source directory set")?;
    let dest = state.dest_dir.lock().unwrap().clone().ok_or("No destination directory set")?;
    let mut manifest = load_manifest(&source);

    let done_files: Vec<String> = manifest.files.iter()
        .filter(|(_, v)| v.status == "done")
        .map(|(k, _)| k.clone())
        .collect();

    let total = done_files.len();
    let mut invalid = 0usize;

    for (i, rel) in done_files.iter().enumerate() {
        let dest_path = proxy_path(&source, &dest, rel);
        let _ = app.emit("progress", ProgressEvent {
            current: i + 1, total, file: rel.clone(), status: "validating".into(), percent: 0,
        });

        let valid = dest_path.exists() && validate_file(&dest_path, &state);
        if !valid {
            manifest.files.insert(rel.clone(), FileEntry {
                status: "invalid".into(), timestamp: Some(now_iso()), error: None,
            });
            invalid += 1;
        }
    }
    save_manifest(&source, &manifest);
    Ok(format!("Validated: {total}, Invalid: {invalid}"))
}

#[tauri::command]
fn retry(state: State<AppState>) -> Result<String, String> {
    let source = state.source_dir.lock().unwrap().clone().ok_or("No source directory set")?;
    let mut manifest = load_manifest(&source);
    let mut count = 0usize;

    for entry in manifest.files.values_mut() {
        if entry.status == "failed" || entry.status == "invalid" {
            entry.status = "pending".into();
            entry.error = None;
            count += 1;
        }
    }
    save_manifest(&source, &manifest);
    Ok(format!("Queued {count} file(s) for retry"))
}

fn count_status(manifest: &Manifest) -> StatusCounts {
    let mut counts = StatusCounts { total: 0, done: 0, failed: 0, pending: 0, invalid: 0 };
    for entry in manifest.files.values() {
        counts.total += 1;
        match entry.status.as_str() {
            "done" => counts.done += 1,
            "failed" => counts.failed += 1,
            "pending" => counts.pending += 1,
            "invalid" => counts.invalid += 1,
            _ => {}
        }
    }
    counts
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let state = app.state::<AppState>();
            let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
            let ffmpeg = exe_dir.join("ffmpeg");
            let ffprobe = exe_dir.join("ffprobe");
            *state.ffmpeg_path.lock().unwrap() = Some(if ffmpeg.exists() { ffmpeg } else { PathBuf::from("ffmpeg") });
            *state.ffprobe_path.lock().unwrap() = Some(if ffprobe.exists() { ffprobe } else { PathBuf::from("ffprobe") });
            Ok(())
        })
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![set_source, set_dest, scan, start, validate, retry])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
