//! Slide and Beamer-frame navigation, blackout/whiteout, screen swap, TOC and thumbnails jump.


use slint::ComponentHandle;

use super::context::AppContext;

/// Wires navigation callbacks.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext { state, presenter_window, update_slide_view, .. } = ctx;

    // --- SLIDE NAVIGATION CALLBACKS ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_next_slide(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                return;
            }
            st.blank_mode = 0;
            if st.deck.next() {
                drop(st);
                update();
            } else {
                // Reached past the last slide: display black screen
                st.is_end_of_show = true;
                st.blank_mode = 1;
                drop(st);
                update();
                println!("Reached end of presentation: displaying black screen.");
            }
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_prev_slide(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                st.is_end_of_show = false;
                st.blank_mode = 0;
                drop(st);
                update();
                println!("Returned from end-of-show black screen to last slide.");
                return;
            }
            st.blank_mode = 0;
            if st.deck.prev() {
                drop(st);
                update();
            }
        });
    }

    // --- BEAMER FRAME NAVIGATION CALLBACKS (\pause overlay jumps) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_next_frame(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                return;
            }
            st.blank_mode = 0;
            if st.deck.next_frame() {
                drop(st);
                update();
            } else if st.deck.next() {
                drop(st);
                update();
            } else {
                // Reached past the last frame: display black screen
                st.is_end_of_show = true;
                st.blank_mode = 1;
                drop(st);
                update();
                println!("Reached end of frames: displaying black screen.");
            }
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_prev_frame(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                st.is_end_of_show = false;
                st.blank_mode = 0;
                drop(st);
                update();
                return;
            }
            st.blank_mode = 0;
            if st.deck.prev_frame() {
                drop(st);
                update();
            }
        });
    }

    // --- SCREEN BLACKOUT / WHITEOUT & SCREEN SWAP CALLBACKS ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_toggle_blackout(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                st.is_end_of_show = false;
                st.blank_mode = 0;
            } else {
                st.blank_mode = if st.blank_mode == 1 { 0 } else { 1 };
            }
            drop(st);
            update();
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_toggle_whiteout(move || {
            let mut st = state.borrow_mut();
            st.blank_mode = if st.blank_mode == 2 { 0 } else { 2 };
            drop(st);
            update();
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_swap_screens(move || {
            let mut st = state.borrow_mut();
            st.is_screen_swapped = !st.is_screen_swapped;
            drop(st);
            update();
        });
    }

    // --- TOC OUTLINE DRAWER CALLBACKS ---
    {
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_toggle_toc(move || {
            if let Some(p) = presenter_handle.upgrade() {
                let cur = p.get_show_toc_drawer();
                p.set_show_toc_drawer(!cur);
            }
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_jump_to_toc(move |page_index| {
            let mut st = state.borrow_mut();
            st.is_end_of_show = false;
            st.blank_mode = 0;
            let target = if page_index > 0 { (page_index - 1) as usize } else { 0 };
            if st.deck.jump_to(target) {
                drop(st);
                update();
            }
        });
    }

    // --- JUMP TO SLIDE (Thumbnails overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_jump_to_slide(move |slide_num| {
            let mut st = state.borrow_mut();
            st.is_end_of_show = false;
            st.blank_mode = 0;
            if slide_num >= 1 && (slide_num as usize) <= st.deck.total_slides() {
                st.deck.current_index = (slide_num - 1) as usize;
                drop(st);
                update();
            }
        });
    }

    Vec::new()
}
