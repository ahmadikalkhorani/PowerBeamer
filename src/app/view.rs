//! Pushing state to the windows: the ink/laser overlay and the full slide view.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use image::RgbaImage;
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel,
};

use super::gpu_images::*;
use super::state::*;
use super::ui::*;
use crate::ink::InkingRasterizer;
use crate::media::InPlaceVideoPlayer;
use crate::pdf::TocEntry;
use crate::*;

/// One `LaserBand` row per brightness level, in level order; levels without segments get empty commands.
fn laser_band_rows(bands: &[ink::LaserBandPath]) -> Vec<LaserBand> {
    (0..ink::LASER_BAND_COUNT)
        .map(|level| {
            let intensity = (level + 1) as f32 / ink::LASER_BAND_COUNT as f32;
            match bands
                .iter()
                .find(|b| (b.intensity - intensity).abs() < 1e-4)
            {
                Some(b) => LaserBand {
                    commands: b.commands.as_str().into(),
                    intensity: b.intensity,
                    radius: b.radius,
                },
                None => LaserBand {
                    commands: SharedString::new(),
                    intensity,
                    radius: 0.0,
                },
            }
        })
        .collect()
}

/// Builds the closure that re-renders the ink overlay and laser layer for the current slide.
pub(crate) fn build_ink_overlay(
    state: &Rc<RefCell<AppState>>,
    presenter_window: &PresenterWindow,
    audience_window: &AudienceWindow,
) -> Rc<dyn Fn()> {
    // Helper to update only the inking overlay (called on draw, erase, undo, redo)
    let update_ink_overlay: Rc<dyn Fn()> = {
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        let state = state.clone();

        // Vector laser trail: one shared model, stroked by the renderer in both windows. It always
        // holds one row per brightness level (empty commands when unused) so the path items are
        // never destroyed and recreated: the renderer caches paths per item, and a recycled item
        // could otherwise briefly show a stale trail.
        let laser_bands: Rc<VecModel<LaserBand>> = Rc::new(VecModel::from(laser_band_rows(&[])));
        presenter_window.set_laser_bands(ModelRc::from(laser_bands.clone()));
        audience_window.set_laser_bands(ModelRc::from(laser_bands.clone()));

        Rc::new(move || {
            let mut guard = state.borrow_mut();
            let st = &mut *guard;
            let current_idx = st.deck.current_index;
            let current_slide = &st.deck.slides[current_idx];
            let aspect = current_slide.aspect_ratio.max(0.5f32);
            let overlay_w: u32 = 1600;
            let overlay_h: u32 = (1600.0f32 / aspect).round() as u32;

            // The laser lives in its own small layer cropped to the trail, so pointing never
            // re-copies or re-uploads the full-size ink overlay.
            let bands = st
                .laser_trail
                .as_ref()
                .map(InkingRasterizer::laser_trail_bands)
                .unwrap_or_default();
            for (row, band) in laser_band_rows(&bands).into_iter().enumerate() {
                if laser_bands.row_data(row).as_ref() != Some(&band) {
                    laser_bands.set_row_data(row, band);
                }
            }

            // The comet-tail mode (no lines) is still rasterized into its own small layer
            let laser_layer = st
                .laser_trail
                .as_ref()
                .filter(|trail| !trail.draw_lines)
                .and_then(|trail| {
                    let (x0, y0, x1, y1) =
                        InkingRasterizer::laser_trail_bounds(trail, overlay_w, overlay_h)?;
                    let (bw, bh) = (x1 - x0, y1 - y0);
                    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(bw, bh);
                    let mut canvas = image::ImageBuffer::<image::Rgba<u8>, &mut [u8]>::from_raw(
                        bw,
                        bh,
                        buffer.make_mut_bytes(),
                    )
                    .expect("laser buffer size");
                    InkingRasterizer::draw_laser_trail_at(
                        &mut canvas,
                        trail,
                        overlay_w as f32,
                        overlay_h as f32,
                        x0 as f32,
                        y0 as f32,
                    );
                    let (fw, fh) = (overlay_w as f32, overlay_h as f32);
                    Some((
                        Image::from_rgba8(buffer),
                        [
                            x0 as f32 / fw,
                            y0 as f32 / fh,
                            bw as f32 / fw,
                            bh as f32 / fh,
                        ],
                    ))
                });
            let publish_laser = |laser_layer: Option<(Image, [f32; 4])>| {
                let (img, rect, visible) = match laser_layer {
                    Some((img, rect)) => (img, rect, true),
                    None => (Image::default(), [0.0; 4], false),
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_laser_layer_image(img.clone());
                    p.set_laser_layer_x(rect[0]);
                    p.set_laser_layer_y(rect[1]);
                    p.set_laser_layer_w(rect[2]);
                    p.set_laser_layer_h(rect[3]);
                    p.set_laser_layer_visible(visible);
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_laser_layer_image(img);
                    a.set_laser_layer_x(rect[0]);
                    a.set_laser_layer_y(rect[1]);
                    a.set_laser_layer_w(rect[2]);
                    a.set_laser_layer_h(rect[3]);
                    a.set_laser_layer_visible(visible);
                }
            };
            // While only the laser changes, the ink overlay is already up to date.
            let fingerprint = ink_fingerprint(&current_slide.inking.strokes);
            let ink_dirty = st.active_stroke.is_some()
                || st.cached_ink_slide_idx != current_idx
                || st.cached_ink_fingerprint != fingerprint
                || (st.cached_ink_overlay.is_none() && !current_slide.inking.strokes.is_empty());
            if st.laser_trail.is_some() && !ink_dirty {
                drop(guard);
                publish_laser(laser_layer);
                return;
            }

            let stroke_count = current_slide.inking.strokes.len();
            if stroke_count == 0 && st.active_stroke.is_none() {
                // Nothing to draw: use a tiny transparent image instead of a full-size overlay.
                st.cached_ink_overlay = None;
                st.cached_ink_slint = None;
                st.active_ink_layer = None;
                st.cached_ink_slide_idx = current_idx;
                st.cached_ink_fingerprint = fingerprint;
                drop(guard);
                let empty = rgba_to_slint_image(&RgbaImage::new(1, 1));
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_ink_overlay_image(empty.clone());
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_ink_image(empty);
                }
                publish_laser(laser_layer);
                return;
            }

            let cache_valid = st.cached_ink_overlay.is_some()
                && st.cached_ink_slide_idx == current_idx
                && st.cached_ink_fingerprint == fingerprint;

            if !cache_valid {
                let fresh_base = InkingRasterizer::render_strokes(
                    overlay_w,
                    overlay_h,
                    &current_slide.inking.strokes,
                    None,
                    None,
                );
                st.cached_ink_slint = Some(rgba_to_slint_image(&fresh_base));
                st.cached_ink_overlay = Some(fresh_base);
                st.cached_ink_slide_idx = current_idx;
                st.cached_ink_fingerprint = fingerprint;
                st.ink_base_generation = st.ink_base_generation.wrapping_add(1);
                st.active_ink_layer = None;
            }

            let base_img = st.cached_ink_overlay.as_ref().unwrap();
            let slint_ink = if st.active_stroke.is_none() {
                // Idle: reuse the already-uploaded image (refcounted, no pixel copy).
                st.active_ink_layer = None;
                match &st.cached_ink_slint {
                    Some(img) => img.clone(),
                    None => rgba_to_slint_image(base_img),
                }
            } else {
                let (w, h) = base_img.dimensions();
                let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);

                // Freehand strokes: keep finalized segments in a persistent layer and only
                // draw newly finalized segments + the (still changing) tail each frame.
                let incremental = st
                    .active_stroke
                    .as_ref()
                    .filter(|s| !s.is_shape && s.points.len() >= 3);

                if let Some(active) = incremental {
                    let n = active.points.len();
                    let generation = st.ink_base_generation;
                    let layer_ok = st.active_ink_layer.as_ref().is_some_and(|l| {
                        l.base_generation == generation
                            && l.stroke_id == active.id
                            && l.committed_segs <= n.saturating_sub(2)
                            && l.anchor
                                .map_or(true, |a| active.points.get(l.committed_segs) == Some(&a))
                    });
                    if !layer_ok {
                        st.active_ink_layer = Some(ActiveInkLayer {
                            img: base_img.clone(),
                            base_generation: generation,
                            stroke_id: active.id,
                            committed_segs: 0,
                            anchor: None,
                        });
                    }
                    let layer = st.active_ink_layer.as_mut().unwrap();
                    let final_end = n - 2; // segments < n-2 no longer change
                    if final_end > layer.committed_segs {
                        InkingRasterizer::draw_stroke_segments(
                            &mut layer.img,
                            active,
                            layer.committed_segs,
                            final_end,
                        );
                        layer.committed_segs = final_end;
                        layer.anchor = Some(active.points[final_end]);
                    }
                    buffer.make_mut_bytes().copy_from_slice(layer.img.as_raw());
                    let mut canvas = image::ImageBuffer::<image::Rgba<u8>, &mut [u8]>::from_raw(
                        w,
                        h,
                        buffer.make_mut_bytes(),
                    )
                    .expect("ink buffer size");
                    InkingRasterizer::draw_stroke_segments(
                        &mut canvas,
                        active,
                        layer.committed_segs,
                        n - 1,
                    );
                } else {
                    st.active_ink_layer = None;
                    buffer.make_mut_bytes().copy_from_slice(base_img.as_raw());
                    let mut canvas = image::ImageBuffer::<image::Rgba<u8>, &mut [u8]>::from_raw(
                        w,
                        h,
                        buffer.make_mut_bytes(),
                    )
                    .expect("ink buffer size");
                    if let Some(active) = st.active_stroke.as_ref() {
                        InkingRasterizer::draw_stroke(&mut canvas, active);
                    }
                }
                Image::from_rgba8(buffer)
            };
            drop(guard);

            if let Some(p) = presenter_handle.upgrade() {
                p.set_ink_overlay_image(slint_ink.clone());
            }
            if let Some(a) = audience_handle.upgrade() {
                a.set_ink_image(slint_ink);
            }
            publish_laser(laser_layer);
        })
    };

    update_ink_overlay
}

/// Builds the closure that pushes the current slide (images, notes, media, TOC) to all windows.
pub(crate) fn build_slide_view(
    state: &Rc<RefCell<AppState>>,
    presenter_window: &PresenterWindow,
    audience_window: &AudienceWindow,
    notes_window: &NotesWindow,
    update_ink_overlay: &Rc<dyn Fn()>,
    video_player: &Arc<Mutex<InPlaceVideoPlayer>>,
) -> Rc<dyn Fn()> {
    // Helper to update slide background, metadata, notes, and triggers ink update
    let update_slide_view: Rc<dyn Fn()> = {
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        let notes_handle = notes_window.as_weak();
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let video_player = video_player.clone();

        let prewarm_timer = slint::Timer::default();
        Rc::new(move || {
            lock(&video_player).stop();
            let mut st = state.borrow_mut();
            st.cached_ink_overlay = None;
            st.media_playing = false;
            st.media_progress = 0.0;
            st.media_current_time_secs = 0.0;
            st.active_link_target = None;
            st.link_press_start = None;
            st.link_triggered = false;

            let current_idx = st.deck.current_index;
            let total = st.deck.total_slides();
            let current_slide = &st.deck.slides[current_idx];
            let slide_track_box = current_slide.audio_progress_box;

            let slint_current = slide_presenter_image(current_slide);
            let aud_slide = slide_audience_image(current_slide);

            let (disp_presenter, disp_audience) = if st.is_screen_swapped {
                (aud_slide.clone(), slint_current.clone())
            } else {
                (slint_current.clone(), aud_slide.clone())
            };

            let cur_frame = current_slide.frame_index;
            let tot_frames = current_slide.total_frames;
            let over_step = current_slide.overlay_step;
            let over_tot = current_slide.overlay_total;
            let frame_info = if tot_frames > 0 && over_tot > 1 {
                format!(
                    "Frame {}/{} • Step {}/{}",
                    cur_frame, tot_frames, over_step, over_tot
                )
            } else if tot_frames > 0 {
                format!("Frame {}/{}", cur_frame, tot_frames)
            } else {
                String::new()
            };

            let toc_is_active = |entry: &TocEntry| {
                (entry.page_index == current_idx + 1)
                    || (entry.frame_index == cur_frame && entry.page_index <= current_idx + 1)
            };
            let new_toc_model = if st.toc_model.is_none() {
                let toc_models: Vec<TocItemModel> = st
                    .deck
                    .toc
                    .iter()
                    .map(|entry| TocItemModel {
                        title: SharedString::from(&entry.title),
                        page_index: entry.page_index as i32,
                        level: entry.level as i32,
                        frame_index: entry.frame_index as i32,
                        is_active: toc_is_active(entry),
                    })
                    .collect();
                Some(Rc::new(slint::VecModel::from(toc_models)))
            } else {
                use slint::Model;
                let model = st.toc_model.as_ref().unwrap();
                for (row, entry) in st.deck.toc.iter().enumerate() {
                    let active = toc_is_active(entry);
                    if let Some(mut item) = model.row_data(row) {
                        if item.is_active != active {
                            item.is_active = active;
                            model.set_row_data(row, item);
                        }
                    }
                }
                None
            };

            let next_img = if current_idx + 1 < total {
                slide_presenter_image(&st.deck.slides[current_idx + 1])
            } else {
                end_of_deck_image()
            };

            let has_media = !current_slide.media_clips.is_empty();
            let clip_count = current_slide.media_clips.len();
            let clip_index = st.active_clip_index();
            let first_clip = st.active_clip();
            let slide_aspect = current_slide.aspect_ratio;
            let is_dual_screen = current_slide.is_dual_screen;
            let slide_notes = current_slide.notes.clone();
            let (has_notes_img, slint_notes_img) =
                if let Some(notes_img) = &current_slide.notes_image {
                    (true, cached_slint_image(notes_img))
                } else {
                    (false, slint::Image::default())
                };

            let mut media_rect = [0.15f32, 0.26f32, 0.70f32, 0.50f32];
            let media_label = if let Some(first) = first_clip {
                media_rect = first.rect;
                if first.is_resolved {
                    // Never probe on the UI thread: use the prefetched value, or let the
                    // timer tick pick it up once the background probe finishes.
                    let known = lock(&st.media_durations).get(&first.file_path).copied();
                    match known {
                        Some(d) => {
                            st.media_duration_secs = d;
                            st.media_duration_pending = None;
                        }
                        None => {
                            st.media_duration_pending = Some(first.file_path.clone());
                            probe_media_durations(
                                vec![first.file_path.clone()],
                                &st.media_durations,
                            );
                        }
                    }
                    first.display_path
                } else {
                    format!("{} (Not Found)", first.display_path)
                }
            } else {
                String::new()
            };

            let doc_title = if let Some(path) = &st.deck.file_path {
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            } else {
                "Interactive Beamer Presentation.pdf".to_string()
            };

            // Update Presenter Window
            if let Some(p) = presenter_handle.upgrade() {
                p.set_slide_aspect(slide_aspect);
                p.set_document_title(SharedString::from(&doc_title));
                p.set_current_slide((current_idx + 1) as i32);
                p.set_total_slides(total as i32);
                p.set_current_slide_image(disp_presenter.clone());
                p.set_next_slide_image(next_img);
                p.set_has_notes_image(has_notes_img);
                p.set_is_dual_screen(is_dual_screen);
                p.set_notes_slide_image(slint_notes_img.clone());
                p.set_notes_text(SharedString::from(&slide_notes));
                p.set_render_quality(SharedString::from(st.deck.quality.label()));
                p.set_has_source_file(st.deck.file_path.is_some());
                p.set_sync_with_source(st.sync_with_source);
                p.set_current_has_media(has_media);
                p.set_media_clip_count(clip_count as i32);
                p.set_media_clip_index(clip_index as i32);
                p.set_media_status_text(SharedString::from(&media_label));
                p.set_media_playing(false);
                p.set_media_video_visible(false);
                p.set_media_rect_x(media_rect[0]);
                p.set_media_rect_y(media_rect[1]);
                p.set_media_rect_w(media_rect[2]);
                p.set_media_rect_h(media_rect[3]);
                p.set_media_progress(0.0);
                p.set_media_time_text(SharedString::from(&st.format_media_time()));
                p.set_media_speed_text(SharedString::from(&st.format_media_speed()));
                p.set_media_speech_active(st.media_speech_active);

                p.set_blank_mode(st.blank_mode);
                p.set_is_end_of_show(st.is_end_of_show);
                p.set_is_screen_swapped(st.is_screen_swapped);
                p.set_laser_fade_duration_s(st.laser_fade_duration_s);
                p.set_laser_fade_duration_text(SharedString::from(format!(
                    "{:.1}s",
                    st.laser_fade_duration_s
                )));
                p.set_current_frame(cur_frame as i32);
                p.set_total_frames(tot_frames as i32);
                p.set_overlay_step(over_step as i32);
                p.set_overlay_total(over_tot as i32);
                p.set_frame_info_text(SharedString::from(&frame_info));
                if let Some(model) = &new_toc_model {
                    p.set_toc_items(slint::ModelRc::from(model.clone()));
                }

                // Spectrogram / Waveform Progress Box
                let track_box = slide_track_box;
                p.set_media_track_box_defined(track_box.is_some());
                if let Some(b) = track_box {
                    p.set_media_track_box_x(b[0]);
                    p.set_media_track_box_y(b[1]);
                    p.set_media_track_box_w(b[2]);
                    p.set_media_track_box_h(b[3]);
                }
                p.set_media_track_tool_active(st.media_tracking_tool_active);

                p.set_hovered_on_link(false);
                p.set_hovered_link_tooltip(SharedString::new());
                p.set_link_holding(false);
                p.set_link_hold_progress(0.0);
                p.set_zoom_level(st.zoom_level);
                p.set_zoom_pan_x(st.zoom_pan_x);
                p.set_zoom_pan_y(st.zoom_pan_y);
            }

            // Update Audience Window
            if let Some(a) = audience_handle.upgrade() {
                a.set_slide_aspect(slide_aspect);
                a.set_slide_image(disp_audience);
                a.set_blank_mode(st.blank_mode);
                a.set_is_end_of_show(st.is_end_of_show);
                a.set_media_video_visible(false);
                a.set_media_rect_x(media_rect[0]);
                a.set_media_rect_y(media_rect[1]);
                a.set_media_rect_w(media_rect[2]);
                a.set_media_rect_h(media_rect[3]);
                a.set_zoom_level(st.zoom_level);
                a.set_zoom_pan_x(st.zoom_pan_x);
                a.set_zoom_pan_y(st.zoom_pan_y);
                let col = st.active_color;
                a.set_tracker_color(slint::Color::from_argb_u8(col[3], col[0], col[1], col[2]));
                let track_box = slide_track_box;
                a.set_media_track_box_defined(track_box.is_some());
                if let Some(b) = track_box {
                    a.set_media_track_box_x(b[0]);
                    a.set_media_track_box_y(b[1]);
                    a.set_media_track_box_w(b[2]);
                    a.set_media_track_box_h(b[3]);
                }
                a.set_media_playing(false);
                a.set_media_progress(0.0);
                a.set_media_track_show_box(st.media_track_show_audience_box);
                a.set_media_track_needle_width(st.media_track_needle_width);
                a.set_media_track_needle_glow(st.media_track_needle_glow);
                a.set_laser_color(slint::Color::from_argb_u8(
                    st.laser_color_rgba[3],
                    st.laser_color_rgba[0],
                    st.laser_color_rgba[1],
                    st.laser_color_rgba[2],
                ));
            }

            // Update Notes Window
            if let Some(n) = notes_handle.upgrade() {
                n.set_slide_num((current_idx + 1) as i32);
                n.set_total_slides(total as i32);
                n.set_has_notes_image(has_notes_img);
                n.set_is_dual_screen(is_dual_screen);
                n.set_notes_slide_image(slint_notes_img);
                n.set_notes_body(SharedString::from(&slide_notes));
                n.set_mini_slide(slint_current);
            }

            if new_toc_model.is_some() {
                st.toc_model = new_toc_model;
            }
            // Evict on every navigation, even if rapid input keeps postponing idle
            // prewarming. Otherwise the display cache could grow without bound.
            let window = current_idx.saturating_sub(1)..=current_idx + 2;
            let keep: Vec<&Arc<crate::pdf::SlideImage>> = st.deck.slides
                [*window.start()..(*window.end() + 1).min(st.deck.slides.len())]
                .iter()
                .flat_map(|s| {
                    [&s.audience_image, &s.presenter_image]
                        .into_iter()
                        .chain(s.notes_image.as_ref())
                })
                .collect();
            evict_gpu_images_except(&keep);
            drop(st);
            update_ink();

            // Prewarm neighbouring slides' GPU images while idle so the next step is instant.
            let state_pw = state.clone();
            prewarm_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(30),
                move || {
                    let st = state_pw.borrow();
                    let idx = st.deck.current_index;
                    for i in [idx + 1, idx.wrapping_sub(1), idx + 2] {
                        if let Some(s) = st.deck.slides.get(i) {
                            slide_presenter_image(s);
                            slide_audience_image(s);
                            if let Some(n) = &s.notes_image {
                                cached_slint_image(n);
                            }
                        }
                    }
                },
            );
        })
    };

    update_slide_view
}
