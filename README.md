# PowerBeamer 📽️

[![CI & Release](https://github.com/ahmadikalkhorani/PowerBeamer/actions/workflows/release.yml/badge.svg)](https://github.com/ahmadikalkhorani/PowerBeamer/actions/workflows/release.yml)
[![GitHub Release](https://img.shields.io/github/v/release/ahmadikalkhorani/PowerBeamer?color=blue)](https://github.com/ahmadikalkhorani/PowerBeamer/releases)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-green)](LICENSE)

A high-performance, low-footprint desktop presentation studio purpose-built for **LaTeX Beamer PDF presentations**. Engineered in **Rust** with **Slint / WGPU** and **Google PDFium** for instant sub-millisecond slide transitions, fluid vector inking, native Beamer dual-screen auto-detection, speaker notes extraction, and modular multi-window docking.

---

## ✨ Key Features

- **🚀 Ultra-Fast & Low Resource Consumption**:
  - Native compiled Rust binary (< 50MB RAM baseline, zero Electron/Chromium runtime).
  - Sub-10ms inking latency with Catmull-Rom spline smoothing.
- **🎨 Vector Inking & Editing Suite**:
  - **✍️ Fountain Pen (`P`)**: Pressure-sensitive vector inking with customizable colors.
  - **🖍️ Highlighter (`H`)**: Multiply blend mode preserving crisp black LaTeX text beneath highlights.
  - **▱ Eraser (`E`)**: Real-time distance-to-segment collision eraser.
  - **◌ Lasso Selection (`S`)**: Select, move, and recolor vector strokes.
  - **⚡ Laser Pointer (`L`)**: Dynamic glowing cursor with decaying trail mirrored to audience.
  - **⊕ Dynamic Whiteboard (`N` / `+`)**: Insert blank blackboard or dotted grid slides on the fly.
  - **↶ Full History (`Cmd/Ctrl+Z`)**: Deep multi-level undo and redo.
- **🔍 LaTeX Beamer Native Intelligence**:
  - **Automatic Dual-Screen Detection**: Automatically recognizes `pgfpages` side-by-side slides (aspect ratio $\ge 2.0$, e.g., 8:3 or 32:9) and cleanly splits the audience view from presenter notes.
  - **Comment / Note Extraction**: Displays speaker notes and comments in a dedicated resizable panel.
  - **Multimedia Engine**: Detects `\movie` and `\includemedia` annotations for smooth video and audio playback.
- **🖥️ Modular Multi-Window Architecture**:
  - **Presenter View**: Current slide, interactive inking canvas, laser cursor, next slide preview, speaker notes, and presentation countdown timer.
  - **Audience View**: Clean, distraction-free display for projectors and external monitors with real-time synchronized strokes and laser trail.
  - **Detached Notes View**: Floating, resizable speaker notes window for multi-monitor setups or sidecar tablets.
- **💾 PDF & Session Export**:
  - Exports annotated slides and non-destructive `.presnotes.json` sidecar session files.
  - Configurable rendering quality: **Eco (150 DPI)**, **Balanced (300 DPI)**, and **Ultra Vector (600 DPI)**.
- **🔄 Built-In Auto-Updater**:
  - One-click GitHub Releases check and background updater directly from the in-app options menu.

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `L` | Activate Laser Pointer |
| `P` | Activate Pen Tool |
| `H` | Activate Highlighter |
| `E` | Activate Eraser |
| `S` | Activate Lasso Selection |
| `Space` / `→` / `↓` / `J` | Next Slide |
| `←` / `↑` / `K` | Previous Slide |
| `N` or `+` | Insert New Whiteboard / Scratchpad Slide |
| `Cmd/Ctrl + Z` | Undo |
| `Cmd/Ctrl + Shift + Z` | Redo |

---

## 🛠️ Building & Running

### Prerequisites
- [Rust & Cargo](https://rustup.rs/) (edition 2024 / 1.85+)
- CMake (used by Slint build dependencies)

### Run in Development
```bash
# Run with interactive LaTeX Beamer demo deck
cargo run

# Run with your own LaTeX Beamer PDF
cargo run -- /path/to/my_presentation.pdf
```

### Build Optimized Release Binary & macOS App
```bash
# Build binary
cargo build --release

# Build standalone macOS application bundle (PowerBeamer.app)
./scripts/build_macos_app.sh
```

---

## 📦 Publishing Releases

To release a new version to [GitHub Releases](https://github.com/ahmadikalkhorani/PowerBeamer/releases):

```bash
./scripts/publish_to_github.sh
```
Or tag and push manually:
```bash
git tag v0.1.0
git push origin v0.1.0
```
This triggers the multi-platform GitHub Actions release matrix for macOS (arm64 + x86_64) and Linux.
