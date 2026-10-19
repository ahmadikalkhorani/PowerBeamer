# PowerBeamer 📽️

[![CI & Release](https://github.com/ahmadikalkhorani/PowerBeamer/actions/workflows/release.yml/badge.svg)](https://github.com/ahmadikalkhorani/PowerBeamer/actions/workflows/release.yml)
[![GitHub Release](https://img.shields.io/github/v/release/ahmadikalkhorani/PowerBeamer?color=blue)](https://github.com/ahmadikalkhorani/PowerBeamer/releases)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-green)](LICENSE)

A high-performance, low-footprint desktop presentation studio purpose-built for **LaTeX Beamer PDF presentations**. Engineered in **Rust** with **Slint / WGPU** and **Google PDFium** for instant sub-millisecond slide transitions, fluid vector inking, native Beamer dual-screen auto-detection, speaker notes extraction, and modular multi-window docking.

---

## 📥 Download & Installation

Pre-compiled standalone packages are available under **[GitHub Releases](https://github.com/ahmadikalkhorani/PowerBeamer/releases/latest)**:

- 🍏 **macOS (Apple Silicon M1/M2/M3/M4)**: Download `PowerBeamer-macos-arm64.zip`, extract it, and drag `PowerBeamer.app` into your `/Applications` folder.
- 💻 **macOS (Intel x86_64)**: Download `PowerBeamer-macos-x86_64.tar.gz`.
- 🪟 **Windows (x86_64 / 64-bit)**: Download `PowerBeamer-windows-x86_64.zip`, extract the folder, and double-click `powerbeamer.exe` (bundles `pdfium.dll`, standalone portable).
- 🐧 **Linux (x86_64)**: Download `PowerBeamer-linux-x86_64.tar.gz`, extract the archive, and run `./powerbeamer`.

> **Tip:** PowerBeamer includes an automatic in-app updater. Once installed, future updates can be installed with a single click from the Options menu (`•••`).

### 🍏 macOS First-Launch Note (Gatekeeper / "App is damaged")

Because PowerBeamer is an independent open-source release, macOS Gatekeeper attaches a quarantine flag to internet-downloaded apps, often displaying:
> *"PowerBeamer is damaged and can't be opened. You should move it to the Trash."*

To open the app, run this single command in your **Terminal**:
```bash
xattr -d com.apple.quarantine /Applications/PowerBeamer.app
```
*(Or if the app is still in your Downloads folder: `xattr -d com.apple.quarantine ~/Downloads/PowerBeamer.app`)*

> **Why does this happen?**
> On **macOS Sequoia (15+)**, Apple removed the *"Open Anyway"* bypass button in System Settings for web-downloaded open-source apps. Running the `xattr -d com.apple.quarantine` command removes the browser download flag and permits macOS to launch PowerBeamer immediately.

---

## ✨ Key Features

- **🚀 Ultra-Fast & Low Resource Consumption**:
  - Native compiled Rust binary (< 50MB RAM baseline, zero Electron/Chromium runtime).
  - Sub-10ms inking latency with Catmull-Rom spline smoothing.
- **🎨 Vector Inking & Annotation Suite**:
  - **✍️ Fountain Pen (`P`)**: Pressure-sensitive vector inking with customizable colors and line weights.
  - **🖍️ Highlighter (`H`)**: Multiply blend mode preserving crisp black LaTeX text beneath highlights.
  - **▱ Eraser (`E`)**: Real-time distance-to-segment collision eraser.
  - **⚡ Laser Pointer (`L`)**: Dynamic glowing cursor with decaying trail mirrored live to audience.
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
  - Exports annotated slides as high-resolution PDFs and non-destructive `.presnotes.json` sidecar session files.
  - Configurable rendering quality: **Eco (150 DPI)**, **Balanced (300 DPI)**, and **Ultra Vector (600 DPI)**.

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `L` | Activate Laser Pointer |
| `P` | Activate Pen Tool |
| `H` | Activate Highlighter |
| `E` | Activate Eraser |
| `Space` / `→` / `↓` / `J` | Next Slide |
| `←` / `↑` / `K` | Previous Slide |
| `N` or `+` | Insert New Whiteboard / Scratchpad Slide |
| `Cmd/Ctrl + Z` | Undo |
| `Cmd/Ctrl + Shift + Z` | Redo |

---

## 🛠️ Building from Source

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
# Build release binary
cargo build --release

# Build standalone macOS application bundle (PowerBeamer.app)
./scripts/build_macos_app.sh
```

---

## 🙏 Acknowledgments & Built With

PowerBeamer is built on the shoulders of fantastic open-source projects:

- **[Slint](https://slint.dev/)**: The next-generation declarative GUI toolkit and rendering engine powering the ultra-fluid, multi-window presentation interface.
- **[Google PDFium](https://pdfium.googlesource.com/pdfium/)** & **[pdfium-render](https://github.com/ajrcore/pdfium-render)**: Fast, rock-solid vector PDF rasterization and outline parsing.
- **[The Rust Community](https://www.rust-lang.org/)**: For foundational crates including `image`, `serde`, `chrono`, `rfd`, `ureq`, `flate2`, and `tar`.
- **[LaTeX Beamer](https://github.com/josephwright/beamer)**: Standardizing academic slide formats and `pgfpages` dual-screen presentations.

---

## 📄 License

PowerBeamer is released under the [MIT License](LICENSE). Third-party libraries retain their respective copyrights and licenses.
