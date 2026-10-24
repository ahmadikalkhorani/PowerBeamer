//! Inking/laser canvas: pointer handlers, laser decay timer and hold-to-shape / link timer.

use std::time::Duration;

use slint::ComponentHandle;

use super::context::AppContext;
use super::ui::*;
use crate::ink::{self, LaserTrail, Point, Stroke};
use crate::pdf::PdfLinkTarget;
use crate::*;

/// Wires canvas input and its animation timers.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext { state, presenter_window, audience_window, update_ink_overlay, update_slide_view, .. } = ctx;

    // --- LASER TRAIL DECAY TIMER (16ms / 60 FPS) ---
    let laser_decay_timer = slint::Timer::default();
    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        laser_decay_timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
            let mut st = state.borrow_mut();
            if let Some(trail) = st.laser_trail.as_mut() {
                if !trail.is_empty() {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    trail.prune(now_ms);
                    if trail.is_empty() {
                        st.laser_trail = None;
                    }
                    drop(st);
                    update_ink();
                }
            }
        });
    }

    // --- DRAW AND HOLD AUTO SHAPE & LINK INTERACTIVE TIMER (25ms) ---
    let shape_check_timer = slint::Timer::default();
    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let update_view = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        shape_check_timer.start(slint::TimerMode::Repeated, Duration::from_millis(25), move || {
            let mut st = state.borrow_mut();

            // Check interactive PDF link touch & hold (activates on ~380ms hold)
            if st.pointer_is_down && !st.link_triggered {
                if let Some(link) = st.active_link_target.clone() {
                    if let Some(start_time) = st.link_press_start {
                        let elapsed_ms = start_time.elapsed().as_millis() as f32;
                        let hold_thresh = 380.0;
                        let progress = (elapsed_ms / hold_thresh).min(1.0);

                        if let Some(p) = presenter_handle.upgrade() {
                            p.set_link_hold_progress(progress);
                            p.set_link_holding(true);
                        }

                        if elapsed_ms >= hold_thresh {
                            st.link_triggered = true;
                            st.link_press_start = None;
                            st.active_stroke = None;
                            st.raw_stroke_backup.clear();

                            if let Some(p) = presenter_handle.upgrade() {
                                p.set_link_holding(false);
                                p.set_link_hold_progress(0.0);
                            }

                            match link.target {
                                PdfLinkTarget::Url(url) => {
                                    println!("Touch & Hold triggered PDF link: {}", url);
                                    open_external_url(&url);
                                    drop(st);
                                    update_ink();
                                    return;
                                }
                                PdfLinkTarget::Page(dest_idx) => {
                                    println!("Touch & Hold triggered PDF destination: jump to slide {}", dest_idx + 1);
                                    if st.deck.jump_to(dest_idx) {
                                        drop(st);
                                        update_view();
                                        update_ink();
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !st.pointer_is_down || st.is_shape_snapped || !(st.auto_shape_enabled || st.scribble_to_erase) {
                return;
            }
            let elapsed_ms = st.last_pointer_move_time.elapsed().as_millis() as u64;
            if elapsed_ms < st.hold_delay_ms {
                return;
            }

            // Check scribble to erase first if enabled
            if st.scribble_to_erase && ink::ShapeRecognizer::is_scribble(&st.raw_stroke_backup) {
                println!("Scribble-to-erase triggered on hold!");
                let (min_x, min_y, max_x, max_y) = ink::ShapeRecognizer::bounding_box(&st.raw_stroke_backup);
                let cx = (min_x + max_x) * 0.5;
                let cy = (min_y + max_y) * 0.5;
                let r = ((max_x - min_x).max(max_y - min_y) * 0.7).max(0.04);
                st.deck.current_slide_mut().inking.erase_at(cx, cy, r);
                st.active_stroke = None;
                st.is_shape_snapped = true;
                drop(st);
                update_ink();
                return;
            }

            if !st.auto_shape_enabled {
                return;
            }

            // Auto shape recognition: snap to regular shape while pointer is held down!
            let aspect = st.deck.current_slide().aspect_ratio;
            if let Some(stroke) = st.active_stroke.as_mut() {
                if stroke.points.len() >= 3 {
                    if let Some(recognized) = ink::ShapeRecognizer::recognize_with_aspect(&stroke.points, stroke.is_highlighter, aspect) {
                        println!("Auto Shape recognized while holding: {:?}", recognized.kind);
                        stroke.points = recognized.points;
                        stroke.is_shape = true;
                        stroke.recompute_bounding_box();
                        st.is_shape_snapped = true;
                        drop(st);
                        update_ink();
                    }
                }
            }
        });
    }

    // --- INKING & LASER CANVAS POINTER HANDLERS ---
    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();

        presenter_window.on_canvas_pointer_down(move |x, y, pressure| {
            let mut st = state.borrow_mut();
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            st.pointer_is_down = true;
            st.last_pointer_move_time = std::time::Instant::now();
            st.hold_anchor_pos = (x, y);
            st.is_shape_snapped = false;
            st.raw_stroke_backup.clear();

            // Detect interactive PDF link on pointer down
            let hit_link = st.deck.current_slide().find_link_at(x, y, 0.015).cloned();
            st.active_link_target = hit_link.clone();
            st.link_press_start = if hit_link.is_some() { Some(std::time::Instant::now()) } else { None };
            st.link_triggered = false;

            if let Some(ref link) = hit_link {
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_link_holding(true);
                    p.set_link_hold_progress(0.0);
                    p.set_hovered_link_tooltip(slint::SharedString::from(&link.tooltip));
                }
            }

            match st.current_tool {
                ActiveTool::Pen => {
                    st.stroke_counter += 1;
                    let mut stroke = Stroke::new(st.stroke_counter, st.active_color, st.stroke_width, false);
                    let pt = Point::new(x, y, pressure, now_ms);
                    stroke.add_point(pt);
                    st.raw_stroke_backup.push(pt);
                    st.active_stroke = Some(stroke);
                    drop(st);
                    update_ink();
                }
                ActiveTool::Highlighter => {
                    st.stroke_counter += 1;
                    let mut stroke = Stroke::new(st.stroke_counter, st.active_color, st.stroke_width, true);
                    stroke.is_tape = st.highlighter_mode == 1;
                    if st.highlighter_straight_lines {
                        stroke.is_shape = true;
                    }
                    let pt = Point::new(x, y, pressure, now_ms);
                    stroke.add_point(pt);
                    st.raw_stroke_backup.push(pt);
                    st.active_stroke = Some(stroke);
                    drop(st);
                    update_ink();
                }

                ActiveTool::Eraser => {
                    let opts = ink::EraserOptions {
                        radius: st.eraser_radius,
                        erase_whole_line: st.erase_whole_line,
                        erase_highlight_only: st.erase_highlight_only,
                        erase_shapes: st.erase_shapes,
                        erase_tapes: st.erase_tapes,
                    };
                    let erased = st.deck.current_slide_mut().inking.erase_at_with_options(x, y, &opts);
                    if erased {
                        drop(st);
                        update_ink();
                    }
                }
                ActiveTool::Laser => {
                    // Check if tapping on any Tape stroke to toggle reveal / cover
                    let mut toggled_tape = false;
                    for stroke in &mut st.deck.current_slide_mut().inking.strokes {
                        if stroke.is_tape && stroke.intersects_circle(x, y, 0.03) {
                            stroke.is_tape_revealed = !stroke.is_tape_revealed;
                            toggled_tape = true;
                            println!("Tape #{} toggled: revealed = {}", stroke.id, stroke.is_tape_revealed);
                            break;
                        }
                    }
                    if toggled_tape {
                        drop(st);
                        update_ink();
                        return;
                    }

                    st.laser_active = true;
                    let duration = if st.laser_draw_lines {
                        (st.laser_fade_duration_s * 1000.0).round() as u64
                    } else {
                        (st.laser_fade_duration_s * 1000.0).round().min(350.0) as u64
                    };
                    let mut trail = st.laser_trail.take().unwrap_or_else(|| {
                        LaserTrail::new(duration, st.laser_color_rgba, st.laser_draw_lines)
                    });
                    trail.duration_ms = duration;
                    trail.color_rgba = st.laser_color_rgba;
                    trail.draw_lines = st.laser_draw_lines;
                    trail.start_stroke();
                    trail.add(x, y, now_ms);
                    st.laser_trail = Some(trail);

                    if let Some(p) = presenter_handle.upgrade() {
                        p.set_laser_active(true);
                        p.set_laser_x(x);
                        p.set_laser_y(y);
                    }
                    if let Some(aud) = audience_handle.upgrade() {
                        aud.set_laser_visible(true);
                        aud.set_laser_x(x);
                        aud.set_laser_y(y);
                    }
                    drop(st);
                    update_ink();
                }
                ActiveTool::Select => {
                    st.hold_anchor_pos = (x, y);
                    st.selection_box = Some((x, y, x, y));
                }
                _ => {}
            }
        });
    }

    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();

        presenter_window.on_canvas_pointer_move(move |x, y, pressure| {
            let mut st = state.borrow_mut();
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            if !st.pointer_is_down {
                let hover_link = st.deck.current_slide().find_link_at(x, y, 0.015);
                if let Some(p) = presenter_handle.upgrade() {
                    if let Some(link) = hover_link {
                        p.set_hovered_on_link(true);
                        let hint = if st.current_tool == ActiveTool::Laser { "Click to open" } else { "Touch & hold to open" };
                        p.set_hovered_link_tooltip(slint::SharedString::from(format!("{} ({})", link.tooltip, hint)));
                    } else {
                        p.set_hovered_on_link(false);
                        p.set_hovered_link_tooltip(slint::SharedString::new());
                    }
                }
            } else if st.active_link_target.is_some() {
                let dist = ((x - st.hold_anchor_pos.0).powi(2) + (y - st.hold_anchor_pos.1).powi(2)).sqrt();
                if dist > 0.02 {
                    st.active_link_target = None;
                    st.link_press_start = None;
                    if let Some(p) = presenter_handle.upgrade() {
                        p.set_link_holding(false);
                        p.set_link_hold_progress(0.0);
                    }
                }
            }

            if st.active_stroke.is_none() {
                if st.current_tool == ActiveTool::Select && st.pointer_is_down {
                    if let Some(ref mut sel_box) = st.selection_box {
                        sel_box.2 = x;
                        sel_box.3 = y;
                    }
                    return;
                } else if st.current_tool == ActiveTool::Eraser && pressure > 0.0 {
                    let opts = ink::EraserOptions {
                        radius: st.eraser_radius,
                        erase_whole_line: st.erase_whole_line,
                        erase_highlight_only: st.erase_highlight_only,
                        erase_shapes: st.erase_shapes,
                        erase_tapes: st.erase_tapes,
                    };
                    let erased = st.deck.current_slide_mut().inking.erase_at_with_options(x, y, &opts);
                    if erased {
                        st.cached_ink_overlay = None;
                        drop(st);
                        update_ink();
                    }
                } else if st.current_tool == ActiveTool::Laser {
                    if st.pointer_is_down && pressure > 0.0 {
                        let duration = if st.laser_draw_lines {
                            (st.laser_fade_duration_s * 1000.0).round() as u64
                        } else {
                            (st.laser_fade_duration_s * 1000.0).round().min(350.0) as u64
                        };
                        let mut trail = st.laser_trail.take().unwrap_or_else(|| {
                            LaserTrail::new(duration, st.laser_color_rgba, st.laser_draw_lines)
                        });
                        trail.duration_ms = duration;
                        trail.color_rgba = st.laser_color_rgba;
                        trail.draw_lines = st.laser_draw_lines;
                        trail.add(x, y, now_ms);
                        st.laser_trail = Some(trail);

                        if let Some(aud) = audience_handle.upgrade() {
                            aud.set_laser_visible(true);
                            aud.set_laser_x(x);
                            aud.set_laser_y(y);
                        }

                        // Line trails are vector paths and cheap to update on every move; the
                        // rasterized comet tail is still paced to ~60 FPS.
                        if st.laser_draw_lines || st.last_laser_update.elapsed().as_millis() >= 16 {
                            st.last_laser_update = std::time::Instant::now();
                            drop(st);
                            update_ink();
                        }
                    } else {
                        if let Some(aud) = audience_handle.upgrade() {
                            aud.set_laser_visible(false);
                        }
                    }
                    return;
                }
                return;
            }

            // Direct "Draw Straight Lines" mode for Highlighter and Tape
            if st.current_tool == ActiveTool::Highlighter && st.highlighter_straight_lines {
                if let Some(stroke) = st.active_stroke.as_mut() {
                    let start_p = stroke.points[0];
                    let dx = (x - start_p.x).abs();
                    let dy = (y - start_p.y).abs();
                    // Text line horizontal snap: lock end_y to start_p.y when moving roughly horizontally
                    let end_y = if dy < 0.04 || (dx > 0.02 && dy < dx * 0.35) {
                        start_p.y
                    } else {
                        y
                    };
                    let end_pt = Point::new(x, end_y, pressure, now_ms);
                    stroke.points = vec![start_p, end_pt];
                    stroke.is_shape = true;
                    stroke.recompute_bounding_box();
                    drop(st);
                    update_ink();
                    return;
                }
            }

            if st.is_shape_snapped {
                let stroke = st.active_stroke.as_mut().unwrap();
                if stroke.points.len() == 2 {
                    stroke.points[1].x = x;
                    if stroke.is_highlighter {
                        let dy = (y - stroke.points[0].y).abs();
                        let dx = (x - stroke.points[0].x).abs();
                        if dy < 0.04 || dy < dx * 0.35 {
                            stroke.points[1].y = stroke.points[0].y;
                        } else {
                            stroke.points[1].y = y;
                        }
                    } else {
                        stroke.points[1].y = y;
                    }
                    stroke.recompute_bounding_box();
                    drop(st);
                    update_ink();
                }
                return;
            }

            let dist = ((x - st.hold_anchor_pos.0).powi(2) + (y - st.hold_anchor_pos.1).powi(2)).sqrt();
            if dist > 0.012 {
                st.hold_anchor_pos = (x, y);
                st.last_pointer_move_time = std::time::Instant::now();
            }

            // Straight highlighting assistance across text lines
            let mut pt_y = y;
            if st.current_tool == ActiveTool::Highlighter && !st.raw_stroke_backup.is_empty() {
                let start_p = st.raw_stroke_backup[0];
                let dx = (x - start_p.x).abs();
                let dy = (y - start_p.y).abs();
                if dx > 0.025 && dy < 0.035 {
                    pt_y = start_p.y;
                }
            }

            let p = Point::new(x, pt_y, pressure, now_ms);
            st.raw_stroke_backup.push(p);
            st.active_stroke.as_mut().unwrap().add_point(p);
            drop(st);
            update_ink();
        });
    }

    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let update_view = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();

        presenter_window.on_canvas_pointer_up(move || {
            let mut st = state.borrow_mut();
            st.pointer_is_down = false;

            if let Some(p) = presenter_handle.upgrade() {
                p.set_link_holding(false);
                p.set_link_hold_progress(0.0);
            }

            // Check if pointer up activates an interactive link (click or tap on link)
            if !st.link_triggered {
                if let Some(link) = st.active_link_target.take() {
                    let elapsed_ms = st.last_pointer_move_time.elapsed().as_millis() as u64;
                    let is_click = elapsed_ms < 400;
                    let is_click_or_laser = is_click || st.current_tool == ActiveTool::Laser;
                    if is_click_or_laser {
                        st.active_stroke = None;
                        st.raw_stroke_backup.clear();
                        st.link_triggered = true;

                        match link.target {
                            PdfLinkTarget::Url(url) => {
                                println!("Clicked PDF link: {}", url);
                                open_external_url(&url);
                            }
                            PdfLinkTarget::Page(dest_idx) => {
                                println!("Clicked PDF destination: jump to slide {}", dest_idx + 1);
                                if st.deck.jump_to(dest_idx) {
                                    drop(st);
                                    update_view();
                                    update_ink();
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            st.active_link_target = None;

            let elapsed_ms = st.last_pointer_move_time.elapsed().as_millis() as u64;
            let should_snap = !st.is_shape_snapped && st.auto_shape_enabled && elapsed_ms >= st.hold_delay_ms;

            if should_snap {
                let aspect = st.deck.current_slide().aspect_ratio;
                if let Some(stroke) = st.active_stroke.as_mut() {
                    if stroke.points.len() >= 3 {
                        if let Some(recognized) = ink::ShapeRecognizer::recognize_with_aspect(&stroke.points, stroke.is_highlighter, aspect) {
                            stroke.points = recognized.points;
                            stroke.is_shape = true;
                            stroke.recompute_bounding_box();
                        }
                    }
                }
            }

            if st.current_tool == ActiveTool::Highlighter && st.highlighter_straight_lines {
                if let Some(stroke) = st.active_stroke.as_mut() {
                    stroke.is_shape = true;
                    stroke.recompute_bounding_box();
                }
            }

            let has_stroke = if let Some(stroke) = st.active_stroke.take() {
                st.deck.current_slide_mut().inking.add_stroke(stroke);
                true
            } else {
                false
            };
            st.is_shape_snapped = false;
            st.raw_stroke_backup.clear();

            if st.current_tool == ActiveTool::Select {
                if let Some((x1, y1, x2, y2)) = st.selection_box.take() {
                    let min_x = x1.min(x2);
                    let max_x = x1.max(x2);
                    let min_y = y1.min(y2);
                    let max_y = y1.max(y2);
                    let box_width = max_x - min_x;
                    let box_height = max_y - min_y;

                    if box_width > 0.01 && box_height > 0.01 {
                        // Coordinates are normalized slide coords (0..1 on both axes),
                        // so the zoom that fits the box is simply 1 / box extent.
                        let zoom_x = 1.0 / box_width;
                        let zoom_y = 1.0 / box_height;
                        let new_zoom = zoom_x.min(zoom_y).max(1.0).min(10.0);

                        // Keep the zoomed viewport inside the slide bounds
                        let half = 0.5 / new_zoom;
                        let center_x = ((min_x + max_x) / 2.0).clamp(half, 1.0 - half);
                        let center_y = ((min_y + max_y) / 2.0).clamp(half, 1.0 - half);

                        st.zoom_level = new_zoom;
                        st.zoom_pan_x = center_x;
                        st.zoom_pan_y = center_y;
                        println!("Selected zoom: {:.2}x at ({:.3}, {:.3})", new_zoom, center_x, center_y);
                        drop(st);
                        update_view();
                        update_ink();
                        return;
                    }
                }
                st.selection_box = None;
                return;
            }

            if st.current_tool == ActiveTool::Laser {
                st.laser_active = false;
                if let Some(trail) = st.laser_trail.as_mut() {
                    trail.end_stroke();
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_laser_active(false);
                }
                if let Some(aud) = audience_handle.upgrade() {
                    aud.set_laser_visible(false);
                }
                drop(st);
                update_ink();
                return;
            }

            if st.current_tool == ActiveTool::Eraser && st.auto_select_previous_tool {
                let prev = st.previous_tool;
                st.current_tool = prev;
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_current_tool(prev);
                }
                println!("Auto-selected previous tool: {:?}", prev);
            }
            drop(st);
            if has_stroke {
                update_ink();
            }
        });
    }

    // --- AUDIENCE CANVAS INPUT ---
    // Forwarded to the presenter handlers above so both windows share one inking/laser path.
    // The presenter's laser dot is placed here because its own TouchArea isn't the one moving.
    {
        let presenter_handle = presenter_window.as_weak();
        audience_window.on_canvas_pointer_down(move |x, y, pressure| {
            if let Some(p) = presenter_handle.upgrade() {
                p.set_laser_x(x);
                p.set_laser_y(y);
                p.invoke_canvas_pointer_down(x, y, pressure);
            }
        });
    }
    {
        let presenter_handle = presenter_window.as_weak();
        audience_window.on_canvas_pointer_move(move |x, y, pressure| {
            if let Some(p) = presenter_handle.upgrade() {
                p.set_laser_x(x);
                p.set_laser_y(y);
                p.invoke_canvas_pointer_move(x, y, pressure);
            }
        });
    }
    {
        let presenter_handle = presenter_window.as_weak();
        audience_window.on_canvas_pointer_up(move || {
            if let Some(p) = presenter_handle.upgrade() {
                p.invoke_canvas_pointer_up();
            }
        });
    }

    {
        let presenter_handle = presenter_window.as_weak();
        audience_window.on_key_forward(move |text, ctrl, meta, shift| {
            presenter_handle
                .upgrade()
                .map(|p| p.invoke_handle_key(text, ctrl, meta, shift))
                .unwrap_or(false)
        });
    }

    vec![laser_decay_timer, shape_check_timer]
}
