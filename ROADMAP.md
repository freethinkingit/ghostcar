# GhostCar Roadmap

Lightweight proxy video generator for DaVinci Resolve.
Built by [Freethinking IT](https://github.com/freethinkingit) • Distributed via [86 Pieces](https://86pieces.com/ghostcar)

---

## v0.1 — MVP (Current)

- [x] Tauri app scaffold (React + Rust)
- [x] Source/destination folder pickers
- [x] Scan for video files (.mp4, .mov, .mxf)
- [x] Encode to H.264 720p/5Mbps via VideoToolbox
- [x] Validate proxies (moov atom / ffprobe check)
- [x] Auto-redo invalid files
- [x] Progress bar + live log
- [x] macOS .app and .dmg build
- [ ] Bundle ffmpeg/ffprobe in app resources
- [ ] Branding (app icon, about screen)
- [ ] MIT license
- [ ] README + initial git commit

## v0.2 — Polish

- [ ] Pause / Resume / Cancel mid-batch
- [ ] Manifest persistence (survive app restart)
- [ ] Encoding presets (720p, 1080p, custom)
- [ ] Bitrate slider
- [ ] Codec picker (H.264 / ProRes Proxy)
- [ ] System notification on batch complete
- [ ] Drag-and-drop folders onto app window
- [ ] Dark / Light theme toggle

## v0.3 — Power Features

- [ ] Concurrency control (1–4 parallel encodes)
- [ ] Duration match validation (proxy vs original)
- [ ] Compression ratio outlier detection
- [ ] File type filters (include/exclude patterns)
- [ ] Detailed stats view (total size saved, time elapsed, avg speed)
- [ ] Export log to file

## v0.4 — Cross-Platform & Distribution

- [ ] Windows support (QSV / NVENC / libx264 fallback)
- [ ] Linux AppImage
- [ ] Auto-updater (Tauri updater plugin)
- [ ] Code signing (macOS notarization, Windows Authenticode)
- [ ] Landing page on 86pieces.com
- [ ] Homebrew cask

## v0.5 — i18n & Accessibility

- [ ] i18n framework (react-i18next)
- [ ] English (default)
- [ ] Spanish
- [ ] French
- [ ] Japanese
- [ ] Locale-aware formatting (file sizes, durations)
- [ ] Keyboard navigation
- [ ] Screen reader support

## v1.0 — Production Ready

- [ ] Unit tests (Rust: scan, path mapping, validation)
- [ ] Integration tests (encode test clip, verify output)
- [ ] Frontend tests (React Testing Library)
- [ ] E2E tests (Tauri WebDriver)
- [ ] CI/CD pipeline (GitHub Actions: build + test + release)
- [ ] Crash reporting
- [ ] Usage analytics (opt-in)
- [ ] User documentation / help screen

## Future Ideas

- Watch mode (auto-encode new files as they appear)
- Cloud sync manifest (team workflows)
- Resolve project file integration (auto-link proxies)
- Mobile companion (monitor progress remotely)
- Plugin system for custom encode pipelines
- Batch rename / reorganize proxies
