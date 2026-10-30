//! Presentation timer: the 500ms clock tick (also autosaves preferences) and start/reset.

use std::cell::RefCell;
use std::time::Duration;

use chrono::Local;
use slint::{ComponentHandle, SharedString};

use super::context::AppContext;
use super::scheduling::DemandTimer;
use super::state::DeckLoadKind;
use super::ui::*;
use crate::pdf::RenderQuality;
use crate::timer::TimerStatus;

/// Wires the clock tick and timer controls.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext {
        state,
        presenter_window,
        audience_window,
        notes_window,
        update_slide_view,
        ..
    } = ctx;

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
                if st
                    .timer_armed_at_slide
                    .is_some_and(|slide| slide != st.deck.current_index)
                {
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
                let (timer_running, timer_armed) =
                    (st.timer.is_running, st.timer_armed_at_slide.is_some());
                let now = Local::now().format("%I:%M %p").to_string();

                // Persist preferences whenever any of them changed (cheap struct compare).
                let current_settings = st.capture_settings();
                if current_settings != *last_saved_settings.borrow() {
                    current_settings.save_background();
                    *last_saved_settings.borrow_mut() = current_settings;
                }

                if let Some(path) = st.media_duration_pending.clone() {
                    let probed = lock(&st.media_durations).get(&path).copied();
                    if let Some(d) = probed {
                        st.media_duration_secs = d;
                        st.media_duration_pending = None;
                    }
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
                }
                if let Some(n) = notes_handle.upgrade() {
                    n.set_timer_str(SharedString::from(&elapsed));
                }
            },
        );
    }

    // High-refresh audio playback and spectrogram tracking needle tick timer
    let audio_tick = {
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        let state = state.clone();
        let mut last_tick = std::time::Instant::now();
        let mut accum_dt = 0.0f32;

        let active_state = state.clone();
        DemandTimer::new(
            Duration::from_millis(16),
            move || {
                let st = active_state.borrow();
                st.media_playing && st.active_clip().is_some_and(|c| c.is_audio())
            },
            move || {
                let now = std::time::Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f32().min(0.2);
                last_tick = now;

                let mut st = state.borrow_mut();
                if !st.media_playing {
                    accum_dt = 0.0;
                    return;
                }

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

                // Video frames drive their own progress in `InPlaceVideoPlayer`
                if is_video_clip {
                    accum_dt = 0.0;
                    return;
                }

                // Target interval based on FPS mode or presentation quality:
                // mode 0: Auto (Eco = 15 fps / 66ms, Balanced = 30 fps / 33ms, Ultra = 60 fps / 16ms)
                // mode 1: 15 fps (66ms)
                // mode 2: 30 fps (33ms)
                // mode 3: 60 fps (16ms)
                let min_interval = match st.media_track_fps_mode {
                    1 => 0.066,
                    2 => 0.033,
                    3 => 0.016,
                    _ => match st.deck.quality {
                        RenderQuality::Eco => 0.066,
                        RenderQuality::Balanced => 0.033,
                        RenderQuality::UltraVector => 0.016,
                    },
                };

                accum_dt += dt;
                if accum_dt < min_interval {
                    return;
                }
                let step = accum_dt;
                accum_dt = 0.0;

                // Advance audio playback time accurately synchronized with process initialization
                let speed = st.media_speed;
                if let Some(start_instant) = st.media_playback_started {
                    // ffplay process startup + CoreAudio/device buffer initialization typically takes ~180ms
                    const AUDIO_PROCESS_INIT_LATENCY_SECS: f32 = 0.18;
                    let elapsed = start_instant.elapsed().as_secs_f32();
                    let effective_elapsed = (elapsed - AUDIO_PROCESS_INIT_LATENCY_SECS).max(0.0);
                    st.media_current_time_secs =
                        st.media_playback_start_offset + (effective_elapsed * speed);
                } else {
                    st.media_current_time_secs += step * speed;
                }

                let mut restart_loop = false;
                if st.media_duration_secs > 0.0
                    && st.media_current_time_secs >= st.media_duration_secs
                {
                    if st.media_track_loop {
                        restart_loop = true;
                        st.media_current_time_secs = 0.0;
                        st.media_playback_started = Some(std::time::Instant::now());
                        st.media_playback_start_offset = 0.0;
                    } else {
                        st.media_current_time_secs = 0.0;
                        st.media_playing = false;
                        st.media_playback_started = None;
                    }
                }

                let progress = if st.media_duration_secs > 0.0 {
                    (st.media_current_time_secs / st.media_duration_secs).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                st.media_progress = progress;
                let playing = st.media_playing;
                let time_text = st.format_media_time();

                drop(st);

                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(playing);
                    p.set_media_progress(progress);
                    p.set_media_time_text(SharedString::from(&time_text));
                    if restart_loop { p.invoke_seek_media(0.0); }
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_playing(playing);
                    a.set_media_progress(progress);
                }
            },
        )
    };
    *ctx.audio_tick.borrow_mut() = Some(audio_tick);

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
            let Some(p) = presenter_handle.upgrade() else {
                return;
            };
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
