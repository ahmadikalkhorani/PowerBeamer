//! Synthetic accuracy harness for the auto-shape recognizer.
//!
//! Each class is drawn many times the way a person would: random size, rotation and slide
//! aspect, rounded corners, hand jitter, and a small gap or overshoot where the stroke closes.
//! Run with `cargo test --test shape_recognition -- --nocapture` to see the confusion table.

use presenter::ink::{Point, ShapeKind, ShapeRecognizer};
use std::collections::BTreeMap;
use std::f32::consts::PI;

/// Small deterministic PRNG so failures are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
    fn gauss(&mut self) -> f32 {
        (0..4).map(|_| self.next()).sum::<f32>() - 2.0
    }
}

const ASPECTS: [f32; 3] = [1.0, 16.0 / 9.0, 4.0 / 3.0];

fn rotate(pts: &mut [(f32, f32)], c: (f32, f32), a: f32) {
    let (s, co) = a.sin_cos();
    for p in pts.iter_mut() {
        let (dx, dy) = (p.0 - c.0, p.1 - c.1);
        *p = (c.0 + dx * co - dy * s, c.1 + dx * s + dy * co);
    }
}

/// Densely samples a closed polygon outline, rounds the corners, adds jitter and a closing gap/overshoot,
/// and converts from isotropic units to normalized slide coordinates.
fn humanize_closed(outline: &[(f32, f32)], rng: &mut Rng, aspect: f32) -> Vec<Point> {
    let k = outline.len();
    let perim: f32 = (0..k)
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % k]);
            ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
        })
        .sum();
    let n = 240usize;
    let start = rng.next() * perim;
    let min_side = (0..k)
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % k]);
            ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
        })
        .fold(f32::MAX, f32::min);
    // Negative = gap, positive = overshoot; never so large that a whole short side goes missing.
    let closing = (rng.range(-0.04, 0.06) * perim).clamp(-0.4 * min_side, 0.4 * min_side.max(0.1 * perim));
    let total = perim + closing;
    let at = |d: f32| -> (f32, f32) {
        let mut d = d.rem_euclid(perim);
        for i in 0..k {
            let (a, b) = (outline[i], outline[(i + 1) % k]);
            let l = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            if d <= l {
                let t = d / l.max(1e-9);
                return (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
            }
            d -= l;
        }
        outline[0]
    };
    let raw: Vec<(f32, f32)> = (0..n).map(|i| at(start + total * i as f32 / (n - 1) as f32)).collect();

    // Round corners with a moving average, then add low-frequency hand wobble.
    // People round a corner by roughly a tenth of the (shortest) side.
    let win = ((n as f32 * min_side / perim / 10.0) as usize).clamp(1, 6);
    let sigma = 0.004 * perim / 4.0;
    let (mut wx, mut wy) = (0.0f32, 0.0f32);
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(win);
            let hi = (i + win).min(n - 1);
            let m = (hi - lo + 1) as f32;
            let (sx, sy) = raw[lo..=hi].iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
            wx = 0.7 * wx + 0.3 * rng.gauss() * sigma;
            wy = 0.7 * wy + 0.3 * rng.gauss() * sigma;
            Point::new((sx / m + wx) / aspect, sy / m + wy, 1.0, i as u64 * 8)
        })
        .collect()
}

fn regular(c: (f32, f32), r: f32, k: usize, phase: f32) -> Vec<(f32, f32)> {
    (0..k).map(|j| {
        let a = phase + j as f32 * 2.0 * PI / k as f32;
        (c.0 + r * a.cos(), c.1 + r * a.sin())
    }).collect()
}

fn ellipse(c: (f32, f32), a: f32, b: f32, rot: f32) -> Vec<(f32, f32)> {
    let mut pts: Vec<(f32, f32)> = (0..72).map(|i| {
        let t = i as f32 / 72.0 * 2.0 * PI;
        (c.0 + a * t.cos(), c.1 + b * t.sin())
    }).collect();
    rotate(&mut pts, c, rot);
    pts
}

fn rect(c: (f32, f32), w: f32, h: f32, rot: f32) -> Vec<(f32, f32)> {
    let mut pts = vec![(c.0 - w, c.1 - h), (c.0 + w, c.1 - h), (c.0 + w, c.1 + h), (c.0 - w, c.1 + h)];
    rotate(&mut pts, c, rot);
    pts
}

/// Builds one hand-drawn sample of `class`, returning the stroke, its slide aspect and the kinds accepted as correct.
fn sample(class: &str, rng: &mut Rng) -> (Vec<Point>, f32, Vec<ShapeKind>) {
    let aspect = ASPECTS[(rng.next() * 3.0) as usize % 3];
    let c = (0.5 * aspect, 0.5);
    let size = rng.range(0.08, 0.2);
    let deg = |d: f32| d.to_radians();
    use ShapeKind::*;
    let (outline, ok) = match class {
        "circle" => (ellipse(c, size, size * rng.range(0.92, 1.0), rng.range(0.0, PI)), vec![Circle]),
        "ellipse" => (ellipse(c, size, size * rng.range(0.4, 0.65), rng.range(0.0, PI)), vec![Ellipse]),
        "square" => (rect(c, size, size * rng.range(0.93, 1.0), deg(rng.range(-25.0, 25.0))), vec![Square]),
        "rectangle" => {
            let r = rng.range(1.5, 4.0);
            let (w, h) = if rng.next() < 0.5 { (size, size / r) } else { (size / r, size) };
            (rect(c, w, h, deg(rng.range(-25.0, 25.0))), vec![Rectangle])
        }
        "thin rectangle" => {
            let r = rng.range(5.0, 11.0);
            (rect(c, size * 1.5, size * 1.5 / r, deg(rng.range(-8.0, 8.0))), vec![Rectangle])
        }
        "diamond" => {
            let r = rng.range(0.55, 1.0);
            let mut pts = vec![(c.0, c.1 - size), (c.0 + size * r, c.1), (c.0, c.1 + size), (c.0 - size * r, c.1)];
            rotate(&mut pts, c, deg(rng.range(-6.0, 6.0)));
            (pts, vec![Diamond])
        }
        "triangle" => {
            let a0 = rng.range(0.0, 2.0 * PI);
            let pts = (0..3).map(|j| {
                let a = a0 + j as f32 * 2.0 * PI / 3.0 + rng.range(-0.35, 0.35);
                let r = size * rng.range(0.8, 1.1);
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            }).collect();
            (pts, vec![Triangle])
        }
        "parallelogram" => {
            let (w, h, k) = (size, size * rng.range(0.35, 0.5), size * rng.range(0.3, 0.5));
            let mut pts = vec![(c.0 - w + k, c.1 - h), (c.0 + w + k, c.1 - h), (c.0 + w - k, c.1 + h), (c.0 - w - k, c.1 + h)];
            rotate(&mut pts, c, deg(rng.range(-5.0, 5.0)));
            (pts, vec![Parallelogram])
        }
        "trapezoid" => {
            let (w, h, top) = (size, size * rng.range(0.5, 0.8), rng.range(0.35, 0.65));
            let mut pts = vec![(c.0 - w * top, c.1 - h), (c.0 + w * top, c.1 - h), (c.0 + w, c.1 + h), (c.0 - w, c.1 + h)];
            rotate(&mut pts, c, deg(rng.range(-5.0, 5.0)));
            (pts, vec![Trapezoid])
        }
        "octagon" => (regular(c, size, 8, rng.range(0.0, PI)), vec![Polygon]),
        "pentagon" => (regular(c, size, 5, rng.range(0.0, PI)), vec![Pentagon]),
        "hexagon" => (regular(c, size, 6, rng.range(0.0, PI)), vec![Hexagon]),
        _ => unreachable!(),
    };
    (humanize_closed(&outline, rng, aspect), aspect, ok)
}

const CLASSES: [&str; 12] = [
    "circle", "ellipse", "square", "rectangle", "thin rectangle", "diamond", "triangle", "parallelogram", "trapezoid", "pentagon", "hexagon",
    "octagon",
];
const TRIALS: usize = 100;
const MIN_ACCURACY: f32 = 0.95;

/// A hand-drawn octagon is genuinely close to a circle; misses land on Circle, which is an
/// acceptable reading, so this class gets a lower floor rather than over-tuning for it.
fn min_accuracy(class: &str) -> f32 {
    if class == "octagon" { 0.7 } else { MIN_ACCURACY }
}

#[test]
fn synthetic_shapes_are_classified() {
    let mut rng = Rng(0x5eed);
    let mut failures = Vec::new();
    for class in CLASSES {
        let mut hits = 0;
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for _ in 0..TRIALS {
            let (stroke, aspect, ok) = sample(class, &mut rng);
            let got = ShapeRecognizer::recognize_with_aspect(&stroke, false, aspect).map(|s| s.kind);
            *seen.entry(got.map_or("None".to_string(), |k| format!("{k:?}"))).or_default() += 1;
            if got.is_some_and(|k| ok.contains(&k)) {
                hits += 1;
            }
        }
        let acc = hits as f32 / TRIALS as f32;
        eprintln!("{class:>10}: {:5.1}%  {:?}", acc * 100.0, seen);
        if acc < min_accuracy(class) {
            failures.push(format!("{class}: {:.0}%", acc * 100.0));
        }
    }
    assert!(failures.is_empty(), "classes below their accuracy floor: {failures:?}");
}

/// Regression: a tall, thin box drawn on a 4:3 slide used to snap to a diamond.
#[test]
fn tall_thin_rectangle_is_not_a_diamond() {
    let aspect = 4.0 / 3.0;
    // Screen-space box 56×188 px inside a 700×525 px slide, drawn clockwise with a slight closing gap.
    let corners = [(0.40f32, 0.30f32), (0.48, 0.30), (0.48, 0.658), (0.40, 0.658), (0.40, 0.31)];
    let mut pts = Vec::new();
    for w in corners.windows(2) {
        for j in 0..12 {
            let t = j as f32 / 12.0;
            pts.push(Point::new(w[0].0 + t * (w[1].0 - w[0].0), w[0].1 + t * (w[1].1 - w[0].1), 1.0, pts.len() as u64 * 8));
        }
    }
    let got = ShapeRecognizer::recognize_with_aspect(&pts, false, aspect).expect("should snap");
    assert_eq!(got.kind, ShapeKind::Rectangle);
    // Output stays axis-aligned in slide coordinates.
    let xs: Vec<f32> = got.points.iter().map(|p| p.x).collect();
    assert!(xs.iter().all(|x| (x - 0.40).abs() < 0.01 || (x - 0.48).abs() < 0.01), "{xs:?}");
}

/// A square in screen space is not square in normalized coordinates on a 16:9 slide.
#[test]
fn aspect_ratio_is_respected() {
    let aspect = 16.0 / 9.0;
    let (cx, cy, r) = (0.5f32, 0.5f32, 0.15f32);
    let circle: Vec<Point> = (0..=48)
        .map(|i| {
            let t = i as f32 / 48.0 * 2.0 * PI;
            Point::new(cx + r * t.cos() / aspect, cy + r * t.sin(), 1.0, i as u64 * 8)
        })
        .collect();
    let got = ShapeRecognizer::recognize_with_aspect(&circle, false, aspect).expect("should snap");
    assert_eq!(got.kind, ShapeKind::Circle);
}

/// Recognition runs on the UI thread every hold tick, so it must stay well under a frame.
#[test]
fn recognition_is_fast() {
    let mut rng = Rng(7);
    let strokes: Vec<(Vec<Point>, f32)> = (0..200)
        .map(|i| {
            let (s, a, _) = sample(CLASSES[i % CLASSES.len()], &mut rng);
            (s, a)
        })
        .collect();
    let start = std::time::Instant::now();
    for (s, a) in &strokes {
        std::hint::black_box(ShapeRecognizer::recognize_with_aspect(s, false, *a));
    }
    let per = start.elapsed() / strokes.len() as u32;
    eprintln!("recognize: {per:?} per stroke");
    // Generous bound so unoptimized debug builds on slow CI still pass; release is ~10–20× faster.
    assert!(per < std::time::Duration::from_millis(4), "too slow: {per:?}");
}
