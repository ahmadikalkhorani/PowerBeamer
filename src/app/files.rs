//! Opening, background loading, source-PDF sync and annotated export.

use std::time::Duration;

use chrono::Local;
use slint::{ComponentHandle, SharedString};

use super::context::AppContext;
use super::gpu_images::*;
use super::state::*;
use super::ui::*;
use crate::pdf::PdfExporter;

/// Wires file callbacks and the loader/sync timers.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext { state, presenter_window, update_slide_view, .. } = ctx;

    // --- BACKGROUND DECK LOADING (open / CLI / source sync) ---
    // Rendering runs on a worker thread; this timer installs the result on the UI thread.
    let deck_load_timer = slint::Timer::default();
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        deck_load_timer.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || {
            let mut st = state.borrow_mut();
            let Some(job) = st.deck_load.as_ref() else { return; };
            let result = match job.rx.try_recv() {
                Ok(result) => result,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("loader thread stopped unexpectedly".to_string()),
            };
            let job = st.deck_load.take().unwrap();
            let toast = match (result, job.kind) {
                (Ok((slides, toc)), DeckLoadKind::Open) => {
                    println!("Loaded {} slides and {} TOC entries from {}", slides.len(), toc.len(), job.path.display());
                    st.install_deck(slides, toc, job.path);
                    None
                }
                (Ok((slides, toc)), DeckLoadKind::Sync) => {
                    st.deck.merge_reloaded(slides, toc);
                    st.on_deck_contents_changed();
                    Some(("Slides updated from the source PDF".to_string(), false))
                }
                (Ok((slides, toc)), DeckLoadKind::Requality) => {
                    st.deck.merge_reloaded(slides, toc);
                    st.on_deck_contents_changed();
                    Some((format!("Slides re-rendered at {}", st.deck.quality.label()), false))
                }
                (Err(err), DeckLoadKind::Requality) => Some((format!("Couldn't re-render the slides: {}", err), true)),
                (Err(err), DeckLoadKind::Open) => Some((format!("Couldn't open {}: {}", job.path.display(), err), true)),
                // Keep showing the last good version (e.g. LaTeX wrote a broken PDF); the next
                // successful rebuild changes the file stamp and triggers another attempt.
                (Err(err), DeckLoadKind::Sync) => {
                    Some((format!("Source PDF changed but couldn't be reloaded: {}", err), true))
                }
            };
            let reloaded = toast.as_ref().is_none_or(|(_, is_err)| !is_err);
            let synced = matches!(job.kind, DeckLoadKind::Sync) && reloaded;
            drop(st);
            if reloaded {
                update();
            }
            if let Some(p) = presenter_handle.upgrade() {
                p.set_deck_loading(false);
                if reloaded {
                    update_slide_thumbnails_model(&state.borrow().deck, &p);
                }
                if synced {
                    p.set_sync_status_text(SharedString::from(format!("Last synced {}", Local::now().format("%H:%M:%S"))));
                }
                match toast {
                    Some((msg, is_err)) => show_toast(&p, msg, is_err),
                    None => p.set_toast_visible(false),
                }
            }
        });
    }

    // --- SOURCE PDF SYNC (reload when the original PDF is rebuilt, e.g. by latexmk) ---
    let source_sync_timer = slint::Timer::default();
    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        source_sync_timer.start(slint::TimerMode::Repeated, Duration::from_millis(1000), move || {
            let mut st = state.borrow_mut();
            if !st.sync_with_source || st.deck_load.is_some() {
                return;
            }
            let Some(path) = st.deck.file_path.clone() else { return; };
            // Missing file usually means LaTeX is mid-rewrite; check again next tick.
            let Some(stamp) = file_stamp(&path) else { return; };
            if st.source_stamp == Some(stamp) {
                st.pending_source_stamp = None;
                return;
            }
            // Wait until the file stops changing for one tick so we never read a half-written PDF,
            // and don't swap slides out from under an in-progress stroke.
            if st.pending_source_stamp != Some(stamp) || st.active_stroke.is_some() {
                st.pending_source_stamp = Some(stamp);
                return;
            }
            st.pending_source_stamp = None;
            st.source_stamp = Some(stamp);
            st.start_deck_load(path, DeckLoadKind::Sync);
            drop(st);
            if let Some(p) = presenter_handle.upgrade() {
                p.set_deck_loading(true);
            }
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_sync_with_source(move |enabled| {
            // Turning sync back on picks up any rebuild that happened while it was off.
            state.borrow_mut().sync_with_source = enabled;
        });
    }

    // --- OPEN PDF & EXPORT PDF ---
    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_open_pdf(move || {
            let picked = rfd::FileDialog::new()
                .add_filter("PDF Presentations", &["pdf"])
                .set_title("Open Beamer LaTeX PDF Presentation")
                .pick_file();

            if let Some(path) = picked {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                state.borrow_mut().start_deck_load(path, DeckLoadKind::Open);
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_deck_loading(true);
                    show_toast(&p, format!("Opening {}…", name), false);
                }
            }
        });
    }


    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_export_pdf(move || {
            let dest = rfd::FileDialog::new()
                .add_filter("PDF Presentation", &["pdf"])
                .set_file_name("presentation_annotated.pdf")
                .set_title("Save Presentation Annotations")
                .save_file();

            if let Some(path) = dest {
                let st = state.borrow();
                match PdfExporter::export_annotated_presentation(&st.deck, &path) {
                    Ok(msg) => {
                        if let Some(p) = presenter_handle.upgrade() {
                            show_toast(&p, format!("Exported: {}", msg), false);
                        }
                    }
                    Err(err) => {
                        if let Some(p) = presenter_handle.upgrade() {
                            show_toast(&p, format!("Export failed: {}", err), true);
                        }
                    }
                }
            }
        });
    }

    vec![deck_load_timer, source_sync_timer]
}
