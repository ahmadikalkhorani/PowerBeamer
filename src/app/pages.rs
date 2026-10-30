//! Page management: insert, delete, duplicate, rotate and reorder slides.

use std::sync::Arc;

use slint::ComponentHandle;

use super::context::AppContext;
use super::gpu_images::*;

/// Wires page-management callbacks.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext {
        state,
        presenter_window,
        update_slide_view,
        ..
    } = ctx;

    // --- ADD WHITEBOARD PAGE (+) / (N) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_add_whiteboard_page(move || {
            let mut st = state.borrow_mut();
            st.is_end_of_show = false;
            st.blank_mode = 0;
            st.deck.insert_whiteboard_slide(true); // Blackboard with grid dots
            println!(
                "Inserted new whiteboard scratchpad page at index {}",
                st.deck.current_index + 1
            );
            drop(st);
            update();
            if let Some(p) = presenter_handle.upgrade() {
                let st = state.borrow();
                update_slide_thumbnails_model(&st.deck, &p);
            }
        });
    }

    // --- DELETE SLIDE (Trash icon in Page Overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_delete_slide(move |idx| {
            let mut st = state.borrow_mut();
            if st.deck.slides.len() > 1 {
                let remove_idx = (idx - 1).clamp(0, st.deck.slides.len() as i32 - 1) as usize;
                st.deck.slides.remove(remove_idx);
                if remove_idx < st.deck.current_index { st.deck.current_index -= 1; }
                // Re-index slides
                for (i, s) in st.deck.slides.iter_mut().enumerate() {
                    s.page_index = i + 1;
                }
                if st.deck.current_index >= st.deck.slides.len() {
                    st.deck.current_index = st.deck.slides.len() - 1;
                }
                drop(st);
                update();
                if let Some(p) = presenter_handle.upgrade() {
                    let st = state.borrow();
                    update_slide_thumbnails_model(&st.deck, &p);
                }
            }
        });
    }

    // --- DUPLICATE / COPY SLIDE (Copy icon in Page Overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_duplicate_slide(move |idx| {
            let mut st = state.borrow_mut();
            let copy_idx = (idx - 1).clamp(0, st.deck.slides.len() as i32 - 1) as usize;
            let mut cloned = st.deck.slides[copy_idx].clone();
            cloned.title = format!("{} (Copy)", cloned.title);
            let insert_idx = copy_idx + 1;
            st.deck.slides.insert(insert_idx, cloned);
            for (i, s) in st.deck.slides.iter_mut().enumerate() {
                s.page_index = i + 1;
            }
            st.deck.current_index = insert_idx;
            drop(st);
            update();
            if let Some(p) = presenter_handle.upgrade() {
                let st = state.borrow();
                update_slide_thumbnails_model(&st.deck, &p);
            }
        });
    }

    // --- ROTATE SLIDE (Rotate icon in Page Overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_rotate_slide(move |idx| {
            let mut st = state.borrow_mut();
            let target_idx = (idx - 1).clamp(0, st.deck.slides.len() as i32 - 1) as usize;
            let slide = &mut st.deck.slides[target_idx];
            let rotated = Arc::new(crate::pdf::SlideImage::new(image::imageops::rotate90(
                &*slide.audience_image.pixels(),
            )));
            slide.presenter_image = if Arc::ptr_eq(&slide.presenter_image, &slide.audience_image) {
                rotated.clone()
            } else {
                Arc::new(crate::pdf::SlideImage::new(image::imageops::rotate90(
                    &*slide.presenter_image.pixels(),
                )))
            };
            slide.audience_image = rotated;
            slide.aspect_ratio =
                slide.audience_image.width() as f32 / slide.audience_image.height() as f32;
            drop(st);
            update();
            if let Some(p) = presenter_handle.upgrade() {
                let st = state.borrow();
                update_slide_thumbnails_model(&st.deck, &p);
            }
        });
    }

    // --- INSERT CONFIGURED PAGE (From Insert Page Modal) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_insert_page_configured(move |pos_mode, template_kind, bg_color| {
            let r = bg_color.red();
            let g = bg_color.green();
            let b = bg_color.blue();
            let a = bg_color.alpha();
            let bg_rgba = [r, g, b, a];

            let mut st = state.borrow_mut();
            let new_idx = st.deck.insert_page_with_template(
                pos_mode as usize,
                template_kind.as_str(),
                bg_rgba,
            );
            println!(
                "Inserted new page (template: '{}', bg: {:?}, pos_mode: {}) at index {}",
                template_kind,
                bg_rgba,
                pos_mode,
                new_idx + 1
            );
            drop(st);
            update();
            if let Some(p) = presenter_handle.upgrade() {
                let st = state.borrow();
                update_slide_thumbnails_model(&st.deck, &p);
            }
        });
    }

    // --- REORDER / MOVE SLIDE (Move Left / Move Right in Page Overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_move_slide(move |from_slide, to_slide| {
            let mut st = state.borrow_mut();
            let total = st.deck.slides.len() as i32;
            if from_slide >= 1
                && from_slide <= total
                && to_slide >= 1
                && to_slide <= total
                && from_slide != to_slide
            {
                let from_idx = (from_slide - 1) as usize;
                let to_idx = (to_slide - 1) as usize;
                if st.deck.move_slide(from_idx, to_idx) {
                    println!("Reordered slide {} -> {}", from_slide, to_slide);
                    drop(st);
                    update();
                    if let Some(p) = presenter_handle.upgrade() {
                        let st = state.borrow();
                        update_slide_thumbnails_model(&st.deck, &p);
                    }
                }
            }
        });
    }

    Vec::new()
}
