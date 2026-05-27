use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{Emitter, Manager, State};
use walkdir::WalkDir;

const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mxf"];

// --- Hardware Detection ---

fn detect_chip_name() -> String {
    Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn is_ultra_chip(chip: &str) -> bool {
    chip.to_lowercase().contains("ultra")
}

fn detect_worker_count() -> usize {
    let chip = detect_chip_name();
    if is_ultra_chip(&chip) { 3 } else { 2 }
}

fn check_videotoolbox(ffmpeg: &Path) -> bool {
    Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("h264_videotoolbox"))
        .unwrap_or(false)
}

#[derive(Clone, Serialize)]
struct HwInfo {
    chip: String,
    workers: usize,
    has_videotoolbox: bool,
}

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
    std::fs::read_to_string(manifest_path(source_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_manifest(source_dir: &str, manifest: &Manifest) {
    let path = manifest_path(source_dir);
    if let Some(p) = path.parent() { let _ = std::fs::create_dir_all(p); }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(manifest).unwrap());
}

fn now_iso() -> String {
    format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
}

// --- State ---

#[derive(Default)]
pub struct AppState {
    source_dir: Mutex<Option<String>>,
    dest_dir: Mutex<Option<String>>,
    ffmpeg_path: Mutex<Option<PathBuf>>,
    ffprobe_path: Mutex<Option<PathBuf>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    file: String,
    status: String,
    percent: u8,
    done_count: usize,
    failed_count: usize,
    total: usize,
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

fn proxy_dest(dest_dir: &str, rel: &str) -> PathBuf {
    let mut p = PathBuf::from(dest_dir).join(rel);
    p.set_extension("mp4");
    p
}

fn get_duration(path: &Path, ffprobe: &Path) -> Option<f64> {
    Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
}

fn parse_time(line: &str) -> Option<f64> {
    let start = line.find("time=")?;
    let after = &line[start + 5..];
    let end = after.find(|c: char| c == ' ' || c == '\r' || c == '\n').unwrap_or(after.len());
    let parts: Vec<&str> = after[..end].split(':').collect();
    if parts.len() == 3 {
        Some(parts[0].parse::<f64>().ok()? * 3600.0 + parts[1].parse::<f64>().ok()? * 60.0 + parts[2].parse::<f64>().ok()?)
    } else { None }
}

fn validate_file(path: &Path, ffprobe: &Path) -> bool {
    Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn encode_one(
    source: &Path, dest: &Path, ffmpeg: &Path, duration: Option<f64>,
    app: &tauri::AppHandle, rel: &str, counters: &Arc<Mutex<(usize, usize, usize)>>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    if let Some(p) = dest.parent() { std::fs::create_dir_all(p).map_err(|e| e.to_string())?; }

    let mut child = Command::new(ffmpeg)
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
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(dest);
                return Err("cancelled".into());
            }
            if let Some(elapsed) = parse_time(&line) {
                let pct = duration.map(|d| ((elapsed / d) * 100.0).min(99.0) as u8).unwrap_or(0);
                let (done, failed, total) = *counters.lock().unwrap();
                let _ = app.emit("progress", ProgressEvent {
                    file: rel.to_string(), status: "encoding".into(), percent: pct,
                    done_count: done, failed_count: failed, total,
                });
            }
        }
    }

    if cancel.load(Ordering::Relaxed) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(dest);
        return Err("cancelled".into());
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else {
        let _ = std::fs::remove_file(dest);
        Err("ffmpeg error".into())
    }
}

// --- Commands ---

#[tauri::command]
fn hw_info(state: State<AppState>) -> HwInfo {
    let ffmpeg = state.ffmpeg_path.lock().unwrap().clone().unwrap_or_else(|| PathBuf::from("ffmpeg"));
    let chip = detect_chip_name();
    let workers = detect_worker_count();
    let has_videotoolbox = check_videotoolbox(&ffmpeg);
    HwInfo { chip, workers, has_videotoolbox }
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
fn scan(state: State<AppState>) -> Result<StatusCounts, String> {
    let source = state.source_dir.lock().unwrap().clone().ok_or("No source directory set")?;
    let dest = state.dest_dir.lock().unwrap().clone();
    let ffprobe = state.ffprobe_path.lock().unwrap().clone().unwrap_or_else(|| PathBuf::from("ffprobe"));
    let files = find_videos(&source);
    let mut manifest = load_manifest(&source);
    for file in &files {
        let rel = file.strip_prefix(&source).unwrap().to_string_lossy().to_string();
        if !manifest.files.contains_key(&rel) {
            // Check if proxy already exists and is valid
            let already_done = dest.as_ref().map(|d| {
                let p = proxy_dest(d, &rel);
                p.exists() && validate_file(&p, &ffprobe)
            }).unwrap_or(false);
            let status = if already_done { "done" } else { "pending" };
            manifest.files.insert(rel, FileEntry { status: status.into(), timestamp: None, error: None });
        }
    }
    save_manifest(&source, &manifest);
    Ok(count_status(&manifest))
}

#[tauri::command]
fn get_files(state: State<AppState>) -> Result<Vec<(String, String)>, String> {
    let source = state.source_dir.lock().unwrap().clone().ok_or("No source directory set")?;
    let manifest = load_manifest(&source);
    let mut files: Vec<(String, String)> = manifest.files.iter()
        .map(|(k, v)| (k.clone(), v.status.clone()))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

#[tauri::command]
async fn start(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let source = state.source_dir.lock().unwrap().clone().ok_or("No source directory set")?;
    let dest = state.dest_dir.lock().unwrap().clone().ok_or("No destination directory set")?;
    let ffmpeg = state.ffmpeg_path.lock().unwrap().clone().unwrap_or_else(|| PathBuf::from("ffmpeg"));
    let ffprobe = state.ffprobe_path.lock().unwrap().clone().unwrap_or_else(|| PathBuf::from("ffprobe"));
    let cancel = state.cancel.clone();
    cancel.store(false, Ordering::Relaxed);

    let manifest = load_manifest(&source);
    let pending: Vec<String> = manifest.files.iter()
        .filter(|(_, v)| v.status == "pending")
        .map(|(k, _)| k.clone())
        .collect();

    let total = pending.len();
    if total == 0 { return Ok("Nothing to encode".into()); }

    let queue = Arc::new(Mutex::new(pending.into_iter().collect::<Vec<_>>()));
    let counters = Arc::new(Mutex::new((0usize, 0usize, total))); // (done, failed, total)
    let manifest_lock = Arc::new(Mutex::new(manifest));

    let mut handles = Vec::new();

    for _ in 0..detect_worker_count() {
        let queue = queue.clone();
        let counters = counters.clone();
        let manifest_lock = manifest_lock.clone();
        let app = app.clone();
        let source = source.clone();
        let dest = dest.clone();
        let ffmpeg = ffmpeg.clone();
        let ffprobe = ffprobe.clone();
        let cancel = cancel.clone();

        handles.push(thread::spawn(move || {
            loop {
                if cancel.load(Ordering::Relaxed) { break; }
                let rel = {
                    let mut q = queue.lock().unwrap();
                    if q.is_empty() { break; }
                    q.remove(0)
                };

                let source_path = PathBuf::from(&source).join(&rel);
                let dest_path = proxy_dest(&dest, &rel);

                // Skip if already exists and valid
                if dest_path.exists() && validate_file(&dest_path, &ffprobe) {
                    let mut c = counters.lock().unwrap();
                    c.0 += 1;
                    let (done, failed, total) = *c;
                    drop(c);
                    let mut m = manifest_lock.lock().unwrap();
                    m.files.insert(rel.clone(), FileEntry { status: "done".into(), timestamp: Some(now_iso()), error: None });
                    save_manifest(&source, &m);
                    drop(m);
                    let _ = app.emit("progress", ProgressEvent {
                        file: rel, status: "done".into(), percent: 100, done_count: done, failed_count: failed, total,
                    });
                    continue;
                }

                let duration = get_duration(&source_path, &ffprobe);
                let (done, failed, total) = *counters.lock().unwrap();
                let _ = app.emit("progress", ProgressEvent {
                    file: rel.clone(), status: "encoding".into(), percent: 0,
                    done_count: done, failed_count: failed, total,
                });

                match encode_one(&source_path, &dest_path, &ffmpeg, duration, &app, &rel, &counters, &cancel) {
                    Ok(()) => {
                        let mut c = counters.lock().unwrap();
                        c.0 += 1;
                        let (done, failed, total) = *c;
                        drop(c);
                        let mut m = manifest_lock.lock().unwrap();
                        m.files.insert(rel.clone(), FileEntry { status: "done".into(), timestamp: Some(now_iso()), error: None });
                        save_manifest(&source, &m);
                        drop(m);
                        let _ = app.emit("progress", ProgressEvent {
                            file: rel, status: "done".into(), percent: 100, done_count: done, failed_count: failed, total,
                        });
                    }
                    Err(e) => {
                        if e == "cancelled" { break; }
                        let mut c = counters.lock().unwrap();
                        c.1 += 1;
                        let (done, failed, total) = *c;
                        drop(c);
                        let mut m = manifest_lock.lock().unwrap();
                        m.files.insert(rel.clone(), FileEntry { status: "failed".into(), timestamp: Some(now_iso()), error: Some(e) });
                        save_manifest(&source, &m);
                        drop(m);
                        let _ = app.emit("progress", ProgressEvent {
                            file: rel, status: "failed".into(), percent: 0, done_count: done, failed_count: failed, total,
                        });
                    }
                }
            }
        }));
    }

    // Wait for all workers (in a blocking task so we don't block the main thread)
    for h in handles { let _ = h.join(); }

    let (done, failed, _) = *counters.lock().unwrap();
    Ok(format!("Done: {done}, Failed: {failed}, Total: {total}"))
}

#[tauri::command]
async fn validate(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let source = state.source_dir.lock().unwrap().clone().ok_or("No source directory set")?;
    let dest = state.dest_dir.lock().unwrap().clone().ok_or("No destination directory set")?;
    let ffprobe = state.ffprobe_path.lock().unwrap().clone().unwrap_or_else(|| PathBuf::from("ffprobe"));
    let manifest = load_manifest(&source);

    let check_files: Vec<String> = manifest.files.keys().cloned().collect();

    let total = check_files.len();
    if total == 0 { return Ok("Nothing to validate".into()); }

    let queue = Arc::new(Mutex::new(check_files));
    let results: Arc<Mutex<Vec<(String, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let counters = Arc::new(Mutex::new((0usize, 0usize))); // (checked, invalid)
    let validate_workers = 8;

    let mut handles = Vec::new();
    for _ in 0..validate_workers {
        let queue = queue.clone();
        let results = results.clone();
        let counters = counters.clone();
        let app = app.clone();
        let dest = dest.clone();
        let ffprobe = ffprobe.clone();

        handles.push(thread::spawn(move || {
            loop {
                let rel = {
                    let mut q = queue.lock().unwrap();
                    if q.is_empty() { break; }
                    q.remove(0)
                };
                let dest_path = proxy_dest(&dest, &rel);
                let valid = dest_path.exists() && validate_file(&dest_path, &ffprobe);

                let mut c = counters.lock().unwrap();
                c.0 += 1;
                if !valid { c.1 += 1; }
                let (checked, invalid) = *c;
                drop(c);

                results.lock().unwrap().push((rel.clone(), valid));

                let _ = app.emit("progress", ProgressEvent {
                    file: rel, status: "validating".into(), percent: 0,
                    done_count: checked, failed_count: invalid, total,
                });
            }
        }));
    }

    for h in handles { let _ = h.join(); }

    let mut manifest = load_manifest(&source);
    let mut invalid = 0usize;
    for (rel, valid) in results.lock().unwrap().iter() {
        if *valid {
            manifest.files.insert(rel.clone(), FileEntry { status: "done".into(), timestamp: Some(now_iso()), error: None });
        } else {
            manifest.files.insert(rel.clone(), FileEntry { status: "invalid".into(), timestamp: Some(now_iso()), error: None });
            invalid += 1;
        }
    }
    save_manifest(&source, &manifest);
    Ok(format!("Validated: {total}, Invalid: {invalid}"))
}

#[tauri::command]
fn stop(state: State<AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
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
    Ok(format!("Queued {count} for retry"))
}

fn count_status(manifest: &Manifest) -> StatusCounts {
    let mut c = StatusCounts { total: 0, done: 0, failed: 0, pending: 0, invalid: 0 };
    for e in manifest.files.values() {
        c.total += 1;
        match e.status.as_str() {
            "done" => c.done += 1,
            "failed" => c.failed += 1,
            "pending" => c.pending += 1,
            "invalid" => c.invalid += 1,
            _ => {}
        }
    }
    c
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
        .invoke_handler(tauri::generate_handler![hw_info, set_source, set_dest, scan, get_files, start, stop, validate, retry])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
