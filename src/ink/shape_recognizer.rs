#![allow(dead_code)]
use super::stroke::Point;
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeKind {
    Line,
    HorizontalLine,
    VerticalLine,
    Arrow,
    DoubleArrow,
    Rectangle,
    Square,
    Diamond,
    Circle,
    Ellipse,
    Triangle,
    Pentagon,
    Hexagon,
    Star,
    Heart,
    Arc,
}

#[derive(Clone, Debug)]
pub struct RecognizedShape {
    pub kind: ShapeKind,
    pub points: Vec<Point>,
}

pub struct ShapeRecognizer;

impl ShapeRecognizer {
    /// Attempts to recognize a freehand stroke as a regular geometric shape.
    /// Returns the canonical regularized points for the recognized shape if a match is found.
    pub fn recognize(raw_points: &[Point], is_highlighter: bool) -> Option<RecognizedShape> {
        if raw_points.len() < 3 {
            return None;
        }

        // 1. Highlighter priority: check if it should straighten as a horizontal/straight highlight
        if is_highlighter {
            if let Some(shape) = Self::recognize_straight_line(raw_points, 0.08, 1.45, true) {
                return Some(shape);
            }
        }

        // 2. Open Arrow & Double Arrow (check before straight line / closed loops)
        if let Some(arrow) = Self::recognize_arrow(raw_points) {
            return Some(arrow);
        }

        // 3. Straight Line (horizontal, vertical, or diagonal)
        if let Some(line) = Self::recognize_straight_line(raw_points, 0.035, 1.25, false) {
            return Some(line);
        }

        // 4. Closed Shapes
        if Self::is_closed_loop(raw_points) {
            // 4a. Star (5-pointed star with deep alternating peaks & valleys)
            if let Some(star) = Self::recognize_star(raw_points) {
                return Some(star);
            }

            // 4b. Heart (cardioid cleft + lobes + bottom tip)
            if let Some(heart) = Self::recognize_heart(raw_points) {
                return Some(heart);
            }

            // 4c. Diamond / Rhombus (4 straight edges connecting edge midpoints)
            if let Some(diamond) = Self::recognize_diamond(raw_points) {
                return Some(diamond);
            }

            // 4d. Rectangle / Square (4 corners near bounding box corners)
            if let Some(rect) = Self::recognize_rectangle(raw_points) {
                return Some(rect);
            }

            // 4e. Circle / Ellipse (smooth low-variance radial curve)
            if let Some(ellipse) = Self::recognize_ellipse_or_circle(raw_points) {
                return Some(ellipse);
            }

            // 4f. Triangle (3 prominent corners)
            if let Some(tri) = Self::recognize_triangle(raw_points) {
                return Some(tri);
            }

            // 4g. Regular Polygons: Pentagon (5 corners) & Hexagon (6 corners)
            if let Some(poly) = Self::recognize_regular_polygon(raw_points) {
                return Some(poly);
            }
        }

        // 5. Open Curved Arc
        if let Some(arc) = Self::recognize_arc(raw_points) {
            return Some(arc);
        }

        None
    }

    /// Detects scribble pattern (rapid back-and-forth oscillations)
    pub fn is_scribble(points: &[Point]) -> bool {
        if points.len() < 7 {
            return false;
        }
        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let diag = ((max_x - min_x).powi(2) + (max_y - min_y).powi(2)).sqrt();
        if diag < 0.01 || diag > 0.35 {
            return false;
        }

        let mut reversals = 0;
        let mut prev_dir = 0i32;

        for w in points.windows(2) {
            let dx = w[1].x - w[0].x;
            if dx.abs() > 0.004 {
                let dir = if dx > 0.0 { 1 } else { -1 };
                if prev_dir != 0 && dir != prev_dir {
                    reversals += 1;
                }
                prev_dir = dir;
            }
        }

        // If user changed directions at least 3 times within a compact area, it's a scribble!
        reversals >= 3
    }

    /// Check if start and end points are close enough to form a closed shape
    fn is_closed_loop(points: &[Point]) -> bool {
        let first = &points[0];
        let last = &points[points.len() - 1];
        let d = first.distance_to(last);

        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let diag = ((max_x - min_x).powi(2) + (max_y - min_y).powi(2)).sqrt();

        // Close if endpoint is within 28% of bounding diagonal or < 0.08 normalized distance
        d < 0.08 || (diag > 0.05 && d / diag < 0.28)
    }

    /// Calculate bounding box [min_x, min_y, max_x, max_y]
    pub fn bounding_box(points: &[Point]) -> (f32, f32, f32, f32) {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for p in points {
            if p.x < min_x { min_x = p.x; }
            if p.y < min_y { min_y = p.y; }
            if p.x > max_x { max_x = p.x; }
            if p.y > max_y { max_y = p.y; }
        }
        (min_x, min_y, max_x, max_y)
    }

    /// Total path length along points
    fn path_length(points: &[Point]) -> f32 {
        let mut len = 0.0;
        for w in points.windows(2) {
            len += w[0].distance_to(&w[1]);
        }
        len
    }

    /// Recognizes a straight line between start and end.
    /// Snaps to exact horizontal or vertical if angle is within tolerance.
    pub fn recognize_straight_line(
        points: &[Point],
        max_deviation: f32,
        max_length_ratio: f32,
        is_highlighter: bool,
    ) -> Option<RecognizedShape> {
        let p0 = points.first()?;
        let pn = points.last()?;
        let direct_dist = p0.distance_to(pn);

        if direct_dist < 0.02 {
            return None;
        }

        let total_len = Self::path_length(points);
        if total_len / direct_dist > max_length_ratio {
            return None;
        }

        // Check maximum distance of any intermediate point to the line segment (p0, pn)
        let mut max_d = 0.0f32;
        for p in points {
            let d = Self::distance_to_segment(p.x, p.y, p0.x, p0.y, pn.x, pn.y);
            if d > max_d {
                max_d = d;
            }
        }

        if max_d > max_deviation {
            return None;
        }

        let dx = (pn.x - p0.x).abs();
        let dy = (pn.y - p0.y).abs();
        let angle_deg = (dy / dx.max(0.0001)).atan().to_degrees();

        let avg_pressure = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
        let t_start = p0.timestamp_ms;
        let t_end = pn.timestamp_ms;

        // 1. Snap to Horizontal Line (Angle < 16 degrees or dy < 0.02, or up to 25 deg for highlighter)
        let horiz_max_angle = if is_highlighter { 25.0 } else { 16.0 };
        let horiz_max_dy = if is_highlighter { 0.035 } else { 0.02 };

        if angle_deg < horiz_max_angle || dy < horiz_max_dy {
            let y_avg = (p0.y + pn.y) * 0.5;
            let straight_points = vec![
                Point::new(p0.x, y_avg, avg_pressure, t_start),
                Point::new(pn.x, y_avg, avg_pressure, t_end),
            ];
            return Some(RecognizedShape {
                kind: ShapeKind::HorizontalLine,
                points: straight_points,
            });
        }

        // 2. Snap to Vertical Line (Angle > 74 degrees or dx < 0.02)
        if angle_deg > 74.0 || dx < 0.02 {
            let x_avg = (p0.x + pn.x) * 0.5;
            let straight_points = vec![
                Point::new(x_avg, p0.y, avg_pressure, t_start),
                Point::new(x_avg, pn.y, avg_pressure, t_end),
            ];
            return Some(RecognizedShape {
                kind: ShapeKind::VerticalLine,
                points: straight_points,
            });
        }

        // 3. Regular Straight Line
        let straight_points = vec![
            Point::new(p0.x, p0.y, avg_pressure, t_start),
            Point::new(pn.x, pn.y, avg_pressure, t_end),
        ];

        Some(RecognizedShape {
            kind: ShapeKind::Line,
            points: straight_points,
        })
    }

    /// Recognizes Single-ended Arrow
    pub fn recognize_arrow(points: &[Point]) -> Option<RecognizedShape> {
        let n = points.len();
        if n < 5 {
            return None;
        }

        let p0 = points[0];

        // Find point furthest from start p0
        let mut max_idx = 0;
        let mut max_dist = 0.0f32;
        for (i, p) in points.iter().enumerate() {
            let d = p0.distance_to(p);
            if d > max_dist {
                max_dist = d;
                max_idx = i;
            }
        }

        if max_dist < 0.04 || max_idx < 3 {
            return None;
        }

        let p_tip = points[max_idx];
        let shaft_len = max_dist;

        // Check straightness of shaft (points from 0 to max_idx)
        let mut max_shaft_dev = 0.0f32;
        for p in &points[0..=max_idx] {
            let dev = Self::distance_to_segment(p.x, p.y, p0.x, p0.y, p_tip.x, p_tip.y);
            if dev > max_shaft_dev {
                max_shaft_dev = dev;
            }
        }
        if max_shaft_dev > 0.12 * shaft_len {
            return None;
        }

        // Check if there are arrowhead barb points after max_idx
        let barb_pts = &points[max_idx..];
        let has_end_barb = if barb_pts.len() >= 2 {
            let barb_len = Self::path_length(barb_pts);
            let s_dx = p_tip.x - p0.x;
            let s_dy = p_tip.y - p0.y;

            // Barb should be between 8% and 65% of shaft length
            let len_ok = barb_len >= 0.08 * shaft_len && barb_len <= 0.65 * shaft_len;

            // Barb points should project backwards against the shaft vector
            let mut backward_pts = 0;
            for p in &barb_pts[1..] {
                let v_dx = p.x - p_tip.x;
                let v_dy = p.y - p_tip.y;
                let dot = v_dx * s_dx + v_dy * s_dy;
                if dot <= 0.08 * shaft_len * shaft_len {
                    backward_pts += 1;
                }
            }

            len_ok && (backward_pts as f32 / (barb_pts.len() - 1) as f32) >= 0.60
        } else {
            false
        };

        if has_end_barb {
            let ux = (p_tip.x - p0.x) / shaft_len;
            let uy = (p_tip.y - p0.y) / shaft_len;
            let nx = -uy;
            let ny = ux;

            let head_len = (shaft_len * 0.22).clamp(0.018, 0.045);
            let head_w = head_len * 0.58;

            let w1_x = p_tip.x - ux * head_len + nx * head_w;
            let w1_y = p_tip.y - uy * head_len + ny * head_w;
            let w2_x = p_tip.x - ux * head_len - nx * head_w;
            let w2_y = p_tip.y - uy * head_len - ny * head_w;

            let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
            let t0 = points[0].timestamp_ms;

            // Canonical 5-point vector arrow: Start -> Tip -> Wing1 -> Tip -> Wing2
            let arrow_pts = vec![
                Point::new(p0.x, p0.y, avg_p, t0),
                Point::new(p_tip.x, p_tip.y, avg_p, t0 + 10),
                Point::new(w1_x, w1_y, avg_p, t0 + 20),
                Point::new(p_tip.x, p_tip.y, avg_p, t0 + 30),
                Point::new(w2_x, w2_y, avg_p, t0 + 40),
            ];

            return Some(RecognizedShape {
                kind: ShapeKind::Arrow,
                points: arrow_pts,
            });
        }

        None
    }

    /// Helper to test if 4 corners correspond to diamond edge midpoints
    fn is_diamond_corners(corners: &[Point], min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> bool {
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let w = max_x - min_x;
        let h = max_y - min_y;

        let midpoints = [(cx, min_y), (max_x, cy), (cx, max_y), (min_x, cy)];
        let mut matched = 0;
        for c in corners {
            for m in &midpoints {
                let d = ((c.x - m.0).powi(2) + (c.y - m.1).powi(2)).sqrt();
                if d < 0.16 * w.max(h) {
                    matched += 1;
                    break;
                }
            }
        }
        matched >= 3
    }

    /// Recognizes Diamond / Rhombus (4 corners at midpoint of bounding box)
    fn recognize_diamond(points: &[Point]) -> Option<RecognizedShape> {
        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let w = max_x - min_x;
        let h = max_y - min_y;
        let diag = (w * w + h * h).sqrt();

        if w < 0.03 || h < 0.03 {
            return None;
        }

        let simplified = Self::ramer_douglas_peucker(points, 0.045 * diag);
        // Diamond must have 4 corners (5 points including closed vertex)
        if simplified.len() != 5 {
            return None;
        }

        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let scale = w.max(h);

        // Check that corners are near the diamond midpoints and NOT the rectangle corners
        if !Self::is_diamond_corners(&simplified[0..4], min_x, min_y, max_x, max_y) {
            return None;
        }

        let top = (cx, min_y);
        let right = (max_x, cy);
        let bottom = (cx, max_y);
        let left = (min_x, cy);

        // Check straightness: every stroke point must be close to at least one of the 4 diamond edges
        let diamond_corners = [top, right, bottom, left];
        let mut far_count = 0;
        for p in points {
            let mut min_edge_d = f32::MAX;
            for k in 0..4 {
                let p_a = diamond_corners[k];
                let p_b = diamond_corners[(k + 1) % 4];
                let d = Self::distance_to_segment(p.x, p.y, p_a.0, p_a.1, p_b.0, p_b.1);
                if d < min_edge_d {
                    min_edge_d = d;
                }
            }
            if min_edge_d > 0.07 * scale {
                far_count += 1;
            }
        }
        if far_count as f32 / points.len() as f32 > 0.15 {
            return None; // Too many points bulge away from diamond edges (e.g. circle)
        }

        let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
        let t0 = points[0].timestamp_ms;
        let diamond_pts = vec![
            Point::new(top.0, top.1, avg_p, t0),
            Point::new(right.0, right.1, avg_p, t0 + 10),
            Point::new(bottom.0, bottom.1, avg_p, t0 + 20),
            Point::new(left.0, left.1, avg_p, t0 + 30),
            Point::new(top.0, top.1, avg_p, t0 + 40),
        ];
        Some(RecognizedShape {
            kind: ShapeKind::Diamond,
            points: diamond_pts,
        })
    }

    /// Recognizes Rectangle or Square
    fn recognize_rectangle(points: &[Point]) -> Option<RecognizedShape> {
        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let w = max_x - min_x;
        let h = max_y - min_y;
        let diag = (w * w + h * h).sqrt();

        if w < 0.03 || h < 0.03 {
            return None;
        }

        let rect_perimeter = 2.0 * (w + h);
        let actual_len = Self::path_length(points);

        // Path length within 35% of perimeter
        if (actual_len - rect_perimeter).abs() / rect_perimeter > 0.35 {
            return None;
        }

        // A rectangle MUST have stroke points near at least 3 of the 4 bounding box corners.
        // Circles, ellipses, and diamonds do not reach into bounding box corners.
        let scale = w.max(h);
        let box_corners = [
            (min_x, min_y),
            (max_x, min_y),
            (max_x, max_y),
            (min_x, max_y),
        ];
        let mut corners_reached = 0;
        for c in &box_corners {
            if points.iter().any(|p| ((p.x - c.0).powi(2) + (p.y - c.1).powi(2)).sqrt() < 0.16 * scale) {
                corners_reached += 1;
            }
        }
        if corners_reached < 3 {
            return None;
        }

        // Check if RDP simplified corners match a 4-corner polygon (not diamond)
        let simplified = Self::ramer_douglas_peucker(points, 0.045 * diag);
        let has_4_corners = simplified.len() == 5 && !Self::is_diamond_corners(&simplified[0..4], min_x, min_y, max_x, max_y);

        // Check that points stay near the 4 sides
        let mut outside_count = 0;
        let tolerance = 0.12 * scale;

        for p in points {
            let d_left = (p.x - min_x).abs();
            let d_right = (p.x - max_x).abs();
            let d_top = (p.y - min_y).abs();
            let d_bottom = (p.y - max_y).abs();
            let min_d = d_left.min(d_right).min(d_top).min(d_bottom);
            if min_d > tolerance {
                outside_count += 1;
            }
        }

        let point_match = (outside_count as f32 / points.len() as f32) < 0.32;

        if has_4_corners || point_match {
            let is_square = (w - h).abs() / w.max(h) < 0.18;
            let (final_min_x, final_min_y, final_max_x, final_max_y) = if is_square {
                let s = (w + h) * 0.5;
                let cx = (min_x + max_x) * 0.5;
                let cy = (min_y + max_y) * 0.5;
                (cx - s * 0.5, cy - s * 0.5, cx + s * 0.5, cy + s * 0.5)
            } else {
                (min_x, min_y, max_x, max_y)
            };

            let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
            let t0 = points[0].timestamp_ms;

            // Generate clean 5-point closed polygon (4 corners + closing vertex)
            let corners = vec![
                Point::new(final_min_x, final_min_y, avg_p, t0),
                Point::new(final_max_x, final_min_y, avg_p, t0 + 10),
                Point::new(final_max_x, final_max_y, avg_p, t0 + 20),
                Point::new(final_min_x, final_max_y, avg_p, t0 + 30),
                Point::new(final_min_x, final_min_y, avg_p, t0 + 40),
            ];

            return Some(RecognizedShape {
                kind: if is_square { ShapeKind::Square } else { ShapeKind::Rectangle },
                points: corners,
            });
        }

        None
    }

    /// Recognizes 5-Pointed Star
    fn recognize_star(points: &[Point]) -> Option<RecognizedShape> {
        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let w = max_x - min_x;
        let h = max_y - min_y;

        if w < 0.04 || h < 0.04 {
            return None;
        }

        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let r_max = (w.max(h) * 0.5).max(0.02);

        // Radial distance of points from center
        let dists: Vec<f32> = points.iter().map(|p| ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()).collect();
        let mut peak_vals = Vec::new();
        let mut valley_vals = Vec::new();

        for i in 1..dists.len().saturating_sub(1) {
            if dists[i] > dists[i - 1] && dists[i] > dists[i + 1] && dists[i] > r_max * 0.65 {
                peak_vals.push(dists[i]);
            } else if dists[i] < dists[i - 1] && dists[i] < dists[i + 1] && dists[i] < r_max * 0.55 {
                valley_vals.push(dists[i]);
            }
        }

        // True 5-pointed star has 4-6 prominent peaks and deep valleys
        let has_peaks = peak_vals.len() >= 4 && peak_vals.len() <= 6;
        let has_valleys = valley_vals.len() >= 4;

        if has_peaks && has_valleys {
            let avg_p_val = peak_vals.iter().sum::<f32>() / peak_vals.len() as f32;
            let avg_v_val = valley_vals.iter().sum::<f32>() / valley_vals.len() as f32;
            let amplitude = avg_p_val - avg_v_val;

            // Must have deep star valleys (amplitude at least 22% of outer radius)
            if amplitude >= 0.22 * r_max {
                let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
                let t0 = points[0].timestamp_ms;
                let r_outer = r_max;
                let r_inner = r_max * 0.382;

                let mut star_pts = Vec::with_capacity(11);
                for k in 0..5 {
                    let angle_out = -std::f32::consts::FRAC_PI_2 + (k as f32 * 2.0 * PI / 5.0);
                    star_pts.push(Point::new(cx + r_outer * angle_out.cos(), cy + r_outer * angle_out.sin(), avg_p, t0 + (k as u64 * 20)));
                    let angle_in = angle_out + (PI / 5.0);
                    star_pts.push(Point::new(cx + r_inner * angle_in.cos(), cy + r_inner * angle_in.sin(), avg_p, t0 + (k as u64 * 20 + 10)));
                }
                star_pts.push(star_pts[0]);

                return Some(RecognizedShape {
                    kind: ShapeKind::Star,
                    points: star_pts,
                });
            }
        }
        None
    }

    /// Recognizes Heart shape
    fn recognize_heart(points: &[Point]) -> Option<RecognizedShape> {
        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let w = max_x - min_x;
        let h = max_y - min_y;

        if w < 0.04 || h < 0.04 {
            return None;
        }

        let cx = (min_x + max_x) * 0.5;

        // Bottom point check: bottommost point (highest y) should be near center x
        let bottom_pt = points.iter().max_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))?;
        if (bottom_pt.x - cx).abs() > 0.22 * w {
            return None;
        }

        // Top cleft check: in the middle (x ≈ cx), minimum y should dip down compared to lobes
        let cleft_pts: Vec<&Point> = points.iter().filter(|p| (p.x - cx).abs() < 0.12 * w).collect();
        let left_lobe_pts: Vec<&Point> = points.iter().filter(|p| p.x >= min_x + 0.10 * w && p.x <= cx - 0.12 * w).collect();
        let right_lobe_pts: Vec<&Point> = points.iter().filter(|p| p.x >= cx + 0.12 * w && p.x <= max_x - 0.10 * w).collect();

        if cleft_pts.is_empty() || left_lobe_pts.is_empty() || right_lobe_pts.is_empty() {
            return None;
        }

        let cleft_min_y = cleft_pts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let left_min_y = left_lobe_pts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let right_min_y = right_lobe_pts.iter().map(|p| p.y).fold(f32::MAX, f32::min);

        let lobe_min_y = left_min_y.min(right_min_y);
        if cleft_min_y > lobe_min_y + 0.05 * h {
            let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
            let t0 = points[0].timestamp_ms;
            let n_pts = 36;
            let mut heart_pts = Vec::with_capacity(n_pts + 1);

            for i in 0..=n_pts {
                let t = (i as f32 / n_pts as f32) * 2.0 * PI;
                let sin_t = t.sin();
                let x_raw = 16.0 * sin_t * sin_t * sin_t;
                let y_raw = -(13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos());

                let norm_x = cx + (x_raw / 32.0) * w;
                let norm_y = min_y + ((y_raw + 17.0) / 33.0) * h;
                heart_pts.push(Point::new(norm_x, norm_y, avg_p, t0 + (i as u64 * 10)));
            }

            return Some(RecognizedShape {
                kind: ShapeKind::Heart,
                points: heart_pts,
            });
        }
        None
    }

    /// Recognizes Circle or Ellipse
    fn recognize_ellipse_or_circle(points: &[Point]) -> Option<RecognizedShape> {
        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let w = max_x - min_x;
        let h = max_y - min_y;

        if w < 0.03 || h < 0.03 {
            return None;
        }

        // Center computed from average of points (more robust than bounding box center)
        let cx = points.iter().map(|p| p.x).sum::<f32>() / points.len() as f32;
        let cy = points.iter().map(|p| p.y).sum::<f32>() / points.len() as f32;
        let a = w * 0.5;
        let b = h * 0.5;

        // Compute error of normalized radial distances: ((x-cx)/a)^2 + ((y-cy)/b)^2 ≈ 1
        let mut variance = 0.0f32;
        for p in points {
            let nx = (p.x - cx) / a;
            let ny = (p.y - cy) / b;
            let norm_r = (nx * nx + ny * ny).sqrt();
            let err = (norm_r - 1.0).abs();
            variance += err * err;
        }
        let mse = variance / points.len() as f32;

        if mse < 0.032 {
            let is_circle = (w - h).abs() / w.max(h) < 0.22;
            let (final_a, final_b) = if is_circle {
                let r = (a + b) * 0.5;
                (r, r)
            } else {
                (a, b)
            };

            // Generate 36 smooth points forming the perfect circle/ellipse
            let num_pts = 36;
            let mut result_pts = Vec::with_capacity(num_pts + 1);
            let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
            let t0 = points[0].timestamp_ms;

            for i in 0..=num_pts {
                let angle = (i as f32 / num_pts as f32) * 2.0 * PI;
                let px = cx + final_a * angle.cos();
                let py = cy + final_b * angle.sin();
                result_pts.push(Point::new(px, py, avg_p, t0 + (i as u64 * 10)));
            }

            return Some(RecognizedShape {
                kind: if is_circle { ShapeKind::Circle } else { ShapeKind::Ellipse },
                points: result_pts,
            });
        }

        None
    }

    /// Recognizes Regular Polygon: Pentagon (5 sides) or Hexagon (6 sides)
    fn recognize_regular_polygon(points: &[Point]) -> Option<RecognizedShape> {
        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let w = max_x - min_x;
        let h = max_y - min_y;
        let diag = (w * w + h * h).sqrt();

        if w < 0.04 || h < 0.04 {
            return None;
        }

        let simplified = Self::ramer_douglas_peucker(points, 0.045 * diag);
        let num_corners = simplified.len().saturating_sub(1);

        if num_corners == 5 || num_corners == 6 {
            let cx = (min_x + max_x) * 0.5;
            let cy = (min_y + max_y) * 0.5;
            let r = (w.max(h) * 0.5).max(0.02);
            let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
            let t0 = points[0].timestamp_ms;

            let n = num_corners;
            let mut poly_pts = Vec::with_capacity(n + 1);
            let start_angle = -std::f32::consts::FRAC_PI_2;

            for i in 0..=n {
                let angle = start_angle + (i as f32 * 2.0 * PI / n as f32);
                poly_pts.push(Point::new(cx + r * angle.cos(), cy + r * angle.sin(), avg_p, t0 + (i as u64 * 10)));
            }

            return Some(RecognizedShape {
                kind: if n == 5 { ShapeKind::Pentagon } else { ShapeKind::Hexagon },
                points: poly_pts,
            });
        }
        None
    }

    /// Recognizes Triangle
    fn recognize_triangle(points: &[Point]) -> Option<RecognizedShape> {
        let (min_x, min_y, max_x, max_y) = Self::bounding_box(points);
        let w = max_x - min_x;
        let h = max_y - min_y;

        if w < 0.04 || h < 0.04 {
            return None;
        }

        let simplified = Self::ramer_douglas_peucker(points, 0.05 * (w * w + h * h).sqrt());
        if simplified.len() == 4 {
            let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
            let t0 = points[0].timestamp_ms;

            let mut tri_pts = Vec::with_capacity(4);
            for (idx, p) in simplified.iter().take(3).enumerate() {
                tri_pts.push(Point::new(p.x, p.y, avg_p, t0 + (idx as u64 * 10)));
            }
            let first = tri_pts[0];
            tri_pts.push(first);

            // Area must be non-trivial
            let area = 0.5 * ((tri_pts[1].x - tri_pts[0].x) * (tri_pts[2].y - tri_pts[0].y) - (tri_pts[2].x - tri_pts[0].x) * (tri_pts[1].y - tri_pts[0].y)).abs();
            if area > 0.001 {
                return Some(RecognizedShape {
                    kind: ShapeKind::Triangle,
                    points: tri_pts,
                });
            }
        }

        None
    }

    /// Recognizes Smooth Curved Arc (single convex open curve)
    fn recognize_arc(points: &[Point]) -> Option<RecognizedShape> {
        let p0 = points.first()?;
        let pn = points.last()?;
        let direct_dist = p0.distance_to(pn);

        if direct_dist < 0.03 {
            return None;
        }

        let total_len = Self::path_length(points);
        let ratio = total_len / direct_dist;

        // An arc has path length longer than straight line (1.10 to 2.40)
        if ratio < 1.10 || ratio > 2.40 {
            return None;
        }

        let mut max_h = 0.0f32;
        let mut mid_pt = *p0;
        let mut positive_cross = 0;
        let mut negative_cross = 0;

        let chord_dx = pn.x - p0.x;
        let chord_dy = pn.y - p0.y;

        for p in points {
            let d = Self::distance_to_segment(p.x, p.y, p0.x, p0.y, pn.x, pn.y);
            if d > max_h {
                max_h = d;
                mid_pt = *p;
            }
            let cross = chord_dx * (p.y - p0.y) - chord_dy * (p.x - p0.x);
            if cross > 0.002 {
                positive_cross += 1;
            } else if cross < -0.002 {
                negative_cross += 1;
            }
        }

        if max_h < 0.025 {
            return None;
        }

        let total_side = positive_cross + negative_cross;
        if total_side > 0 {
            let max_side = positive_cross.max(negative_cross);
            if (max_side as f32 / total_side as f32) < 0.82 {
                return None; // Too much S-curve reversal
            }
        }

        // Fit quadratic Bezier arc through p0, mid_pt, pn
        let ctrl_x = 2.0 * mid_pt.x - 0.5 * p0.x - 0.5 * pn.x;
        let ctrl_y = 2.0 * mid_pt.y - 0.5 * p0.y - 0.5 * pn.y;

        let n_pts = 24;
        let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
        let t0 = p0.timestamp_ms;
        let mut arc_pts = Vec::with_capacity(n_pts + 1);

        for i in 0..=n_pts {
            let t = i as f32 / n_pts as f32;
            let mt = 1.0 - t;
            let px = mt * mt * p0.x + 2.0 * mt * t * ctrl_x + t * t * pn.x;
            let py = mt * mt * p0.y + 2.0 * mt * t * ctrl_y + t * t * pn.y;
            arc_pts.push(Point::new(px, py, avg_p, t0 + (i as u64 * 10)));
        }

        Some(RecognizedShape {
            kind: ShapeKind::Arc,
            points: arc_pts,
        })
    }

    /// Ramer-Douglas-Peucker line simplification
    pub fn ramer_douglas_peucker(points: &[Point], epsilon: f32) -> Vec<Point> {
        if points.len() < 3 {
            return points.to_vec();
        }

        let mut dmax = 0.0;
        let mut index = 0;
        let end = points.len() - 1;

        for i in 1..end {
            let d = Self::distance_to_segment(
                points[i].x, points[i].y,
                points[0].x, points[0].y,
                points[end].x, points[end].y,
            );
            if d > dmax {
                index = i;
                dmax = d;
            }
        }

        if dmax > epsilon {
            let mut rec1 = Self::ramer_douglas_peucker(&points[..=index], epsilon);
            let rec2 = Self::ramer_douglas_peucker(&points[index..], epsilon);
            rec1.pop();
            rec1.extend(rec2);
            rec1
        } else {
            vec![points[0], points[end]]
        }
    }

    /// Distance from point (px, py) to line segment (x1, y1) - (x2, y2)
    fn distance_to_segment(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len_sq = dx * dx + dy * dy;

        if len_sq == 0.0 {
            return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
        }

        let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
        let proj_x = x1 + t * dx;
        let proj_y = y1 + t * dy;

        ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
    }
}
