# Performance audit — 2026-10-30

The baseline is a source snapshot taken before these changes, including the pre-existing working-tree edits. Measurements use the same fixtures, machine, compiler, and release profile. Values are medians of three fresh processes unless marked otherwise.

## What changed

- Slide images keep exact RGBA pixels in private temporary-file mappings. A 64 MiB LRU holds decoded heap buffers; audience and presenter share the same asset. Audience thumbnails are prepared by the loader, while unused note thumbnails are omitted. The fixed demo stays resident. Temporary-file failure falls back to RAM rather than reducing quality.
- Undo/redo moves added/deleted strokes between the live document and operation history. It no longer snapshots all points after each edit, and history remains unlimited.
- Laser, shape, and audio timers sleep when inactive. Source polling sleeps without an enabled source. Loader completion is delivered through the event loop, replacing its permanent poll.
- Drawing renders at actual window redraws on supported backends, with a zero-delay event-loop fallback. Every input point is retained. No 60 Hz cap is introduced. The original rasterizer, highlighter blending, full overlay image, and image sampling remain in use.
- One deck worker and one pending request replace unbounded concurrent loaders. PDF cancellation is checked between pages. Stale completions cannot replace a newer deck. PowerPoint conversion already in progress must finish before the pending request can run.
- Companion TeX is parsed once per load. Path lookups, including failed lookups, are cached for the load.
- Video decodes directly into two reusable Slint pixel buffers, with copy-on-write protection for frames still displayed. A latest-frame mailbox bounds the frame queue to one notification and one pending frame. Paused seeking uses a debounced worker and generation checks. Playback is paced once, at the existing 30 displayed FPS. The audio process explicitly disables video decoding.
- Export uses a worker and a consistent snapshot that omits undo history. Sidecar JSON is streamed. Pages without ink avoid transparent-overlay/composite allocations; output-write failures are reported.
- Overview cards are instantiated only for visible rows plus overscan. The scroll extent remains the entire deck. A persistent thumbnail model updates changed rows. Prewarming uses one cancellable timer; display-image eviction also runs immediately during rapid navigation.
- Settings writes use a latest-value worker, retain the existing 500 ms change check, and flush the final settings at shutdown. Duplicate/in-flight media duration requests share a single probe worker. Unchanged held strokes are recognized only once. Updater progress is limited to 10 notifications/sec plus completion.
- Startup creates one demo deck. The local macOS bundle packages a release binary. The web preview lazily loads overview images and coalesces resize events.

## Measurements

Measured on macOS arm64 with rustc 1.99.0, thin LTO and one release codegen unit. Raw records: [measurements.json](measurements.json).

| Workload | Before | After | Change |
|---|---:|---:|---:|
| Startup demo construction | 14.260 ms | 7.494 ms | 47.4% less |
| Startup peak heap | 54.94 MiB | 27.47 MiB | 50.0% less |
| 500-stroke undo history retained heap | 581.65 MiB | 2.98 MiB | 99.5% less |
| 500 adds + 500 undos + 500 redos | 178.893 ms | 0.849 ms | 99.5% less |
| 140 slides with notes retained heap | 2143.26 MiB | 31.10 MiB | 98.5% less |
| 140 slides with notes load time | 3054.054 ms | 3564.229 ms | 16.7% more |
| 2,000-slide overview heap | 26.79 MiB | 4.87 MiB | 81.8% less |
| Overview setup + software snapshot | 54.803 ms | 21.751 ms | 60.3% less |
| 240 drawing events, two events per displayed frame | 74.432 ms | 59.504 ms | 20.1% less |
| 300 companion-media resolutions | 275.180 ms | 1.348 ms | 99.5% less |
| Demo construction + export of four unannotated pages | 27.070 ms | 13.823 ms | 48.9% less |
| Demo + export peak heap | 38.53 MiB | 27.55 MiB | 28.5% less |
| Deck peak RSS, one macOS counter run | 1579.66 MiB | 82.77 MiB | 94.8% less |
| Deck peak OS memory footprint, one counter run | 2186.42 MiB | 84.88 MiB | 96.1% less |
| 300 video-frame buffer handoffs | 51.485 ms | 6.564 ms | 87.3% less |
| Video-buffer benchmark peak heap | 3.52 MiB | 2.64 MiB | 25.0% less |
| Idle feature-timer callbacks over 10 simulated seconds | 1,850 | 0 | 100.0% less |
| Same ink workload: development build → release build | 421.225 ms | 75.609 ms | 82.1% less |

The allocation counters cover Rust-owned heap buffers, not GPU allocations or the entire operating system's page cache. Separate macOS process counters provide peak RSS and memory footprint for the deck workload.

The deck workload retains 20 independently loaded copies of the seven-slide PDF, including its note images: 140 slides. It intentionally exposes retained-memory scaling; it is not a claim about every presentation. The drawing workload retains 240 input points and compares one render per event with two events per displayed frame. Hardware refresh rate and event arrival affect real-world savings.

Disk-backed pixels trade temporary disk space and first-load work for substantially less retained memory. Raw backing uses four bytes per rendered pixel, including notes. OS file-cache pages remain reclaimable under memory pressure. The 64 MiB budget applies to retained pixel-cache entries, not all app memory, display images, or buffers currently borrowed by a worker.

## Quality and verification

- All 42 rendered PNGs (audience and notes across 150, 300, and 600 DPI) are compared byte-for-byte against the baseline.
- Lossless backing is compared to original RGBA data across repeated cache eviction, including nonopaque pixels and shared owners.
- Coalesced ink retains every point and produces the same final pixels for pen and highlighter strokes. Real presenter pointer callbacks are exercised with the compiled Slint UI.
- The headless overview paints 10,000 slides, scrolls, and verifies that clicking recycled cards uses global slide indices.
- Tests cover mixed undo/redo/erase/clear edits, cancelled PDF loads, bounded worker backlogs, duplicate probes, bounded video notifications, and playback cadence at 0.5×, 1×, and 2×.

A tiled-image upload experiment was rejected: CPU raster pixels matched, but Slint sampled separate tiles differently at fractional display sizes. It is not shipped and its experimental savings are not included. Full overlay uploads still occur once per rendered ink frame.

The native GPU backend, projector hardware, battery use, and external PowerPoint/LibreOffice conversions were not profiled end-to-end. The overview measurements use the real Slint components with a headless software renderer. The video copy benchmark isolates pixel-buffer handling and does not claim equivalent savings for total FFmpeg CPU use.

## Reproduce

Preserve a pre-change checkout or source snapshot, then run from the optimized tree:

```sh
python3 scripts/measure_performance.py --baseline /path/to/before --runs 3
cargo test --offline --lib --tests
node --check web/app.js
bash -n scripts/build_macos_app.sh
```

The script copies two compatible measurement runners into the baseline's `examples` directory and builds both versions offline. Distinct runner names prevent shared Cargo output paths from mixing baseline and optimized executables. Results and render comparisons go to `target/perf-results`. Add `--rss` for macOS process counters when those counters are permitted by the execution environment.

## Responsiveness and smaller changes

Paused frame extraction took **111.795 ms synchronously** versus **0.002 ms to submit** to the seek worker. The preview completed in **179.680 ms**, including the deliberate 60 ms debounce. This removes the UI stall rather than shortening a single seek's end-to-end latency. Extracted preview pixels are compared for equality.

The same optimized export took **9.125 ms synchronously** versus **0.027 ms to snapshot and dispatch**; the worker completed in **7.955 ms** in this small warm-cache workload. The separate export table includes demo construction and measures the combined allocation/export changes.

Drawing published **1,382,400,000 bytes → 691,200,000 bytes** in the controlled two-events-per-frame workload, a 50% reduction. There is no image downsampling or input-point deletion. The video-buffer workload is an in-memory read/copy test over 300 frames, including a fixture input buffer in both peak-heap results; it excludes decoding, GPU upload, audio, and rendering. Its improvement is not a total-video-playback CPU percentage.

The following smaller changes have operation-count or correctness checks rather than isolated whole-app CPU/RAM measurements:

| Change | Evidence / limit |
|---|---|
| Superseded deck loads | A controlled burst of 1,001 submissions executes only the active request and the newest pending request (2 jobs); a cancelled real PDF load stops before rendering. |
| Video UI backlog | 10,000 publications while the UI is blocked retain one latest frame and request one UI notification. |
| Duplicate duration probes | 100 requests for the same real clip cause one probe. |
| Settings | Disk writes run on one worker, queued values coalesce, and shutdown flushes the final value. File contents and atomic rename behavior are retained. No isolated I/O-time claim. |
| Thumbnail preparation and model | Prepared off the UI thread as part of the measured loader; overview and real UI scroll/click tests cover the resulting model. The model instance survives updates. |
| Prewarming | One timer replaces pending work during rapid navigation. A real UI test advances through 100 distinct image assets without idle ticks and checks that the display cache stays bounded. No isolated CPU/RAM number. |
| Held shape recognition | An unchanged stroke signature skips repeated recognition; stroke changes still permit another attempt. No isolated CPU-time number. |
| Updater | Progress is rate-limited to at most 10 notifications/sec plus completion. No real update was downloaded or installed for this measurement. |
| Web preview | Lazy overview images and coalesced resize callbacks; JavaScript syntax checked. No separate browser CPU/RAM measurement. |

**Validation:** 88 test executions passed with `cargo test --offline --lib --tests`, including the unchanged shape-recognition timing threshold in a quiet run. All-target compilation, the release desktop binary build, JavaScript syntax, and shell syntax were also checked. GPU and battery savings are intentionally not extrapolated from these results.

## PPTX debug-loading correction

A follow-up runtime stack sample showed the PPTX loader had finished conversion and was spending time in thumbnail generation. The generic image thumbnail helper was instantiated in the unoptimized application crate during `cargo run`, despite dependencies being optimized. Thumbnail generation now calls `DynamicImage::thumbnail_exact`, a non-generic entry point compiled in the optimized image dependency, and retains the same raster algorithm and pixels. The original full-resolution buffer is moved through the wrapper without copying.

In one debug-build run for a 4,000 × 2,250 RGBA page, the original thumbnail call took **645.1 ms**; the replacement's complete slide-asset creation (thumbnail plus disk backing) took **47.3 ms**. This is a separate regression benchmark, not a rerun of the release measurements above. Run `cargo run --offline --example thumbnail_probe` to repeat it.

The loading notification now remains visible until the operation finishes. A regression test opens a real PPTX with a prepared three-page, PowerPoint-sized PDF companion through the actual background loader and event-loop delivery, then checks that the previous deck is replaced, PowerPoint titles are applied, and the loading flag clears. It also checks that loading status remains painted after the ordinary four-second toast timer expires. This test avoids modifying the user's running Office session; it does not automate a new Office conversion.

Follow-up validation: all 89 test executions passed, including exact thumbnail/source-pixel comparisons and the asynchronous PPTX open test.
