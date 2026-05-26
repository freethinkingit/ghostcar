# 🏎️ GhostCar

Big drives are cheap. Fast drives are not. If you're an indie creative shooting TBs of footage, you shouldn't have to choose between storage you can afford and an editing timeline that doesn't stutter.

GhostCar bridges the gap. It generates lightweight proxy files from your high-capacity HDD footage using Apple Silicon's built-in hardware encoder, then you edit with those proxies on a smaller SSD. When you're ready to export, DaVinci Resolve pulls from the full-res originals automatically. You get the speed of expensive storage without the price tag.

This tool was born out of [86 Pieces](https://86pieces.com) — a racing team that shoots a lot of video and doesn't have a Hollywood budget. We built it for ourselves, and now it's yours.

## How it works

GhostCar takes advantage of two things most people don't know they have:

1. **DaVinci Resolve's Proxy Media** — Resolve can link lightweight copies to your original clips. Edit with the small ones, export with the big ones. It's built into the free version.

2. **Apple Silicon's VideoToolbox** — Every M-series Mac has a dedicated hardware encoder that converts video fast without taxing your CPU. GhostCar uses it to churn through footage quickly.

You point it at a folder, pick where the proxies should go, and hit Start. It encodes everything, validates the output, and re-does anything that didn't come out right.

## Get it

Download from [86pieces.com/ghostcar](https://86pieces.com/ghostcar) — macOS, Apple Silicon.

No terminal. No dependencies. Just drag to Applications and go.

## Development

```bash
git clone https://github.com/freethinkingit/ghostcar.git
cd ghostcar
npm install
npm run tauri dev
```

Requires Rust, Node.js 18+, and Xcode Command Line Tools.

## License

GPL-3.0 — Free to use and modify. Forks must stay open source. See [LICENSE](./LICENSE).

---

A [Freethinking IT](https://github.com/freethinkingit) project.
