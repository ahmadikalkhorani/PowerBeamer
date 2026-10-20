//! Audience/notes pop-out windows and live speaker-notes sync.


use slint::ComponentHandle;

use super::context::AppContext;

/// Wires window toggles and notes editing.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext { state, presenter_window, audience_window, notes_window, update_slide_view, .. } = ctx;

    // --- MULTI-WINDOW POP-OUT TOGGLES ---
    {
        let audience_win = audience_window.as_weak();
        let presenter_win = presenter_window.as_weak();
        let mut is_open = false;

        presenter_window.on_toggle_audience_window(move || {
            if let Some(aud) = audience_win.upgrade() {
                is_open = !is_open;
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
        let update_view = update_slide_view.clone();
        let mut is_open = false;

        presenter_window.on_toggle_notes_window(move || {
            if let Some(notes) = notes_win.upgrade() {
                is_open = !is_open;
                if is_open {
                    update_view();
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

    Vec::new()
}
