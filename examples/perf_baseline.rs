//! Reference workloads compatible with the pre-optimization app.
use presenter::{
    ink::{InkingRasterizer, Point, SlideInking, Stroke},
    pdf::{MediaResolver, PdfExporter, PdfiumLoader, PresentationDeck, RenderQuality},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
struct Alloc;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let n = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(n, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) };
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() {
            LIVE.fetch_sub(l.size(), Ordering::Relaxed);
            let v = LIVE.fetch_add(n, Ordering::Relaxed) + n;
            PEAK.fetch_max(v, Ordering::Relaxed);
        }
        q
    }
}
#[global_allocator]
static ALLOC: Alloc = Alloc;
fn stroke(id: u64) -> Stroke {
    let mut s = Stroke::new(id, [20, 40, 80, 255], 3.5, false);
    for i in 0..200 {
        s.add_point(Point::new(i as f32 / 400., (id % 50) as f32 / 100., 1., i));
    }
    s
}
fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "undo".into());
    let start = Instant::now();
    match mode.as_str() {
        "startup" => {
            let shown = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
            let redundant = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
            black_box(&redundant);
            drop(redundant);
            black_box(&shown);
            println!(
                "{{\"mode\":\"startup\",\"ms\":{:.3},\"peak_heap_bytes\":{}}}",
                start.elapsed().as_secs_f64() * 1000.,
                PEAK.load(Ordering::Relaxed)
            );
        }
        "undo" => {
            let mut ink = SlideInking::new();
            for i in 0..500 {
                ink.add_stroke(stroke(i));
            }
            let retained = LIVE.load(Ordering::Relaxed);
            let build = start.elapsed().as_secs_f64() * 1000.;
            for _ in 0..500 {
                assert!(ink.undo());
            }
            for _ in 0..500 {
                assert!(ink.redo());
            }
            assert_eq!(ink.strokes.len(), 500);
            println!(
                "{{\"mode\":\"undo\",\"build_ms\":{build:.3},\"retained_bytes\":{retained},\"total_ms\":{:.3},\"peak_heap_bytes\":{}}}",
                start.elapsed().as_secs_f64() * 1000.,
                PEAK.load(Ordering::Relaxed)
            );
        }
        "deck" => {
            let mut all = Vec::new();
            for _ in 0..20 {
                let (slides, _) = PdfiumLoader::load_deck_from_file(
                    std::path::Path::new("test_deck/presentation.pdf"),
                    RenderQuality::Balanced,
                )
                .unwrap();
                all.extend(slides);
            }
            let bytes: u64 = all
                .iter()
                .map(|s| {
                    u64::from(s.audience_image.width()) * u64::from(s.audience_image.height()) * 4
                })
                .sum();
            println!(
                "{{\"mode\":\"deck\",\"slides\":{},\"ms\":{:.3},\"raw_pixel_bytes\":{bytes},\"retained_heap_bytes\":{},\"peak_heap_bytes\":{}}}",
                all.len(),
                start.elapsed().as_secs_f64() * 1000.,
                LIVE.load(Ordering::Relaxed),
                PEAK.load(Ordering::Relaxed)
            );
            black_box(&all);
        }
        "ink" => {
            let base = InkingRasterizer::render_strokes(
                1600,
                900,
                &(0..100).map(stroke).collect::<Vec<_>>(),
                None,
                None,
            );
            let mut active = Stroke::new(101, [20, 40, 80, 255], 3.5, false);
            let mut layer = base.clone();
            let mut committed = 0;
            for i in 0..240 {
                active.add_point(Point::new(
                    0.05 + i as f32 / 400.,
                    0.5 + (i as f32 * 0.1).sin() * 0.05,
                    1.,
                    i,
                ));
                let n = active.points.len();
                let mut frame;
                if n >= 3 {
                    let end = n - 2;
                    InkingRasterizer::draw_stroke_segments(&mut layer, &active, committed, end);
                    committed = end;
                    frame = layer.clone();
                    InkingRasterizer::draw_stroke_segments(&mut frame, &active, committed, n - 1);
                } else {
                    frame = base.clone();
                    InkingRasterizer::draw_stroke(&mut frame, &active);
                }
                black_box(frame);
            }
            println!(
                "{{\"mode\":\"ink_reference\",\"ms\":{:.3},\"published_bytes\":{},\"peak_heap_bytes\":{}}}",
                start.elapsed().as_secs_f64() * 1000.,
                240u64 * 1600 * 900 * 4,
                PEAK.load(Ordering::Relaxed)
            );
        }
        "tex" => {
            let path = std::path::Path::new("test_deck/presentation.pdf");
            let map = MediaResolver::scan_companion_tex(path);
            for i in 0..300 {
                black_box(MediaResolver::resolve_page_clips(path, i % 7 + 1, "", &map));
            }
            println!(
                "{{\"mode\":\"tex_reference\",\"ms\":{:.3}}}",
                start.elapsed().as_secs_f64() * 1000.
            );
        }
        "fingerprint" => {
            let output = std::env::args().nth(2).expect("output directory");
            std::fs::create_dir_all(&output).unwrap();
            for (name, quality) in [
                ("eco", RenderQuality::Eco),
                ("balanced", RenderQuality::Balanced),
                ("ultra", RenderQuality::UltraVector),
            ] {
                let (slides, _) = PdfiumLoader::load_deck_from_file(
                    std::path::Path::new("test_deck/presentation.pdf"),
                    quality,
                )
                .unwrap();
                for (i, slide) in slides.iter().enumerate() {
                    slide
                        .audience_image
                        .save(format!("{output}/{name}_{i}_aud.png"))
                        .unwrap();
                    if let Some(notes) = &slide.notes_image {
                        notes
                            .save(format!("{output}/{name}_{i}_notes.png"))
                            .unwrap();
                    }
                }
            }
        }
        "export" => {
            let deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
            let dir = std::env::temp_dir().join("presenter-perf-export");
            std::fs::create_dir_all(&dir).unwrap();
            PdfExporter::export_annotated_presentation(&deck, &dir.join("deck.pdf")).unwrap();
            println!(
                "{{\"mode\":\"export\",\"ms\":{:.3},\"peak_heap_bytes\":{}}}",
                start.elapsed().as_secs_f64() * 1000.,
                PEAK.load(Ordering::Relaxed)
            );
        }
        _ => panic!("unknown mode"),
    }
}
