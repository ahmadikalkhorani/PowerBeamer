# Controls and shortcuts audit — 2026-10-30

## Fixes

- Tab now toggles the page overview (T remains an alias). S now invokes screen swap, as documented.
- Ctrl/Cmd combinations no longer fall through to single-letter presentation actions. Undo/redo handle uppercase key text too.
- The presenter keyboard scope now encloses the controls, so it can receive unhandled events after focus moves. Note editing keeps normal typing behavior. Escape closes the notes drawer and restores presentation focus.
- Dotted, grid and ruled page dimensions scale with the source raster. The dotted template uses antialiased circles that remain visible when a high-DPI page is reduced to the presentation viewport. Source page resolution is retained.
- Pen and highlighter restore their own sizes when selected. Pen size is saved independently of the active highlighter size. Returning from the eraser uses the tool-selection callback.
- Color changes and wheel/brightness edits update the selected saved slot rather than stale slot 1.
- Precision erasing clips strokes at the circular boundary, including a line crossing the eraser with both endpoints outside it. Whole-line mode removes the complete stroke. Both support undo/redo and the existing type filters.
- Opening audience/notes windows no longer refreshes the entire slide and interrupts playback. Titlebar close updates the toolbar state, and the next toggle reopens the window.
- Audio-only playback now receives mute and speed settings. Audio loops restart playback rather than just resetting the timer. Video completion pauses shared state and both views. Playing a slide without media no longer leaves a false playing state.
- Deleting a page before the current page preserves the current page's position in the remaining deck.
- Selecting the zoom tool resets zoom in both windows as well as application state.

## Verification

All 76 presenter callback declarations have a Rust handler. This is a wiring check, not proof of behavior.

`cargo test --offline --lib --tests` exercises 93 tests. The headless UI test uses the real compiled Slint components, event routing, software rendering and application callbacks. Coverage added to that test includes Tab key dispatch, overview, tool selection, independent pen/highlighter sizes, colors 1–5, modifier isolation, navigation keys, frame navigation, blackout/whiteout, TOC/Escape, notes drawer/Escape, screen swap, popout toggles/titlebar close/reopen, absence of media, tool settings, timer controls, notes synchronization, page insertion/duplication/rotation/deletion, whiteboard insertion and drawing/undo/redo. It also retains the asynchronous PPTX load check.

Additional regression tests check precision versus whole-stroke erasing with exact undo/redo, audio tempo filter factors, and dotted/grid/ruled/blank pages on light and dark backgrounds at 1600px and 6400px source widths, reduced to 800px. Dotted previews are written to `target/perf-results/dotted-*.png` and were visually inspected.

Existing tests cover PDF/media parsing, Beamer frames, shapes, laser trails, ink rendering, session serialization, export and other model behavior. They do not all constitute native UI interaction tests.

## Manual coverage still required

Physical multi-monitor placement/fullscreen behavior, native GPU rendering, audible playback/device latency, native file dialogs, source-file reveal, live download/install/restart of updates, and browser opening were not exercised end to end. The updater installation was not triggered. PDF/PPTX loading tests use fixtures and a prepared converted PPTX companion PDF; they do not automate an open Office document.
