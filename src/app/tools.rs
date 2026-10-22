//! Drawing tool settings: tool selection, stroke width, colors, undo/redo, pen, highlighter, eraser, laser.


use slint::{ComponentHandle, SharedString};

use super::context::AppContext;
use crate::*;

/// Wires tool and color callbacks.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext { state, presenter_window, audience_window, update_ink_overlay, .. } = ctx;

    // --- STROKE WIDTH CHANGED ---
    {
        let state = state.clone();
        presenter_window.on_width_changed(move |len| {
            let mut st = state.borrow_mut();
            st.stroke_width = (len / 1.0).max(1.0);
            println!("Updated stroke width: {}px", st.stroke_width);
        });
    }

    // --- TOOL SELECTION ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_tool_selected(move |tool| {
            let mut st = state.borrow_mut();
            if st.current_tool != ActiveTool::Eraser && tool == ActiveTool::Eraser {
                st.previous_tool = st.current_tool;
            }
            st.current_tool = tool;
            if tool == ActiveTool::Laser {
                let color = st.active_color;
                st.laser_color_rgba = color;
                if let Some(trail) = st.laser_trail.as_mut() {
                    trail.color_rgba = color;
                }
                let c = slint::Color::from_argb_u8(color[3], color[0], color[1], color[2]);
                if let Some(p) = pres_handle.upgrade() {
                    p.set_laser_color(c);
                }
                if let Some(aud) = audience_handle.upgrade() {
                    aud.set_laser_color(c);
                }
            }
            if tool == ActiveTool::Select {
                st.zoom_level = 1.0;
                st.zoom_pan_x = 0.0;
                st.zoom_pan_y = 0.0;
            }
            println!("Selected tool: {:?}", tool);
        });
    }

    // --- COLOR PICKER & COLOR WHEEL ---
    fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
        if s <= 0.0 {
            let val = (v * 255.0).round().clamp(0.0, 255.0) as u8;
            return (val, val, val);
        }
        let h_norm = ((h % 360.0) + 360.0) % 360.0 / 60.0;
        let i = h_norm.floor() as i32;
        let f = h_norm - i as f32;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        let (r, g, b) = match i % 6 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        (
            (r * 255.0).round().clamp(0.0, 255.0) as u8,
            (g * 255.0).round().clamp(0.0, 255.0) as u8,
            (b * 255.0).round().clamp(0.0, 255.0) as u8,
        )
    }

    fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
        let rf = r as f32 / 255.0;
        let gf = g as f32 / 255.0;
        let bf = b as f32 / 255.0;
        let max = rf.max(gf).max(bf);
        let min = rf.min(gf).min(bf);
        let delta = max - min;
        let v = max;
        let s = if max > 0.0 { delta / max } else { 0.0 };
        let mut h = if delta == 0.0 {
            0.0
        } else if (max - rf).abs() < 1e-4 {
            60.0 * (((gf - bf) / delta) % 6.0)
        } else if (max - gf).abs() < 1e-4 {
            60.0 * (((bf - rf) / delta) + 2.0)
        } else {
            60.0 * (((rf - gf) / delta) + 4.0)
        };
        if h < 0.0 {
            h += 360.0;
        }
        (h, s, v)
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_color_changed(move |c| {
            let mut st = state.borrow_mut();
            let rgba = [c.red(), c.green(), c.blue(), c.alpha()];
            st.active_color = rgba;
            st.laser_color_rgba = rgba;
            match st.active_color_slot {
                0 => st.color_slot_1 = rgba,
                1 => st.color_slot_2 = rgba,
                2 => st.color_slot_3 = rgba,
                3 => st.color_slot_4 = rgba,
                4 => st.color_slot_5 = rgba,
                _ => {}
            }
            if let Some(trail) = st.laser_trail.as_mut() {
                trail.color_rgba = rgba;
            }
            let (h, s, v) = rgb_to_hsv(c.red(), c.green(), c.blue());
            let (pr, pg, pb) = hsv_to_rgb(h, s, 1.0);
            let cx = 60.0f32;
            let cy = 60.0f32;
            let max_r = 56.0f32;
            let rad = h.to_radians();
            let tx = cx + rad.cos() * s * max_r;
            let ty = cy + rad.sin() * s * max_r;

            if let Some(p) = pres_handle.upgrade() {
                p.set_active_color(c);
                p.set_laser_color(c);
                p.set_wheel_hue(h);
                p.set_wheel_sat(s);
                p.set_wheel_val(v);
                p.set_wheel_pure_color(slint::Color::from_rgb_u8(pr, pg, pb));
                p.set_wheel_thumb_x(tx * 1.0);
                p.set_wheel_thumb_y(ty * 1.0);
            }
            if let Some(aud) = audience_handle.upgrade() {
                aud.set_laser_color(c);
            }
            println!("Updated stroke and laser color: RGBA({}, {}, {}, {})", c.red(), c.green(), c.blue(), c.alpha());
        });
    }

    // --- COLOR WHEEL TOUCHED ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_color_wheel_touched(move |mx, my| {
            let cx = 60.0f32;
            let cy = 60.0f32;
            let max_r = 56.0f32;
            let dx = (mx / 1.0) - cx;
            let dy = (my / 1.0) - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let clamped_dist = dist.min(max_r);
            let angle = dy.atan2(dx);
            let mut hue = angle.to_degrees();
            if hue < 0.0 { hue += 360.0; }
            let sat = (clamped_dist / max_r).clamp(0.0, 1.0);

            if let Some(p) = pres_handle.upgrade() {
                let val = p.get_wheel_val();
                let (r, g, b) = hsv_to_rgb(hue, sat, val);
                let (pr, pg, pb) = hsv_to_rgb(hue, sat, 1.0);
                let selected_color = slint::Color::from_rgb_u8(r, g, b);
                let pure_color = slint::Color::from_rgb_u8(pr, pg, pb);

                let thumb_x = cx + angle.cos() * clamped_dist;
                let thumb_y = cy + angle.sin() * clamped_dist;

                p.set_wheel_hue(hue);
                p.set_wheel_sat(sat);
                p.set_wheel_pure_color(pure_color);
                p.set_wheel_thumb_x(thumb_x * 1.0);
                p.set_wheel_thumb_y(thumb_y * 1.0);
                p.set_active_color(selected_color);
                p.set_laser_color(selected_color);

                let mut st = state.borrow_mut();
                let rgba = [r, g, b, 255];
                st.active_color = rgba;
                st.laser_color_rgba = rgba;
                match st.active_color_slot {
                    0 => { st.color_slot_1 = rgba; p.set_color_slot_1(selected_color); }
                    1 => { st.color_slot_2 = rgba; p.set_color_slot_2(selected_color); }
                    2 => { st.color_slot_3 = rgba; p.set_color_slot_3(selected_color); }
                    3 => { st.color_slot_4 = rgba; p.set_color_slot_4(selected_color); }
                    4 => { st.color_slot_5 = rgba; p.set_color_slot_5(selected_color); }
                    _ => {}
                }
                if let Some(trail) = st.laser_trail.as_mut() {
                    trail.color_rgba = rgba;
                }
            }
            if let Some(aud) = audience_handle.upgrade() {
                if let Some(p) = pres_handle.upgrade() {
                    aud.set_laser_color(p.get_active_color());
                }
            }
        });
    }

    // --- COLOR BRIGHTNESS SLIDER TOUCHED ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_color_brightness_touched(move |val| {
            let val_clamped = val.clamp(0.0, 1.0);
            if let Some(p) = pres_handle.upgrade() {
                let hue = p.get_wheel_hue();
                let sat = p.get_wheel_sat();
                let (r, g, b) = hsv_to_rgb(hue, sat, val_clamped);
                let selected_color = slint::Color::from_rgb_u8(r, g, b);

                p.set_wheel_val(val_clamped);
                p.set_active_color(selected_color);
                p.set_laser_color(selected_color);

                let mut st = state.borrow_mut();
                let rgba = [r, g, b, 255];
                st.active_color = rgba;
                st.laser_color_rgba = rgba;
                match st.active_color_slot {
                    0 => { st.color_slot_1 = rgba; p.set_color_slot_1(selected_color); }
                    1 => { st.color_slot_2 = rgba; p.set_color_slot_2(selected_color); }
                    2 => { st.color_slot_3 = rgba; p.set_color_slot_3(selected_color); }
                    3 => { st.color_slot_4 = rgba; p.set_color_slot_4(selected_color); }
                    4 => { st.color_slot_5 = rgba; p.set_color_slot_5(selected_color); }
                    _ => {}
                }
                if let Some(trail) = st.laser_trail.as_mut() {
                    trail.color_rgba = rgba;
                }
            }
            if let Some(aud) = audience_handle.upgrade() {
                if let Some(p) = pres_handle.upgrade() {
                    aud.set_laser_color(p.get_active_color());
                }
            }
        });
    }

    // --- ADD CUSTOM COLOR TO PALETTE ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_add_custom_color(move |c| {
            let mut st = state.borrow_mut();
            let new_rgba = [c.red(), c.green(), c.blue(), c.alpha()];
            if !st.custom_palette.iter().any(|&col| col == new_rgba) {
                st.custom_palette.push(new_rgba);
                println!("Added custom color to palette: RGBA({}, {}, {}, {})", c.red(), c.green(), c.blue(), c.alpha());
            }
            if let Some(p) = pres_handle.upgrade() {
                let slint_colors: Vec<slint::Color> = st.custom_palette.iter().map(|&rgba| {
                    slint::Color::from_argb_u8(rgba[3], rgba[0], rgba[1], rgba[2])
                }).collect();
                p.set_custom_palette(std::rc::Rc::new(slint::VecModel::from(slint_colors)).into());
            }
        });
    }

    // --- RESET ZOOM (Z) ---
    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_reset_zoom(move || {
            let mut st = state.borrow_mut();
            st.zoom_level = 1.0;
            st.zoom_pan_x = 0.0;
            st.zoom_pan_y = 0.0;
            st.selection_box = None;
            if let Some(p) = presenter_handle.upgrade() {
                p.set_zoom_level(1.0);
                p.set_zoom_pan_x(0.0);
                p.set_zoom_pan_y(0.0);
            }
            if let Some(a) = audience_handle.upgrade() {
                a.set_zoom_level(1.0);
                a.set_zoom_pan_x(0.0);
                a.set_zoom_pan_y(0.0);
            }
            println!("Zoom reset to full slide");
        });
    }

    // --- MIDDLE-MOUSE PAN ---
    {
        let state = state.clone();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_zoom_panned(move |x, y| {
            let mut st = state.borrow_mut();
            st.zoom_pan_x = x;
            st.zoom_pan_y = y;
            if let Some(a) = audience_handle.upgrade() {
                a.set_zoom_pan_x(x);
                a.set_zoom_pan_y(y);
            }
        });
    }

    // --- UNDO & REDO ---
    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        presenter_window.on_undo(move || {
            let mut st = state.borrow_mut();
            if st.deck.current_slide_mut().inking.undo() {
                st.cached_ink_overlay = None;
                drop(st);
                update_ink();
            }
        });
    }

    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        presenter_window.on_redo(move || {
            let mut st = state.borrow_mut();
            if st.deck.current_slide_mut().inking.redo() {
                st.cached_ink_overlay = None;
                drop(st);
                update_ink();
            }
        });
    }

    // --- PEN TOOL SETTINGS CALLBACKS ---
    {
        let state = state.clone();
        presenter_window.on_toggle_auto_shape(move |enabled| {
            let mut st = state.borrow_mut();
            st.auto_shape_enabled = enabled;
            println!("Auto Shape toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_scribble_to_erase(move |enabled| {
            let mut st = state.borrow_mut();
            st.scribble_to_erase = enabled;
            println!("Scribble to Erase toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_change_hold_delay(move |delay_s| {
            let mut st = state.borrow_mut();
            st.hold_delay_ms = (delay_s * 1000.0).round() as u64;
            if let Some(p) = pres_handle.upgrade() {
                p.set_hold_delay_text(format!("{:.1}s", delay_s).into());
            }
            println!("Draw and hold delay updated: {}ms", st.hold_delay_ms);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_enhanced_sensitivity(move |enabled| {
            let mut st = state.borrow_mut();
            st.enhanced_sensitivity = enabled;
            println!("Enhanced Sensitivity toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_draw_on_hover(move |enabled| {
            let mut st = state.borrow_mut();
            st.draw_on_hover = enabled;
            println!("Draw on Hover toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_select_pen_type(move |ptype| {
            let mut st = state.borrow_mut();
            st.pen_type = ptype;
            println!("Selected Pen Type: {}", match ptype {
                0 => "Fineliner",
                1 => "Fountain Pen",
                2 => "Marker",
                _ => "Custom",
            });
        });
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_select_pen_size(move |size_val, label| {
            let mut st = state.borrow_mut();
            let px = (size_val * 4.2).max(1.0);
            st.stroke_width = px;
            if let Some(p) = pres_handle.upgrade() {
                let lbl = if label.is_empty() {
                    format!("{:.1}mm", size_val)
                } else {
                    label.to_string()
                };
                p.set_pen_size_mm(lbl.into());
                p.set_stroke_width(px * 1.0);
            }
            println!("Pen size selected: {:.2}mm ({}px)", size_val, px);
        });
    }

    // --- HIGHLIGHTER TOOL SETTINGS CALLBACKS ---
    {
        let state = state.clone();
        presenter_window.on_select_highlighter_mode(move |mode| {
            let mut st = state.borrow_mut();
            st.highlighter_mode = mode;
            println!("Highlighter mode selected: {}", if mode == 1 { "Tape" } else { "Highlighter" });
        });
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_select_highlighter_size(move |size_val, label| {
            let mut st = state.borrow_mut();
            let px = (size_val * 4.2).max(2.0);
            st.stroke_width = px;
            if let Some(p) = pres_handle.upgrade() {
                let lbl = if label.is_empty() {
                    if (size_val - 1.9).abs() < 0.15 {
                        "1.9mm".to_string()
                    } else if (size_val - 2.7).abs() < 0.15 {
                        "2.7mm".to_string()
                    } else if (size_val - 4.6).abs() < 0.15 {
                        "4.6mm".to_string()
                    } else if (size_val - 5.4).abs() < 0.15 {
                        "5.4mm".to_string()
                    } else if (size_val - 6.2).abs() < 0.15 {
                        "6.2mm".to_string()
                    } else {
                        format!("{:.1}mm", size_val)
                    }
                } else {
                    label.to_string()
                };
                st.highlighter_size_mm = lbl.clone();
                p.set_highlighter_size_mm(lbl.into());
                p.set_highlighter_size_value(size_val);
                p.set_stroke_width(px * 1.0);
            }
            println!("Highlighter size selected: {:.2}mm ({}px)", size_val, px);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_highlighter_straight_lines(move |enabled| {
            let mut st = state.borrow_mut();
            st.highlighter_straight_lines = enabled;
            println!("Highlighter Draw Straight Lines toggled: {}", enabled);
        });
    }

    // --- ERASER TOOL SETTINGS CALLBACKS ---
    {
        let state = state.clone();
        presenter_window.on_select_eraser_size(move |index, radius| {
            let mut st = state.borrow_mut();
            st.eraser_size_index = index;
            st.eraser_radius = radius;
            println!("Eraser size selected: slot {} (radius: {:.3})", index, radius);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_erase_whole_line(move |enabled| {
            let mut st = state.borrow_mut();
            st.erase_whole_line = enabled;
            println!("Erase Whole Line toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_erase_highlight_only(move |enabled| {
            let mut st = state.borrow_mut();
            st.erase_highlight_only = enabled;
            println!("Erase Highlight Only toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_auto_select_previous_tool(move |enabled| {
            let mut st = state.borrow_mut();
            st.auto_select_previous_tool = enabled;
            println!("Auto-select Previous Tool toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_erase_shapes(move |enabled| {
            let mut st = state.borrow_mut();
            st.erase_shapes = enabled;
            println!("Erase Shapes toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_erase_tapes(move |enabled| {
            let mut st = state.borrow_mut();
            st.erase_tapes = enabled;
            println!("Erase Tapes toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_reset_eraser_settings(move || {
            let mut st = state.borrow_mut();
            st.eraser_size_index = 2;
            st.eraser_radius = 0.04;
            st.erase_whole_line = false;
            st.erase_highlight_only = false;
            st.auto_select_previous_tool = false;
            st.erase_shapes = true;
            st.erase_tapes = false;
            if let Some(p) = pres_handle.upgrade() {
                p.set_eraser_size_index(2);
                p.set_eraser_radius(0.04);
                p.set_erase_whole_line(false);
                p.set_erase_highlight_only(false);
                p.set_auto_select_previous_tool(false);
                p.set_erase_shapes(true);
                p.set_erase_tapes(false);
            }
            println!("Eraser settings reset to defaults.");
        });
    }

    // --- LASER POINTER & DRAW LINES SETTINGS ---
    {
        let state = state.clone();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_select_laser_color(move |idx, _color| {
            let color = match idx {
                0 => [255, 59, 48, 255],   // Red #ff3b30
                1 => [0, 85, 255, 255],    // Blue #0055ff
                2 => [0, 229, 58, 255],    // Green #00e53a
                3 => [184, 0, 255, 255],   // Purple #b800ff
                _ => [255, 59, 48, 255],
            };
            let mut st = state.borrow_mut();
            st.laser_color_index = idx;
            st.laser_color_rgba = color;
            if let Some(trail) = st.laser_trail.as_mut() {
                trail.color_rgba = color;
            }
            if let Some(aud) = audience_handle.upgrade() {
                aud.set_laser_color(slint::Color::from_argb_u8(color[3], color[0], color[1], color[2]));
            }
            println!("Selected laser color index: {}, rgba: {:?}", idx, color);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_laser_draw_lines(move |val| {
            let mut st = state.borrow_mut();
            st.laser_draw_lines = val;
            let duration = if val {
                (st.laser_fade_duration_s * 1000.0).round() as u64
            } else {
                (st.laser_fade_duration_s * 1000.0).round().min(350.0) as u64
            };
            if let Some(trail) = st.laser_trail.as_mut() {
                trail.draw_lines = val;
                trail.duration_ms = duration;
            }
            println!("Laser draw lines toggled: {}", val);
        });
    }

    // --- LASER FADE DURATION CONTROLLER (0.0s to 5.0s with decimals) ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_change_laser_fade_duration(move |duration| {
            let mut st = state.borrow_mut();
            let clamped = duration.clamp(0.0, 5.0);
            st.laser_fade_duration_s = clamped;
            let duration_ms = (clamped * 1000.0).round() as u64;
            if let Some(trail) = st.laser_trail.as_mut() {
                trail.duration_ms = duration_ms;
            }
            if let Some(p) = pres_handle.upgrade() {
                p.set_laser_fade_duration_s(clamped);
                p.set_laser_fade_duration_text(SharedString::from(format!("{:.1}s", clamped)));
            }
            println!("Updated laser fade duration: {:.1}s ({}ms)", clamped, duration_ms);
        });
    }

    Vec::new()
}
