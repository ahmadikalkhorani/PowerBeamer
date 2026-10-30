use image::RgbaImage;
use presenter::{
    ink::{Point, SlideInking, Stroke},
    pdf::SlideImage,
    work::{LatestFrame, LatestWorker},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

#[test]
fn lossless_pixels_survive_cache_eviction_and_shared_owners() {
    let mut originals = Vec::new();
    let mut assets = Vec::new();
    for n in 0..20u32 {
        let image = RgbaImage::from_fn(1100, 800, |x, y| {
            image::Rgba([(x + n) as u8, y as u8, (x ^ y) as u8, ((x + y) % 256) as u8])
        });
        originals.push(image.clone());
        assets.push(Arc::new(SlideImage::new(image)));
    }
    for _ in 0..2 {
        for (asset, original) in assets.iter().zip(&originals) {
            assert_eq!(asset.pixels().as_raw(), original.as_raw());
            assert!(SlideImage::cached_bytes() <= 64 * 1024 * 1024);
        }
    }
    let copy = assets[0].clone();
    drop(assets);
    assert_eq!(copy.pixels().as_raw(), originals[0].as_raw());
}

#[test]
fn undo_redo_preserves_order_and_points_across_mixed_edits() {
    let mut ink = SlideInking::new();
    for id in 0..12 {
        let mut s = Stroke::new(id, [id as u8, 20, 30, 255], 3., false);
        s.add_point(Point::new(id as f32 / 20., 0.5, 1., id));
        ink.add_stroke(s);
    }
    let original = serde_json::to_string(&ink.strokes).unwrap();
    assert!(ink.erase_at(0.25, 0.5, 0.06));
    let erased = serde_json::to_string(&ink.strokes).unwrap();
    ink.clear();
    assert!(ink.undo());
    assert_eq!(serde_json::to_string(&ink.strokes).unwrap(), erased);
    assert!(ink.undo());
    assert_eq!(serde_json::to_string(&ink.strokes).unwrap(), original);
    assert!(ink.redo());
    assert_eq!(serde_json::to_string(&ink.strokes).unwrap(), erased);
    assert!(ink.redo());
    assert!(ink.strokes.is_empty());
    assert!(ink.undo());
    ink.add_stroke(Stroke::new(100, [0; 4], 1., false));
    assert!(!ink.redo());
}

#[test]
fn latest_worker_bounds_backlog_and_cancels_superseded_work() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let worker = LatestWorker::new(move |id, n: usize, generation| {
        observed.fetch_add(1, Ordering::Relaxed);
        if n == 0 {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            assert_ne!(id, generation.load(Ordering::Acquire));
        }
    });
    worker.submit(0);
    started_rx.recv().unwrap();
    for i in 1..=1000 {
        worker.submit(i);
    }
    release_tx.send(()).unwrap();
    worker.flush();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn frame_mailbox_keeps_only_latest_and_one_notification() {
    let frames = LatestFrame::default();
    assert!(frames.publish(0));
    for i in 1..10000 {
        assert!(!frames.publish(i));
    }
    assert_eq!(frames.take(), Some(9999));
    assert_eq!(frames.take(), None);
    assert!(frames.publish(10000));
    assert_eq!(frames.take(), Some(10000));
}

#[test]
fn video_cadence_uses_one_clock_at_half_normal_and_double_speed() {
    use presenter::media::{InPlaceVideoPlayer, find_executable};
    use std::time::{Duration, Instant};
    if find_executable("ffmpeg").is_none() {
        return;
    }
    for speed in [0.5, 1.0, 2.0] {
        let (tx, rx) = mpsc::channel();
        let mut player = InPlaceVideoPlayer::new();
        assert!(player.play(
            "test_deck/sample_video.mp4",
            0.,
            speed,
            6.,
            true,
            move |frame, time, _| {
                tx.send((Instant::now(), time, frame.width(), frame.height()))
                    .ok();
            },
            || {}
        ));
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut last = first;
        for _ in 0..29 {
            last = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        player.pause();
        assert_eq!((last.2, last.3), (640, 360));
        assert!(
            (last.1 - speed).abs() < 0.001,
            "media time at {speed}x: {}",
            last.1
        );
        let wall = last.0.duration_since(first.0).as_secs_f32();
        // CI runners can experience scheduling delays and high CPU contention
        // while decoding and processing frames, so allow a reasonable wall-clock tolerance.
        assert!(
            (0.70..2.5).contains(&wall),
            "30 frames at {speed}x took {wall}s"
        );
    }
}

#[test]
fn cancellable_pdf_load_stops_before_rendering() {
    use presenter::pdf::{PdfiumLoader, RenderQuality};
    let error = match PdfiumLoader::load_deck_cancellable(
        std::path::Path::new("test_deck/presentation.pdf"),
        RenderQuality::UltraVector,
        &|| true,
    ) {
        Ok(_) => panic!("cancelled load completed"),
        Err(e) => e,
    };
    assert!(error.contains("cancelled"));
}

#[test]
fn coalescing_keeps_every_input_point_and_final_ink_pixels() {
    use presenter::ink::InkingRasterizer;
    let base = RgbaImage::new(1600, 900);
    for highlighter in [false, true] {
        let mut active = Stroke::new(1, [20, 70, 255, 255], 8., highlighter);
        let mut layer = base.clone();
        let mut committed = 0;
        let mut frame = base.clone();
        for i in 0..240 {
            active.add_point(Point::new(
                0.05 + i as f32 / 400.,
                0.5 + (i as f32 * 0.1).sin() * 0.05,
                1.,
                i,
            ));
            if i % 2 == 0 {
                continue;
            }
            let n = active.points.len();
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
        }
        let mut expected = base.clone();
        InkingRasterizer::draw_stroke(&mut expected, &active);
        assert_eq!(active.points.len(), 240);
        assert_eq!(frame.as_raw(), expected.as_raw());
    }
}

#[test]
fn worker_recovers_after_a_panicking_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let target = calls.clone();
    let worker = LatestWorker::new(move |_, panic: bool, _| {
        if panic {
            panic!("test task failure");
        }
        target.fetch_add(1, Ordering::Relaxed);
    });
    worker.submit(true);
    worker.flush();
    worker.submit(false);
    worker.flush();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn recycled_video_buffers_do_not_mutate_a_displayed_frame() {
    let mut buffers = presenter::media::video_player::VideoFrameBuffers::new(640, 360);
    let frame = buffers.next_mut();
    frame.make_mut_bytes().fill(47);
    let held = frame.clone();
    for i in 0..20 {
        buffers.next_mut().make_mut_bytes().fill(i);
    }
    assert!(held.as_bytes().iter().all(|&b| b == 47));
}

#[test]
fn optimized_thumbnail_preserves_exact_pixels_and_full_resolution_source() {
    let source = RgbaImage::from_fn(1200, 675, |x, y| {
        image::Rgba([x as u8, y as u8, (x ^ y) as u8, ((x + y) % 256) as u8])
    });
    let expected = image::imageops::thumbnail(&source, 320, 180);
    let asset = SlideImage::new(source.clone());
    assert_eq!(asset.thumbnail().as_raw(), expected.as_raw());
    assert_eq!(asset.pixels().as_raw(), source.as_raw());
    let resident = SlideImage::new_resident(source);
    assert_eq!(resident.thumbnail().as_raw(), expected.as_raw());
}

#[test]
fn page_patterns_remain_visible_when_high_resolution_pages_are_scaled_down() {
    use presenter::pdf::{PresentationDeck, RenderQuality};
    for width in [1600, 6400] {
        for bg in [[255, 255, 255, 255], [25, 25, 28, 255]] {
            let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
            let base = Arc::new(SlideImage::new(image::RgbaImage::from_pixel(
                width,
                width * 9 / 16,
                image::Rgba(bg),
            )));
            deck.slides[0].audience_image = base.clone();
            deck.slides[0].presenter_image = base;
            for template in ["dotted", "grid", "ruled", "blank"] {
                deck.current_index = 0;
                let index = deck.insert_page_with_template(0, template, bg);
                let pixels = deck.slides[index].audience_image.pixels();
                let preview = image::DynamicImage::ImageRgba8((*pixels).clone())
                    .resize_exact(800, 450, image::imageops::FilterType::Lanczos3)
                    .into_rgba8();
                let visible = preview
                    .pixels()
                    .filter(|p| (p[0] as i32 - bg[0] as i32).abs() >= 8)
                    .count();
                if template == "blank" {
                    assert_eq!(visible, 0);
                } else {
                    assert!(
                        visible > 1500,
                        "{template} at {width}: only {visible} visible pixels"
                    );
                }
                if template == "dotted" {
                    std::fs::create_dir_all("target/perf-results").unwrap();
                    preview
                        .save(format!("target/perf-results/dotted-{width}-{}.png", bg[0]))
                        .unwrap();
                }
            }
        }
    }
}

#[test]
fn precision_eraser_splits_sparse_strokes_and_undo_restores_exact_original() {
    use presenter::ink::EraserOptions;
    let mut ink = SlideInking::new();
    let mut stroke = Stroke::new(1, [10, 20, 30, 255], 2., false);
    stroke.add_point(Point::new(0.1, 0.5, 0.5, 10));
    stroke.add_point(Point::new(0.9, 0.5, 1.0, 100));
    ink.add_stroke(stroke);
    let original = serde_json::to_string(&ink.strokes).unwrap();
    assert!(ink.erase_at_with_options(
        0.5,
        0.5,
        &EraserOptions {
            radius: 0.05,
            ..Default::default()
        }
    ));
    assert_eq!(ink.strokes.len(), 2);
    assert!(ink.strokes[0].points.last().unwrap().x < 0.46);
    assert!(ink.strokes[1].points[0].x > 0.54);
    let split = serde_json::to_string(&ink.strokes).unwrap();
    assert!(ink.undo());
    assert_eq!(serde_json::to_string(&ink.strokes).unwrap(), original);
    assert!(ink.redo());
    assert_eq!(serde_json::to_string(&ink.strokes).unwrap(), split);
    assert!(ink.undo());
    assert!(ink.erase_at_with_options(
        0.5,
        0.5,
        &EraserOptions {
            radius: 0.05,
            erase_whole_line: true,
            ..Default::default()
        }
    ));
    assert!(ink.strokes.is_empty());
}
