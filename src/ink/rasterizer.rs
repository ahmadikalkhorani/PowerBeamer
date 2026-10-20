use super::stroke::{LaserTrail, Point, Stroke};
use image::{ImageBuffer, Rgba, RgbaImage};
use std::ops::{Deref, DerefMut};

/// Any RGBA8 pixel surface: an owned `RgbaImage` or a borrowed view over
/// external memory (e.g. a Slint `SharedPixelBuffer`), so we can rasterize
/// directly into the GPU upload buffer without intermediate copies.
pub type Canvas<C> = ImageBuffer<Rgba<u8>, C>;

pub trait PixelContainer: Deref<Target = [u8]> + DerefMut {}
impl<T: Deref<Target = [u8]> + DerefMut> PixelContainer for T {}

pub struct InkingRasterizer;

/// Resolved paint parameters for a stroke.
struct StrokePaint {
    color: Rgba<u8>,
    width: f32,
    hl_blend: bool,
}

fn stroke_paint(stroke: &Stroke) -> StrokePaint {
    let base_rgba = stroke.color_rgba;
    // Tape is opaque blackout strip (alpha 255), or translucent when revealed (alpha 35).
    // Highlighter maintains uniform translucent blend (alpha 100).
    let (color, width) = if stroke.is_tape {
        let alpha = if stroke.is_tape_revealed { 35 } else { 255 };
        (Rgba([45, 45, 50, alpha]), stroke.width * 2.8)
    } else if stroke.is_highlighter {
        (Rgba([base_rgba[0], base_rgba[1], base_rgba[2], 100]), stroke.width * 2.8)
    } else {
        (Rgba(base_rgba), stroke.width)
    };
    let hl_blend = (stroke.is_highlighter && !stroke.is_tape) || (stroke.is_tape && stroke.is_tape_revealed);
    StrokePaint { color, width, hl_blend }
}

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

    pub fn draw_stroke<C: PixelContainer>(img: &mut Canvas<C>, stroke: &Stroke) {
        if stroke.is_shape || stroke.points.len() < 3 {
            let paint = stroke_paint(stroke);
            draw_polyline(img, &stroke.points, &paint);
        } else {
            Self::draw_stroke_segments(img, stroke, 0, stroke.points.len() - 1);
        }
    }

    /// Draws raw segments `[seg_from, seg_to)` of a freehand stroke with Catmull-Rom smoothing.
    /// Used to rasterize an in-progress stroke incrementally (only new segments per frame).
    pub fn draw_stroke_segments<C: PixelContainer>(
        img: &mut Canvas<C>,
        stroke: &Stroke,
        seg_from: usize,
        seg_to: usize,
    ) {
        let paint = stroke_paint(stroke);
        if stroke.is_shape || stroke.points.len() < 3 {
            draw_polyline(img, &stroke.points, &paint);
            return;
        }
        let points = stroke.smoothed_segments(seg_from, seg_to);
        draw_polyline(img, &points, &paint);
    }

    pub fn draw_laser_trail<C: PixelContainer>(img: &mut Canvas<C>, trail: &LaserTrail) {
        let (w, h) = (img.width() as f32, img.height() as f32);
        Self::draw_laser_trail_at(img, trail, w, h, 0.0, 0.0);
    }

    /// Pixel bounds `(x0, y0, x1, y1)` of everything `draw_laser_trail` can paint on a
    /// `full_w × full_h` surface (trail glow plus the head bloom), or `None` when empty.
    pub fn laser_trail_bounds(trail: &LaserTrail, full_w: u32, full_h: u32) -> Option<(u32, u32, u32, u32)> {
        // Widest paint: head bloom (15px) / glow pass (~14px) + feathering + spline overshoot.
        const MARGIN: f32 = 40.0;
        let (fw, fh) = (full_w as f32, full_h as f32);
        let mut bounds: Option<(f32, f32, f32, f32)> = None;
        for &(x, y, _) in trail.strokes.iter().flatten() {
            let (px, py) = (x * fw, y * fh);
            bounds = Some(match bounds {
                None => (px, py, px, py),
                Some((a, b, c, d)) => (a.min(px), b.min(py), c.max(px), d.max(py)),
            });
        }
        let (x0, y0, x1, y1) = bounds?;
        let x0 = (x0 - MARGIN).floor().clamp(0.0, fw) as u32;
        let y0 = (y0 - MARGIN).floor().clamp(0.0, fh) as u32;
        let x1 = (x1 + MARGIN).ceil().clamp(0.0, fw) as u32;
        let y1 = (y1 + MARGIN).ceil().clamp(0.0, fh) as u32;
        (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
    }

    /// Draws the laser trail as if onto a `full_w × full_h` surface, into `img` whose top-left
    /// corner sits at `(ox, oy)` on that surface. Lets the caller render only the trail's region.
    pub fn draw_laser_trail_at<C: PixelContainer>(
        img: &mut Canvas<C>,
        trail: &LaserTrail,
        full_w: f32,
        full_h: f32,
        ox: f32,
        oy: f32,
    ) {
        if trail.is_empty() {
            return;
        }

        let wall_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let last_t = trail.last_point().map(|p| p.2).unwrap_or(wall_now);
        let now_ms = if wall_now.saturating_sub(last_t) > trail.duration_ms * 10 {
            last_t
        } else {
            wall_now.max(last_t)
        };

        let (w, h) = (full_w, full_h);

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
                    let px = pt.0 * w - ox;
                    let py = pt.1 * h - oy;
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
                // Neon-tube look, composited back to front:
                // Passes 1-3: stacked halo bands that fade outward (soft glow falloff)
                // Pass 4: solid saturated tube wall
                // Pass 5: bright white filament core
                let passes: [(f32, f32, [u8; 3]); 5] = [
                    (3.4, 0.07, [base_r, base_g, base_b]),
                    (2.5, 0.12, [base_r, base_g, base_b]),
                    (1.8, 0.22, [base_r, base_g, base_b]),
                    (1.3, 1.0, [base_r, base_g, base_b]),
                    (0.75, 1.0, [255, 255, 255]),
                ];

                // Reuse one scratch buffer across frames instead of allocating per frame.
                thread_local! {
                    static PASS_BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
                }
                PASS_BUF.with_borrow_mut(|scratch| {
                    scratch.clear();
                    scratch.resize((bw * bh * 4) as usize, 0);
                    let mut pass_buf = Canvas::<&mut [u8]>::from_raw(bw, bh, scratch.as_mut_slice())
                        .expect("laser pass buffer size");

                    for (radius_mul, alpha_mul, rgb) in passes {
                        pass_buf.as_mut().fill(0);
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
                                    let r1 = (2.6 + laser_radius_curve(p1.2) * 1.2) * radius_mul;
                                    let r2 = (2.6 + laser_radius_curve(p2.2) * 1.2) * radius_mul;

                                    let x1 = (p1.0 * w).clamp(0.0, w - 1.0) - ox;
                                    let y1 = (p1.1 * h).clamp(0.0, h - 1.0) - oy;
                                    let x2 = (p2.0 * w).clamp(0.0, w - 1.0) - ox;
                                    let y2 = (p2.1 * h).clamp(0.0, h - 1.0) - oy;

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
                                    let r = (2.6 + laser_radius_curve(p.2) * 1.2) * radius_mul;
                                    let cx = (p.0 * w).clamp(0.0, w - 1.0) - ox - bx0 as f32;
                                    let cy = (p.1 * h).clamp(0.0, h - 1.0) - oy - by0 as f32;
                                    draw_circle(&mut pass_buf, cx, cy, r, Rgba([rgb[0], rgb[1], rgb[2], alpha as u8]), false);
                                }
                            }
                        }

                        // Each pass is max-alpha within itself (no beading at joints), then
                        // blended over the previous ones so the white core sits on the colored wall.
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
                });
            }

            // Draw glowing head pointer dot at the tip if laser is actively being drawn
            if trail.is_active {
                if let Some(active_stroke) = trail.strokes.last() {
                    if let Some(&(hx, hy, _)) = active_stroke.last() {
                        let px = (hx * w).clamp(0.0, w - 1.0) - ox;
                        let py = (hy * h).clamp(0.0, h - 1.0) - oy;
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
                    let px = (x * w).clamp(0.0, w - 1.0) - ox;
                    let py = (y * h).clamp(0.0, h - 1.0) - oy;
                    draw_circle(img, px, py, radius, color, false);
                }
            }
            if trail.is_active {
                if let Some(active_stroke) = trail.strokes.last() {
                    if let Some(&(hx, hy, _)) = active_stroke.last() {
                        let px = (hx * w).clamp(0.0, w - 1.0) - ox;
                        let py = (hy * h).clamp(0.0, h - 1.0) - oy;
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

/// Laser filament radius term `intensity^0.65`, tabulated so per-point `powf` is avoided.
#[inline]
fn laser_radius_curve(intensity: f32) -> f32 {
    static LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let lut = LUT.get_or_init(|| std::array::from_fn(|i| (i as f32 / 255.0).powf(0.65)));
    lut[(intensity.clamp(0.0, 1.0) * 255.0).round() as usize]
}

/// Draws a normalized-coordinate polyline as a chain of anti-aliased capsules.
fn draw_polyline<C: PixelContainer>(img: &mut Canvas<C>, points: &[Point], paint: &StrokePaint) {
    if points.is_empty() {
        return;
    }
    let w = img.width() as f32;
    let h = img.height() as f32;
    let map = |p: &Point| {
        (
            (p.x * w).clamp(0.0, w - 1.0),
            (p.y * h).clamp(0.0, h - 1.0),
            (paint.width * 0.5 * p.pressure).max(1.0),
        )
    };

    if points.len() == 1 {
        let (x, y, r) = map(&points[0]);
        draw_circle(img, x, y, r, paint.color, paint.hl_blend);
        return;
    }

    for window in points.windows(2) {
        let (x1, y1, r1) = map(&window[0]);
        let (x2, y2, r2) = map(&window[1]);
        draw_capsule(img, x1, y1, r1, x2, y2, r2, paint.color, paint.hl_blend);
    }
}

/// Anti-aliased variable-radius capsule (segment with round caps).
/// Touches each covered pixel exactly once, unlike stamping overlapping circles.
fn draw_capsule<C: PixelContainer>(
    img: &mut Canvas<C>,
    x0: f32,
    y0: f32,
    r0: f32,
    x1: f32,
    y1: f32,
    r1: f32,
    color: Rgba<u8>,
    is_highlighter: bool,
) {
    let r0 = r0.max(0.8);
    let r1 = r1.max(0.8);
    let w = img.width() as i32;
    let h = img.height() as i32;
    let feather = 1.15f32;
    let pad = r0.max(r1) + feather + 1.0;

    let min_x = ((x0.min(x1) - pad).floor() as i32).max(0);
    let max_x = ((x0.max(x1) + pad).ceil() as i32).min(w - 1);
    let min_y = ((y0.min(y1) - pad).floor() as i32).max(0);
    let max_y = ((y0.max(y1) + pad).ceil() as i32).min(h - 1);
    if min_x > max_x || min_y > max_y {
        return;
    }

    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;
    let inv_len_sq = if len_sq < 1e-6 { 0.0 } else { 1.0 / len_sq };
    let src_a = color[3] as f32;

    for y in min_y..=max_y {
        let py = y as f32 - y0;
        for x in min_x..=max_x {
            let px = x as f32 - x0;
            let t = ((px * dx + py * dy) * inv_len_sq).clamp(0.0, 1.0);
            let ex = px - t * dx;
            let ey = py - t * dy;
            let dist_sq = ex * ex + ey * ey;
            let r = r0 + t * (r1 - r0);
            let r_outer = r + feather;
            if dist_sq > r_outer * r_outer {
                continue;
            }
            let r_inner = (r - feather).max(0.0);
            let coverage = if dist_sq <= r_inner * r_inner {
                1.0
            } else {
                let u = ((r_outer - dist_sq.sqrt()) / (r_outer - r_inner)).clamp(0.0, 1.0);
                u * u * (3.0 - 2.0 * u)
            };
            let a = (src_a * coverage).round() as u8;
            blend_pixel(img, x as u32, y as u32, Rgba([color[0], color[1], color[2], a]), is_highlighter);
        }
    }
}

/// Anti-aliased filled circle with smooth Hermite subpixel geometry
fn draw_circle<C: PixelContainer>(img: &mut Canvas<C>, cx: f32, cy: f32, radius: f32, color: Rgba<u8>, is_highlighter: bool) {
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
fn blend_pixel<C: PixelContainer>(img: &mut Canvas<C>, x: u32, y: u32, src: Rgba<u8>, is_highlighter: bool) {
    let dst = img.get_pixel_mut(x, y);
    if src[3] == 0 {
        return;
    }
    if is_highlighter {
        // Highlighters maintain uniform translucency without accumulating into dark opaque bands
        let out_a = src[3].max(dst[3]);
        *dst = Rgba([src[0], src[1], src[2], out_a]);
        return;
    }

    // Fast path: fully opaque source needs no blending math.
    if src[3] == 255 {
        *dst = src;
        return;
    }
    blend_pixel_direct(dst, src);
}

/// Renders an anti-aliased capsule segment with Euclidean distance and continuous alpha
#[inline]
fn stamp_smooth_capsule<C: PixelContainer>(
    img: &mut Canvas<C>,
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
/// Non-premultiplied source-over in integer math (weights scaled by 255²).
fn blend_pixel_direct(dst: &mut Rgba<u8>, src: Rgba<u8>) {
    let sa = src[3] as u32;
    if sa == 0 {
        return;
    }
    if dst[3] == 0 {
        *dst = src;
        return;
    }
    let ws = sa * 255;
    let wd = dst[3] as u32 * (255 - sa);
    let out_w = ws + wd;
    let half = out_w / 2;
    let mix = |s: u8, d: u8| ((s as u32 * ws + d as u32 * wd + half) / out_w) as u8;
    *dst = Rgba([
        mix(src[0], dst[0]),
        mix(src[1], dst[1]),
        mix(src[2], dst[2]),
        ((out_w + 127) / 255).min(255) as u8,
    ]);
}
