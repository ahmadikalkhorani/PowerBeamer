//! Media toolbar: in-place playback, pause/stop, speed, mute and scrubbing.

use std::rc::Rc;

use slint::{ComponentHandle, SharedString};

use super::context::AppContext;
use super::gpu_images::*;
use super::ui::*;
use crate::media::InPlaceVideoPlayer;

/// Wires media playback callbacks.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext { state, presenter_window, audience_window, video_player, update_slide_view, .. } = ctx;

    // --- MEDIA PLAYER TOOLBAR (IN-PLACE NATIVE PLAYBACK, STOP, SPEED, SCRUBBING) ---
    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        let video_player = video_player.clone();
        let update_slide_view = update_slide_view.clone();

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
                                show_toast(&p, format!("Media file not found: {}", clip.file_path), true);
                            }
                            return;
                        }
                        // If already at or past the end, restart from beginning
                        if st.media_duration_secs > 0.0 && (st.media_current_time_secs >= st.media_duration_secs - 0.1 || st.media_progress >= 0.99) {
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
                        return;
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
                }

                if is_audio {
                    lock(&video_player).play_audio(&clip_path, cur_time);
                    return;
                }

                let p_weak = presenter_handle.clone();
                let a_weak = audience_handle.clone();
                let p_weak_eof = presenter_handle.clone();
                let duration_secs = duration;

                let mut vp = lock(&video_player);
                let started = vp.play(
                    &clip_path,
                    cur_time,
                    speed,
                    duration,
                    muted,
                    move |pixel_buf, time_secs, progress| {
                        let p_weak = p_weak.clone();
                        let a_weak = a_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            let frame = rgba_to_slint_image(&pixel_buf);
                            let cur_u = time_secs as u32;
                            let dur_u = duration_secs as u32;
                            let time_text = format!("{:02}:{:02} / {:02}:{:02}", cur_u / 60, cur_u % 60, dur_u / 60, dur_u % 60);

                            if let Some(p) = p_weak.upgrade() {
                                p.set_video_frame_image(frame.clone());
                                p.set_media_progress(progress);
                                p.set_media_time_text(SharedString::from(&time_text));
                            }
                            if let Some(a) = a_weak.upgrade() {
                                a.set_video_frame_image(frame);
                            }
                        });
                    },
                    move || {
                        let p_weak = p_weak_eof.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(p) = p_weak.upgrade() {
                                p.set_media_playing(false);
                                p.set_media_progress(1.0);
                            }
                        });
                    },
                );

                if !started {
                    if let Some(p) = presenter_handle.upgrade() {
                        show_toast(&p, format!("Couldn't play {} (is ffmpeg installed?)", clip_path), true);
                    }
                }
            })
        };

        // 1. Play Media
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_play_media(move || {
                let is_at_end = presenter_handle.upgrade().map(|p| p.get_media_progress() >= 0.99).unwrap_or(false);
                {
                    let mut st = state.borrow_mut();
                    if is_at_end {
                        st.media_current_time_secs = 0.0;
                        st.media_progress = 0.0;
                    }
                    st.media_playing = true;
                }
                start_playback();
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(true);
                }
            });
        }

        // 2. Pause Media
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let video_player = video_player.clone();
            presenter_window.on_pause_media(move || {
                let cur_t = {
                    let mut vp = lock(&video_player);
                    vp.pause();
                    vp.current_time()
                };
                {
                    let mut st = state.borrow_mut();
                    st.media_playing = false;
                    st.media_current_time_secs = cur_t;
                    if st.media_duration_secs > 0.0 {
                        st.media_progress = (cur_t / st.media_duration_secs).clamp(0.0, 1.0);
                    }
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(false);
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
                    st.media_current_time_secs = 0.0;
                    st.media_progress = 0.0;
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(false);
                    p.set_media_video_visible(false);
                    p.set_media_progress(0.0);
                }
                if let Some(a) = audience_handle.upgrade() {
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
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_toggle_media_playback(move || {
                let is_at_end = presenter_handle.upgrade().map(|p| p.get_media_progress() >= 0.99).unwrap_or(false);
                let will_play = {
                    let mut st = state.borrow_mut();
                    if is_at_end {
                        st.media_current_time_secs = 0.0;
                        st.media_progress = 0.0;
                    }
                    st.media_playing = !st.media_playing;
                    st.media_playing
                };
                if will_play {
                    start_playback();
                } else {
                    let cur_t = {
                        let mut vp = lock(&video_player);
                        vp.pause();
                        vp.current_time()
                    };
                        {
                        let mut st = state.borrow_mut();
                        st.media_current_time_secs = cur_t;
                        if st.media_duration_secs > 0.0 {
                            st.media_progress = (cur_t / st.media_duration_secs).clamp(0.0, 1.0);
                        }
                    }
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(will_play);
                }
            });
        }

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
                if is_playing {
                    start_playback();
                } else if let Some(clip) = clip_opt {
                    if clip.is_resolved {
                        if let Some(pixel_buf) = InPlaceVideoPlayer::extract_single_frame(&clip.file_path, seek_time) {
                            let frame = rgba_to_slint_image(&pixel_buf);
                            if let Some(p) = presenter_handle.upgrade() {
                                p.set_video_frame_image(frame.clone());
                                p.set_media_video_visible(true);
                            }
                            if let Some(a) = audience_handle.upgrade() {
                                a.set_video_frame_image(frame);
                                a.set_media_video_visible(true);
                            }
                        }
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
                    if st.media_playing {
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
                    if st.media_playing {
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
    }

    Vec::new()
}
