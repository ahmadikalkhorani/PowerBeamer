//! Media toolbar: in-place playback, pause/stop, speed, mute and scrubbing.

use std::rc::Rc;

use slint::{ComponentHandle, SharedString};

use super::context::AppContext;
use super::gpu_images::*;
use super::ui::*;
use crate::media::InPlaceVideoPlayer;

/// Wires media playback callbacks.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext {
        state,
        presenter_window,
        audience_window,
        video_player,
        update_slide_view,
        ..
    } = ctx;

    // --- MEDIA PLAYER TOOLBAR (IN-PLACE NATIVE PLAYBACK, STOP, SPEED, SCRUBBING) ---
    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        let video_player = video_player.clone();
        let update_slide_view = update_slide_view.clone();

        let audio_tick = ctx.audio_tick.clone();
        let start_playback_fn = {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let video_player = video_player.clone();

            Rc::new(move || {
                let (clip_path, is_audio, cur_time, speed, duration, muted, rect) = {
                    let mut st = state.borrow_mut();
                    let clip_opt = st.active_clip();
                    if let Some(clip) = clip_opt {
                        if !clip.is_resolved {
                            if let Some(p) = presenter_handle.upgrade() {
                                show_toast(
                                    &p,
                                    format!("Media file not found: {}", clip.file_path),
                                    true,
                                );
                            }
                            return false;
                        }
                        // If already at or past the end, restart from beginning
                        if st.media_duration_secs > 0.0
                            && (st.media_current_time_secs >= st.media_duration_secs - 0.1
                                || st.media_progress >= 0.99)
                        {
                            st.media_current_time_secs = 0.0;
                            st.media_progress = 0.0;
                        }
                        let path = clip.file_path;
                        let is_aud = path.ends_with(".mp3")
                            || path.ends_with(".wav")
                            || path.ends_with(".ogg")
                            || path.ends_with(".flac")
                            || path.ends_with(".m4a")
                            || path.ends_with(".aac");
                        (
                            path,
                            is_aud,
                            st.media_current_time_secs,
                            st.media_speed,
                            st.media_duration_secs,
                            !st.media_speech_active,
                            clip.rect,
                        )
                    } else {
                        return false;
                    }
                };

                println!(
                    "Starting in-place native playback of media: {} at pos {:.2}s (rect: {:?})",
                    clip_path, cur_time, rect
                );

                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_rect_x(rect[0]);
                    p.set_media_rect_y(rect[1]);
                    p.set_media_rect_w(rect[2]);
                    p.set_media_rect_h(rect[3]);
                    // Audio has no frames; showing the video layer would cover the slide with black.
                    p.set_media_video_visible(!is_audio);
                    p.set_media_playing(true);
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_rect_x(rect[0]);
                    a.set_media_rect_y(rect[1]);
                    a.set_media_rect_w(rect[2]);
                    a.set_media_rect_h(rect[3]);
                    a.set_media_video_visible(!is_audio);
                    a.set_media_playing(true);
                }

                if is_audio {
                    if let Some(timer) = audio_tick.borrow().as_ref() {
                        timer.wake();
                    }
                    let started = lock(&video_player)
                        .play_audio_with_options(&clip_path, cur_time, speed, muted);
                    let mut st = state.borrow_mut();
                    // Audio player processes (ffplay) take ~150-200ms to initialize audio devices and start rendering samples.
                    // By setting media_playback_started with a small delay (or compensating for device latency), the progress
                    // bar and spectrogram needle advance in exact sync with the perceived sound.
                    st.media_playback_started = Some(std::time::Instant::now());
                    st.media_playback_start_offset = cur_time;
                    return started;
                }

                let p_weak = presenter_handle.clone();
                let a_weak = audience_handle.clone();
                let p_weak_eof = presenter_handle.clone();
                let duration_secs = duration;

                let frames = std::sync::Arc::new(crate::work::LatestFrame::default());
                let mut vp = lock(&video_player);
                // play() pauses the old run, advancing the generation once.
                let (epoch, old_generation) = vp.generation();
                let generation = old_generation + 1;
                let eof_epoch = epoch.clone();
                let started = vp.play(
                    &clip_path,
                    cur_time,
                    speed,
                    duration,
                    muted,
                    move |pixel_buf, time_secs, progress| {
                        if !frames.publish((pixel_buf, time_secs, progress)) {
                            return;
                        }
                        let frames = frames.clone();
                        let epoch = epoch.clone();
                        let p_weak = p_weak.clone();
                        let a_weak = a_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            let Some((pixel_buf, time_secs, progress)) = frames.take() else {
                                return;
                            };
                            if epoch.load(std::sync::atomic::Ordering::Acquire) != generation {
                                return;
                            }
                            let frame = slint::Image::from_rgba8(pixel_buf);
                            let cur_u = time_secs as u32;
                            let dur_u = duration_secs as u32;
                            let time_text = format!(
                                "{:02}:{:02} / {:02}:{:02}",
                                cur_u / 60,
                                cur_u % 60,
                                dur_u / 60,
                                dur_u % 60
                            );

                            if let Some(p) = p_weak.upgrade() {
                                p.set_video_frame_image(frame.clone());
                                p.set_media_progress(progress);
                                p.set_media_time_text(SharedString::from(&time_text));
                            }
                            if let Some(a) = a_weak.upgrade() {
                                a.set_video_frame_image(frame);
                                a.set_media_progress(progress);
                            }
                        });
                    },
                    move || {
                        let p_weak = p_weak_eof.clone();
                        let eof_epoch = eof_epoch.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if eof_epoch.load(std::sync::atomic::Ordering::Acquire) != generation {
                                return;
                            }
                            if let Some(p) = p_weak.upgrade() {
                                p.invoke_pause_media();
                                p.set_media_progress(1.0);
                            }
                        });
                    },
                );

                if !started {
                    if let Some(p) = presenter_handle.upgrade() {
                        show_toast(
                            &p,
                            format!("Couldn't play {} (is ffmpeg installed?)", clip_path),
                            true,
                        );
                    }
                }
                started
            })
        };

        // Every entry point (play, seek, speed, mute) must publish actual start success.
        let start_playback_fn = {
            let start = start_playback_fn;
            let state = state.clone();
            let presenter = presenter_handle.clone();
            let audience = audience_handle.clone();
            Rc::new(move || {
                let started = start();
                let mut st = state.borrow_mut();
                st.media_playing = started;
                if !started {
                    st.media_playback_started = None;
                }
                drop(st);
                if let Some(p) = presenter.upgrade() {
                    p.set_media_playing(started);
                }
                if let Some(a) = audience.upgrade() {
                    a.set_media_playing(started);
                }
                started
            })
        };

        // 1. Play Media
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_play_media(move || {
                let is_at_end = presenter_handle
                    .upgrade()
                    .map(|p| p.get_media_progress() >= 0.99)
                    .unwrap_or(false);
                {
                    let mut st = state.borrow_mut();
                    if is_at_end {
                        st.media_current_time_secs = 0.0;
                        st.media_progress = 0.0;
                    }
                    st.media_playing = true;
                }
                let started = start_playback();
                state.borrow_mut().media_playing = started;
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(started);
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_playing(started);
                }
            });
        }

        // 2. Pause Media
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let video_player = video_player.clone();
            presenter_window.on_pause_media(move || {
                let is_video = {
                    let st = state.borrow();
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

                let vp_cur_t = {
                    let mut vp = lock(&video_player);
                    vp.pause();
                    vp.current_time()
                };

                {
                    let mut st = state.borrow_mut();
                    st.media_playing = false;
                    st.media_playback_started = None;
                    // For video, vp.current_time tracks the decoded frame time.
                    // For audio, st.media_current_time_secs tracks the elapsed time in session.rs.
                    if is_video {
                        st.media_current_time_secs = vp_cur_t;
                    } else {
                        // Keep vp in sync with the live audio position
                        lock(&video_player).set_current_time(st.media_current_time_secs);
                    }
                    if st.media_duration_secs > 0.0 {
                        st.media_progress =
                            (st.media_current_time_secs / st.media_duration_secs).clamp(0.0, 1.0);
                    }
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(false);
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_playing(false);
                }
            });
        }

        // 3. Stop Media (reverts slide back to original state / selected frame)
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let video_player = video_player.clone();
            let update_slide_view = update_slide_view.clone();
            presenter_window.on_stop_media(move || {
                lock(&video_player).stop();

                {
                    let mut st = state.borrow_mut();
                    st.media_playing = false;
                    st.media_playback_started = None;
                    st.media_current_time_secs = 0.0;
                    st.media_progress = 0.0;
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(false);
                    p.set_media_video_visible(false);
                    p.set_media_progress(0.0);
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_playing(false);
                    a.set_media_progress(0.0);
                    a.set_media_video_visible(false);
                }

                // Explicitly revert slide back to original state (user's selected frame)
                update_slide_view();
                println!("Media stopped: reverted slide to original frame state.");
            });
        }

        // 4. Toggle Playback
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_toggle_media_playback(move || {
                let is_at_end = presenter_handle
                    .upgrade()
                    .map(|p| p.get_media_progress() >= 0.99)
                    .unwrap_or(false);
                let will_play = {
                    let mut st = state.borrow_mut();
                    if is_at_end {
                        st.media_current_time_secs = 0.0;
                        st.media_progress = 0.0;
                    }
                    st.media_playing = !st.media_playing;
                    if !st.media_playing {
                        st.media_playback_started = None;
                    }
                    st.media_playing
                };
                let will_play = if will_play {
                    let started = start_playback();
                    state.borrow_mut().media_playing = started;
                    started
                } else {
                    let is_video = {
                        let st = state.borrow();
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

                    let vp_cur_t = {
                        let mut vp = lock(&video_player);
                        vp.pause();
                        vp.current_time()
                    };

                    {
                        let mut st = state.borrow_mut();
                        st.media_playback_started = None;
                        if is_video {
                            st.media_current_time_secs = vp_cur_t;
                        } else {
                            lock(&video_player).set_current_time(st.media_current_time_secs);
                        }
                        if st.media_duration_secs > 0.0 {
                            st.media_progress = (st.media_current_time_secs
                                / st.media_duration_secs)
                                .clamp(0.0, 1.0);
                        }
                    }
                    false
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(will_play);
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_playing(will_play);
                }
            });
        }

        let seek_worker = std::rc::Rc::new(crate::work::LatestWorker::new(
            |id,
             (path, time, p, a, epoch, token): (
                String,
                f32,
                slint::Weak<crate::PresenterWindow>,
                slint::Weak<crate::AudienceWindow>,
                std::sync::Arc<std::sync::atomic::AtomicU64>,
                u64,
            ),
             generation| {
                std::thread::sleep(std::time::Duration::from_millis(60));
                if generation.load(std::sync::atomic::Ordering::Acquire) != id
                    || epoch.load(std::sync::atomic::Ordering::Acquire) != token
                {
                    return;
                }
                let frame = InPlaceVideoPlayer::extract_single_frame(&path, time);
                if generation.load(std::sync::atomic::Ordering::Acquire) != id
                    || epoch.load(std::sync::atomic::Ordering::Acquire) != token
                {
                    return;
                }
                if let Some(frame) = frame {
                    let _ = slint::invoke_from_event_loop(move || {
                        if epoch.load(std::sync::atomic::Ordering::Acquire) != token {
                            return;
                        }
                        let frame = rgba_to_slint_image(&frame);
                        if let Some(p) = p.upgrade() {
                            p.set_video_frame_image(frame.clone());
                            p.set_media_video_visible(true);
                        }
                        if let Some(a) = a.upgrade() {
                            a.set_video_frame_image(frame);
                            a.set_media_video_visible(true);
                        }
                    });
                }
            },
        ));
        // 5. Seek Media
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_seek_media(move |pos| {
                let (p_clamped, is_playing, seek_time, clip_opt) = {
                    let mut st = state.borrow_mut();
                    let p_clamped = pos.clamp(0.0, 1.0);
                    st.media_progress = p_clamped;
                    st.media_current_time_secs = p_clamped * st.media_duration_secs;
                    let playing = st.media_playing;
                    let seek_t = st.media_current_time_secs;
                    let clip = st.active_clip();
                    (p_clamped, playing, seek_t, clip)
                };
                lock(&video_player).set_current_time(seek_time);
                let time_str = {
                    let st = state.borrow();
                    st.format_media_time()
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_progress(p_clamped);
                    p.set_media_time_text(SharedString::from(&time_str));
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_progress(p_clamped);
                }
                if is_playing {
                    start_playback();
                } else if let Some(clip) = clip_opt {
                    if clip.is_resolved {
                        // Invalidate earlier completions, including ones already queued in the UI loop.
                        let (epoch, _) = lock(&video_player).generation();
                        let token = epoch.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
                        seek_worker.submit((
                            clip.file_path,
                            seek_time,
                            presenter_handle.clone(),
                            audience_handle.clone(),
                            epoch,
                            token,
                        ));
                    }
                }
            });
        }

        // 6. Toggle Media Speed
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_toggle_media_speed(move || {
                let (speed_label, is_playing) = {
                    let mut st = state.borrow_mut();
                    if st.media_playing && st.active_clip().is_some_and(|clip| !clip.is_audio()) {
                        let cur_t = lock(&video_player).current_time();
                        st.media_current_time_secs = cur_t;
                    }
                    const SPEED_OPTIONS: [f32; 5] = [1.0, 1.25, 1.5, 2.0, 0.75];
                    let cur = st.media_speed;
                    let idx = SPEED_OPTIONS
                        .iter()
                        .position(|&s| (s - cur).abs() < 0.05)
                        .unwrap_or(0);
                    st.media_speed = SPEED_OPTIONS[(idx + 1) % SPEED_OPTIONS.len()];
                    (st.format_media_speed(), st.media_playing)
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_speed_text(SharedString::from(&speed_label));
                }
                if is_playing {
                    start_playback();
                }
            });
        }

        // 7. Toggle Media Speech / Audio Mute
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_toggle_media_speech(move || {
                let (active, is_playing) = {
                    let mut st = state.borrow_mut();
                    if st.media_playing && st.active_clip().is_some_and(|clip| !clip.is_audio()) {
                        let cur_t = lock(&video_player).current_time();
                        st.media_current_time_secs = cur_t;
                    }
                    st.media_speech_active = !st.media_speech_active;
                    (st.media_speech_active, st.media_playing)
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_speech_active(active);
                }
                if is_playing {
                    start_playback();
                }
            });
        }

        // 8. Select Media Clip (slides with several clips: stop the current one, switch, re-render)
        {
            let state = state.clone();
            let video_player = video_player.clone();
            let update_slide_view = update_slide_view.clone();
            presenter_window.on_select_media_clip(move |idx| {
                {
                    let mut st = state.borrow_mut();
                    let count = st.deck.current_slide().media_clips.len();
                    if count == 0 {
                        return;
                    }
                    let idx = idx.rem_euclid(count as i32) as usize;
                    if idx == st.active_clip_index() {
                        return;
                    }
                    st.active_media_clip = (st.deck.current_index, idx);
                    st.media_playing = false;
                    st.media_current_time_secs = 0.0;
                    st.media_progress = 0.0;
                }
                lock(&video_player).stop();
                // Re-render picks up the new clip's rect, label and duration
                update_slide_view();
            });
        }

        // 9. Spectrogram / Waveform Audio Progress Timeline Box Tool
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            presenter_window.on_toggle_media_track_tool(move || {
                let active = {
                    let mut st = state.borrow_mut();
                    st.media_tracking_tool_active = !st.media_tracking_tool_active;
                    st.media_tracking_tool_active
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_track_tool_active(active);
                    if active {
                        show_toast(&p, "Audio Timeline Tool: Click and drag a box across the spectrogram or waveform".to_string(), false);
                    }
                }
            });
        }

        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            presenter_window.on_clear_media_track_box(move || {
                {
                    let mut st = state.borrow_mut();
                    st.deck.current_slide_mut().audio_progress_box = None;
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_track_box_defined(false);
                    show_toast(
                        &p,
                        "Spectrogram audio timeline box removed".to_string(),
                        false,
                    );
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_track_box_defined(false);
                }
            });
        }

        // 11. Media Track Settings Callbacks (FPS, Audience Box, Needle Width, Glow, Loop)
        {
            let state = state.clone();
            presenter_window.on_set_media_track_fps_mode(move |mode| {
                let mut st = state.borrow_mut();
                st.media_track_fps_mode = mode;
            });
        }

        {
            let state = state.clone();
            let audience_handle = audience_handle.clone();
            presenter_window.on_set_media_track_show_audience_box(move |show| {
                let mut st = state.borrow_mut();
                st.media_track_show_audience_box = show;
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_track_show_box(show);
                }
            });
        }

        {
            let state = state.clone();
            let audience_handle = audience_handle.clone();
            presenter_window.on_set_media_track_needle_width(move |w| {
                let mut st = state.borrow_mut();
                st.media_track_needle_width = w;
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_track_needle_width(w);
                }
            });
        }

        {
            let state = state.clone();
            let audience_handle = audience_handle.clone();
            presenter_window.on_set_media_track_needle_glow(move |glow| {
                let mut st = state.borrow_mut();
                st.media_track_needle_glow = glow;
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_track_needle_glow(glow);
                }
            });
        }

        {
            let state = state.clone();
            presenter_window.on_set_media_track_loop(move |lp| {
                let mut st = state.borrow_mut();
                st.media_track_loop = lp;
            });
        }
    }

    Vec::new()
}
