#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub timestamp_ms: u64,
}

impl Point {
    pub fn new(x: f32, y: f32, pressure: f32, timestamp_ms: u64) -> Self {
        Self {
            x,
            y,
            pressure,
            timestamp_ms,
        }
    }

    pub fn distance_to(&self, other: &Point) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InkingTool {
    Pen,
    Highlighter,
    Eraser,
    Lasso,
    Laser,
    Hand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stroke {
    pub id: u64,
    pub points: Vec<Point>,
    pub color_rgba: [u8; 4],
    pub width: f32,
    pub is_highlighter: bool,
    #[serde(default)]
    pub is_shape: bool,
    #[serde(default)]
    pub is_tape: bool,
    #[serde(default)]
    pub is_tape_revealed: bool,
    pub bounding_box: [f32; 4], // [min_x, min_y, max_x, max_y]
}

impl Stroke {
    pub fn new(id: u64, color_rgba: [u8; 4], width: f32, is_highlighter: bool) -> Self {
        Self {
            id,
            points: Vec::new(),
            color_rgba,
            width,
            is_highlighter,
            is_shape: false,
            is_tape: false,
            is_tape_revealed: false,
            bounding_box: [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
        }
    }

    pub fn add_point(&mut self, p: Point) {
        if p.x < self.bounding_box[0] {
            self.bounding_box[0] = p.x;
        }
        if p.y < self.bounding_box[1] {
            self.bounding_box[1] = p.y;
        }
        if p.x > self.bounding_box[2] {
            self.bounding_box[2] = p.x;
        }
        if p.y > self.bounding_box[3] {
            self.bounding_box[3] = p.y;
        }
        self.points.push(p);
    }

    pub fn recompute_bounding_box(&mut self) {
        self.bounding_box = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for p in &self.points {
            if p.x < self.bounding_box[0] {
                self.bounding_box[0] = p.x;
            }
            if p.y < self.bounding_box[1] {
                self.bounding_box[1] = p.y;
            }
            if p.x > self.bounding_box[2] {
                self.bounding_box[2] = p.x;
            }
            if p.y > self.bounding_box[3] {
                self.bounding_box[3] = p.y;
            }
        }
    }

    /// Check if an eraser circle at (cx, cy) with radius r intersects this stroke
    pub fn intersects_circle(&self, cx: f32, cy: f32, r: f32) -> bool {
        let norm_width = if r < 1.0 { self.width / 1600.0 } else { self.width };
        let effective_r = r + norm_width * 0.5;
        // Bounding box quick rejection
        if cx + effective_r < self.bounding_box[0]
            || cx - effective_r > self.bounding_box[2]
            || cy + effective_r < self.bounding_box[1]
            || cy - effective_r > self.bounding_box[3]
        {
            return false;
        }

        if self.points.len() == 1 {
            return self.points[0].distance_to(&Point::new(cx, cy, 1.0, 0)) <= effective_r;
        }

        for window in self.points.windows(2) {
            let p1 = &window[0];
            let p2 = &window[1];
            if dist_to_segment(cx, cy, p1.x, p1.y, p2.x, p2.y) <= effective_r {
                return true;
            }
        }

        false
    }

    /// Catmull-Rom smoothing filter to eliminate jitter from raw tablet input
    pub fn smoothed(&self) -> Vec<Point> {
        if self.is_shape || self.points.len() < 3 {
            return self.points.clone();
        }
        self.smoothed_segments(0, self.points.len() - 1)
    }

    /// Catmull-Rom smoothed polyline covering raw segments `[seg_from, seg_to)`
    /// (segment `i` joins `points[i]` and `points[i + 1]`). The result starts at
    /// `points[seg_from]` and ends at `points[seg_to]`. Segment `i` only depends on
    /// points `i-1..=i+2`, so segments `< len - 2` never change as new points arrive.
    pub fn smoothed_segments(&self, seg_from: usize, seg_to: usize) -> Vec<Point> {
        let n = self.points.len();
        let seg_to = seg_to.min(n.saturating_sub(1));
        if seg_from >= seg_to {
            return Vec::new();
        }

        let mut smoothed = Vec::with_capacity((seg_to - seg_from) * 4 + 1);
        smoothed.push(self.points[seg_from]);

        for i in seg_from..seg_to {
            let p0 = if i == 0 { &self.points[0] } else { &self.points[i - 1] };
            let p1 = &self.points[i];
            let p2 = &self.points[i + 1];
            let p3 = if i + 2 < n { &self.points[i + 2] } else { p2 };

            let steps = 4;
            for s in 1..steps {
                let t = s as f32 / steps as f32;
                let t2 = t * t;
                let t3 = t2 * t;

                let x = 0.5 * ((2.0 * p1.x) +
                    (-p0.x + p2.x) * t +
                    (2.0 * p0.x - 5.0 * p1.x + 4.0 * p2.x - p3.x) * t2 +
                    (-p0.x + 3.0 * p1.x - 3.0 * p2.x + p3.x) * t3);

                let y = 0.5 * ((2.0 * p1.y) +
                    (-p0.y + p2.y) * t +
                    (2.0 * p0.y - 5.0 * p1.y + 4.0 * p2.y - p3.y) * t2 +
                    (-p0.y + 3.0 * p1.y - 3.0 * p2.y + p3.y) * t3);

                let pressure = p1.pressure + (p2.pressure - p1.pressure) * t;
                smoothed.push(Point::new(x, y, pressure, p1.timestamp_ms));
            }
            smoothed.push(*p2);
        }

        smoothed
    }
}

/// Shortest distance from point (px, py) to line segment (x1, y1) -> (x2, y2)
fn dist_to_segment(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 1e-6 {
        let ex = px - x1;
        let ey = py - y1;
        return (ex * ex + ey * ey).sqrt();
    }

    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    let ex = px - proj_x;
    let ey = py - proj_y;
    (ex * ex + ey * ey).sqrt()
}

/// Point-in-polygon test for Lasso tool
pub fn point_in_polygon(px: f32, py: f32, polygon: &[Point]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let pi = &polygon[i];
        let pj = &polygon[j];
        if ((pi.y > py) != (pj.y > py))
            && (px < (pj.x - pi.x) * (py - pi.y) / (pj.y - pi.y) + pi.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Laser pointer with decaying trail history
#[derive(Clone, Debug)]
pub struct LaserTrail {
    pub strokes: Vec<Vec<(f32, f32, u64)>>, // Disjoint strokes: Vec<Vec<(x, y, timestamp_ms)>>
    pub duration_ms: u64,
    pub color_rgba: [u8; 4],
    pub draw_lines: bool,
    pub is_active: bool,
}

impl LaserTrail {
    pub fn new(duration_ms: u64, color_rgba: [u8; 4], draw_lines: bool) -> Self {
        Self {
            strokes: Vec::new(),
            duration_ms,
            color_rgba,
            draw_lines,
            is_active: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        // `prune` drops empty strokes, but the current stroke may still be empty after `start_stroke`.
        self.strokes.iter().all(|s| s.is_empty())
    }

    /// Total live points across all strokes.
    pub fn point_count(&self) -> usize {
        self.strokes.iter().map(Vec::len).sum()
    }

    /// Most recently added point.
    pub fn last_point(&self) -> Option<(f32, f32, u64)> {
        self.strokes.iter().rev().find_map(|s| s.last().copied())
    }

    pub fn start_stroke(&mut self) {
        if !self.is_active || self.strokes.is_empty() {
            if self.strokes.last().map_or(false, |s| !s.is_empty()) || self.strokes.is_empty() {
                self.strokes.push(Vec::new());
            }
        }
        self.is_active = true;
    }

    pub fn end_stroke(&mut self) {
        // Line trails stay fully visible while drawn and fade as a whole from the release
        // moment: restamp the finished stroke so every point shares the release time.
        if self.draw_lines && self.is_active {
            let wall_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            // Trust the points' own clock when the wall clock is far off (synthetic timestamps)
            let last_t = self.last_point().map(|p| p.2).unwrap_or(wall_ms);
            let now_ms = if wall_ms.saturating_sub(last_t) > self.duration_ms * 10 { last_t } else { wall_ms };
            if let Some(stroke) = self.strokes.last_mut() {
                for p in stroke.iter_mut() {
                    p.2 = p.2.max(now_ms);
                }
            }
        }
        self.is_active = false;
    }

    /// The stroke still being drawn in line mode, which neither fades nor gets pruned.
    fn held_stroke_index(&self) -> Option<usize> {
        (self.draw_lines && self.is_active && !self.strokes.is_empty()).then(|| self.strokes.len() - 1)
    }

    pub fn add(&mut self, x: f32, y: f32, now_ms: u64) {
        if !self.is_active || self.strokes.is_empty() {
            if self.strokes.last().map_or(false, |s| !s.is_empty()) || self.strokes.is_empty() {
                self.strokes.push(Vec::new());
            }
            self.is_active = true;
        }
        let current_stroke = self.strokes.last_mut().unwrap();
        if let Some(last) = current_stroke.last_mut() {
            let dx = x - last.0;
            let dy = y - last.1;
            if dx * dx + dy * dy < 1e-7 {
                last.2 = now_ms;
                return;
            }
        }
        current_stroke.push((x, y, now_ms));
        self.prune(now_ms);
    }

    pub fn prune(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(self.duration_ms);
        let held = self.held_stroke_index();
        for (i, stroke) in self.strokes.iter_mut().enumerate() {
            if Some(i) != held {
                stroke.retain(|&(_, _, t)| t >= cutoff);
            }
        }
        self.strokes.retain(|stroke| !stroke.is_empty());
    }

    /// Generates smooth Centripetal Catmull-Rom spline interpolated points for a stroke.
    /// Returns Vec<(x: f32, y: f32, intensity: f32)> with silky continuous curvature and smooth decay.
    pub fn smooth_stroke_points(points: &[(f32, f32, u64)], duration_ms: u64, now_ms: u64) -> Vec<(f32, f32, f32)> {
        if points.is_empty() {
            return Vec::new();
        }
        if points.len() == 1 {
            let age = now_ms.saturating_sub(points[0].2);
            let factor = 1.0 - (age as f32 / duration_ms.max(1) as f32).clamp(0.0, 1.0);
            let intensity = factor * factor * (3.0 - 2.0 * factor);
            return vec![(points[0].0, points[0].1, intensity)];
        }

        let n = points.len();
        let mut result = Vec::with_capacity(n * 16);

        let get_intensity = |t: u64| -> f32 {
            let age = now_ms.saturating_sub(t);
            let factor = 1.0 - (age as f32 / duration_ms.max(1) as f32).clamp(0.0, 1.0);
            // Cubic Hermite smoothstep for optical gradual decay without abrupt cutoff
            factor * factor * (3.0 - 2.0 * factor)
        };

        if n == 2 {
            let p1 = &points[0];
            let p2 = &points[1];
            let int1 = get_intensity(p1.2);
            let int2 = get_intensity(p2.2);
            let dx = p2.0 - p1.0;
            let dy = p2.1 - p1.1;
            let dist = (dx * dx + dy * dy).sqrt();
            let steps = ((dist * 500.0).ceil() as usize).clamp(1, 10);
            for s in 0..=steps {
                let t = s as f32 / steps as f32;
                result.push((p1.0 + dx * t, p1.1 + dy * t, int1 + (int2 - int1) * t));
            }
            return result;
        }

        for i in 0..n - 1 {
            let p1 = points[i];
            let p2 = points[i + 1];

            // Synthesize virtual exterior control points at boundaries for seamless endpoints
            let p0 = if i == 0 {
                (2.0 * p1.0 - p2.0, 2.0 * p1.1 - p2.1, p1.2)
            } else {
                points[i - 1]
            };

            let p3 = if i + 2 < n {
                points[i + 2]
            } else {
                (2.0 * p2.0 - p1.0, 2.0 * p2.1 - p1.1, p2.2)
            };

            let int1 = get_intensity(p1.2);
            let int2 = get_intensity(p2.2);

            let dx = p2.0 - p1.0;
            let dy = p2.1 - p1.1;
            let dist = (dx * dx + dy * dy).sqrt();
            let steps = ((dist * 500.0).ceil() as usize).clamp(1, 12);

            // Centripetal parameterization (alpha = 0.5) to prevent cusps, self-intersections, and overshoots
            let d01 = (((p1.0 - p0.0).powi(2) + (p1.1 - p0.1).powi(2)).sqrt().max(1e-5)).sqrt();
            let d12 = (((p2.0 - p1.0).powi(2) + (p2.1 - p1.1).powi(2)).sqrt().max(1e-5)).sqrt();
            let d23 = (((p3.0 - p2.0).powi(2) + (p3.1 - p2.1).powi(2)).sqrt().max(1e-5)).sqrt();

            let t0 = 0.0f32;
            let t1 = t0 + d01;
            let t2 = t1 + d12;
            let t3 = t2 + d23;

            for s in 0..steps {
                let u = s as f32 / steps as f32;
                let t = t1 + u * (t2 - t1);

                // Barry and Goldman's pyramidal formulation
                let a1_x = (t1 - t) / (t1 - t0) * p0.0 + (t - t0) / (t1 - t0) * p1.0;
                let a1_y = (t1 - t) / (t1 - t0) * p0.1 + (t - t0) / (t1 - t0) * p1.1;

                let a2_x = (t2 - t) / (t2 - t1) * p1.0 + (t - t1) / (t2 - t1) * p2.0;
                let a2_y = (t2 - t) / (t2 - t1) * p1.1 + (t - t1) / (t2 - t1) * p2.1;

                let a3_x = (t3 - t) / (t3 - t2) * p2.0 + (t - t2) / (t3 - t2) * p3.0;
                let a3_y = (t3 - t) / (t3 - t2) * p2.1 + (t - t2) / (t3 - t2) * p3.1;

                let b1_x = (t2 - t) / (t2 - t0) * a1_x + (t - t0) / (t2 - t0) * a2_x;
                let b1_y = (t2 - t) / (t2 - t0) * a1_y + (t - t0) / (t2 - t0) * a2_y;

                let b2_x = (t3 - t) / (t3 - t1) * a2_x + (t - t1) / (t3 - t1) * a3_x;
                let b2_y = (t3 - t) / (t3 - t1) * a2_y + (t - t1) / (t3 - t1) * a3_y;

                let cx = (t2 - t) / (t2 - t1) * b1_x + (t - t1) / (t2 - t1) * b2_x;
                let cy = (t2 - t) / (t2 - t1) * b1_y + (t - t1) / (t2 - t1) * b2_y;

                let intensity = int1 + (int2 - int1) * u;
                result.push((cx, cy, intensity));
            }
        }

        if let Some(last) = points.last() {
            result.push((last.0, last.1, get_intensity(last.2)));
        }

        result
    }

    pub fn smoothed_strokes(&self, now_ms: u64) -> Vec<Vec<(f32, f32, f32)>> {
        let held = self.held_stroke_index();
        self.strokes
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.is_empty())
            .map(|(i, s)| {
                if Some(i) == held {
                    // Full intensity while held
                    let fresh: Vec<_> = s.iter().map(|&(x, y, _)| (x, y, now_ms)).collect();
                    Self::smooth_stroke_points(&fresh, self.duration_ms, now_ms)
                } else {
                    Self::smooth_stroke_points(s, self.duration_ms, now_ms)
                }
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn smoothed_positions(&self, now_ms: u64) -> Vec<(f32, f32, f32)> {
        let strokes = self.smoothed_strokes(now_ms);
        strokes.into_iter().flatten().collect()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EraserOptions {
    pub radius: f32,
    pub erase_whole_line: bool,
    pub erase_highlight_only: bool,
    pub erase_shapes: bool,
    pub erase_tapes: bool,
}

impl Default for EraserOptions {
    fn default() -> Self {
        Self {
            radius: 0.04,
            erase_whole_line: false,
            erase_highlight_only: false,
            erase_shapes: true,
            erase_tapes: false,
        }
    }
}

/// Slide Inking Model managing strokes, undo/redo, and active drawing
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SlideInking {
    pub strokes: Vec<Stroke>,
    #[serde(skip)]
    pub undo_stack: Vec<Vec<Stroke>>,
    #[serde(skip)]
    pub redo_stack: Vec<Vec<Stroke>>,
}

impl SlideInking {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_undo_state(&mut self) {
        self.undo_stack.push(self.strokes.clone());
        self.redo_stack.clear();
    }

    pub fn add_stroke(&mut self, stroke: Stroke) {
        self.push_undo_state();
        self.strokes.push(stroke);
    }

    pub fn erase_at(&mut self, x: f32, y: f32, radius: f32) -> bool {
        let mut opts = EraserOptions::default();
        opts.radius = radius;
        self.erase_at_with_options(x, y, &opts)
    }

    pub fn erase_at_with_options(&mut self, x: f32, y: f32, opts: &EraserOptions) -> bool {
        let is_target = |stroke: &Stroke| -> bool {
            if opts.erase_highlight_only && (!stroke.is_highlighter || stroke.is_tape) {
                return false;
            }
            if !opts.erase_shapes && stroke.is_shape {
                return false;
            }
            if !opts.erase_tapes && stroke.is_tape {
                return false;
            }
            true
        };

        let any_hit = self.strokes.iter().any(|s| is_target(s) && s.intersects_circle(x, y, opts.radius));
        if !any_hit {
            return false;
        }

        self.push_undo_state();
        self.strokes.retain(|stroke| !(is_target(stroke) && stroke.intersects_circle(x, y, opts.radius)));
        true
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.strokes.clone());
            self.strokes = prev;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.strokes.clone());
            self.strokes = next;
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        if !self.strokes.is_empty() {
            self.push_undo_state();
            self.strokes.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stroke_smoothing_and_bounds() {
        let mut stroke = Stroke::new(1, [255, 0, 0, 255], 3.0, false);
        stroke.add_point(Point::new(10.0, 10.0, 1.0, 0));
        stroke.add_point(Point::new(50.0, 50.0, 1.0, 10));
        stroke.add_point(Point::new(100.0, 20.0, 1.0, 20));

        assert_eq!(stroke.bounding_box, [10.0, 10.0, 100.0, 50.0]);
        let smoothed = stroke.smoothed();
        assert!(smoothed.len() > stroke.points.len());
    }

    #[test]
    fn test_eraser_and_undo_redo() {
        let mut inking = SlideInking::new();
        let mut stroke = Stroke::new(1, [0, 255, 0, 255], 4.0, false);
        stroke.add_point(Point::new(100.0, 100.0, 1.0, 0));
        stroke.add_point(Point::new(200.0, 100.0, 1.0, 10));
        inking.add_stroke(stroke);

        assert_eq!(inking.strokes.len(), 1);

        // Erase away from stroke (should miss)
        let hit_miss = inking.erase_at(500.0, 500.0, 10.0);
        assert!(!hit_miss);
        assert_eq!(inking.strokes.len(), 1);

        // Erase right at stroke (150, 100)
        let hit = inking.erase_at(150.0, 100.0, 15.0);
        assert!(hit);
        assert_eq!(inking.strokes.len(), 0);

        // Undo should restore stroke
        assert!(inking.undo());
        assert_eq!(inking.strokes.len(), 1);

        // Redo should remove it again
        assert!(inking.redo());
        assert_eq!(inking.strokes.len(), 0);
    }
}

