//! Audience/notes pop-out windows and live speaker-notes sync.

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;

use super::context::AppContext;
use crate::NoteSegment;

pub(crate) fn parse_notes_to_segments(text: &str) -> ModelRc<NoteSegment> {
    let mut segments = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('•')
            || trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || (trimmed.chars().next().map_or(false, |c| c.is_ascii_digit())
                && trimmed.contains(". "))
        {
            segments.push(NoteSegment {
                text: SharedString::from(trimmed),
                is_bullet: true,
            });
        } else {
            let mut current = String::new();
            let mut chars = trimmed.chars().peekable();
            while let Some(c) = chars.next() {
                current.push(c);
                if (c == '.' || c == '!' || c == '?')
                    && (chars.peek().map_or(true, |next| next.is_whitespace()))
                {
                    let seg = current.trim().to_string();
                    if !seg.is_empty() {
                        segments.push(NoteSegment {
                            text: SharedString::from(seg),
                            is_bullet: false,
                        });
                    }
                    current.clear();
                }
            }
            let remaining = current.trim().to_string();
            if !remaining.is_empty() {
                segments.push(NoteSegment {
                    text: SharedString::from(remaining),
                    is_bullet: false,
                });
            }
        }
    }
    ModelRc::new(Rc::new(VecModel::from(segments)))
}

/// Wires window toggles and notes editing.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext {
        state,
        presenter_window,
        audience_window,
        notes_window,
        ..
    } = ctx;

    audience_window.set_borderless(state.borrow().audience_borderless);
    audience_window.set_keep_on_top(state.borrow().audience_always_on_top);
    presenter_window.set_audience_borderless(state.borrow().audience_borderless);
    presenter_window.set_audience_always_on_top(state.borrow().audience_always_on_top);

    #[cfg(target_os = "macos")]
    if state.borrow().audience_borderless {
        apply_macos_audience_titlebar_hidden(&audience_window.window(), true);
    }

    {
        let audience_win = audience_window.as_weak();
        audience_window.on_start_native_window_drag(move || {
            #[cfg(target_os = "macos")]
            if let Some(aud) = audience_win.upgrade() {
                perform_macos_window_drag(&aud.window());
            }
        });
    }

    {
        let state = state.clone();
        let audience = audience_window.as_weak();
        let presenter = presenter_window.as_weak();
        presenter_window.on_set_audience_borderless(move |enabled| {
            state.borrow_mut().audience_borderless = enabled;
            if let Some(a) = audience.upgrade() {
                a.set_borderless(enabled);
                #[cfg(target_os = "macos")]
                apply_macos_audience_titlebar_hidden(&a.window(), enabled);
            }
            if let Some(p) = presenter.upgrade() {
                p.set_audience_borderless(enabled);
            }
        });
    }
    {
        let state = state.clone();
        let audience = audience_window.as_weak();
        let presenter = presenter_window.as_weak();
        presenter_window.on_set_audience_always_on_top(move |enabled| {
            state.borrow_mut().audience_always_on_top = enabled;
            if let Some(a) = audience.upgrade() {
                a.set_keep_on_top(enabled);
            }
            if let Some(p) = presenter.upgrade() {
                p.set_audience_always_on_top(enabled);
            }
        });
    }
    {
        let audience = audience_window.as_weak();
        let presenter = presenter_window.as_weak();
        presenter_window.on_set_audience_fullscreen(move |enabled| {
            if let (Some(a), Some(p)) = (audience.upgrade(), presenter.upgrade()) {
                a.set_is_fullscreen(enabled);
                if enabled && !a.window().is_visible() {
                    if let Err(error) = a.show() {
                        a.set_is_fullscreen(false);
                        p.set_audience_fullscreen(false);
                        super::ui::show_toast(
                            &p,
                            &format!("Couldn't open audience window: {error}"),
                            true,
                        );
                        return;
                    }
                    p.set_audience_window_open(true);
                }
                p.set_audience_fullscreen(enabled);
            }
        });
    }
    {
        let presenter = presenter_window.as_weak();
        audience_window.on_exit_fullscreen(move || {
            if let Some(p) = presenter.upgrade() {
                p.invoke_set_audience_fullscreen(false);
            }
        });
    }
    {
        let presenter = presenter_window.as_weak();
        audience_window.on_fullscreen_changed(move |enabled| {
            if let Some(p) = presenter.upgrade() {
                p.set_audience_fullscreen(enabled);
            }
        });
    }

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
                    #[cfg(target_os = "macos")]
                    if aud.get_borderless() {
                        apply_macos_audience_titlebar_hidden(&aud.window(), true);
                    }
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
        let presenter_handle = presenter_window.as_weak();
        let notes_handle = notes_window.as_weak();
        presenter_window.on_update_slide_notes(move |slide_num, new_notes| {
            let mut st = state.borrow_mut();
            let idx = (slide_num - 1) as usize;
            if idx < st.deck.slides.len() {
                st.deck.slides[idx].notes = new_notes.to_string();
                let segs = parse_notes_to_segments(&new_notes);
                if let Some(p) = presenter_handle.upgrade() {
                    if idx == st.deck.current_index {
                        p.set_notes_segments(segs.clone());
                    }
                }
                if let Some(n) = notes_handle.upgrade() {
                    if idx == st.deck.current_index {
                        n.set_notes_body(new_notes);
                        n.set_notes_segments(segs);
                    }
                }
            }
        });
    }

    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        let notes_handle = notes_window.as_weak();
        notes_window.on_update_slide_notes(move |slide_num, new_notes| {
            let mut st = state.borrow_mut();
            let idx = (slide_num - 1) as usize;
            if idx < st.deck.slides.len() {
                st.deck.slides[idx].notes = new_notes.to_string();
                let segs = parse_notes_to_segments(&new_notes);
                if let Some(p) = presenter_handle.upgrade() {
                    if idx == st.deck.current_index {
                        p.set_notes_text(new_notes.clone());
                        p.set_notes_segments(segs.clone());
                    }
                }
                if let Some(n) = notes_handle.upgrade() {
                    if idx == st.deck.current_index {
                        n.set_notes_segments(segs);
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
    notes_window.set_notes_font_family(presenter_window.get_notes_font_family());
    notes_window.set_notes_font_bold(presenter_window.get_notes_font_bold());
    notes_window.set_keep_on_top(presenter_window.get_notes_always_on_top());
    notes_window.set_notes_highlight_style(presenter_window.get_notes_highlight_style());
    notes_window.set_notes_active_index(presenter_window.get_notes_active_index());

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

    for from_detached in [false, true] {
        let state = state.clone();
        let presenter = presenter_window.as_weak();
        let notes = notes_window.as_weak();
        let set_font = move |family: slint::SharedString| {
            let mut st = state.borrow_mut();
            st.notes_font_family = family.to_string();
            st.capture_settings().save_background();
            if let Some(p) = presenter.upgrade() {
                p.set_notes_font_family(family.clone());
            }
            if let Some(n) = notes.upgrade() {
                n.set_notes_font_family(family);
            }
        };
        if from_detached {
            notes_window.on_set_notes_font_family(set_font);
        } else {
            presenter_window.on_set_notes_font_family(set_font);
        }
    }

    for from_detached in [false, true] {
        let state = state.clone();
        let presenter = presenter_window.as_weak();
        let notes = notes_window.as_weak();
        let set_bold = move |bold: bool| {
            let mut st = state.borrow_mut();
            st.notes_font_bold = bold;
            st.capture_settings().save_background();
            if let Some(p) = presenter.upgrade() {
                p.set_notes_font_bold(bold);
            }
            if let Some(n) = notes.upgrade() {
                n.set_notes_font_bold(bold);
            }
        };
        if from_detached {
            notes_window.on_set_notes_font_bold(set_bold);
        } else {
            presenter_window.on_set_notes_font_bold(set_bold);
        }
    }

    for from_detached in [false, true] {
        let state = state.clone();
        let presenter = presenter_window.as_weak();
        let notes = notes_window.as_weak();
        let set_top = move |always_on_top: bool| {
            let mut st = state.borrow_mut();
            st.notes_always_on_top = always_on_top;
            st.capture_settings().save_background();
            if let Some(p) = presenter.upgrade() {
                p.set_notes_always_on_top(always_on_top);
            }
            if let Some(n) = notes.upgrade() {
                n.set_keep_on_top(always_on_top);
            }
        };
        if from_detached {
            notes_window.on_set_notes_always_on_top(set_top);
        } else {
            presenter_window.on_set_notes_always_on_top(set_top);
        }
    }

    for from_detached in [false, true] {
        let state = state.clone();
        let presenter = presenter_window.as_weak();
        let notes = notes_window.as_weak();
        let set_style = move |style: i32| {
            let mut st = state.borrow_mut();
            st.notes_highlight_style = style;
            st.capture_settings().save_background();
            if let Some(p) = presenter.upgrade() {
                p.set_notes_highlight_style(style);
            }
            if let Some(n) = notes.upgrade() {
                n.set_notes_highlight_style(style);
            }
        };
        if from_detached {
            notes_window.on_set_notes_highlight_style(set_style);
        } else {
            presenter_window.on_set_notes_highlight_style(set_style);
        }
    }

    for from_detached in [false, true] {
        let state = state.clone();
        let presenter = presenter_window.as_weak();
        let notes = notes_window.as_weak();
        let set_idx = move |idx: i32| {
            let mut st = state.borrow_mut();
            st.notes_active_index = idx;
            if let Some(p) = presenter.upgrade() {
                p.set_notes_active_index(idx);
            }
            if let Some(n) = notes.upgrade() {
                n.set_notes_active_index(idx);
            }
        };
        if from_detached {
            notes_window.on_set_notes_active_index(set_idx);
        } else {
            presenter_window.on_set_notes_active_index(set_idx);
        }
    }

    for from_detached in [false, true] {
        let state = state.clone();
        let presenter = presenter_window.as_weak();
        let notes = notes_window.as_weak();
        let step_next = move || {
            let mut st = state.borrow_mut();
            let seg_count = if let Some(p) = presenter.upgrade() {
                p.get_notes_segments().row_count()
            } else {
                0
            };
            if seg_count == 0 || st.notes_active_index >= (seg_count as i32 - 1) {
                // Already at the last line (or no notes): trigger next slide transition
                drop(st);
                if let Some(p) = presenter.upgrade() {
                    p.invoke_next_slide();
                }
            } else {
                let next_idx = (st.notes_active_index + 1).min(seg_count as i32 - 1);
                st.notes_active_index = next_idx;
                if let Some(p) = presenter.upgrade() {
                    p.set_notes_active_index(next_idx);
                }
                if let Some(n) = notes.upgrade() {
                    n.set_notes_active_index(next_idx);
                }
            }
        };
        if from_detached {
            notes_window.on_next_notes_sentence(step_next);
        } else {
            presenter_window.on_next_notes_sentence(step_next);
        }
    }

    for from_detached in [false, true] {
        let state = state.clone();
        let presenter = presenter_window.as_weak();
        let notes = notes_window.as_weak();
        let step_prev = move || {
            let mut st = state.borrow_mut();
            if st.notes_active_index <= 0 {
                // Already at the first line (or no notes): trigger previous slide transition
                drop(st);
                if let Some(p) = presenter.upgrade() {
                    p.invoke_prev_slide();
                }
            } else {
                let next_idx = (st.notes_active_index - 1).max(0);
                st.notes_active_index = next_idx;
                if let Some(p) = presenter.upgrade() {
                    p.set_notes_active_index(next_idx);
                }
                if let Some(n) = notes.upgrade() {
                    n.set_notes_active_index(next_idx);
                }
            }
        };
        if from_detached {
            notes_window.on_prev_notes_sentence(step_prev);
        } else {
            presenter_window.on_prev_notes_sentence(step_prev);
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

#[cfg(target_os = "macos")]
fn perform_macos_window_drag(window: &slint::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::{c_char, c_void};

    #[link(name = "AppKit", kind = "framework")]
    #[link(name = "objc", kind = "dylib")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    if let Ok(handle) = window.window_handle().window_handle() {
        if let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
            unsafe {
                let view_ptr = appkit_handle.ns_view.as_ptr() as *mut c_void;
                let sel_window = sel_registerName(b"window\0".as_ptr() as *const c_char);
                let msg_send_window: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                let ns_window = msg_send_window(view_ptr, sel_window);
                if !ns_window.is_null() {
                    let cls_app = objc_getClass(b"NSApplication\0".as_ptr() as *const c_char);
                    let sel_shared_app = sel_registerName(b"sharedApplication\0".as_ptr() as *const c_char);
                    let sel_current_event = sel_registerName(b"currentEvent\0".as_ptr() as *const c_char);
                    let msg_send_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());

                    let app = msg_send_ptr(cls_app, sel_shared_app);
                    if !app.is_null() {
                        let event = msg_send_ptr(app, sel_current_event);
                        if !event.is_null() {
                            let sel_drag = sel_registerName(b"performWindowDragWithEvent:\0".as_ptr() as *const c_char);
                            let msg_send_drag: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
                                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                            msg_send_drag(ns_window, sel_drag, event);
                        }
                    }
                }
            }
        }
    }
}
#[cfg(target_os = "macos")]
fn apply_macos_audience_titlebar_hidden(window: &slint::Window, hidden: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::{c_char, c_void};

    #[link(name = "AppKit", kind = "framework")]
    #[link(name = "objc", kind = "dylib")]
    unsafe extern "C" {
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    if let Ok(handle) = window.window_handle().window_handle() {
        if let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
            unsafe {
                let view_ptr = appkit_handle.ns_view.as_ptr() as *mut c_void;
                let sel_window = sel_registerName(b"window\0".as_ptr() as *const c_char);
                let msg_send_window: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                let ns_window = msg_send_window(view_ptr, sel_window);
                if !ns_window.is_null() {
                    let sel_style_mask = sel_registerName(b"styleMask\0".as_ptr() as *const c_char);
                    let sel_set_style_mask = sel_registerName(b"setStyleMask:\0".as_ptr() as *const c_char);
                    let msg_send_get_mask: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                    let msg_send_set_mask: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());

                    let current_mask = msg_send_get_mask(ns_window, sel_style_mask);
                    const NS_TITLED: usize = 1 << 0;
                    const NS_CLOSABLE: usize = 1 << 1;
                    const NS_MINIATURIZABLE: usize = 1 << 2;
                    const NS_RESIZABLE: usize = 1 << 3;
                    const NS_FULLSIZE_CONTENT_VIEW: usize = 1 << 15;

                    let mut target_mask = current_mask | NS_TITLED | NS_CLOSABLE | NS_MINIATURIZABLE | NS_RESIZABLE;
                    if hidden {
                        target_mask |= NS_FULLSIZE_CONTENT_VIEW;
                    } else {
                        target_mask &= !NS_FULLSIZE_CONTENT_VIEW;
                    }
                    msg_send_set_mask(ns_window, sel_set_style_mask, target_mask);

                    let sel_invalidate_shadow = sel_registerName(b"invalidateShadow\0".as_ptr() as *const c_char);
                    let msg_send_void: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                    msg_send_void(ns_window, sel_invalidate_shadow);

                    let sel_set_title_vis = sel_registerName(b"setTitleVisibility:\0".as_ptr() as *const c_char);
                    let msg_send_vis: unsafe extern "C" fn(*mut c_void, *mut c_void, isize) =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                    // 1 = NSWindowTitleHidden, 0 = NSWindowTitleVisible
                    msg_send_vis(ns_window, sel_set_title_vis, if hidden { 1 } else { 0 });

                    let sel_set_titlebar_trans = sel_registerName(b"setTitlebarAppearsTransparent:\0".as_ptr() as *const c_char);
                    let msg_send_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                    msg_send_bool(ns_window, sel_set_titlebar_trans, hidden);

                    let sel_standard_btn = sel_registerName(b"standardWindowButton:\0".as_ptr() as *const c_char);
                    let sel_set_hidden = sel_registerName(b"setHidden:\0".as_ptr() as *const c_char);
                    let msg_send_btn: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());

                    // Hide/show Close (0), Miniaturize (1), Zoom (2) buttons
                    for btn_type in 0..=2 {
                        let btn = msg_send_btn(ns_window, sel_standard_btn, btn_type);
                        if !btn.is_null() {
                            msg_send_bool(btn, sel_set_hidden, hidden);
                        }
                    }

                    // Window remains fully movable by dragging its top bar or header
                    let sel_set_movable = sel_registerName(b"setMovable:\0".as_ptr() as *const c_char);
                    msg_send_bool(ns_window, sel_set_movable, true);

                    let sel_set_movable_bg = sel_registerName(b"setMovableByWindowBackground:\0".as_ptr() as *const c_char);
                    msg_send_bool(ns_window, sel_set_movable_bg, false);

                    // Ensure the main Slint view retains first responder status for keyboard shortcuts
                    let sel_content_view = sel_registerName(b"contentView\0".as_ptr() as *const c_char);
                    let sel_make_first_responder = sel_registerName(b"makeFirstResponder:\0".as_ptr() as *const c_char);
                    let msg_send_get_view: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                    let content_view = msg_send_get_view(ns_window, sel_content_view);
                    if !content_view.is_null() {
                        let msg_send_resp: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
                            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                        msg_send_resp(ns_window, sel_make_first_responder, content_view);
                    }
                }
            }
        }
    }
}
