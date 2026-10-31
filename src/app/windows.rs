//! Audience/notes pop-out windows and live speaker-notes sync.

use slint::ComponentHandle;

use super::context::AppContext;

/// Wires window toggles and notes editing.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext {
        state,
        presenter_window,
        audience_window,
        notes_window,
        ..
    } = ctx;

    let presenter = presenter_window.as_weak();
    audience_window.window().on_close_requested(move || {
        if let Some(p) = presenter.upgrade() {
            p.set_audience_window_open(false);
        }
        slint::CloseRequestResponse::HideWindow
    });
    let presenter = presenter_window.as_weak();
    notes_window.window().on_close_requested(move || {
        if let Some(p) = presenter.upgrade() {
            p.set_notes_window_open(false);
        }
        slint::CloseRequestResponse::HideWindow
    });

    // --- MULTI-WINDOW POP-OUT TOGGLES ---
    {
        let audience_win = audience_window.as_weak();
        let presenter_win = presenter_window.as_weak();

        presenter_window.on_toggle_audience_window(move || {
            if let Some(aud) = audience_win.upgrade() {
                let is_open = !aud.window().is_visible();
                if is_open {
                    let _ = aud.show();
                    println!("Audience Window popped out to separate monitor / display.");
                } else {
                    let _ = aud.hide();
                    println!("Audience Window docked / hidden.");
                }
                if let Some(pres) = presenter_win.upgrade() {
                    pres.set_audience_window_open(is_open);
                }
            }
        });
    }

    {
        let notes_win = notes_window.as_weak();
        let presenter_win = presenter_window.as_weak();

        presenter_window.on_toggle_notes_window(move || {
            if let Some(notes) = notes_win.upgrade() {
                let is_open = !notes.window().is_visible();
                if is_open {
                    let _ = notes.show();
                    println!("Speaker Notes window popped out to floating monitor.");
                } else {
                    let _ = notes.hide();
                    println!("Speaker Notes window closed.");
                }
                if let Some(pres) = presenter_win.upgrade() {
                    pres.set_notes_window_open(is_open);
                }
            }
        });
    }

    // --- SPEAKER COMMENTS & NOTES REAL-TIME SYNC ---
    {
        let state = state.clone();
        let notes_handle = notes_window.as_weak();
        presenter_window.on_update_slide_notes(move |slide_num, new_notes| {
            let mut st = state.borrow_mut();
            let idx = (slide_num - 1) as usize;
            if idx < st.deck.slides.len() {
                st.deck.slides[idx].notes = new_notes.to_string();
                if let Some(n) = notes_handle.upgrade() {
                    if idx == st.deck.current_index {
                        n.set_notes_body(new_notes);
                    }
                }
            }
        });
    }

    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        notes_window.on_update_slide_notes(move |slide_num, new_notes| {
            let mut st = state.borrow_mut();
            let idx = (slide_num - 1) as usize;
            if idx < st.deck.slides.len() {
                st.deck.slides[idx].notes = new_notes.to_string();
                if let Some(p) = presenter_handle.upgrade() {
                    if idx == st.deck.current_index {
                        p.set_notes_text(new_notes);
                    }
                }
            }
        });
    }

    // Each preview has its own native, freely resizable window. Keep strong handles
    // in the sync timer so closing a preview hides it without destroying it.
    let next = crate::PreviewWindow::new().expect("next-slide window");
    let beamer = crate::PreviewWindow::new().expect("Beamer notes window");
    next.set_heading("Next slide".into());
    beamer.set_heading("Presenter speaker note".into());
    for (window, is_beamer) in [(&next, false), (&beamer, true)] {
        let presenter = presenter_window.as_weak();
        window.window().on_close_requested(move || {
            if let Some(p) = presenter.upgrade() {
                if is_beamer {
                    p.set_beamer_detached(false);
                } else {
                    p.set_next_detached(false);
                }
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    let next_handle = next.as_weak();
    let beamer_handle = beamer.as_weak();
    let presenter = presenter_window.as_weak();
    presenter_window.on_toggle_preview_window(move |kind| {
        let window = if kind == 0 {
            next_handle.upgrade()
        } else {
            beamer_handle.upgrade()
        };
        if let (Some(w), Some(p)) = (window, presenter.upgrade()) {
            let open = !w.window().is_visible();
            if kind == 0 {
                w.set_picture(p.get_next_slide_image());
                p.set_next_detached(open);
            } else {
                w.set_picture(p.get_notes_slide_image());
                p.set_beamer_detached(open);
            }
            if open {
                let _ = w.show();
            } else {
                let _ = w.hide();
            }
        }
    });
    notes_window.set_notes_font_size(presenter_window.get_notes_font_size());
    for from_detached in [false, true] {
        let state = state.clone();
        let presenter = presenter_window.as_weak();
        let notes = notes_window.as_weak();
        let resize = move |size: i32| {
            let size = size.clamp(8, 48);
            let mut st = state.borrow_mut();
            st.notes_font_size = size;
            st.capture_settings().save_background();
            if let Some(p) = presenter.upgrade() {
                p.set_notes_font_size(size);
            }
            if let Some(n) = notes.upgrade() {
                n.set_notes_font_size(size);
            }
        };
        if from_detached {
            notes_window.on_resize_notes_font(resize);
        } else {
            presenter_window.on_resize_notes_font(resize);
        }
    }
    let sync = slint::Timer::default();
    let presenter = presenter_window.as_weak();
    let notes = notes_window.as_weak();
    sync.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            if let Some(p) = presenter.upgrade() {
                for (window, visible) in [
                    (&next, p.get_next_detached() && p.get_show_next_panel()),
                    (
                        &beamer,
                        p.get_beamer_detached() && p.get_show_beamer_panel(),
                    ),
                ] {
                    if visible != window.window().is_visible() {
                        if visible {
                            let _ = window.show();
                        } else {
                            let _ = window.hide();
                        }
                    }
                }
                if let Some(n) = notes.upgrade() {
                    let visible = p.get_notes_window_open() && p.get_show_text_panel();
                    if visible != n.window().is_visible() {
                        if visible {
                            let _ = n.show();
                        } else {
                            let _ = n.hide();
                        }
                    }
                }
                if next.window().is_visible() {
                    next.set_picture(p.get_next_slide_image());
                }
                if beamer.window().is_visible() {
                    beamer.set_picture(p.get_notes_slide_image());
                }
            }
        },
    );
    vec![sync]
}
