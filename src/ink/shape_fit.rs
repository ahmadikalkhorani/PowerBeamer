//! Closed-stroke fitting: resample the stroke, find its corners, fit an ellipse and a
//! polygon candidate, then keep whichever outline explains the stroke best.
//!
//! All work happens on a fixed-size resampled ring, so cost is constant per stroke
//! (a few thousand float ops) regardless of how many raw points were captured.
//! Coordinates must be isotropic (x and y in the same units) — see
//! `ShapeRecognizer::recognize_with_aspect`.

use super::shape_recognizer::{RecognizedShape, ShapeKind};
use super::stroke::Point;
use std::f32::consts::{FRAC_PI_2, PI};

/// Ring resolution. 64 keeps 8 samples per side even for an octagon.
const N: usize = 64;
/// Upper bound on adaptive resampling for very elongated strokes.
const N_MAX: usize = 192;
/// Samples wanted along the shortest side of a thin shape.
const MIN_SAMPLES_PER_SIDE: f32 = 7.0;
/// ShortStraw window (in samples) on each side of a point.
const STRAW_W: usize = 3;
/// A sample is a corner candidate when its straw is shorter than this fraction of the median.
const STRAW_RATIO: f32 = 0.95;
/// Corners flatter than this interior angle (degrees) are treated as wobble, not corners.
const MAX_CORNER_ANGLE: f32 = 155.0;
const MAX_POLY_VERTICES: usize = 8;
/// Chord / path ratio below which a side between two corners is considered bent.
const SEGMENT_STRAIGHTNESS: f32 = 0.95;
/// Extra score charged to the as-drawn fallback polygon, so the ellipse wins ties against it.
const AS_DRAWN_PENALTY: f32 = 0.002;
/// Refined vertices with an interior angle above this (degrees) lie on a straight side and are dropped.
const COLLINEAR_ANGLE: f32 = 160.0;
/// Edges shorter than this fraction of the mean side, with both end angles above
/// `CHAMFER_ANGLE` degrees, are one rounded corner detected twice and get merged.
const SHORT_EDGE_RATIO: f32 = 0.2;
const CHAMFER_ANGLE: f32 = 115.0;
/// Mean symmetric fit error, relative to the stroke perimeter, above which the stroke stays freehand.
const MAX_FIT_ERR: f32 = 0.014;
/// Score penalty per vertex, so at equal fit the simpler outline wins. Ellipses count as `ELLIPSE_VERTICES`.
const VERTEX_PENALTY: f32 = 0.0003;
const ELLIPSE_VERTICES: f32 = 5.0;
/// Complexity charged to a template outline (star, heart) regardless of how many points draw it.
const TEMPLATE_VERTICES: usize = 10;
/// Score charged to the unsnapped variant of a polygon, so axis-aligned output wins ties.
const UNSNAPPED_PENALTY: f32 = 0.0008;
/// Edges / axes within this many degrees of horizontal or vertical are snapped onto it.
const AXIS_SNAP_DEG: f32 = 8.0;
const ELLIPSE_OUTLINE_PTS: usize = 36;

type V = (f32, f32);

#[inline]
fn sub(a: V, b: V) -> V {
    (a.0 - b.0, a.1 - b.1)
}
#[inline]
fn norm(a: V) -> f32 {
    (a.0 * a.0 + a.1 * a.1).sqrt()
}
#[inline]
fn dist(a: V, b: V) -> f32 {
    norm(sub(a, b))
}
#[inline]
fn dist_sq(a: V, b: V) -> f32 {
    let d = sub(a, b);
    d.0 * d.0 + d.1 * d.1
}

fn seg_dist_sq(p: V, a: V, b: V) -> f32 {
    let ab = sub(b, a);
    let len_sq = ab.0 * ab.0 + ab.1 * ab.1;
    if len_sq == 0.0 {
        return dist_sq(p, a);
    }
    let ap = sub(p, a);
    let t = ((ap.0 * ab.0 + ap.1 * ab.1) / len_sq).clamp(0.0, 1.0);
    dist_sq(p, (a.0 + t * ab.0, a.1 + t * ab.1))
}

fn closed_len(ring: &[V]) -> f32 {
    (0..ring.len()).map(|i| dist(ring[i], ring[(i + 1) % ring.len()])).sum()
}

fn centroid(pts: &[V]) -> V {
    let n = pts.len() as f32;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |acc, p| (acc.0 + p.0, acc.1 + p.1));
    (sx / n, sy / n)
}

/// Principal direction angle of a point set (radians).
fn principal_angle(pts: &[V], c: V) -> f32 {
    let (mut sxx, mut syy, mut sxy) = (0.0f32, 0.0f32, 0.0f32);
    for p in pts {
        let d = sub(*p, c);
        sxx += d.0 * d.0;
        syy += d.1 * d.1;
        sxy += d.0 * d.1;
    }
    0.5 * (2.0 * sxy).atan2(sxx - syy)
}

/// Width of the ring across its principal axis.
fn minor_extent(ring: &[V]) -> f32 {
    let c = centroid(ring);
    let (s, co) = principal_angle(ring, c).sin_cos();
    let (lo, hi) = ring.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        let v = -(p.0 - c.0) * s + (p.1 - c.1) * co;
        (lo.min(v), hi.max(v))
    });
    hi - lo
}

/// Resamples a closed stroke into `n` evenly spaced points (no duplicated end point).
/// Any overshoot past the starting point is trimmed first.
fn resample_closed(points: &[Point], n: usize) -> Option<Vec<V>> {
    let mut pts: Vec<V> = points.iter().map(|p| (p.x, p.y)).collect();
    if pts.len() < 3 {
        return None;
    }

    // Trim overshoot: within the last quarter of the stroke, cut at the sample closest to the start.
    let start = pts[0];
    let tail_from = pts.len() * 3 / 4;
    let (cut, _) = pts[tail_from..]
        .iter()
        .enumerate()
        .fold((pts.len() - 1, f32::MAX), |best, (i, p)| {
            let d = dist_sq(*p, start);
            if d < best.1 { (tail_from + i, d) } else { best }
        });
    pts.truncate(cut + 1);
    pts.push(start);

    let total: f32 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
    if total <= 0.0 {
        return None;
    }
    let step = total / n as f32;
    let mut out = Vec::with_capacity(n);
    out.push(pts[0]);
    let mut acc = 0.0f32;
    let mut prev = pts[0];
    let mut i = 1;
    while i < pts.len() && out.len() < n {
        let cur = pts[i];
        let d = dist(prev, cur);
        if acc + d >= step && d > 0.0 {
            let t = (step - acc) / d;
            let q = (prev.0 + t * (cur.0 - prev.0), prev.1 + t * (cur.1 - prev.1));
            out.push(q);
            prev = q;
            acc = 0.0;
        } else {
            acc += d;
            prev = cur;
            i += 1;
        }
    }
    while out.len() < n {
        out.push(start);
    }
    Some(out)
}

/// Resamples a closed polygon outline into `n` evenly spaced points.
fn sample_outline(outline: &[V], n: usize) -> Vec<V> {
    let k = outline.len();
    let total = closed_len(outline);
    let step = total / n as f32;
    let mut out = Vec::with_capacity(n);
    let mut seg = 0;
    let mut seg_start = 0.0f32;
    for s in 0..n {
        let target = s as f32 * step;
        loop {
            let seg_len = dist(outline[seg], outline[(seg + 1) % k]);
            if target <= seg_start + seg_len || seg == k - 1 {
                let t = if seg_len > 0.0 { ((target - seg_start) / seg_len).clamp(0.0, 1.0) } else { 0.0 };
                let a = outline[seg];
                let b = outline[(seg + 1) % k];
                out.push((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
                break;
            }
            seg_start += seg_len;
            seg += 1;
        }
    }
    out
}

/// Symmetric mean distance between the stroke ring and a closed outline:
/// stroke→outline catches bulges, outline→stroke catches parts of the outline the user never drew.
///
/// Returns `None` as soon as the error is known to exceed `limit`. The total is at least half the
/// cheap stroke→outline term, so hopeless candidates skip the O(N²) outline→stroke term entirely.
fn fit_error(ring: &[V], outline: &[V], limit: f32) -> Option<f32> {
    let k = outline.len();
    let n = ring.len() as f32;
    let fwd: f32 = ring
        .iter()
        .map(|p| (0..k).map(|j| seg_dist_sq(*p, outline[j], outline[(j + 1) % k])).fold(f32::MAX, f32::min).sqrt())
        .sum::<f32>()
        / n;
    if 0.5 * fwd > limit {
        return None;
    }
    // Running sum of the outline→stroke term may not exceed this.
    let bwd_budget = (2.0 * limit - fwd) * n;
    let mut bwd = 0.0f32;
    for q in sample_outline(outline, ring.len()) {
        bwd += ring.iter().map(|p| dist_sq(*p, q)).fold(f32::MAX, f32::min).sqrt();
        if bwd > bwd_budget {
            return None;
        }
    }
    Some(0.5 * (fwd + bwd / n))
}

/// Rounds an angle to the nearest multiple of 90° when within `AXIS_SNAP_DEG`.
fn snap_axis(theta: f32) -> f32 {
    let q = (theta / FRAC_PI_2).round() * FRAC_PI_2;
    if (theta - q).abs() < AXIS_SNAP_DEG.to_radians() { q } else { theta }
}

fn rotate_about(pts: &mut [V], c: V, angle: f32) {
    let (s, co) = angle.sin_cos();
    for p in pts.iter_mut() {
        let d = sub(*p, c);
        *p = (c.0 + d.0 * co - d.1 * s, c.1 + d.0 * s + d.1 * co);
    }
}

/// Rotates the polygon so its most axis-aligned edge becomes exactly horizontal/vertical, if close.
fn snap_polygon_to_axis(verts: &mut [V]) {
    let k = verts.len();
    let mut best = f32::MAX;
    for j in 0..k {
        let e = sub(verts[(j + 1) % k], verts[j]);
        let a = e.1.atan2(e.0);
        let delta = snap_axis(a) - a;
        if delta != 0.0 && delta.abs() < best.abs() {
            best = delta;
        }
        if delta == 0.0 && (a / FRAC_PI_2).fract().abs() < 1e-6 {
            return; // already exact
        }
    }
    if best != f32::MAX {
        let c = centroid(verts);
        rotate_about(verts, c, best);
    }
}

// ---------------------------------------------------------------------------
// Ellipse candidate
// ---------------------------------------------------------------------------

fn fit_ellipse(ring: &[V]) -> Option<(ShapeKind, Vec<V>)> {
    let c = centroid(ring);
    let mut theta = principal_angle(ring, c);
    let (s, co) = theta.sin_cos();
    let (mut min_u, mut max_u, mut min_v, mut max_v) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in ring {
        let d = sub(*p, c);
        let u = d.0 * co + d.1 * s;
        let v = -d.0 * s + d.1 * co;
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    let (mut a, mut b) = ((max_u - min_u) * 0.5, (max_v - min_v) * 0.5);
    if a <= 0.0 || b <= 0.0 {
        return None;
    }
    let (mu, mv) = ((max_u + min_u) * 0.5, (max_v + min_v) * 0.5);
    let center = (c.0 + mu * co - mv * s, c.1 + mu * s + mv * co);

    let kind = if a.min(b) / a.max(b) > 0.85 {
        let r = (a + b) * 0.5;
        a = r;
        b = r;
        theta = 0.0;
        ShapeKind::Circle
    } else {
        theta = snap_axis(theta);
        ShapeKind::Ellipse
    };

    let (s, co) = theta.sin_cos();
    let outline = (0..ELLIPSE_OUTLINE_PTS)
        .map(|i| {
            let t = i as f32 / ELLIPSE_OUTLINE_PTS as f32 * 2.0 * PI;
            let (u, v) = (a * t.cos(), b * t.sin());
            (center.0 + u * co - v * s, center.1 + u * s + v * co)
        })
        .collect();
    Some((kind, outline))
}

// ---------------------------------------------------------------------------
// Polygon candidate
// ---------------------------------------------------------------------------

/// ShortStraw corner detection on a closed, evenly resampled ring. Returns ring indices in order.
fn find_corners(ring: &[V]) -> Vec<usize> {
    let n = ring.len();
    let straws: Vec<f32> = (0..n).map(|i| dist(ring[(i + n - STRAW_W) % n], ring[(i + STRAW_W) % n])).collect();
    let mut sorted = straws.clone();
    sorted.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let thr = sorted[n / 2] * STRAW_RATIO;

    // Start scanning just after a non-corner sample so runs never wrap.
    let Some(start) = (0..n).find(|&i| straws[i] >= thr) else { return Vec::new() };
    let mut corners = Vec::new();
    let mut run_min: Option<usize> = None;
    for k in 1..=n {
        let i = (start + k) % n;
        if straws[i] < thr {
            if run_min.is_none_or(|m| straws[i] < straws[m]) {
                run_min = Some(i);
            }
        } else if let Some(m) = run_min.take() {
            corners.push(m);
        }
    }

    // Top-down pass (from the ShortStraw paper): a segment between two corners that is not
    // straight must hide another corner, so add the tightest straw inside it. Shallow
    // corners (octagons, parallelograms) often only surface here. Seed two corners when the
    // first pass finds fewer, so smooth strokes still yield a polygon candidate to score.
    if corners.len() < 2 {
        let m = (0..n).min_by(|&a, &b| straws[a].total_cmp(&straws[b])).unwrap_or(0);
        corners = vec![m.min((m + n / 2) % n), m.max((m + n / 2) % n)];
    }
    corners.sort_unstable();
    loop {
        let k = corners.len();
        if k > MAX_POLY_VERTICES {
            break;
        }
        let mut added = None;
        for j in 0..k {
            let (a, b) = (corners[j], corners[(j + 1) % k]);
            let len = (b + n - a) % n;
            if len < 2 * STRAW_W + 2 {
                continue;
            }
            let path: f32 = (0..len).map(|t| dist(ring[(a + t) % n], ring[(a + t + 1) % n])).sum();
            if dist(ring[a], ring[b]) / path.max(1e-9) >= SEGMENT_STRAIGHTNESS {
                continue;
            }
            let inner = (STRAW_W..=len - STRAW_W).map(|t| (a + t) % n);
            if let Some(m) = inner.min_by(|&x, &y| straws[x].total_cmp(&straws[y])) {
                added = Some(m);
                break;
            }
        }
        match added {
            Some(m) => {
                corners.push(m);
                corners.sort_unstable();
            }
            None => break,
        }
    }

    // Drop shallow "corners" (wobble on a straight side or a smooth curve).
    let span = 2 * STRAW_W;
    corners.retain(|&i| {
        let a = sub(ring[(i + n - span) % n], ring[i]);
        let b = sub(ring[(i + span) % n], ring[i]);
        let cos = (a.0 * b.0 + a.1 * b.1) / (norm(a) * norm(b)).max(1e-9);
        cos.clamp(-1.0, 1.0).acos().to_degrees() < MAX_CORNER_ANGLE
    });
    corners.sort_unstable();
    corners
}

/// Refines corner positions by fitting a line to each side and intersecting neighbours.
/// Hand-drawn corners are rounded, so the raw corner sample sits inside the true vertex.
fn refine_vertices(ring: &[V], corners: &[usize]) -> Vec<V> {
    let n = ring.len();
    let k = corners.len();
    let lines: Vec<(V, V)> = (0..k)
        .map(|j| {
            let (a, b) = (corners[j], corners[(j + 1) % k]);
            let len = (b + n - a) % n;
            let margin = (len / 5).min(2);
            let side: Vec<V> = (margin..=len.saturating_sub(margin)).map(|t| ring[(a + t) % n]).collect();
            let c = centroid(&side);
            let ang = principal_angle(&side, c);
            (c, (ang.cos(), ang.sin()))
        })
        .collect();

    (0..k)
        .map(|j| {
            let (p, r) = lines[(j + k - 1) % k];
            let (q, s) = lines[j];
            let cross = r.0 * s.1 - r.1 * s.0;
            let raw = ring[corners[j]];
            if cross.abs() < 0.05 {
                return raw;
            }
            let qp = sub(q, p);
            let t = (qp.0 * s.1 - qp.1 * s.0) / cross;
            let x = (p.0 + t * r.0, p.1 + t * r.1);
            // Guard against near-parallel sides throwing the vertex far away.
            let side_len = dist(ring[corners[(j + k - 1) % k]], raw).max(dist(raw, ring[corners[(j + 1) % k]]));
            if dist(x, raw) > 0.5 * side_len { raw } else { x }
        })
        .collect()
}

/// Removes vertices that sit on a straight side (flattest first) and merges chamfers
/// (a rounded corner detected as two).
fn drop_collinear(mut verts: Vec<V>) -> Vec<V> {
    while verts.len() > 3 {
        let k = verts.len();
        let sides = side_lengths(&verts);
        let angles = interior_angles(&verts);
        let mean = sides.iter().sum::<f32>() / k as f32;
        // A chamfer has obtuse corners at both ends; the short side of a thin box has right angles.
        let chamfer = |j: usize| {
            sides[j] < SHORT_EDGE_RATIO * mean && angles[j] > CHAMFER_ANGLE && angles[(j + 1) % k] > CHAMFER_ANGLE
        };
        let Some(j) = (0..k).filter(|&j| chamfer(j)).min_by(|&a, &b| sides[a].total_cmp(&sides[b])) else {
            break;
        };
        let (a, b) = (verts[j], verts[(j + 1) % k]);
        verts[j] = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
        verts.remove((j + 1) % k);
    }
    while verts.len() > 3 {
        let angles = interior_angles(&verts);
        let (j, a) = angles.iter().enumerate().fold((0, 0.0f32), |b, (j, &a)| if a > b.1 { (j, a) } else { b });
        if a < COLLINEAR_ANGLE {
            break;
        }
        verts.remove(j);
    }
    verts
}

fn interior_angles(verts: &[V]) -> Vec<f32> {
    let k = verts.len();
    (0..k)
        .map(|j| {
            let a = sub(verts[(j + k - 1) % k], verts[j]);
            let b = sub(verts[(j + 1) % k], verts[j]);
            let cos = (a.0 * b.0 + a.1 * b.1) / (norm(a) * norm(b)).max(1e-9);
            cos.clamp(-1.0, 1.0).acos().to_degrees()
        })
        .collect()
}

fn side_lengths(verts: &[V]) -> Vec<f32> {
    let k = verts.len();
    (0..k).map(|j| dist(verts[j], verts[(j + 1) % k])).collect()
}

fn ratio_max_min(xs: &[f32]) -> f32 {
    let (mn, mx) = xs.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    mx / mn.max(1e-9)
}

fn is_parallel(a: V, b: V, tol_deg: f32) -> bool {
    let cross = (a.0 * b.1 - a.1 * b.0).abs() / (norm(a) * norm(b)).max(1e-9);
    cross < tol_deg.to_radians().sin()
}

/// Regular polygon with the stroke's centre, the vertices' phase and winding, and a radius
/// matched to the whole stroke (vertex estimates sit inside rounded corners, so they run small).
fn regular_polygon(verts: &[V], ring: &[V]) -> Vec<V> {
    let k = verts.len();
    let c = centroid(ring);
    // Average the phase of all vertices (k-fold symmetric) for a stable rotation.
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for v in verts {
        let a = (v.1 - c.1).atan2(v.0 - c.0) * k as f32;
        sx += a.cos();
        sy += a.sin();
    }
    let phase = sy.atan2(sx) / k as f32;
    // Keep the drawn winding direction so the stroke order is preserved.
    let area2: f32 = (0..k).map(|j| verts[j].0 * verts[(j + 1) % k].1 - verts[(j + 1) % k].0 * verts[j].1).sum();
    let dir = if area2 >= 0.0 { 1.0 } else { -1.0 };
    let build = |r: f32| -> Vec<V> {
        (0..k)
            .map(|j| {
                let a = phase + dir * j as f32 * 2.0 * PI / k as f32;
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect()
    };
    // Scale so the outline's mean distance from the centre (by arc length) matches the stroke's.
    let unit = build(1.0);
    let samples = sample_outline(&unit, ring.len());
    let mean_factor = samples.iter().map(|p| dist(*p, c)).sum::<f32>() / samples.len() as f32;
    let ring_mean = ring.iter().map(|p| dist(*p, c)).sum::<f32>() / ring.len() as f32;
    build(ring_mean / mean_factor)
}

/// Names a polygon by its vertex count and parallel sides, without changing it.
fn as_drawn_kind(verts: &[V]) -> ShapeKind {
    match verts.len() {
        3 => ShapeKind::Triangle,
        4 => {
            let p0 = is_parallel(sub(verts[1], verts[0]), sub(verts[2], verts[3]), 10.0);
            let p1 = is_parallel(sub(verts[2], verts[1]), sub(verts[3], verts[0]), 10.0);
            match (p0, p1) {
                (true, true) => ShapeKind::Parallelogram,
                (true, false) | (false, true) => ShapeKind::Trapezoid,
                _ => ShapeKind::Polygon,
            }
        }
        5 => ShapeKind::Pentagon,
        6 => ShapeKind::Hexagon,
        _ => ShapeKind::Polygon,
    }
}

/// Classifies a polygon and returns its cleaned-up outline, plus whether the result is a shape
/// people deliberately draw. Irregular 5+-gons and irregular quadrilaterals are usually a
/// rounded corner read as an extra vertex, so they score as a fallback only.
fn regularize(mut verts: Vec<V>, ring: &[V], snap: bool) -> (ShapeKind, Vec<V>, bool) {
    let k = verts.len();
    let angles = interior_angles(&verts);
    let sides = side_lengths(&verts);
    let side_ratio = ratio_max_min(&sides);

    match k {
        3 => {
            if side_ratio < 1.15 {
                verts = regular_polygon(&verts, ring);
            }
            if snap {
                snap_polygon_to_axis(&mut verts);
            }
            (ShapeKind::Triangle, verts, true)
        }
        4 => {
            // Rectangles have right angles *and* equal diagonals; a near-square rhombus only has the former.
            let diag_ratio = ratio_max_min(&[dist(verts[0], verts[2]), dist(verts[1], verts[3])]);
            // On thin boxes the short sides are too short for reliable angles, but near-equal
            // diagonals alone still identify a rectangle.
            let right = diag_ratio < 1.04 || (diag_ratio < 1.12 && angles.iter().all(|a| (a - 90.0).abs() < 14.0));
            if right {
                // Length-weighted edge orientation modulo 90° (4θ trick), then extents along it.
                // Weighting matters on thin boxes, whose short sides have unreliable directions.
                let (mut sx, mut sy) = (0.0f32, 0.0f32);
                for j in 0..4 {
                    let e = sub(verts[(j + 1) % 4], verts[j]);
                    let a = 4.0 * e.1.atan2(e.0);
                    sx += norm(e) * a.cos();
                    sy += norm(e) * a.sin();
                }
                let mut phi = sy.atan2(sx) / 4.0;
                let c = centroid(&verts);
                let (s, co) = phi.sin_cos();
                let (mut w, mut h) = (0.0f32, 0.0f32);
                for v in &verts {
                    let d = sub(*v, c);
                    w = w.max((d.0 * co + d.1 * s).abs());
                    h = h.max((-d.0 * s + d.1 * co).abs());
                }
                let square = w.min(h) / w.max(h) > 0.85;
                if square {
                    let m = (w + h) * 0.5;
                    w = m;
                    h = m;
                }
                // A square standing on a corner is what people call a diamond.
                let tilt = (phi.rem_euclid(FRAC_PI_2) - FRAC_PI_2 * 0.5).abs().to_degrees();
                let kind = if square && tilt < 12.0 {
                    if snap {
                        phi = FRAC_PI_2 * 0.5;
                    }
                    ShapeKind::Diamond
                } else {
                    if snap {
                        phi = snap_axis(phi);
                    }
                    if square { ShapeKind::Square } else { ShapeKind::Rectangle }
                };
                let (s, co) = phi.sin_cos();
                let corner = |u: f32, v: f32| (c.0 + u * co - v * s, c.1 + u * s + v * co);
                return (kind, vec![corner(-w, -h), corner(w, -h), corner(w, h), corner(-w, h)], true);
            }
            let p0 = is_parallel(sub(verts[1], verts[0]), sub(verts[2], verts[3]), 10.0);
            let p1 = is_parallel(sub(verts[2], verts[1]), sub(verts[3], verts[0]), 10.0);
            if side_ratio < 1.12 && p0 && p1 {
                // Rhombus: keep both diagonals, make them perpendicular and axis-aligned when close.
                let c = centroid(&verts);
                let d1 = sub(verts[2], verts[0]);
                let a1 = if snap { snap_axis(d1.1.atan2(d1.0)) } else { d1.1.atan2(d1.0) };
                let l1 = norm(d1) * 0.5;
                let l2 = dist(verts[3], verts[1]) * 0.5;
                let d2 = sub(verts[3], verts[1]);
                let cross = d1.0 * d2.1 - d1.1 * d2.0;
                let a2 = a1 + if cross >= 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
                let at = |a: f32, l: f32| (c.0 + l * a.cos(), c.1 + l * a.sin());
                return (ShapeKind::Diamond, vec![at(a1 + PI, l1), at(a2 + PI, l2), at(a1, l1), at(a2, l2)], true);
            }
            let kind = as_drawn_kind(&verts);
            if snap {
                snap_polygon_to_axis(&mut verts);
            }
            (kind, verts, kind != ShapeKind::Polygon)
        }
        _ => {
            let (amin, amax) = angles.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
            let regular = side_ratio < 1.35 && amax - amin < 22.0;
            if regular {
                verts = regular_polygon(&verts, ring);
            }
            if snap {
                snap_polygon_to_axis(&mut verts);
            }
            (as_drawn_kind(&verts), verts, regular)
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Fits a closed stroke (isotropic coordinates). Returns `None` when no candidate fits well enough.
/// `templates` are extra candidates from dedicated detectors (star, heart); they compete on the
/// same fit score so a loose detector cannot steal strokes that another outline explains better.
pub(crate) fn fit_closed(points: &[Point], templates: &[RecognizedShape]) -> Option<RecognizedShape> {
    let mut ring = resample_closed(points, N)?;
    let perim = closed_len(&ring);
    if perim < 0.08 {
        return None;
    }
    // Thin shapes need denser sampling, or each short side falls inside the corner window.
    let minor = minor_extent(&ring);
    if minor > 0.0 {
        let needed = (perim / (minor / MIN_SAMPLES_PER_SIDE)).ceil() as usize;
        if needed > N {
            ring = resample_closed(points, needed.min(N_MAX))?;
        }
    }

    let mut best: Option<(f32, ShapeKind, Vec<V>)> = None;
    // Returns whether the candidate became the best. Fitting stops early once it can neither
    // pass MAX_FIT_ERR nor beat the current best score.
    let mut consider = |kind: ShapeKind, outline: Vec<V>, penalty: f32| {
        let beat = best.as_ref().map_or(f32::MAX, |b| b.0 - penalty);
        let Some(err) = fit_error(&ring, &outline, MAX_FIT_ERR.min(beat) * perim).map(|e| e / perim) else {
            return false;
        };
        best = Some((err + penalty, kind, outline));
        true
    };

    if let Some((kind, outline)) = fit_ellipse(&ring) {
        consider(kind, outline, VERTEX_PENALTY * ELLIPSE_VERTICES);
    }
    for t in templates {
        let mut outline: Vec<V> = t.points.iter().map(|p| (p.x, p.y)).collect();
        if outline.len() > 1 && outline.first() == outline.last() {
            outline.pop();
        }
        let k = outline.len().min(TEMPLATE_VERTICES) as f32;
        consider(t.kind, outline, VERTEX_PENALTY * k);
    }
    let corners = find_corners(&ring);
    if (3..=MAX_POLY_VERTICES).contains(&corners.len()) {
        // Score the polygon at every vertex count, dropping the flattest vertex each time,
        // so a corner detected twice cannot force an extra side (at most 6 levels).
        let mut verts = drop_collinear(refine_vertices(&ring, &corners));
        while verts.len() >= 3 {
            let k = verts.len() as f32;
            // Axis snapping is preferred but scored, not forced: on a long thin box a few
            // degrees of rotation moves the ends further than the box is tall.
            let mut won = false;
            for snap in [true, false] {
                let (kind, outline, intended) = regularize(verts.clone(), &ring, snap);
                let penalty = VERTEX_PENALTY * k
                    + if intended { 0.0 } else { AS_DRAWN_PENALTY }
                    + if snap { 0.0 } else { UNSNAPPED_PENALTY };
                won |= consider(kind, outline, penalty);
            }
            // Regularizing always costs some fit, so the as-drawn polygon is only a fallback
            // for when the cleaned-up outline does not win.
            if !won {
                consider(as_drawn_kind(&verts), verts.clone(), VERTEX_PENALTY * k + AS_DRAWN_PENALTY);
            }
            if verts.len() == 3 {
                break;
            }
            let angles = interior_angles(&verts);
            let flattest = (0..angles.len()).max_by(|&a, &b| angles[a].total_cmp(&angles[b])).unwrap_or(0);
            verts.remove(flattest);
        }
    }

    let (_, kind, outline) = best?;
    let avg_p = points.iter().map(|p| p.pressure).sum::<f32>() / points.len() as f32;
    let t0 = points[0].timestamp_ms;
    let mut out: Vec<Point> = outline
        .iter()
        .enumerate()
        .map(|(i, v)| Point::new(v.0, v.1, avg_p, t0 + i as u64 * 10))
        .collect();
    out.push(Point::new(outline[0].0, outline[0].1, avg_p, t0 + outline.len() as u64 * 10));
    Some(RecognizedShape { kind, points: out })
}
