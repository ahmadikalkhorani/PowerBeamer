//! Presentation timer: the 500ms clock tick (also autosaves preferences) and start/reset.

use std::cell::RefCell;
use std::time::Duration;

use chrono::Local;
use slint::{ComponentHandle, SharedString};

use super::context::AppContext;
use super::state::DeckLoadKind;
use super::ui::*;
use crate::pdf::RenderQuality;
use crate::timer::TimerStatus;

/// Wires the clock tick and timer controls.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext { state, presenter_window, notes_window, update_slide_view, .. } = ctx;

    // 1-second presentation timer tick
    let timer_tick = slint::Timer::default();
    {
        let presenter_handle = presenter_window.as_weak();
        let notes_handle = notes_window.as_weak();
        let state = state.clone();
        let last_saved_settings = RefCell::new(state.borrow().capture_settings());

        timer_tick.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(500),
            move || {
                let mut st = state.borrow_mut();
                // Start-on-first-advance: begin counting once the presenter leaves the armed slide
                if st.timer_armed_at_slide.is_some_and(|slide| slide != st.deck.current_index) {
                    st.timer_armed_at_slide = None;
                    st.timer.start();
                }
                let elapsed = st.timer.elapsed_str();
                let remaining = st.timer.remaining_str();
                let timer_text = st.timer.display_str(st.timer_countdown);
                let timer_status = match st.timer.status(st.timer_warn_min.max(0) as u64 * 60) {
                    TimerStatus::Normal => 0,
                    TimerStatus::Warning => 1,
                    TimerStatus::Overtime => 2,
                };
                let flash = timer_status == 2 && st.timer_flash_overtime && st.timer.is_running;
                let (timer_running, timer_armed) = (st.timer.is_running, st.timer_armed_at_slide.is_some());
                let now = Local::now().format("%I:%M %p").to_string();

                // Persist preferences whenever any of them changed (cheap struct compare).
                let current_settings = st.capture_settings();
                if current_settings != *last_saved_settings.borrow() {
                    if let Err(e) = current_settings.save() {
                        eprintln!("Couldn't save settings: {}", e);
                    }
                    *last_saved_settings.borrow_mut() = current_settings;
                }

                if let Some(path) = st.media_duration_pending.clone() {
                    let probed = lock(&st.media_durations).get(&path).copied();
                    if let Some(d) = probed {
                        st.media_duration_secs = d;
                        st.media_duration_pending = None;
                    }
                }

                let mut media_update = None;
                let is_video_clip = {
                    if let Some(clip) = st.active_clip().as_ref() {
                        let path = &clip.file_path;
                        !(path.ends_with(".mp3")
                            || path.ends_with(".wav")
                            || path.ends_with(".ogg")
                            || path.ends_with(".flac")
                            || path.ends_with(".m4a")
                            || path.ends_with(".aac"))
                    } else {
                        false
                    }
                };
                if st.media_playing && !is_video_clip {
                    st.media_current_time_secs += 0.5 * st.media_speed;
                    if st.media_current_time_secs >= st.media_duration_secs {
                        st.media_current_time_secs = 0.0;
                        st.media_playing = false;
                    }
                    st.media_progress = (st.media_current_time_secs / st.media_duration_secs).clamp(0.0, 1.0);
                    media_update = Some((st.media_playing, st.media_progress, st.format_media_time()));
                }

                drop(st);

                if let Some(p) = presenter_handle.upgrade() {
                    p.set_elapsed_time(SharedString::from(&elapsed));
                    p.set_remaining_time(SharedString::from(&remaining));
                    p.set_wall_clock(SharedString::from(&now));
                    p.set_timer_text(SharedString::from(&timer_text));
                    p.set_timer_status(timer_status);
                    p.set_timer_running(timer_running);
                    p.set_timer_armed(timer_armed);
                    // Overtime flash: alternate every 500ms tick
                    p.set_timer_flash_dim(flash && !p.get_timer_flash_dim());

                    if let Some((playing, progress, time_text)) = media_update {
                        p.set_media_playing(playing);
                        p.set_media_progress(progress);
                        p.set_media_time_text(SharedString::from(&time_text));
                    }
                }
                if let Some(n) = notes_handle.upgrade() {
                    n.set_timer_str(SharedString::from(&elapsed));
                }
            },
        );
    }

    // --- TIMER TOGGLE & RESET ---
    {
        let state = state.clone();
        presenter_window.on_toggle_timer(move || {
            let mut st = state.borrow_mut();
            // A manual start overrides waiting for the first slide change
            st.timer_armed_at_slide = None;
            st.timer.toggle();
        });
    }

    {
        let state = state.clone();
        presenter_window.on_reset_timer(move || {
            let mut st = state.borrow_mut();
            if st.timer_start_on_advance {
                st.arm_timer();
            } else {
                st.timer.reset();
            }
            println!("Presentation timer reset to 00:00");
        });
    }

    // --- TIMER MENU OPTIONS ---
    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_timer_options_changed(move || {
            let Some(p) = presenter_handle.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.timer_countdown = p.get_timer_countdown();
            st.timer_duration_min = p.get_timer_duration_min().clamp(1, 600);
            st.timer_warn_min = p.get_timer_warn_min().clamp(0, 120);
            st.timer_flash_overtime = p.get_timer_flash_overtime();
            st.timer_show_clock = p.get_timer_show_clock();
            st.timer_size = p.get_timer_size().clamp(0, 2);
            let start_on_advance = p.get_timer_start_on_advance();
            if start_on_advance != st.timer_start_on_advance {
                st.timer_start_on_advance = start_on_advance;
                // Turning it on arms a fresh timer only if nothing has been timed yet
                if start_on_advance && st.timer.elapsed().as_secs() < 2 {
                    st.arm_timer();
                } else if !start_on_advance && st.timer_armed_at_slide.take().is_some() {
                    st.timer.start();
                }
            }
            let minutes = st.timer_duration_min as u64;
            st.timer.set_target_minutes(minutes);
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_quality_changed(move |quality_str| {
            let mut st = state.borrow_mut();
            st.deck.quality = RenderQuality::from_label(&quality_str);
            println!("Rendering quality set to: {:?}", st.deck.quality);
            // Slides are rasterized at load time, so re-render the open PDF at the new DPI
            let reload = st.deck.file_path.clone();
            if let Some(path) = &reload {
                st.start_deck_load(path.clone(), DeckLoadKind::Requality);
            }
            drop(st);
            if reload.is_some() {
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_deck_loading(true);
                }
            }
            update();
        });
    }

    vec![timer_tick]
}
