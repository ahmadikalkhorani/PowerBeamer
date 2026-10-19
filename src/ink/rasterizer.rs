use super::stroke::{LaserTrail, Stroke};
use image::{Rgba, RgbaImage};

pub struct InkingRasterizer;

impl InkingRasterizer {
    /// Render vector strokes onto a transparent RGBA image of given width and height
    pub fn render_strokes(
        width: u32,
        height: u32,
        strokes: &[Stroke],
        active_stroke: Option<&Stroke>,
        laser_trail: Option<&LaserTrail>,
    ) -> RgbaImage {
        let mut img = RgbaImage::new(width.max(1), height.max(1));

        // 1. Render permanent strokes
        for stroke in strokes {
            Self::draw_stroke(&mut img, stroke);
        }

        // 2. Render in-progress active stroke
        if let Some(stroke) = active_stroke {
            Self::draw_stroke(&mut img, stroke);
        }

        // 3. Render decaying laser trail if active
        if let Some(laser) = laser_trail {
            Self::draw_laser_trail(&mut img, laser);
        }

        img
    }

    pub fn draw_stroke(img: &mut RgbaImage, stroke: &Stroke) {
        let points = stroke.smoothed();
        if points.is_empty() {
            return;
        }

        let base_rgba = stroke.color_rgba;
        let is_highlighter = stroke.is_highlighter;
        let is_tape = stroke.is_tape;

        // Tape is opaque blackout strip (alpha 255), or translucent when revealed (alpha 35).
        // Highlighter maintains uniform translucent blend (alpha 100).
        let (color, effective_width) = if is_tape {
            let alpha = if stroke.is_tape_revealed { 35 } else { 255 };
            (
                Rgba([45, 45, 50, alpha]),
                stroke.width * 2.8,
            )
        } else if is_highlighter {
            (
                Rgba([base_rgba[0], base_rgba[1], base_rgba[2], 100]),
                stroke.width * 2.8,
            )
        } else {
            (
                Rgba([base_rgba[0], base_rgba[1], base_rgba[2], base_rgba[3]]),
                stroke.width,
            )
        };

        let is_hl_blend = (is_highlighter && !is_tape) || (is_tape && stroke.is_tape_revealed);
        let w = img.width() as f32;
        let h = img.height() as f32;

        if points.len() == 1 {
            let p = &points[0];
            let radius = (effective_width * 0.5 * p.pressure).max(1.0);
            let px = (p.x * w).clamp(0.0, w - 1.0);
            let py = (p.y * h).clamp(0.0, h - 1.0);
            draw_circle(img, px, py, radius, color, is_hl_blend);
            return;
        }

        for window in points.windows(2) {
            let p1 = &window[0];
            let p2 = &window[1];
            let x1 = (p1.x * w).clamp(0.0, w - 1.0);
            let y1 = (p1.y * h).clamp(0.0, h - 1.0);
            let x2 = (p2.x * w).clamp(0.0, w - 1.0);
            let y2 = (p2.y * h).clamp(0.0, h - 1.0);
            let r1 = (effective_width * 0.5 * p1.pressure).max(1.0);
            let r2 = (effective_width * 0.5 * p2.pressure).max(1.0);
            draw_thick_line(img, x1, y1, r1, x2, y2, r2, color, is_hl_blend);
        }
    }

    pub fn draw_laser_trail(img: &mut RgbaImage, trail: &LaserTrail) {
        if trail.positions.is_empty() {
            return;
        }

        let wall_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let last_t = trail.positions.last().map(|p| p.2).unwrap_or(wall_now);
        let now_ms = if wall_now.saturating_sub(last_t) > trail.duration_ms * 10 {
            last_t
        } else {
            wall_now.max(last_t)
        };

        let w = img.width() as f32;
        let h = img.height() as f32;

        let base_r = trail.color_rgba[0];
        let base_g = trail.color_rgba[1];
        let base_b = trail.color_rgba[2];

        let strokes = trail.smoothed_strokes(now_ms);
        if strokes.is_empty() {
            return;
        }

        if trail.draw_lines {
            let mut min_px = f32::MAX;
            let mut min_py = f32::MAX;
            let mut max_px = f32::MIN;
            let mut max_py = f32::MIN;
            let mut has_drawable_content = false;

            for stroke in &strokes {
                for pt in stroke {
                    let px = pt.0 * w;
                    let py = pt.1 * h;
                    if px < min_px { min_px = px; }
                    if py < min_py { min_py = py; }
                    if px > max_px { max_px = px; }
                    if py > max_py { max_py = py; }
                    has_drawable_content = true;
                }
            }

            if !has_drawable_content {
                return;
            }

            let margin = 36.0;
            let bx0 = (min_px - margin).max(0.0) as u32;
            let by0 = (min_py - margin).max(0.0) as u32;
            let bx1 = ((max_px + margin).ceil() as u32).min(img.width());
            let by1 = ((max_py + margin).ceil() as u32).min(img.height());
            let bw = bx1.saturating_sub(bx0);
            let bh = by1.saturating_sub(by0);

            if bw > 0 && bh > 0 {
                let core_r = ((base_r as u16 + 255 * 3) / 4) as u8;
                let core_g = ((base_g as u16 + 255 * 3) / 4) as u8;
                let core_b = ((base_b as u16 + 255 * 3) / 4) as u8;

                // 2 high-performance rendering passes:
                // Pass 1: Broad soft neon atmospheric glow
                // Pass 2: Intense brilliant core filament
                let passes: [(f32, f32, [u8; 3]); 2] = [
                    (2.6, 0.45, [base_r, base_g, base_b]),
                    (0.9, 0.95, [core_r, core_g, core_b]),
                ];

                let mut pass_buf = RgbaImage::new(bw, bh);

                for (radius_mul, alpha_mul, rgb) in passes {
                    for stroke in &strokes {
                        if stroke.len() >= 2 {
                            for window in stroke.windows(2) {
                                let p1 = &window[0];
                                let p2 = &window[1];
                                let avg_int = (p1.2 + p2.2) * 0.5;
                                if avg_int < 0.02 {
                                    continue;
                                }

                                let alpha1 = (p1.2 * 255.0 * alpha_mul).clamp(0.0, 255.0);
                                let alpha2 = (p2.2 * 255.0 * alpha_mul).clamp(0.0, 255.0);
                                let r1 = (0.8 + p1.2.powf(0.65) * 4.6) * radius_mul;
                                let r2 = (0.8 + p2.2.powf(0.65) * 4.6) * radius_mul;

                                let x1 = (p1.0 * w).clamp(0.0, w - 1.0);
                                let y1 = (p1.1 * h).clamp(0.0, h - 1.0);
                                let x2 = (p2.0 * w).clamp(0.0, w - 1.0);
                                let y2 = (p2.1 * h).clamp(0.0, h - 1.0);

                                stamp_smooth_capsule(
                                    &mut pass_buf,
                                    bx0,
                                    by0,
                                    x1,
                                    y1,
                                    r1,
                                    alpha1,
                                    x2,
                                    y2,
                                    r2,
                                    alpha2,
                                    rgb,
                                );
                            }
                        } else if stroke.len() == 1 {
                            let p = &stroke[0];
                            if p.2 >= 0.02 {
                                let alpha = (p.2 * 255.0 * alpha_mul).clamp(0.0, 255.0);
                                let r = (0.8 + p.2.powf(0.65) * 4.6) * radius_mul;
                                let cx = (p.0 * w).clamp(0.0, w - 1.0) - bx0 as f32;
                                let cy = (p.1 * h).clamp(0.0, h - 1.0) - by0 as f32;
                                draw_circle(&mut pass_buf, cx, cy, r, Rgba([rgb[0], rgb[1], rgb[2], alpha as u8]), false);
                            }
                        }
                    }
                }

                // Composite pass buffer once onto main image
                for py in 0..bh {
                    for px in 0..bw {
                        let src = *pass_buf.get_pixel(px, py);
                        if src[3] > 0 {
                            let dst = img.get_pixel_mut(bx0 + px, by0 + py);
                            blend_pixel_direct(dst, src);
                        }
                    }
                }
            }

            // Draw glowing head pointer dot at the tip if laser is actively being drawn
            if trail.is_active {
                if let Some(active_stroke) = trail.strokes.last() {
                    if let Some(&(hx, hy, _)) = active_stroke.last() {
                        let px = (hx * w).clamp(0.0, w - 1.0);
                        let py = (hy * h).clamp(0.0, h - 1.0);
                        // Outer soft luminous bloom halo
                        draw_circle(img, px, py, 15.0, Rgba([base_r, base_g, base_b, 70]), false);
                        // Mid saturated glow ring
                        draw_circle(img, px, py, 8.0, Rgba([base_r, base_g, base_b, 200]), false);
                        // Core beam dot
                        draw_circle(img, px, py, 4.5, Rgba([base_r, base_g, base_b, 255]), false);
                        // Hot white filament center
                        draw_circle(img, px, py, 2.2, Rgba([255, 255, 255, 255]), false);
                    }
                }
            }
        } else {
            // Standard pointer comet tail mode (using smoothed path for fluid motion)
            for stroke in &strokes {
                let m = stroke.len();
                for (i, &(x, y, intensity)) in stroke.iter().enumerate() {
                    let progress = (i + 1) as f32 / m as f32;
                    let alpha = (intensity * progress * 240.0) as u8;
                    if alpha < 6 {
                        continue;
                    }
                    let radius = 1.2 + progress.powf(0.75) * 6.5;
                    let color = Rgba([base_r, base_g, base_b, alpha]);
                    let px = (x * w).clamp(0.0, w - 1.0);
                    let py = (y * h).clamp(0.0, h - 1.0);
                    draw_circle(img, px, py, radius, color, false);
                }
            }
            if trail.is_active {
                if let Some(active_stroke) = trail.strokes.last() {
                    if let Some(&(hx, hy, _)) = active_stroke.last() {
                        let px = (hx * w).clamp(0.0, w - 1.0);
                        let py = (hy * h).clamp(0.0, h - 1.0);
                        draw_circle(img, px, py, 14.0, Rgba([base_r, base_g, base_b, 70]), false);
                        draw_circle(img, px, py, 7.5, Rgba([base_r, base_g, base_b, 210]), false);
                        draw_circle(img, px, py, 4.2, Rgba([base_r, base_g, base_b, 255]), false);
                        draw_circle(img, px, py, 2.0, Rgba([255, 255, 255, 255]), false);
                    }
                }
            }
        }
    }
}

/// Bresenham-based thick line with rounded caps and subpixel precision
fn draw_thick_line(
    img: &mut RgbaImage,
    x0: f32,
    y0: f32,
    r0: f32,
    x1: f32,
    y1: f32,
    r1: f32,
    color: Rgba<u8>,
    is_highlighter: bool,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let dist = (dx * dx + dy * dy).sqrt();
    let min_r = r0.min(r1);
    let step_dist = (min_r * 0.35).clamp(0.5, 1.8);
    let steps = (dist / step_dist).ceil().max(1.0) as usize;

    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let cx = x0 + t * dx;
        let cy = y0 + t * dy;
        let r = r0 + t * (r1 - r0);
        draw_circle(img, cx, cy, r.max(0.8), color, is_highlighter);
    }
}

/// Anti-aliased filled circle with smooth Hermite subpixel geometry
fn draw_circle(img: &mut RgbaImage, cx: f32, cy: f32, radius: f32, color: Rgba<u8>, is_highlighter: bool) {
    let w = img.width() as i32;
    let h = img.height() as i32;

    if radius <= 0.6 {
        let ix = cx.round() as i32;
        let iy = cy.round() as i32;
        if ix >= 0 && ix < w && iy >= 0 && iy < h {
            blend_pixel(img, ix as u32, iy as u32, color, is_highlighter);
        }
        return;
    }

    let feather = 1.15f32;
    let r_outer = radius + feather;
    let r_inner = (radius - feather).max(0.0);
    let r_ceil = r_outer.ceil() as i32;
    let max_d_sq = r_outer * r_outer;

    let icx = cx.round() as i32;
    let icy = cy.round() as i32;
    let min_x = (icx - r_ceil).clamp(0, w - 1);
    let max_x = (icx + r_ceil).clamp(0, w - 1);
    let min_y = (icy - r_ceil).clamp(0, h - 1);
    let max_y = (icy + r_ceil).clamp(0, h - 1);

    let src_a = color[3] as f32;

    for y in min_y..=max_y {
        let dy = y as f32 - cy;
        let dy_sq = dy * dy;
        for x in min_x..=max_x {
            let dx = x as f32 - cx;
            let dist_sq = dx * dx + dy_sq;
            if dist_sq <= max_d_sq {
                let dist = dist_sq.sqrt();
                let coverage = if dist <= r_inner {
                    1.0
                } else {
                    let u = ((r_outer - dist) / (r_outer - r_inner)).clamp(0.0, 1.0);
                    u * u * (3.0 - 2.0 * u)
                };
                let blended_color = Rgba([
                    color[0],
                    color[1],
                    color[2],
                    ((src_a * coverage).round() as u8).min(255),
                ]);
                blend_pixel(img, x as u32, y as u32, blended_color, is_highlighter);
            }
        }
    }
}

/// Alpha blend color over existing pixel in-place
#[inline]
fn blend_pixel(img: &mut RgbaImage, x: u32, y: u32, src: Rgba<u8>, is_highlighter: bool) {
    let dst = img.get_pixel_mut(x, y);
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }

    if is_highlighter {
        // Highlighters maintain uniform translucency without accumulating into dark opaque bands
        let out_a = src[3].max(dst[3]);
        *dst = Rgba([src[0], src[1], src[2], out_a]);
        return;
    }

    if dst[3] == 0 {
        *dst = src;
        return;
    }

    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return;
    }

    let r = ((src[0] as f32 * sa + dst[0] as f32 * da * (1.0 - sa)) / out_a).round();
    let g = ((src[1] as f32 * sa + dst[1] as f32 * da * (1.0 - sa)) / out_a).round();
    let b = ((src[2] as f32 * sa + dst[2] as f32 * da * (1.0 - sa)) / out_a).round();

    *dst = Rgba([r as u8, g as u8, b as u8, (out_a * 255.0).round().min(255.0) as u8]);
}

/// Renders an anti-aliased capsule segment with Euclidean distance and continuous alpha
#[inline]
fn stamp_smooth_capsule(
    img: &mut RgbaImage,
    offset_x: u32,
    offset_y: u32,
    x1: f32,
    y1: f32,
    r1: f32,
    alpha1: f32,
    x2: f32,
    y2: f32,
    r2: f32,
    alpha2: f32,
    color_rgb: [u8; 3],
) {
    let w = img.width() as i32;
    let h = img.height() as i32;

    let max_r = r1.max(r2);
    let lx1 = x1 - offset_x as f32;
    let ly1 = y1 - offset_y as f32;
    let lx2 = x2 - offset_x as f32;
    let ly2 = y2 - offset_y as f32;

    let feather = 1.35f32;
    let min_x = (lx1.min(lx2) - max_r - feather - 1.0).floor().max(0.0) as i32;
    let max_x = (lx1.max(lx2) + max_r + feather + 1.0).ceil().min((w - 1) as f32) as i32;
    let min_y = (ly1.min(ly2) - max_r - feather - 1.0).floor().max(0.0) as i32;
    let max_y = (ly1.max(ly2) + max_r + feather + 1.0).ceil().min((h - 1) as f32) as i32;

    if min_x > max_x || min_y > max_y {
        return;
    }

    let dx = lx2 - lx1;
    let dy = ly2 - ly1;
    let len_sq = dx * dx + dy * dy;

    for y in min_y..=max_y {
        let py = y as f32;
        for x in min_x..=max_x {
            let px = x as f32;
            let (proj_x, proj_y, t) = if len_sq < 1e-6 {
                (lx1, ly1, 0.0)
            } else {
                let t = (((px - lx1) * dx + (py - ly1) * dy) / len_sq).clamp(0.0, 1.0);
                (lx1 + t * dx, ly1 + t * dy, t)
            };

            let dist_sq = (px - proj_x).powi(2) + (py - proj_y).powi(2);
            let r = r1 + t * (r2 - r1);
            let r_outer = r + feather;
            let r_inner = (r - feather).max(0.0);

            if dist_sq <= r_outer * r_outer {
                let dist = dist_sq.sqrt();
                let coverage = if dist <= r_inner {
                    1.0
                } else {
                    let u = ((r_outer - dist) / (r_outer - r_inner)).clamp(0.0, 1.0);
                    u * u * (3.0 - 2.0 * u)
                };
                let base_a = alpha1 + t * (alpha2 - alpha1);
                let target_alpha = ((base_a * coverage).round() as u8).min(255);
                let pixel = img.get_pixel_mut(x as u32, y as u32);
                if target_alpha > pixel[3] {
                    *pixel = Rgba([color_rgb[0], color_rgb[1], color_rgb[2], target_alpha]);
                }
            }
        }
    }
}

/// Fast direct alpha blend over target pixel
#[inline]
fn blend_pixel_direct(dst: &mut Rgba<u8>, src: Rgba<u8>) {
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    if dst[3] == 0 {
        *dst = src;
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return;
    }
    let r = ((src[0] as f32 * sa + dst[0] as f32 * da * (1.0 - sa)) / out_a).round();
    let g = ((src[1] as f32 * sa + dst[1] as f32 * da * (1.0 - sa)) / out_a).round();
    let b = ((src[2] as f32 * sa + dst[2] as f32 * da * (1.0 - sa)) / out_a).round();

    *dst = Rgba([r as u8, g as u8, b as u8, (out_a * 255.0).round().min(255.0) as u8]);
}
