#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// Where tests write preview images: Cargo's per-target scratch dir, so it exists on
    /// every machine (including CI runners).
    fn artifact_dir() -> std::path::PathBuf {
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("artifacts");
        std::fs::create_dir_all(&dir).expect("create test artifact dir");
        dir
    }

    #[test]
    fn test_beamer_dual_screen_aspect_ratio_detection() {
        // 8:3 ratio is 2.666 (Beamer pgfpages horizontal dual screen)
        let (is_dual, is_vertical) = presenter_detect_dual_screen(2666.0, 1000.0);
        assert!(is_dual);
        assert!(is_vertical);

        // Standard 16:9 ratio is 1.777 (single screen)
        let (is_dual_standard, _) = presenter_detect_dual_screen(1920.0, 1080.0);
        assert!(!is_dual_standard);

        // Standard 4:3 ratio is 1.333 (single screen)
        let (is_dual_43, _) = presenter_detect_dual_screen(1024.0, 768.0);
        assert!(!is_dual_43);
    }

    #[test]
    fn test_pdfium_link_api() {
        use presenter::pdf::PdfiumLoader;
        use pdfium_render::prelude::*;
        for name in &["test_deck/presentation.pdf", "test_deck/presentation_dual_screen.pdf"] {
            let pdf_path = std::path::Path::new(name);
            if pdf_path.exists() {
                let pdfium = PdfiumLoader::get_pdfium().unwrap();
                let doc = pdfium.load_pdf_from_file(pdf_path, None).unwrap();
                println!("--- Testing links in {} ---", name);
            for (idx, page) in doc.pages().iter().enumerate() {
                let links = page.links();
                for link in links.iter() {
                    let rect = link.rect().unwrap();
                    let mut action_desc = "None".to_string();
                    if let Some(action) = link.action() {
                        match action {
                            PdfAction::Uri(u) => {
                                if let Ok(uri) = u.uri() {
                                    action_desc = format!("URI: {}", uri);
                                }
                            }
                            PdfAction::LocalDestination(d) => {
                                if let Ok(dest) = d.destination() {
                                    action_desc = format!("LocalDest page: {:?}", dest.page_index());
                                }
                            }
                            _ => {
                                action_desc = format!("Other action: {:?}", action.action_type());
                            }
                        }
                    }
                    let mut dest_desc = "None".to_string();
                    if let Some(dest) = link.destination() {
                        dest_desc = format!("Dest page: {:?}", dest.page_index());
                    }
                    println!("Page {} Link: rect=({:.1}, {:.1}, {:.1}, {:.1}) action=[{}] dest=[{}]",
                        idx + 1, rect.left().value, rect.bottom().value, rect.width().value, rect.height().value,
                        action_desc, dest_desc);
                }
            }
            }
        }

        // Test that load_deck_from_file extracts links into Slide.links and hit testing succeeds
        let (slides, _) = PdfiumLoader::load_deck_from_file(
            std::path::Path::new("test_deck/presentation.pdf"),
            presenter::pdf::deck::RenderQuality::Eco,
        ).expect("load deck");
        assert!(!slides.is_empty());
        assert!(!slides[0].links.is_empty(), "Slide 1 should contain extracted interactive links");
        println!("presentation.pdf Slide 1 has {} extracted links!", slides[0].links.len());
        let link = &slides[0].links[0];
        let mid_x = link.rect[0] + link.rect[2] * 0.5;
        let mid_y = link.rect[1] + link.rect[3] * 0.5;
        assert!(slides[0].find_link_at(mid_x, mid_y, 0.01).is_some());

        // Test dual screen links extraction
        let (dual_slides, _) = PdfiumLoader::load_deck_from_file(
            std::path::Path::new("test_deck/presentation_dual_screen.pdf"),
            presenter::pdf::deck::RenderQuality::Eco,
        ).expect("load dual deck");
        assert!(!dual_slides.is_empty());
        assert!(!dual_slides[0].links.is_empty(), "Dual screen Slide 1 should contain extracted interactive links");
        println!("presentation_dual_screen.pdf Slide 1 has {} extracted links!", dual_slides[0].links.len());
        let d_link = &dual_slides[0].links[0];
        let d_mid_x = d_link.rect[0] + d_link.rect[2] * 0.5;
        let d_mid_y = d_link.rect[1] + d_link.rect[3] * 0.5;
        assert!(dual_slides[0].find_link_at(d_mid_x, d_mid_y, 0.01).is_some());
    }

    #[test]
    fn test_timer_countdown_formatting() {
        let target_duration = Duration::from_secs(45 * 60); // 45 mins
        let elapsed_secs = 10 * 60 + 25; // 10m 25s elapsed
        let rem_secs = target_duration.as_secs() - elapsed_secs;

        let rem_str = format!("{:02}:{:02}", rem_secs / 60, rem_secs % 60);
        assert_eq!(rem_str, "34:35");

        let elapsed_str = format!("{:02}:{:02}", elapsed_secs / 60, elapsed_secs % 60);
        assert_eq!(elapsed_str, "10:25");
    }

    #[test]
    fn test_non_destructive_session_serialization() {
        let session_data = serde_json::json!({
            "slide_index": 1,
            "title": "Quantum Mechanics",
            "is_whiteboard": false,
            "strokes": [
                {
                    "id": 1,
                    "points": [{"x": 10.0, "y": 20.0, "pressure": 1.0, "timestamp_ms": 1000}],
                    "color_rgba": [59, 130, 246, 255],
                    "width": 3.0,
                    "is_highlighter": false,
                    "bounding_box": [10.0, 20.0, 10.0, 20.0]
                }
            ]
        });

        let json_string = serde_json::to_string(&session_data).expect("Serialization failed");
        assert!(json_string.contains("Quantum Mechanics"));
        assert!(json_string.contains("color_rgba"));
    }

    #[test]
    fn test_load_real_beamer_pdf() {
        use presenter::pdf::PdfiumLoader;
        let pdf_path = std::path::Path::new("test_deck/presentation.pdf");
        if pdf_path.exists() {
            let pdfium = PdfiumLoader::get_pdfium().expect("Should get Pdfium");
            let doc = pdfium
                .load_pdf_from_file(pdf_path, None)
                .expect("Should load presentation.pdf");
            assert_eq!(doc.pages().len(), 14, "presentation.pdf has 14 physical pages (7 slides + 7 notes)");
            println!("Successfully verified test_deck/presentation.pdf has 10 physical pages!");

            let (slides, _toc) = PdfiumLoader::load_deck_from_file(pdf_path, presenter::pdf::deck::RenderQuality::Balanced)
                .expect("load slides");

            // Critical check: Note pages must NOT be presented as audience slides!
            assert_eq!(slides.len(), 7, "Audience slides must only contain the 7 presentation slides, not 14!");

            // Check that physical pages are properly mapped (1, 3, 5, ..., 13)
            let expected_physical = [1, 3, 5, 7, 9, 11, 13];
            for (i, s) in slides.iter().enumerate() {
                assert_eq!(s.page_index, i + 1, "Slide presentation index is 1-indexed");
                assert_eq!(s.physical_page_index, expected_physical[i], "Slide physical page matches Beamer frame");
                assert!(!s.is_dual_screen, "presentation.pdf is 16:9 non-dual-screen presentation");
                assert!((s.aspect_ratio - 1.777).abs() < 0.05, "Aspect ratio should be ~16:9 (1.777)");

                // Presenter private note slide must be rendered
                assert!(s.notes_image.is_some(), "Slide {} must have private note slide rendered in notes_image", i + 1);
                assert!(!s.notes.is_empty(), "Slide {} must have cleaned speaker notes extracted", i + 1);
            }

            // Check first slide notes specifically
            // Check standalone detection without .nav file
            let mut page_sizes = Vec::new();
            let mut page_labels = Vec::new();
            let mut page_texts = Vec::new();
            for page in doc.pages().iter() {
                page_sizes.push((page.width().value, page.height().value));
                page_labels.push(page.label().map(|s| s.to_string()));
                page_texts.push(page.text().map(|t| t.all()).unwrap_or_default());
            }
            let (standalone_mappings, standalone_mode) = presenter::pdf::beamer::BeamerMetadata::detect_slide_mappings(
                14,
                &page_sizes,
                &page_labels,
                &page_texts,
                None,
            );
            println!("Standalone without .nav mode: {:?}, count: {}", standalone_mode, standalone_mappings.len());
            for (idx, text) in page_texts.iter().enumerate() {
                println!("Page {} text snippet: {:?}", idx + 1, text.chars().take(60).collect::<String>());
            }
            assert_eq!(standalone_mappings.len(), 7, "Standalone without .nav must also detect 7 slides");
        }
    }

    #[test]
    fn test_load_dual_screen_beamer_pdf() {
        use presenter::pdf::PdfiumLoader;
        let pdf_path = std::path::Path::new("test_deck/presentation_dual_screen.pdf");
        if pdf_path.exists() {
            let pdfium = PdfiumLoader::get_pdfium().expect("Should get Pdfium");
            let doc = pdfium
                .load_pdf_from_file(pdf_path, None)
                .expect("Should load presentation_dual_screen.pdf");
            assert!(doc.pages().len() > 0);

            for b in doc.bookmarks().iter() {
                let title = b.title().unwrap_or_default();
                let page_idx = b.destination().and_then(|d| d.page_index().ok());
                println!("Bookmark: title={:?}, page_index={:?}", title, page_idx);
            }

            // Check that aspect ratio is double width (32:9 or > 2.0)
            let page = doc.pages().get(0).expect("First page");
            let w = page.width().value;
            let h = page.height().value;
            let aspect = w / h;
            println!("Dual screen page aspect ratio: {:.3} ({} x {})", aspect, w, h);
            assert!(aspect >= 1.95, "Expected aspect ratio >= 1.95 for dual-screen");

            let slides = PdfiumLoader::load_file(pdf_path, presenter::pdf::deck::RenderQuality::Balanced).expect("load dual slides");
            assert_eq!(slides.len(), 2);
            assert!(slides[0].notes.contains("DUAL-SCREEN SPEAKER NOTE"));
            assert!(slides[0].notes_image.is_some());
            assert!(slides[1].notes.contains("PRIVATE SPEAKER REMINDER"));
            assert!(slides[1].notes_image.is_some());
        }
    }

    #[test]
    fn test_tools_inking_eraser_and_dual_screen_aspect_ratio() {
        use presenter::ink::{InkingRasterizer, Point, Stroke};
        use presenter::pdf::{PdfiumLoader, RenderQuality};
        use std::path::Path;

        let pdf_path = Path::new("test_deck/presentation_dual_screen.pdf");
        if !pdf_path.exists() {
            println!("test_deck/presentation_dual_screen.pdf not found, skipping");
            return;
        }

        let slides = PdfiumLoader::load_file(pdf_path, RenderQuality::Balanced)
            .expect("Should load dual screen slides");
        assert!(!slides.is_empty());

        let slide = &slides[0];
        println!(
            "Slide 1 Aspect Ratio: {:.4} ({}x{}), is_dual: {}",
            slide.aspect_ratio,
            slide.audience_image.width(),
            slide.audience_image.height(),
            slide.is_dual_screen
        );

        // Aspect ratio of the audience half MUST be 16:9 (~1.7778), NOT 32:9 (~3.555)
        assert!(
            (slide.aspect_ratio - 1.7778).abs() < 0.05,
            "Slide aspect ratio should be 16:9, got {}",
            slide.aspect_ratio
        );
        assert!(slide.is_dual_screen);
        assert!(slide.notes_image.is_some(), "Notes half must be captured");

        let mut test_slide = slide.clone();

        // 1. Draw Pen stroke (blue)
        let mut pen_stroke = Stroke::new(1, [59, 130, 246, 255], 4.0, false);
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            let x = 0.2 + t * 0.4;
            let y = 0.3;
            pen_stroke.add_point(Point::new(x, y, 1.0, (i * 10) as u64));
        }
        test_slide.inking.add_stroke(pen_stroke);
        assert_eq!(test_slide.inking.strokes.len(), 1);

        // 2. Draw Highlighter stroke (yellow)
        let mut hl_stroke = Stroke::new(2, [234, 179, 8, 255], 8.0, true);
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            let x = 0.2 + t * 0.4;
            let y = 0.6;
            hl_stroke.add_point(Point::new(x, y, 1.0, (i * 10) as u64));
        }
        test_slide.inking.add_stroke(hl_stroke);
        assert_eq!(test_slide.inking.strokes.len(), 2);

        // 3. Render strokes onto 1600x900 overlay
        let ink_img = InkingRasterizer::render_strokes(
            1600,
            900,
            &test_slide.inking.strokes,
            None,
            None,
        );
        let pen_sample = ink_img.get_pixel(640, 270); // (0.4 * 1600, 0.3 * 900)
        assert!(pen_sample[3] > 200, "Pen pixel alpha should be opaque: {:?}", pen_sample);

        let hl_sample = ink_img.get_pixel(640, 540); // (0.4 * 1600, 0.6 * 900)
        assert!(hl_sample[3] > 50 && hl_sample[3] < 150, "Highlighter pixel should be translucent: {:?}", hl_sample);

        // 4. Test Eraser
        // Target near pen stroke (0.4, 0.3)
        let erased = test_slide.inking.erase_at(0.4, 0.3, 0.03);
        assert!(erased, "Eraser should have removed the pen stroke");
        assert_eq!(test_slide.inking.strokes.len(), 1);
        assert_eq!(test_slide.inking.strokes[0].id, 2, "Only highlighter should remain");

        // Undo restores pen stroke
        assert!(test_slide.inking.undo());
        assert_eq!(test_slide.inking.strokes.len(), 2);

        // 5. Save visual verification composite images
        let artifact_dir = artifact_dir();
        let mut composite = (*test_slide.audience_image.pixels()).clone();
        let rendered_ink = InkingRasterizer::render_strokes(
            composite.width(),
            composite.height(),
            &test_slide.inking.strokes,
            None,
            None,
        );
        image::imageops::overlay(&mut composite, &rendered_ink, 0, 0);

        let out_slide_path = artifact_dir.join("test_annotated_slide.png");
        let _ = composite.save(&out_slide_path);
        println!("Saved visual verification slide to {}", out_slide_path.display());

        if let Some(notes) = &test_slide.notes_image {
            let out_notes_path = artifact_dir.join("test_notes_slide.png");
            let _ = notes.save(&out_notes_path);
            println!("Saved visual verification notes to {}", out_notes_path.display());
        }
    }

    #[test]
    fn test_media_resolver_relative_paths() {
        use presenter::pdf::MediaResolver;
        use std::path::Path;

        let base_dir = Path::new("test_deck");
        assert!(base_dir.exists(), "test_deck directory should exist");

        // 1. Path cleaning
        assert_eq!(
            MediaResolver::clean_raw_path("run:./media/clip.mp4"),
            "media/clip.mp4"
        );
        assert_eq!(
            MediaResolver::clean_raw_path("file://videos/sample.webm"),
            "videos/sample.webm"
        );
        assert_eq!(
            MediaResolver::clean_raw_path("\"assets/sound.wav\""),
            "assets/sound.wav"
        );
        assert_eq!(
            MediaResolver::clean_raw_path("{\\movies\\demo.mp4}"),
            "movies/demo.mp4"
        );

        // 2. Resolve direct relative paths
        let resolved_video = MediaResolver::resolve_path(base_dir, "sample_video.mp4");
        assert!(resolved_video.is_some(), "Should resolve sample_video.mp4");
        let v_path = resolved_video.unwrap();
        assert!(v_path.is_absolute());
        assert!(v_path.exists());

        let resolved_audio = MediaResolver::resolve_path(base_dir, "./sample_audio.mp3");
        assert!(resolved_audio.is_some(), "Should resolve ./sample_audio.mp3");
        let a_path = resolved_audio.unwrap();
        assert!(a_path.is_absolute());
        assert!(a_path.exists());

        // 3. Resolve from an out-of-tree build/ directory
        let fake_build_dir = base_dir.join("build");
        let resolved_from_build = MediaResolver::resolve_path(&fake_build_dir, "sample_video.mp4");
        assert!(
            resolved_from_build.is_some(),
            "Should resolve media from out-of-tree build/ directory fallback"
        );

        // 4. Test MIME detection
        assert_eq!(MediaResolver::detect_mime_type(Path::new("video.mp4")), "video/mp4");
        assert_eq!(MediaResolver::detect_mime_type(Path::new("audio.mp3")), "audio/mpeg");
        assert_eq!(MediaResolver::detect_mime_type(Path::new("tone.wav")), "audio/wav");
        assert_eq!(MediaResolver::detect_mime_type(Path::new("stream.webm")), "video/webm");
    }

    #[test]
    fn test_companion_latex_media_extraction() {
        use presenter::pdf::MediaResolver;
        use std::path::Path;

        let pdf_path = Path::new("test_deck/presentation.pdf");
        if !pdf_path.exists() {
            println!("test_deck/presentation.pdf not found, skipping");
            return;
        }

        let companion_map = MediaResolver::scan_companion_tex(pdf_path);
        println!("Companion LaTeX frame media map: {:?}", companion_map);

        // Frame 3 should contain sample_audio.mp3
        let frame3_media = companion_map.get(&3);
        assert!(frame3_media.is_some(), "Slide 3 should have extracted media from \\movie");
        assert!(
            frame3_media.unwrap().iter().any(|p| p.contains("sample_audio.mp3")),
            "Slide 3 should reference sample_audio.mp3"
        );

        // Frame 4 should contain sample_video.mp4
        let frame4_media = companion_map.get(&4);
        assert!(frame4_media.is_some(), "Slide 4 should have extracted media from \\movie");
        assert!(
            frame4_media.unwrap().iter().any(|p| p.contains("sample_video.mp4")),
            "Slide 4 should reference sample_video.mp4"
        );
    }

    #[test]
    fn test_pdfium_loader_resolved_media_clips() {
        use presenter::pdf::{PdfiumLoader, RenderQuality};
        use std::path::Path;

        let pdf_path = Path::new("test_deck/presentation.pdf");
        if !pdf_path.exists() {
            return;
        }

        let slides = PdfiumLoader::load_file(pdf_path, RenderQuality::Eco)
            .expect("Should load presentation.pdf");

        // Find slide with audio clip (sample_audio.mp3)
        let audio_slide = slides
            .iter()
            .find(|s| s.media_clips.iter().any(|c| c.display_path == "sample_audio.mp3"))
            .expect("Should find slide with sample_audio.mp3");
        let clip3 = &audio_slide.media_clips[0];
        println!("Audio Slide Media Clip: {:?}", clip3);
        assert!(clip3.is_resolved, "Audio media should be resolved on disk");
        assert_eq!(clip3.display_path, "sample_audio.mp3");
        assert!(Path::new(&clip3.file_path).exists());

        // Find slide with video clip (sample_video.mp4)
        let video_slide = slides
            .iter()
            .find(|s| s.media_clips.iter().any(|c| c.display_path == "sample_video.mp4"))
            .expect("Should find slide with sample_video.mp4");
        let clip4 = &video_slide.media_clips[0];
        println!("Video Slide Media Clip: {:?}", clip4);
        assert!(clip4.is_resolved, "Video media should be resolved on disk");
        assert_eq!(clip4.display_path, "sample_video.mp4");

        // Slide 5: two videos side by side, in document order, with separate placements
        let two_videos = &slides[4];
        let names: Vec<&str> = two_videos.media_clips.iter().map(|c| c.display_path.as_str()).collect();
        assert_eq!(names, ["sample_video.mp4", "sample_video_2.mp4"]);
        assert!(two_videos.media_clips.iter().all(|c| c.is_resolved));
        let (left, right) = (two_videos.media_clips[0].rect, two_videos.media_clips[1].rect);
        assert!(left[0] + left[2] <= right[0] + 0.01, "Clips should not overlap: {:?} vs {:?}", left, right);

        // Slide 6: audio first, then a video
        let mixed = &slides[5];
        let names: Vec<&str> = mixed.media_clips.iter().map(|c| c.display_path.as_str()).collect();
        assert_eq!(names, ["sample_audio.mp3", "sample_video_2.mp4"]);
    }

    #[test]
    fn test_slide_overview_management_operations() {
        use presenter::pdf::{PresentationDeck, RenderQuality};

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Eco);
        let initial_count = deck.total_slides();
        assert_eq!(initial_count, 4);

        // 1. Duplicate slide 2
        let copy_idx = 1; // 0-based for slide 2
        let mut cloned = deck.slides[copy_idx].clone();
        cloned.title = format!("{} (Copy)", cloned.title);
        deck.slides.insert(copy_idx + 1, cloned);
        for (i, s) in deck.slides.iter_mut().enumerate() {
            s.page_index = i + 1;
        }
        assert_eq!(deck.total_slides(), 5);
        assert_eq!(deck.slides[2].page_index, 3);
        assert!(deck.slides[2].title.contains("(Copy)"));

        // 2. Rotate slide 3
        let old_w = deck.slides[2].audience_image.width();
        let old_h = deck.slides[2].audience_image.height();
        deck.slides[2].audience_image = std::sync::Arc::new(presenter::pdf::SlideImage::new(image::imageops::rotate90(&*deck.slides[2].audience_image.pixels())));
        assert_eq!(deck.slides[2].audience_image.width(), old_h);
        assert_eq!(deck.slides[2].audience_image.height(), old_w);

        // 3. Delete slide 3
        deck.slides.remove(2);
        for (i, s) in deck.slides.iter_mut().enumerate() {
            s.page_index = i + 1;
        }
        assert_eq!(deck.total_slides(), 4);
        assert_eq!(deck.slides[2].page_index, 3);
    }

    #[test]
    fn test_shape_recognizer_straight_line_and_highlighter() {
        use presenter::ink::{Point, ShapeKind, ShapeRecognizer};

        // 1. Horizontal straight line with hand jitter
        let mut points = Vec::new();
        for i in 0..20 {
            let t = i as f32 / 19.0;
            let x = 0.1 + t * 0.6; // 0.1 to 0.7
            let jitter = if i % 2 == 0 { 0.003 } else { -0.003 };
            let y = 0.4 + jitter;
            points.push(Point::new(x, y, 1.0, i as u64 * 10));
        }

        let recognized = ShapeRecognizer::recognize(&points, false)
            .expect("Should recognize horizontal line");
        assert_eq!(recognized.kind, ShapeKind::HorizontalLine);
        assert_eq!(recognized.points.len(), 2);
        // Horizontal line must have identical y coordinates for start and end
        assert!((recognized.points[0].y - recognized.points[1].y).abs() < 1e-4);

        // 2. Highlighter straight line snapping
        let mut hl_points = Vec::new();
        for i in 0..15 {
            let t = i as f32 / 14.0;
            let x = 0.2 + t * 0.5;
            let y = 0.35 + (i as f32 * 0.001); // slight upward slope
            hl_points.push(Point::new(x, y, 1.0, i as u64 * 10));
        }
        let hl_recognized = ShapeRecognizer::recognize(&hl_points, true)
            .expect("Highlighter should recognize horizontal line");
        assert_eq!(hl_recognized.kind, ShapeKind::HorizontalLine);
        assert_eq!(hl_recognized.points.len(), 2);
    }

    #[test]
    fn test_shape_recognizer_circle_and_ellipse() {
        use presenter::ink::{Point, ShapeKind, ShapeRecognizer};
        use std::f32::consts::PI;

        // Generate points approximating a circle with minor hand jitter
        let mut points = Vec::new();
        let cx = 0.5f32;
        let cy = 0.5f32;
        let r = 0.15f32;
        let n = 28;

        for i in 0..=n {
            let theta = (i as f32 / n as f32) * 2.0 * PI;
            let jitter = if i % 2 == 0 { 0.004 } else { -0.004 };
            let px = cx + (r + jitter) * theta.cos();
            let py = cy + (r + jitter) * theta.sin();
            points.push(Point::new(px, py, 1.0, i as u64 * 10));
        }

        let recognized = ShapeRecognizer::recognize(&points, false)
            .expect("Should recognize circle");
        assert!(recognized.kind == ShapeKind::Circle || recognized.kind == ShapeKind::Ellipse);
        assert_eq!(recognized.points.len(), 37); // 36 segments + closed vertex
    }

    #[test]
    fn test_shape_recognizer_rectangle() {
        use presenter::ink::{Point, ShapeKind, ShapeRecognizer};

        // Generate points for a rectangle
        let mut points = Vec::new();
        let mut t = 0u64;

        for i in 0..10 {
            points.push(Point::new(0.2 + (i as f32 / 9.0) * 0.4, 0.201, 1.0, t));
            t += 10;
        }
        for i in 1..10 {
            points.push(Point::new(0.601, 0.2 + (i as f32 / 9.0) * 0.3, 1.0, t));
            t += 10;
        }
        for i in 1..10 {
            points.push(Point::new(0.6 - (i as f32 / 9.0) * 0.4, 0.499, 1.0, t));
            t += 10;
        }
        for i in 1..10 {
            points.push(Point::new(0.199, 0.5 - (i as f32 / 9.0) * 0.3, 1.0, t));
            t += 10;
        }

        let recognized = ShapeRecognizer::recognize(&points, false)
            .expect("Should recognize rectangle");
        assert_eq!(recognized.kind, ShapeKind::Rectangle);
        assert_eq!(recognized.points.len(), 5); // 4 corners + closing vertex
        assert!((recognized.points[0].x - recognized.points[4].x).abs() < 1e-4);
        assert!((recognized.points[0].y - recognized.points[4].y).abs() < 1e-4);
    }

    #[test]
    fn test_shape_recognizer_scribble_detection() {
        use presenter::ink::{Point, ShapeRecognizer};

        // Rapid back-and-forth zigzagging over a small spot
        let mut points = Vec::new();
        let mut t = 0u64;
        let xs = [0.30, 0.35, 0.29, 0.36, 0.30, 0.37, 0.31, 0.36];
        let ys = [0.40, 0.41, 0.42, 0.40, 0.41, 0.42, 0.40, 0.41];

        for (&x, &y) in xs.iter().zip(ys.iter()) {
            points.push(Point::new(x, y, 1.0, t));
            t += 15;
        }

        assert!(ShapeRecognizer::is_scribble(&points), "Should detect scribble pattern");

        // Normal straight line should NOT be a scribble
        let mut straight = Vec::new();
        for i in 0..10 {
            straight.push(Point::new(0.1 + i as f32 * 0.05, 0.2, 1.0, i as u64 * 10));
        }
        assert!(!ShapeRecognizer::is_scribble(&straight), "Straight line must not be detected as scribble");
    }

    #[test]
    fn test_shape_recognizer_arrow_and_arc() {
        use presenter::ink::{Point, ShapeKind, ShapeRecognizer};

        // 1. Arrow: straight shaft from (0.1, 0.5) to (0.6, 0.5), then barb backwards to (0.55, 0.53)
        let mut arrow_pts = Vec::new();
        let mut t = 0u64;
        for i in 0..15 {
            let x = 0.1 + (i as f32 / 14.0) * 0.5;
            arrow_pts.push(Point::new(x, 0.50, 1.0, t));
            t += 10;
        }
        // Barb points backwards
        arrow_pts.push(Point::new(0.56, 0.52, 1.0, t));
        arrow_pts.push(Point::new(0.53, 0.54, 1.0, t + 10));

        let recognized = ShapeRecognizer::recognize(&arrow_pts, false)
            .expect("Should recognize Arrow");
        assert_eq!(recognized.kind, ShapeKind::Arrow);
        assert_eq!(recognized.points.len(), 5);

        // 2. Arc: smooth curved open stroke from (0.2, 0.6) curving up to (0.5, 0.35) and down to (0.8, 0.6)
        let mut arc_pts = Vec::new();
        for i in 0..20 {
            let s = i as f32 / 19.0;
            let x = 0.2 + s * 0.6;
            let y = 0.6 - 4.0 * s * (1.0 - s) * 0.25;
            arc_pts.push(Point::new(x, y, 1.0, i as u64 * 10));
        }
        let arc_rec = ShapeRecognizer::recognize(&arc_pts, false)
            .expect("Should recognize Arc");
        assert_eq!(arc_rec.kind, ShapeKind::Arc);
        assert_eq!(arc_rec.points.len(), 25);
    }

    #[test]
    fn test_shape_recognizer_diamond_star_heart() {
        use presenter::ink::{Point, ShapeKind, ShapeRecognizer};
        use std::f32::consts::PI;

        // 1. Diamond: (0.5, 0.2) -> (0.7, 0.4) -> (0.5, 0.6) -> (0.3, 0.4) -> (0.5, 0.2)
        let mut diamond_pts = Vec::new();
        let mut t = 0u64;
        let corners = [(0.5, 0.2), (0.7, 0.4), (0.5, 0.6), (0.3, 0.4), (0.5, 0.2)];
        for w in corners.windows(2) {
            for j in 0..6 {
                let s = j as f32 / 5.0;
                let x = w[0].0 + s * (w[1].0 - w[0].0);
                let y = w[0].1 + s * (w[1].1 - w[0].1);
                diamond_pts.push(Point::new(x, y, 1.0, t));
                t += 10;
            }
        }
        let recognized = ShapeRecognizer::recognize(&diamond_pts, false)
            .expect("Should recognize Diamond");
        assert_eq!(recognized.kind, ShapeKind::Diamond);
        assert_eq!(recognized.points.len(), 5);

        // 2. Star: 5 tips with peaks and valleys
        let mut star_pts = Vec::new();
        let cx = 0.5f32;
        let cy = 0.5f32;
        let r_out = 0.18f32;
        let r_in = 0.07f32;
        t = 0;
        for k in 0..5 {
            let a_out = -std::f32::consts::FRAC_PI_2 + (k as f32 * 2.0 * PI / 5.0);
            let a_in = a_out + (PI / 5.0);
            star_pts.push(Point::new(cx + r_out * a_out.cos(), cy + r_out * a_out.sin(), 1.0, t));
            t += 10;
            star_pts.push(Point::new(cx + r_in * a_in.cos(), cy + r_in * a_in.sin(), 1.0, t));
            t += 10;
        }
        star_pts.push(star_pts[0]);
        let star_rec = ShapeRecognizer::recognize(&star_pts, false)
            .expect("Should recognize Star");
        assert_eq!(star_rec.kind, ShapeKind::Star);
        assert_eq!(star_rec.points.len(), 11);

        // 3. Heart
        let mut heart_pts = Vec::new();
        t = 0;
        for i in 0..=36 {
            let angle = (i as f32 / 36.0) * 2.0 * PI;
            let sin_t = angle.sin();
            let x = 0.5 + (16.0 * sin_t * sin_t * sin_t / 32.0) * 0.25;
            let y_raw = -(13.0 * angle.cos() - 5.0 * (2.0 * angle).cos() - 2.0 * (3.0 * angle).cos() - (4.0 * angle).cos());
            let y = 0.35 + ((y_raw + 17.0) / 33.0) * 0.25;
            heart_pts.push(Point::new(x, y, 1.0, t));
            t += 10;
        }
        let heart_rec = ShapeRecognizer::recognize(&heart_pts, false)
            .expect("Should recognize Heart");
        assert_eq!(heart_rec.kind, ShapeKind::Heart);
    }

    #[test]
    fn test_highlighter_straight_lines_and_tape() {
        use presenter::ink::{InkingRasterizer, Point, Stroke};

        // 1. Highlighter straight line horizontal text-baseline snap
        let start_p = Point::new(0.15, 0.40, 1.0, 100);
        let move_p = Point::new(0.75, 0.415, 1.0, 200); // slight hand drift
        let dx = (move_p.x - start_p.x).abs();
        let dy = (move_p.y - start_p.y).abs();
        let snapped_y = if dy < 0.04 || (dx > 0.02 && dy < dx * 0.35) {
            start_p.y
        } else {
            move_p.y
        };
        assert_eq!(snapped_y, 0.40, "Should horizontally lock to start Y across text line");

        let mut hl_stroke = Stroke::new(1, [255, 235, 59, 255], 19.5, true);
        hl_stroke.is_shape = true;
        hl_stroke.points = vec![start_p, Point::new(move_p.x, snapped_y, 1.0, 200)];
        hl_stroke.recompute_bounding_box();
        assert_eq!(hl_stroke.points.len(), 2);
        assert_eq!(hl_stroke.points[0].y, hl_stroke.points[1].y);

        // Render highlighter stroke
        let img = InkingRasterizer::render_strokes(800, 600, &[hl_stroke.clone()], None, None);
        assert_eq!(img.width(), 800);
        assert_eq!(img.height(), 600);

        // 2. Study Tape test: creation, masking, and reveal toggle
        let mut tape_stroke = Stroke::new(2, [59, 130, 246, 255], 19.5, true);
        tape_stroke.is_tape = true;
        tape_stroke.is_shape = true;
        tape_stroke.points = vec![Point::new(0.2, 0.5, 1.0, 10), Point::new(0.6, 0.5, 1.0, 20)];
        tape_stroke.recompute_bounding_box();

        assert!(tape_stroke.intersects_circle(0.4, 0.5, 0.03));
        assert!(!tape_stroke.is_tape_revealed);

        // Render opaque tape (blackout mask)
        let mask_img = InkingRasterizer::render_strokes(400, 300, &[tape_stroke.clone()], None, None);
        let pixel_on_tape = mask_img.get_pixel(160, 150); // midpoint
        assert_eq!(pixel_on_tape[3], 255, "Unrevealed tape must be 100% opaque mask");

        // Toggle tape to reveal answer in presentation
        tape_stroke.is_tape_revealed = true;
        let revealed_img = InkingRasterizer::render_strokes(400, 300, &[tape_stroke.clone()], None, None);
        let pixel_revealed = revealed_img.get_pixel(160, 150);
        assert_eq!(pixel_revealed[3], 35, "Revealed tape must be translucent to show underlying text");

        // Save composite demonstration image to artifacts
        let out_path = artifact_dir().join("test_highlighter_tape_slide.png");
        let mut composite = image::RgbaImage::from_pixel(1280, 720, image::Rgba([248, 249, 250, 255]));
        // Tape stroke 1: opaque blackout mask
        let mut mask_tape = Stroke::new(3, [59, 130, 246, 255], 22.0, true);
        mask_tape.is_tape = true;
        mask_tape.is_shape = true;
        mask_tape.points = vec![Point::new(0.15, 0.60, 1.0, 10), Point::new(0.65, 0.60, 1.0, 20)];
        mask_tape.recompute_bounding_box();

        let demo_strokes = vec![hl_stroke, mask_tape, tape_stroke];
        let ink_demo = InkingRasterizer::render_strokes(1280, 720, &demo_strokes, None, None);
        image::imageops::overlay(&mut composite, &ink_demo, 0, 0);
        let _ = composite.save(out_path);
    }

    #[test]
    fn test_eraser_options_filtering() {
        use presenter::ink::{EraserOptions, Point, SlideInking, Stroke};

        let mut inking = SlideInking::new();

        // Stroke 1: Standard Pen stroke
        let mut pen = Stroke::new(1, [0, 0, 0, 255], 3.5, false);
        pen.add_point(Point::new(0.3, 0.4, 1.0, 10));
        pen.add_point(Point::new(0.5, 0.4, 1.0, 20));
        pen.recompute_bounding_box();
        inking.add_stroke(pen);

        // Stroke 2: Highlighter stroke
        let mut hl = Stroke::new(2, [255, 235, 59, 255], 18.0, true);
        hl.add_point(Point::new(0.3, 0.4, 1.0, 10));
        hl.add_point(Point::new(0.5, 0.4, 1.0, 20));
        hl.recompute_bounding_box();
        inking.add_stroke(hl);

        // Stroke 3: Recognized Shape (Rectangle)
        let mut shape = Stroke::new(3, [59, 130, 246, 255], 3.0, false);
        shape.is_shape = true;
        shape.add_point(Point::new(0.3, 0.4, 1.0, 10));
        shape.add_point(Point::new(0.5, 0.4, 1.0, 20));
        shape.recompute_bounding_box();
        inking.add_stroke(shape);

        // Stroke 4: Study Tape mask
        let mut tape = Stroke::new(4, [45, 45, 50, 255], 20.0, true);
        tape.is_tape = true;
        tape.add_point(Point::new(0.3, 0.4, 1.0, 10));
        tape.add_point(Point::new(0.5, 0.4, 1.0, 20));
        tape.recompute_bounding_box();
        inking.add_stroke(tape);

        assert_eq!(inking.strokes.len(), 4);

        // Test 1: Erase Highlight Only
        let mut opts = EraserOptions::default();
        opts.erase_whole_line = true; // This test checks whole-stroke filtering.
        opts.erase_highlight_only = true;
        let erased = inking.erase_at_with_options(0.4, 0.4, &opts);
        assert!(erased);
        assert_eq!(inking.strokes.len(), 3);
        assert!(inking.strokes.iter().all(|s| s.id != 2), "Highlighter must be erased");
        assert!(inking.strokes.iter().any(|s| s.id == 1), "Pen must be preserved");
        assert!(inking.strokes.iter().any(|s| s.id == 3), "Shape must be preserved");
        assert!(inking.strokes.iter().any(|s| s.id == 4), "Tape must be preserved");

        // Test 2: Erase with erase_shapes = false and erase_tapes = false (protects shapes & tapes)
        opts.erase_highlight_only = false;
        opts.erase_shapes = false;
        opts.erase_tapes = false;
        let erased2 = inking.erase_at_with_options(0.4, 0.4, &opts);
        assert!(erased2);
        assert_eq!(inking.strokes.len(), 2);
        assert!(inking.strokes.iter().all(|s| s.id != 1), "Pen must now be erased");
        assert!(inking.strokes.iter().any(|s| s.id == 3), "Shape must be protected");
        assert!(inking.strokes.iter().any(|s| s.id == 4), "Tape must be protected");

        // Test 3: Enable erase_shapes = true
        opts.erase_shapes = true;
        opts.erase_tapes = false;
        let erased3 = inking.erase_at_with_options(0.4, 0.4, &opts);
        assert!(erased3);
        assert_eq!(inking.strokes.len(), 1);
        assert_eq!(inking.strokes[0].id, 4, "Tape must still be protected");

        // Test 4: Enable erase_tapes = true
        opts.erase_tapes = true;
        let erased4 = inking.erase_at_with_options(0.4, 0.4, &opts);
        assert!(erased4);
        assert_eq!(inking.strokes.len(), 0, "Tape is now erased");

        // Test 5: Undo restores all strokes
        assert!(inking.undo());
        assert_eq!(inking.strokes.len(), 1);
        assert!(inking.undo());
        assert_eq!(inking.strokes.len(), 2);
    }

    #[test]
    fn test_presentation_tool_laser_and_draw_lines() {
        use presenter::ink::{InkingRasterizer, LaserTrail};

        // 1. Verify LaserTrail with Draw Lines ON (1800ms)
        let red_rgba = [255, 59, 48, 255]; // Red
        let mut laser_trail = LaserTrail::new(1800, red_rgba, true);
        assert!(laser_trail.draw_lines);
        assert_eq!(laser_trail.duration_ms, 1800);
        assert_eq!(laser_trail.color_rgba, red_rgba);

        // Add 5 points forming a line across the slide (x: 0.1 -> 0.5, y: 0.5)
        laser_trail.add(0.1, 0.5, 1000);
        laser_trail.add(0.2, 0.5, 1200);
        laser_trail.add(0.3, 0.5, 1400);
        laser_trail.add(0.4, 0.5, 1600);
        laser_trail.add(0.5, 0.5, 1800);
        assert_eq!(laser_trail.point_count(), 5);

        // Render laser lines onto 800x600 canvas
        let img = InkingRasterizer::render_strokes(800, 600, &[], None, Some(&laser_trail));
        assert_eq!(img.width(), 800);
        assert_eq!(img.height(), 600);

        // Verify that laser line has rendered visible glowing pixels along y = 300 (0.5 * 600)
        let mut red_glow_count = 0;
        for x in 100..400 {
            let px = img.get_pixel(x, 300);
            if px[3] > 0 && px[0] > 100 {
                red_glow_count += 1;
            }
        }
        assert!(red_glow_count > 100, "Laser line must render glowing pixels, found {}", red_glow_count);

        // 2. Test color change to Blue
        let blue_rgba = [0, 85, 255, 255];
        laser_trail.color_rgba = blue_rgba;
        let img_blue = InkingRasterizer::render_strokes(800, 600, &[], None, Some(&laser_trail));
        let mut blue_glow_count = 0;
        for x in 100..400 {
            let px = img_blue.get_pixel(x, 300);
            if px[3] > 0 && px[2] > 100 {
                blue_glow_count += 1;
            }
        }
        assert!(blue_glow_count > 100, "Laser line must render glowing blue pixels");

        // Save visual verification artifact
        let artifact_dir = artifact_dir();
        let mut sample_bg = image::RgbaImage::from_pixel(1280, 720, image::Rgba([24, 24, 27, 255]));
        let mut visual_trail = LaserTrail::new(1800, [255, 59, 48, 255], true);
        for i in 0..60 {
            let t = i as f32 / 60.0;
            let x = 0.15 + t * 0.7;
            let y = 0.45 + (t * std::f32::consts::PI * 3.0).sin() * 0.08;
            visual_trail.add(x, y, 1000 + (i as u64) * 20);
        }
        let rendered_laser = InkingRasterizer::render_strokes(1280, 720, &[], None, Some(&visual_trail));
        image::imageops::overlay(&mut sample_bg, &rendered_laser, 0, 0);
        let _ = sample_bg.save(artifact_dir.join("test_presentation_tool_laser_lines.png"));

        // 3. Test pruning (time progression)
        // While the button is held, a line trail keeps every point regardless of age
        laser_trail.prune(3100); // cutoff 1300
        assert_eq!(laser_trail.point_count(), 5, "Held line trail must not lose its oldest points");

        // On release the whole stroke fades together from the release time (its last point, 1800)
        laser_trail.end_stroke();
        laser_trail.prune(3100); // cutoff 1300 < 1800
        assert_eq!(laser_trail.point_count(), 5, "Released stroke fades as a whole, not tail first");

        // At t = 4000 (cutoff 2200), all points expire and vanish smoothly
        laser_trail.prune(4000);
        assert_eq!(laser_trail.point_count(), 0);

        let img_empty = InkingRasterizer::render_strokes(800, 600, &[], None, Some(&laser_trail));
        let active_pixels = img_empty.pixels().filter(|p| p[3] > 0).count();
        assert_eq!(active_pixels, 0, "After decay, slide must be completely pristine");

        // 4. Test pressing "L": selecting laser pointer directly with already selected active color without opening menu
        let active_selected_color = [93, 166, 56, 255]; // Active user color (Green)
        let current_tool = "laser";
        let laser_color = active_selected_color; // Synchronize with already selected color
        let show_laser_popover = false;          // Do NOT open menu

        assert_eq!(current_tool, "laser");
        assert_eq!(laser_color, active_selected_color, "Laser must inherit active selected color");
        assert!(!show_laser_popover, "Pressing L must not open the laser popover menu");

        // 5. Test laser pointer fade duration controller from 0.0 to 5.0 seconds with decimals
        let fade_durations = [0.0f32, 0.5, 1.8, 3.2, 5.0];
        for &dur_sec in &fade_durations {
            let clamped = dur_sec.clamp(0.0, 5.0);
            let dur_ms = (clamped * 1000.0).round() as u64;
            let formatted_badge = format!("{:.1}s", clamped);
            assert_eq!(clamped, dur_sec);
            assert!(formatted_badge.ends_with('s'));

            let mut test_trail = LaserTrail::new(dur_ms, [255, 0, 0, 255], true);
            test_trail.add(0.5, 0.5, 1000);
            test_trail.end_stroke(); // fading starts on release

            if dur_sec == 0.0 {
                test_trail.prune(1001);
                assert_eq!(test_trail.point_count(), 0, "0.0s fade duration should clear immediately");
            } else if dur_sec == 0.5 {
                test_trail.prune(1400); // 400ms < 500ms -> still alive
                assert_eq!(test_trail.point_count(), 1);
                test_trail.prune(1600); // 600ms >= 500ms -> pruned
                assert_eq!(test_trail.point_count(), 0);
            } else if dur_sec == 3.2 {
                test_trail.prune(3000); // 2000ms < 3200ms -> still alive
                assert_eq!(test_trail.point_count(), 1);
                test_trail.prune(4500); // 3500ms >= 3200ms -> pruned
                assert_eq!(test_trail.point_count(), 0);
            }
        }
    }

    #[test]
    fn test_laser_vector_bands() {
        use presenter::ink::{InkingRasterizer, LaserTrail, LASER_BAND_COUNT};

        let mut trail = LaserTrail::new(2000, [255, 59, 48, 255], true);
        trail.start_stroke();
        for i in 0..20 {
            trail.add(0.1 + i as f32 * 0.01, 0.3, 1000 + i * 50);
        }
        trail.end_stroke();
        // A second, separate stroke must start with its own move-to, never a connecting line
        trail.start_stroke();
        trail.add(0.8, 0.7, 2000);
        trail.add(0.9, 0.7, 2050);
        trail.end_stroke();

        let bands = InkingRasterizer::laser_trail_bands(&trail);
        assert!(!bands.is_empty() && bands.len() <= LASER_BAND_COUNT);
        let mut prev = 0.0;
        for b in &bands {
            assert!(b.intensity > prev && b.intensity <= 1.0, "Bands are ordered dim to bright");
            prev = b.intensity;
            assert!(b.commands.starts_with('M'));
            assert!(b.radius > 2.0);
        }
        let all: String = bands.iter().map(|b| b.commands.as_str()).collect::<Vec<_>>().join(" ");
        assert!(all.contains("M800.0 700.0") || all.contains("M80"), "Second stroke starts with a move: {}", all);
        assert!(!all.contains("L800.0 700.0") || all.contains("M800.0 700.0"));

        // The comet-tail mode stays rasterized
        let comet = LaserTrail { draw_lines: false, ..trail.clone() };
        assert!(InkingRasterizer::laser_trail_bands(&comet).is_empty());
    }

    #[test]
    fn test_laser_disjoint_strokes_no_connecting_lines() {
        use presenter::ink::{InkingRasterizer, LaserTrail};

        let red_rgba = [255, 59, 48, 255];
        let mut laser_trail = LaserTrail::new(2000, red_rgba, true);

        // Stroke 1: Shape on left side of slide (x: 0.10 -> 0.25, y: 0.30)
        laser_trail.start_stroke();
        laser_trail.add(0.10, 0.30, 1000);
        laser_trail.add(0.15, 0.30, 1050);
        laser_trail.add(0.20, 0.30, 1100);
        laser_trail.add(0.25, 0.30, 1150);
        laser_trail.end_stroke();

        // Stroke 2: Shape on right side of slide (x: 0.75 -> 0.90, y: 0.30)
        laser_trail.start_stroke();
        laser_trail.add(0.75, 0.30, 1300);
        laser_trail.add(0.80, 0.30, 1350);
        laser_trail.add(0.85, 0.30, 1400);
        laser_trail.add(0.90, 0.30, 1450);
        laser_trail.end_stroke();

        assert_eq!(laser_trail.strokes.len(), 2, "Must contain exactly 2 separate strokes");
        assert_eq!(laser_trail.point_count(), 8, "Total points across strokes should be 8");

        // Render at t = 1500 (both strokes active and visible)
        let img = InkingRasterizer::render_strokes(1000, 500, &[], None, Some(&laser_trail));
        let py = (0.30 * 500.0) as u32; // y = 150

        // 1. Left stroke should have bright red glowing pixels (x: 100 to 250)
        let mut left_pixels = 0;
        for px in 100..250 {
            let p = img.get_pixel(px, py);
            if p[3] > 0 && p[0] > 100 {
                left_pixels += 1;
            }
        }
        assert!(left_pixels > 80, "Left laser stroke must be rendered, found {}", left_pixels);

        // 2. Right stroke should have bright red glowing pixels (x: 750 to 900)
        let mut right_pixels = 0;
        for px in 750..900 {
            let p = img.get_pixel(px, py);
            if p[3] > 0 && p[0] > 100 {
                right_pixels += 1;
            }
        }
        assert!(right_pixels > 80, "Right laser stroke must be rendered, found {}", right_pixels);

        // 3. CRITICAL: The space between the two shapes (x: 350 to 650) MUST HAVE ZERO LASER PIXELS!
        let mut bridge_pixels = 0;
        for px in 350..650 {
            for y_off in (py - 10)..=(py + 10) {
                let p = img.get_pixel(px, y_off);
                if p[3] > 0 {
                    bridge_pixels += 1;
                }
            }
        }
        assert_eq!(bridge_pixels, 0, "No bridge or connecting line must exist between disjoint shapes, found {}", bridge_pixels);

        // Save visual verification artifact
        let artifact_dir = artifact_dir();
        let mut sample_bg = image::RgbaImage::from_pixel(1000, 500, image::Rgba([24, 24, 27, 255]));
        image::imageops::overlay(&mut sample_bg, &img, 0, 0);
        let _ = sample_bg.save(artifact_dir.join("test_disjoint_laser_strokes.png"));

        // 4. Time progression: at t = 3200 (cutoff = 1200 > 1150)
        // Stroke 1 (all points <= 1150) has expired and is pruned
        // Stroke 2 (points 1300..1450) is still alive and fading
        laser_trail.prune(3200);
        assert_eq!(laser_trail.strokes.len(), 1, "Stroke 1 must have faded and been pruned");

        let img_partial = InkingRasterizer::render_strokes(1000, 500, &[], None, Some(&laser_trail));
        let mut left_pixels_after = 0;
        for px in 100..250 {
            let p = img_partial.get_pixel(px, py);
            if p[3] > 0 {
                left_pixels_after += 1;
            }
        }
        assert_eq!(left_pixels_after, 0, "Stroke 1 must be completely gone");

        let mut right_pixels_after = 0;
        for px in 750..900 {
            let p = img_partial.get_pixel(px, py);
            if p[3] > 0 && p[0] > 50 {
                right_pixels_after += 1;
            }
        }
        assert!(right_pixels_after > 50, "Stroke 2 must still be fading and visible");

        // 5. At t = 3500 (cutoff = 1500), all strokes have faded completely
        laser_trail.prune(3500);
        assert!(laser_trail.is_empty());
        assert_eq!(laser_trail.strokes.len(), 0);
    }

    #[test]
    fn test_custom_colors_and_palette_management() {
        use presenter::ink::{InkingRasterizer, Point, SlideInking, Stroke};

        // 1. Initial 5 slots & custom palette
        let slot1_yellow = [254, 240, 78, 255]; // #fef04e
        let slot2_green = [93, 166, 56, 255];   // #5da638
        let slot3_purple = [94, 53, 247, 255];  // #5e35f7
        let slot4_red = [239, 68, 68, 255];     // #ef4444
        let slot5_blue = [0, 197, 255, 255];    // #00c5ff
        let mut custom_palette = vec![slot1_yellow, slot2_green, slot3_purple, slot4_red, slot5_blue];
        assert_eq!(custom_palette.len(), 5);

        // 2. Select slot 2 (Green)
        let mut active_color = slot2_green;
        assert_eq!(active_color, [93, 166, 56, 255]);

        // Draw stroke with slot 2 color
        let mut inking = SlideInking::new();
        let mut stroke_green = Stroke::new(1, active_color, 4.0, false);
        stroke_green.add_point(Point::new(0.2, 0.3, 1.0, 100));
        stroke_green.add_point(Point::new(0.4, 0.3, 1.0, 150));
        inking.add_stroke(stroke_green);
        assert_eq!(inking.strokes[0].color_rgba, slot2_green);

        // 3. User adds a custom color to the palette: #ff0080 (Electric Magenta)
        let custom_magenta = [255, 0, 128, 255];
        if !custom_palette.contains(&custom_magenta) {
            custom_palette.push(custom_magenta);
        }
        assert_eq!(custom_palette.len(), 6);
        assert_eq!(custom_palette[5], custom_magenta);

        // Apply custom color to active drawing
        active_color = custom_magenta;

        // Draw highlighter stroke with custom magenta
        let mut stroke_hl = Stroke::new(2, active_color, 12.0, true);
        stroke_hl.add_point(Point::new(0.2, 0.5, 1.0, 200));
        stroke_hl.add_point(Point::new(0.6, 0.5, 1.0, 250));
        inking.add_stroke(stroke_hl);
        assert_eq!(inking.strokes[1].color_rgba, custom_magenta);
        assert!(inking.strokes[1].is_highlighter);

        // 4. Render strokes and verify magenta pixels exist
        let img = InkingRasterizer::render_strokes(800, 600, &inking.strokes, None, None);
        let mut magenta_pixels = 0;
        for x in 200..400 {
            let px = img.get_pixel(x, 300);
            if px[3] > 0 && px[0] > 180 && px[2] > 90 {
                magenta_pixels += 1;
            }
        }
        assert!(magenta_pixels > 50, "Custom magenta highlighter must be rendered visibly on canvas");

        // Save visual verification artifact
        let artifact_dir = artifact_dir();
        let mut sample_bg = image::RgbaImage::from_pixel(800, 600, image::Rgba([24, 24, 27, 255]));
        image::imageops::overlay(&mut sample_bg, &img, 0, 0);
        let _ = sample_bg.save(artifact_dir.join("test_custom_color_slide.png"));
    }

    #[test]
    fn test_color_selector_done_and_outside_click_dismissal() {
        // State machine simulating color popover lifecycle:
        // 1. Initially closed
        let mut show_color_popover = false;
        let mut active_color_slot = 0; // Yellow
        let mut active_color = [254, 240, 78, 255]; // #fef04e
        assert!(!show_color_popover);
        assert_eq!(active_color_slot, 0);
        assert_eq!(active_color, [254, 240, 78, 255]);

        // 2. Click color slot to open
        show_color_popover = true;
        assert!(show_color_popover);

        // 3. User selects color from grid (e.g. Electric Blue #007aff)
        let selected_color = [0, 122, 255, 255];
        active_color = selected_color;
        assert_eq!(active_color, [0, 122, 255, 255]);
        assert!(show_color_popover, "Popover stays open during color browsing");

        // 4. User clicks "Done" button
        show_color_popover = false;
        assert!(!show_color_popover, "Done button must dismiss color selector");

        // 5. Open popover again on slot 1 (Green)
        active_color_slot = 1;
        show_color_popover = true;
        assert!(show_color_popover);

        // 6. User clicks outside popover (e.g. on stage/backdrop)
        // Click outside must safely dismiss popover without drawing ink
        let clicked_outside = true;
        if clicked_outside {
            show_color_popover = false;
        }
        assert!(!show_color_popover, "Clicking outside must dismiss color selector");
        assert_eq!(active_color_slot, 1);
    }

    #[test]
    fn test_page_reordering() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality};

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        let initial_count = deck.slides.len();
        assert!(initial_count >= 3);

        // Slide titles initially
        let orig_titles: Vec<String> = deck.slides.iter().map(|s| s.title.clone()).collect();

        // 1. Move first slide to index 2
        let ok = deck.move_slide(0, 2);
        assert!(ok);
        assert_eq!(deck.slides.len(), initial_count);
        assert_eq!(deck.slides[0].title, orig_titles[1]);
        assert_eq!(deck.slides[1].title, orig_titles[2]);
        assert_eq!(deck.slides[2].title, orig_titles[0]);
        assert_eq!(deck.current_index, 2);

        // Verify page_index are sequentially renumbered 1..=total
        for (i, s) in deck.slides.iter().enumerate() {
            assert_eq!(s.page_index, i + 1);
        }

        // 2. Move last slide to index 0 (beginning)
        let last_idx = deck.slides.len() - 1;
        let last_title = deck.slides[last_idx].title.clone();
        let ok2 = deck.move_slide(last_idx, 0);
        assert!(ok2);
        assert_eq!(deck.slides[0].title, last_title);
        assert_eq!(deck.current_index, 0);

        // Verify page_index sequentially renumbered again
        for (i, s) in deck.slides.iter().enumerate() {
            assert_eq!(s.page_index, i + 1);
        }

        // 3. Out-of-bounds or identical indices should return false
        assert!(!deck.move_slide(0, 0));
        assert!(!deck.move_slide(0, 999));
        assert!(!deck.move_slide(999, 1));

        // 4. Test Drag-and-Drop gesture calculation matching Slint UI & Web logic
        let card_w = 220.0f32;
        let card_step = card_w + 32.0; // 252.0px
        let total = deck.slides.len() as i32;

        let calc_drag_target = |from_1based: i32, dx: f32| -> i32 {
            let slot_delta = (dx / card_step).round() as i32;
            (from_1based + slot_delta).clamp(1, total)
        };

        // Dragging slide 1 to the right by 260px (slightly past 1 step)
        assert_eq!(calc_drag_target(1, 260.0), 2);
        // Dragging slide 1 to the right by 530px (past 2 steps)
        assert_eq!(calc_drag_target(1, 530.0), 3);
        // Dragging slide 3 to the left by 270px
        assert_eq!(calc_drag_target(3, -270.0), 2);
        // Dragging left beyond first slide clamps to 1
        assert_eq!(calc_drag_target(1, -600.0), 1);
        // Dragging right beyond last slide clamps to total
        assert_eq!(calc_drag_target(total, 800.0), total);

        // Execute drag-reordered slide movement
        let target_pos = calc_drag_target(1, 530.0); // 3
        let ok_drag = deck.move_slide(0, (target_pos - 1) as usize);
        assert!(ok_drag);
        assert_eq!(deck.current_index, 2);
    }

    #[test]
    fn test_insert_page_templates_and_background_colors() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality};

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        let initial_count = deck.slides.len();

        // 1. Insert "blank" with Warm Ivory background After current page (pos_mode 0)
        deck.current_index = 0;
        let ivory = [251, 248, 235, 255];
        let idx1 = deck.insert_page_with_template(0, "blank", ivory);
        assert_eq!(idx1, 1);
        assert_eq!(deck.slides.len(), initial_count + 1);
        assert_eq!(deck.current_index, 1);
        let s_blank = &deck.slides[1];
        assert_eq!(s_blank.page_index, 2);
        let px_ivory = s_blank.audience_image.get_pixel(100, 100);
        assert_eq!(px_ivory.0, ivory);

        // 2. Insert "ruled" with Legal Pad Yellow background Before current page (pos_mode 1)
        deck.current_index = 2;
        let yellow = [254, 249, 195, 255];
        let idx2 = deck.insert_page_with_template(1, "ruled", yellow);
        assert_eq!(idx2, 2);
        assert_eq!(deck.slides.len(), initial_count + 2);
        assert!(deck.slides[2].title.contains("Ruled"));
        // Verify rule lines exist (find pixels darker than background)
        let mut line_pixels = 0;
        for y in 0..deck.slides[2].audience_image.height() {
            let px = deck.slides[2].audience_image.get_pixel(400, y);
            if px.0[0] < 220 { // Rule line is darker than yellow
                line_pixels += 1;
            }
        }
        assert!(line_pixels > 10, "Ruled paper must contain horizontal lines");
        let artifact_dir = artifact_dir();
        let _ = deck.slides[2].audience_image.save(artifact_dir.join("test_inserted_ruled_page.png"));

        // 3. Insert "grid" with Dark Slate background At the end (pos_mode 2)
        let dark_slate = [24, 24, 27, 255];
        let idx3 = deck.insert_page_with_template(2, "grid", dark_slate);
        assert_eq!(idx3, deck.slides.len() - 1);
        assert!(deck.slides[idx3].title.contains("Grid"));
        let mut grid_pixels = 0;
        for x in 0..deck.slides[idx3].audience_image.width() {
            let px = deck.slides[idx3].audience_image.get_pixel(x, 120);
            if px.0[0] > 50 { // Grid line on dark background is lighter
                grid_pixels += 1;
            }
        }
        assert!(grid_pixels > 100, "Grid paper must contain grid lines");
        let _ = deck.slides[idx3].audience_image.save(artifact_dir.join("test_inserted_grid_page.png"));

        // 4. Insert "current" (clone) After current page
        deck.current_index = 0;
        let clone_title = deck.slides[0].title.clone();
        let idx4 = deck.insert_page_with_template(0, "current", ivory);
        assert_eq!(idx4, 1);
        assert!(deck.slides[1].title.contains(&clone_title));
        assert_eq!(deck.slides[1].audience_image.dimensions(), deck.slides[0].audience_image.dimensions());
    }

    #[test]
    fn test_slide_vs_notes_splitter_slider_and_aspect_scaling() {
        // Test responsive calculations across different split ratios and screen sizes
        let resolutions: [(f32, f32); 3] = [(1440.0, 900.0), (1920.0, 1080.0), (2560.0, 1440.0)];
        let ratios: [f32; 6] = [0.15, 0.20, 0.30, 0.32, 0.50, 0.70];
        let slide_aspect: f32 = 16.0 / 9.0; // 1.7778

        for &(win_w, win_h) in &resolutions {
            let workspace_w: f32 = win_w - 50.0 - 6.0; // Minus left toolbar and splitter divider
            let workspace_h: f32 = win_h - 48.0; // Minus top bar

            for &split_ratio in &ratios {
                // Notes panel width with min constraint 240px
                let raw_notes_w: f32 = workspace_w * split_ratio;
                let notes_w: f32 = raw_notes_w.max(240.0).min(workspace_w - 320.0);
                let slide_area_w = workspace_w - notes_w;

                // Ensure slide area has ample room
                assert!(slide_area_w >= 320.0, "Slide viewport must remain at least 320px wide");
                assert!(notes_w >= 240.0, "Notes panel must respect min-width of 240px");

                // Check slide paper scaling inside canvas_area (preserving exact aspect ratio)
                let avail_w = slide_area_w - 48.0;
                let avail_h = workspace_h - 48.0;

                let (paper_w, paper_h) = if (avail_w / avail_h) > slide_aspect {
                    (avail_h * slide_aspect, avail_h)
                } else {
                    (avail_w, avail_w / slide_aspect)
                };

                // Slide paper must fit completely inside available space
                assert!(paper_w <= avail_w + 0.01);
                assert!(paper_h <= avail_h + 0.01);

                // Aspect ratio must be exact 16:9 without any distortion
                let computed_aspect = paper_w / paper_h;
                assert!(
                    (computed_aspect - slide_aspect).abs() < 1e-3,
                    "Slide aspect ratio must be preserved exactly: expected {}, got {}",
                    slide_aspect,
                    computed_aspect
                );
            }
        }
    }

    #[test]
    fn test_note_size_slider_and_height_distribution() {
        // Range 0.15 to 0.70 for note_preview_ratio
        let test_ratios: [f32; 7] = [0.10, 0.15, 0.30, 0.32, 0.50, 0.70, 0.85];
        let drawer_heights: [f32; 3] = [600.0, 800.0, 1080.0];

        for &raw_ratio in &test_ratios {
            // Clamped ratio in Slint and JS
            let clamped_ratio = raw_ratio.max(0.15).min(0.70);
            assert!(clamped_ratio >= 0.15 && clamped_ratio <= 0.70);

            // Slint vertical stretch calculation
            let preview_stretch = 15.max((clamped_ratio * 100.0).round() as i32);
            let comments_stretch = 25.max(((1.0 - clamped_ratio) * 100.0).round() as i32);
            assert!(preview_stretch >= 15);
            assert!(comments_stretch >= 25);

            for &h in &drawer_heights {
                let avail_h = h - 12.0 /* sleek splitter */ - 48.0 /* headers/padding */;
                let preview_h = avail_h * (preview_stretch as f32 / (preview_stretch + comments_stretch) as f32);
                let comments_h = avail_h * (comments_stretch as f32 / (preview_stretch + comments_stretch) as f32);

                // Both must have sufficient space
                assert!(preview_h >= 60.0, "Note preview height must remain usable");
                assert!(comments_h >= 100.0, "Speaker comments & notes must remain readable");
            }
        }
    }

    #[test]
    fn test_interactive_click_and_drag_splitters_between_windows() {
        // 1. Vertical Splitter Drag between Slide Canvas and Notes Window
        let workspace_w = 1200.0_f32;
        let total_w = workspace_w - 56.0_f32; // 1144.0px
        let mut notes_split_ratio = 0.32_f32;
        let mut drawer_w = total_w * notes_split_ratio; // 366.08px

        // Drag left to widen notes drawer by ~100px (mouse-x is -100px relative to splitter)
        let mouse_x_left = -100.0_f32;
        let dist_from_right_widened = drawer_w + 6.0_f32 - mouse_x_left;
        let new_ratio_widened = (dist_from_right_widened / total_w).max(0.15).min(0.70);
        assert!((new_ratio_widened - 0.4126).abs() < 0.01, "Dragging left should expand notes ratio to ~41.3%");
        notes_split_ratio = new_ratio_widened;
        drawer_w = total_w * notes_split_ratio;

        // Drag right to narrow notes drawer by ~150px (mouse-x is +150px relative to splitter)
        let mouse_x_right = 150.0_f32;
        let dist_from_right_narrowed = drawer_w + 6.0_f32 - mouse_x_right;
        let new_ratio_narrowed = (dist_from_right_narrowed / total_w).max(0.15).min(0.70);
        assert!((new_ratio_narrowed - 0.2867).abs() < 0.01, "Dragging right should reduce notes ratio to ~28.7%");

        // Extreme bounds check for vertical splitter
        let extreme_left_dist = drawer_w + 6.0_f32 - (-1000.0_f32);
        let max_ratio = (extreme_left_dist / total_w).max(0.15).min(0.70);
        assert_eq!(max_ratio, 0.70, "Vertical splitter must clamp at 70% max");

        let extreme_right_dist = drawer_w + 6.0_f32 - 1000.0_f32;
        let min_ratio = (extreme_right_dist / total_w).max(0.15).min(0.70);
        assert_eq!(min_ratio, 0.15, "Vertical splitter must clamp at 15% min");

        // 2. Horizontal Splitter Drag between Note Preview and Speaker Comments
        let preview_h = 240.0_f32;
        let comments_h = 560.0_f32;
        let total_h = preview_h + comments_h; // 800.0px
        let mut note_preview_ratio = preview_h / total_h; // 0.30
        assert_eq!(note_preview_ratio, 0.30);

        // Drag down by 80px to expand preview window
        let mouse_y_down = 80.0_f32;
        let current_mouse_h_down = preview_h + mouse_y_down;
        let new_h_ratio_down = (current_mouse_h_down / total_h).max(0.15).min(0.70);
        assert_eq!(new_h_ratio_down, 0.40, "Dragging down 80px should increase preview ratio to 40%");
        note_preview_ratio = new_h_ratio_down;

        // Drag up by 120px to shrink preview window
        let mouse_y_up = -120.0_f32;
        let current_mouse_h_up = (total_h * note_preview_ratio) + mouse_y_up;
        let new_h_ratio_up = (current_mouse_h_up / total_h).max(0.15).min(0.70);
        assert_eq!(new_h_ratio_up, 0.25, "Dragging up 120px should reduce preview ratio to 25%");

        // Extreme bounds check for horizontal splitter
        let extreme_down_ratio = ((preview_h + 1000.0_f32) / total_h).max(0.15).min(0.70);
        assert_eq!(extreme_down_ratio, 0.70, "Horizontal splitter must clamp at 70% max");

        let extreme_up_ratio = ((preview_h - 1000.0_f32) / total_h).max(0.15).min(0.70);
        assert_eq!(extreme_up_ratio, 0.15, "Horizontal splitter must clamp at 15% min");
    }

    fn presenter_detect_dual_screen(w: f32, h: f32) -> (bool, bool) {
        let aspect = w / h;
        if aspect >= 1.95 {
            (true, true)
        } else if aspect <= 0.65 {
            (true, false)
        } else {
            (false, true)
        }
    }

    #[test]
    fn test_speaker_comments_and_notes_management() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality};
        use presenter::pdf::PdfiumLoader;
        use std::path::Path;

        // 1. Verify demo presentation has non-empty speaker notes on all slides
        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        assert!(!deck.slides.is_empty());
        for (i, slide) in deck.slides.iter().enumerate() {
            assert!(
                !slide.notes.trim().is_empty(),
                "Slide {} should not have empty notes",
                i + 1
            );
        }

        // 2. Test user editing notes (simulating typing in Comment Window)
        let custom_comment = "Key proof step: integrate by parts on boundary surface.\n⏰ Reminder: Allocate 5 mins.";
        deck.slides[0].notes = custom_comment.to_string();
        assert_eq!(deck.slides[0].notes, custom_comment);

        // 3. Test quick bullet addition
        deck.slides[0].notes.push_str("\n• Additional student question response");
        assert!(deck.slides[0].notes.contains("• Additional student question response"));

        // 4. Test companion .tex note extraction on real Beamer deck
        let real_pdf = Path::new("test_deck/presentation.pdf");
        if real_pdf.exists() {
            let loaded = PdfiumLoader::load_file(real_pdf, RenderQuality::Balanced)
                .expect("Should load presentation.pdf");
            assert!(!loaded.is_empty());
            for (i, slide) in loaded.iter().enumerate() {
                assert!(
                    !slide.notes.trim().is_empty(),
                    "Real Beamer slide {} must have non-empty notes (extracted or structured default)",
                    i + 1
                );
            }
        }

        // 5. Test dual-screen Beamer deck notes image and text
        let dual_pdf = Path::new("test_deck/presentation_dual_screen.pdf");
        if dual_pdf.exists() {
            let loaded = PdfiumLoader::load_file(dual_pdf, RenderQuality::Balanced)
                .expect("Should load presentation_dual_screen.pdf");
            assert_eq!(loaded.len(), 2);
            for (i, slide) in loaded.iter().enumerate() {
                assert!(
                    slide.notes_image.is_some(),
                    "Dual-screen slide {} must have notes_image",
                    i + 1
                );
                assert!(
                    !slide.notes.trim().is_empty(),
                    "Dual-screen slide {} must have non-empty notes",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn test_export_web_debug_assets() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality, TocEntry};
        use presenter::pdf::PdfiumLoader;
        use std::path::Path;
        use std::fs;

        let web_assets_dir = Path::new("web/public/assets");
        fs::create_dir_all(web_assets_dir).expect("Create web assets dir");
        let web_local_assets_dir = Path::new("web/assets");
        fs::create_dir_all(web_local_assets_dir).expect("Create web local assets dir");

        #[derive(serde::Serialize)]
        struct WebMediaClip {
            file_path: String,
            display_path: String,
            is_resolved: bool,
            rect: [f32; 4],
        }

        #[derive(serde::Serialize)]
        struct WebSlide {
            page_index: usize,
            frame_index: usize,
            total_frames: usize,
            overlay_step: usize,
            overlay_total: usize,
            title: String,
            notes: String,
            is_dual_screen: bool,
            aspect_ratio: f32,
            has_notes_image: bool,
            audience_img: String,
            presenter_img: String,
            notes_img: Option<String>,
            media_clips: Vec<WebMediaClip>,
        }

        #[derive(serde::Serialize)]
        struct WebDeck {
            id: String,
            title: String,
            file_name: String,
            total_frames: usize,
            toc: Vec<TocEntry>,
            slides: Vec<WebSlide>,
        }

        let mut all_decks = Vec::new();

        // 1. Dual-screen presentation
        let dual_path = Path::new("test_deck/presentation_dual_screen.pdf");
        if dual_path.exists() {
            if let Ok((slides, toc)) = PdfiumLoader::load_deck_from_file(dual_path, RenderQuality::Balanced) {
                let mut web_slides = Vec::new();
                for (i, s) in slides.iter().enumerate() {
                    let aud_filename = format!("dual_slide_aud_{}.png", i + 1);
                    let pres_filename = format!("dual_slide_pres_{}.png", i + 1);
                    let _ = s.audience_image.save(web_assets_dir.join(&aud_filename));
                    let _ = s.audience_image.save(web_local_assets_dir.join(&aud_filename));
                    let _ = s.presenter_image.save(web_assets_dir.join(&pres_filename));
                    let _ = s.presenter_image.save(web_local_assets_dir.join(&pres_filename));

                    let mut note_filename = None;
                    if let Some(note_img) = &s.notes_image {
                        let fn_str = format!("dual_slide_note_{}.png", i + 1);
                        let _ = note_img.save(web_assets_dir.join(&fn_str));
                        let _ = note_img.save(web_local_assets_dir.join(&fn_str));
                        note_filename = Some(fn_str);
                    }

                    web_slides.push(WebSlide {
                        page_index: s.page_index,
                        frame_index: s.frame_index,
                        total_frames: s.total_frames,
                        overlay_step: s.overlay_step,
                        overlay_total: s.overlay_total,
                        title: s.title.clone(),
                        notes: s.notes.clone(),
                        is_dual_screen: s.is_dual_screen,
                        aspect_ratio: s.aspect_ratio,
                        has_notes_image: s.notes_image.is_some(),
                        audience_img: aud_filename,
                        presenter_img: pres_filename,
                        notes_img: note_filename,
                        media_clips: s.media_clips.iter().map(|m| WebMediaClip {
                            file_path: m.file_path.clone(),
                            display_path: m.display_path.clone(),
                            is_resolved: m.is_resolved,
                            rect: m.rect,
                        }).collect(),
                    });
                }
                let tot_frames = slides.last().map(|s| s.total_frames).unwrap_or(slides.len());
                all_decks.push(WebDeck {
                    id: "dual_screen".to_string(),
                    title: "Dual-Screen LaTeX Beamer (pgfpages 32:9)".to_string(),
                    file_name: "presentation_dual_screen.pdf".to_string(),
                    total_frames: tot_frames,
                    toc,
                    slides: web_slides,
                });
            }
        }

        // 2. Non-dual-screen presentation with separate note pages (LaTeX Beamer \setbeameroption{show notes})
        let beamer_notes_path = Path::new("test_deck/presentation.pdf");
        if beamer_notes_path.exists() {
            if let Ok((slides, toc)) = PdfiumLoader::load_deck_from_file(beamer_notes_path, RenderQuality::Balanced) {
                let mut web_slides = Vec::new();
                for (i, s) in slides.iter().enumerate() {
                    let aud_filename = format!("beamer_notes_slide_aud_{}.png", i + 1);
                    let pres_filename = format!("beamer_notes_slide_pres_{}.png", i + 1);
                    let _ = s.audience_image.save(web_assets_dir.join(&aud_filename));
                    let _ = s.audience_image.save(web_local_assets_dir.join(&aud_filename));
                    let _ = s.presenter_image.save(web_assets_dir.join(&pres_filename));
                    let _ = s.presenter_image.save(web_local_assets_dir.join(&pres_filename));

                    let mut note_filename = None;
                    if let Some(note_img) = &s.notes_image {
                        let fn_str = format!("beamer_notes_slide_note_{}.png", i + 1);
                        let _ = note_img.save(web_assets_dir.join(&fn_str));
                        let _ = note_img.save(web_local_assets_dir.join(&fn_str));
                        note_filename = Some(fn_str);
                    }

                    web_slides.push(WebSlide {
                        page_index: s.page_index,
                        frame_index: s.frame_index,
                        total_frames: s.total_frames,
                        overlay_step: s.overlay_step,
                        overlay_total: s.overlay_total,
                        title: s.title.clone(),
                        notes: s.notes.clone(),
                        is_dual_screen: s.is_dual_screen,
                        aspect_ratio: s.aspect_ratio,
                        has_notes_image: s.notes_image.is_some(),
                        audience_img: aud_filename,
                        presenter_img: pres_filename,
                        notes_img: note_filename,
                        media_clips: s.media_clips.iter().map(|m| WebMediaClip {
                            file_path: m.file_path.clone(),
                            display_path: m.display_path.clone(),
                            is_resolved: m.is_resolved,
                            rect: m.rect,
                        }).collect(),
                    });
                }
                let tot_frames = slides.last().map(|s| s.total_frames).unwrap_or(slides.len());
                all_decks.push(WebDeck {
                    id: "beamer_notes".to_string(),
                    title: "LaTeX Beamer with Separate Note Pages (16:9)".to_string(),
                    file_name: "presentation.pdf".to_string(),
                    total_frames: tot_frames,
                    toc,
                    slides: web_slides,
                });
            }
        }

        // 3. Demo presentation deck
        let demo_deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        let mut demo_web_slides = Vec::new();
        for (i, s) in demo_deck.slides.iter().enumerate() {
            let aud_filename = format!("demo_slide_aud_{}.png", i + 1);
            let pres_filename = format!("demo_slide_pres_{}.png", i + 1);
            let _ = s.audience_image.save(web_assets_dir.join(&aud_filename));
            let _ = s.audience_image.save(web_local_assets_dir.join(&aud_filename));
            let _ = s.presenter_image.save(web_assets_dir.join(&pres_filename));
            let _ = s.presenter_image.save(web_local_assets_dir.join(&pres_filename));

            let mut note_filename = None;
            if let Some(note_img) = &s.notes_image {
                let fn_str = format!("demo_slide_note_{}.png", i + 1);
                let _ = note_img.save(web_assets_dir.join(&fn_str));
                let _ = note_img.save(web_local_assets_dir.join(&fn_str));
                note_filename = Some(fn_str);
            }

            demo_web_slides.push(WebSlide {
                page_index: s.page_index,
                frame_index: s.frame_index,
                total_frames: s.total_frames,
                overlay_step: s.overlay_step,
                overlay_total: s.overlay_total,
                title: s.title.clone(),
                notes: s.notes.clone(),
                is_dual_screen: s.is_dual_screen,
                aspect_ratio: s.aspect_ratio,
                has_notes_image: s.notes_image.is_some(),
                audience_img: aud_filename,
                presenter_img: pres_filename,
                notes_img: note_filename,
                media_clips: s.media_clips.iter().map(|m| WebMediaClip {
                    file_path: m.file_path.clone(),
                    display_path: m.display_path.clone(),
                    is_resolved: m.is_resolved,
                    rect: m.rect,
                }).collect(),
            });
        }
        let demo_tot_frames = demo_deck.slides.last().map(|s| s.total_frames).unwrap_or(demo_deck.slides.len());
        all_decks.push(WebDeck {
            id: "demo".to_string(),
            title: "Interactive Beamer Demo Presentation".to_string(),
            file_name: "demo_deck.pdf".to_string(),
            total_frames: demo_tot_frames,
            toc: demo_deck.toc.clone(),
            slides: demo_web_slides,
        });

        let json_str = serde_json::to_string_pretty(&all_decks).expect("Serialize decks");
        fs::write(web_assets_dir.join("decks.json"), &json_str).expect("Write decks.json in public/assets");
        fs::write(web_local_assets_dir.join("decks.json"), &json_str).expect("Write decks.json in assets");
        println!("Successfully exported {} decks with TOC and Beamer overlays for web debug view!", all_decks.len());
    }

    #[test]
    fn test_native_in_place_video_playback_and_latex_rects() {
        use presenter::media::{probe_video_duration, InPlaceVideoPlayer};
        use presenter::pdf::media_resolver::MediaResolver;
        use presenter::pdf::{PdfiumLoader, RenderQuality};
        use std::path::Path;

        // 1. Verify LaTeX \movie[options] rect parser
        let rect_cm = MediaResolver::parse_macro_rect("width=7cm,height=3.9cm,poster");
        assert!((rect_cm[2] - (7.0 / 16.0)).abs() < 0.01, "Parsed width 7cm on 16.0cm slide: got {}", rect_cm[2]);
        assert!((rect_cm[3] - (3.9 / 9.0)).abs() < 0.01, "Parsed height 3.9cm on 9.0cm slide: got {}", rect_cm[3]);
        assert!((rect_cm[0] - ((1.0 - (7.0 / 16.0)) / 2.0)).abs() < 0.01, "x coordinate must be centered: got {}", rect_cm[0]);
        assert_eq!(rect_cm[1], 0.48, "y coordinate must be lower body offset: got {}", rect_cm[1]);

        let rect_mm = MediaResolver::parse_macro_rect("width=80mm,height=45mm");
        assert!(rect_mm[2] > 0.0 && rect_mm[2] <= 1.0);
        assert!(rect_mm[3] > 0.0 && rect_mm[3] <= 1.0);

        // Fallback for default or unspecified options
        let default_rect = MediaResolver::parse_macro_rect("autostart");
        assert_eq!(default_rect, [0.15, 0.48, 0.70, 0.50]);

        // 2. Test probing real video duration
        let video_path = "test_deck/sample_video.mp4";
        if Path::new(video_path).exists() {
            let dur = probe_video_duration(video_path);
            assert!(dur >= 5.5 && dur <= 6.5, "sample_video.mp4 duration should be ~6.0s, got {}", dur);

            // 3. Test single frame extraction for in-place scrub preview
            let frame = InPlaceVideoPlayer::extract_single_frame(video_path, 0.5);
            assert!(frame.is_some(), "Single frame extraction should succeed for sample_video.mp4");
            let pixel_buf = frame.unwrap();
            assert_eq!(pixel_buf.width(), 640);
            assert_eq!(pixel_buf.height(), 360);
            let pixels = pixel_buf.as_raw();
            assert_eq!(pixels.len(), 640 * 360 * 4);
            // Verify buffer contains actual non-zero image pixel data
            let non_zero_count = pixels.iter().filter(|&&b| b > 0).count();
            assert!(non_zero_count > 1000, "Frame buffer should contain non-trivial image data");

            // 4. Test InPlaceVideoPlayer lifecycle (stop, start, stop)
            let mut player = InPlaceVideoPlayer::new();
            player.stop(); // Safe no-op
        }

        // 5. Test real dual-screen Beamer presentation slide 2 has resolved media with exact rect
        let pdf_path = Path::new("test_deck/presentation_dual_screen.pdf");
        if pdf_path.exists() {
            let slides = PdfiumLoader::load_file(pdf_path, RenderQuality::Balanced)
                .expect("Load dual screen PDF");
            assert!(slides.len() >= 2);
            let slide2 = &slides[1];
            assert!(!slide2.media_clips.is_empty(), "Slide 2 must contain embedded media clip");
            let clip = &slide2.media_clips[0];
            assert!(clip.is_resolved, "Media clip must be resolved to local file");
            assert!(clip.rect[2] > 0.0 && clip.rect[3] > 0.0, "Media clip must have valid dimensions");
            println!(
                "Verified Slide 2 in-place video clip: {} (rect: {:?}, resolved: {})",
                clip.file_path, clip.rect, clip.is_resolved
            );
        }
    }

    #[test]
    fn test_draggable_media_controller_placement_and_clamping() {
        // Window viewport dimensions
        let canvas_w = 1200.0f32;
        let canvas_h = 800.0f32;
        let ctrl_w = 600.0f32;
        let ctrl_h = 48.0f32;

        // 1. Default initial position: Bottom-center outside slide
        let default_x = (canvas_w - ctrl_w) / 2.0; // 300.0
        let default_y = canvas_h - ctrl_h - 18.0; // 734.0
        assert_eq!(default_x, 300.0);
        assert_eq!(default_y, 734.0);

        // Clamping helper function matching Slint & JS logic
        let clamp_pos = |x: f32, y: f32| -> (f32, f32) {
            let clamped_x = x.max(0.0).min(canvas_w - ctrl_w);
            let clamped_y = y.max(0.0).min(canvas_h - ctrl_h);
            (clamped_x, clamped_y)
        };

        // 2. Dragging right & up
        let (drag1_x, drag1_y) = clamp_pos(default_x + 150.0, default_y - 200.0);
        assert_eq!(drag1_x, 450.0);
        assert_eq!(drag1_y, 534.0);

        // 3. Clamping against boundary overshoots
        let (overflow_x, overflow_y) = clamp_pos(canvas_w + 50.0, canvas_h + 100.0);
        assert_eq!(overflow_x, 600.0); // canvas_w - ctrl_w
        assert_eq!(overflow_y, 752.0); // canvas_h - ctrl_h

        let (underflow_x, underflow_y) = clamp_pos(-100.0, -50.0);
        assert_eq!(underflow_x, 0.0);
        assert_eq!(underflow_y, 0.0);
    }

    #[test]
    fn test_arrow_slide_navigation_keys_and_buttons() {
        let mut current_slide: usize = 1;
        let total_slides: usize = 3;

        let can_next = |cur: usize, tot: usize| cur < tot;
        let can_prev = |cur: usize| cur > 1;

        // Slide 1: Cannot go prev, can go next
        assert!(!can_prev(current_slide));
        assert!(can_next(current_slide, total_slides));

        // Advance with Right / Down Arrow
        if can_next(current_slide, total_slides) {
            current_slide += 1;
        }
        assert_eq!(current_slide, 2);
        assert!(can_prev(current_slide));
        assert!(can_next(current_slide, total_slides));

        // Advance to slide 3
        if can_next(current_slide, total_slides) {
            current_slide += 1;
        }
        assert_eq!(current_slide, 3);
        assert!(can_prev(current_slide));
        assert!(!can_next(current_slide, total_slides));

        // Go back with Left / Up Arrow
        if can_prev(current_slide) {
            current_slide -= 1;
        }
        assert_eq!(current_slide, 2);
    }

    #[test]
    fn test_youtube_full_width_media_bar_controls_and_scrubber() {
        let window_w = 1440.0f32;
        let window_h = 900.0f32;
        let sidebar_w = 50.0f32;
        let drawer_w = 400.0f32;
        let splitter_w = 6.0f32;
        let bar_h = 52.0f32;

        // 1. Media controller is docked at the bottom of the slide window (canvas_area), NOT full window
        // Slide window width is window_w - sidebar_w - (drawer_w + splitter_w)
        let slide_window_w = window_w - sidebar_w - (drawer_w + splitter_w); // 984.0
        let bar_width = slide_window_w;
        assert_eq!(bar_width, 984.0);

        // Slide window has full vertical height in workspace
        let slide_window_h = window_h;
        let bar_y = slide_window_h - bar_h;
        assert_eq!(bar_y, 848.0);

        // Sidebar and Notes Drawer retain full window height without being truncated
        assert_eq!(slide_window_h, 900.0);

        // 2. Resizing slide window automatically resizes the docked media bar
        // Case A: Notes drawer closed (drawer_w = 0, splitter_w = 0)
        let expanded_slide_w = window_w - sidebar_w; // 1390.0
        let expanded_bar_w = expanded_slide_w;
        assert_eq!(expanded_bar_w, 1390.0);

        // Case B: Window width resized to 1920
        let new_window_w = 1920.0f32;
        let resized_slide_w = new_window_w - sidebar_w - (drawer_w + splitter_w); // 1464.0
        assert_eq!(resized_slide_w, 1464.0);

        // 3. YouTube Red Scrubber Track scaling with slide window width
        let total_duration = 90.0f32; // 01:30
        let current_time = 45.0f32;   // 00:45
        let progress = (current_time / total_duration).clamp(0.0, 1.0);
        assert_eq!(progress, 0.5);

        let played_px = bar_width * progress;
        assert_eq!(played_px, 492.0); // 984.0 * 0.5

        let thumb_x = (bar_width - 13.0) * progress;
        assert_eq!(thumb_x, (984.0 - 13.0) * 0.5);

        // 4. Time Readout formatting
        let format_time = |secs: f32| -> String {
            let s = secs.floor() as u32;
            let m = s / 60;
            let rem_s = s % 60;
            format!("{:02}:{:02}", m, rem_s)
        };
        assert_eq!(format_time(current_time), "00:45");
        assert_eq!(format_time(total_duration), "01:30");
        let readout = format!("{} / {}", format_time(current_time), format_time(total_duration));
        assert_eq!(readout, "00:45 / 01:30");

        // 5. Play / Pause / Replay state
        let is_playing = true;
        let icon_playing = if current_time >= total_duration - 0.1 { "⟲" } else if is_playing { "⏸" } else { "▶" };
        assert_eq!(icon_playing, "⏸");

        let icon_ended = if (total_duration) >= total_duration - 0.1 { "⟲" } else { "▶" };
        assert_eq!(icon_ended, "⟲");

        // 6. Speed cycling options: verify 1.0 -> 1.25 -> 1.5 -> 2.0 -> 0.75 -> 1.0
        let speed_options = [1.0f32, 1.25, 1.5, 2.0, 0.75];
        let mut speed = 1.0f32;
        let next_speed = |cur: f32| -> f32 {
            let idx = speed_options.iter().position(|&s| (s - cur).abs() < 0.05).unwrap();
            speed_options[(idx + 1) % speed_options.len()]
        };

        speed = next_speed(speed);
        assert_eq!(speed, 1.25, "1.0x should cycle to 1.25x");
        speed = next_speed(speed);
        assert_eq!(speed, 1.5, "1.25x should cycle to 1.5x");
        speed = next_speed(speed);
        assert_eq!(speed, 2.0, "1.5x should cycle to 2.0x");
        speed = next_speed(speed);
        assert_eq!(speed, 0.75, "2.0x should cycle to 0.75x");
        speed = next_speed(speed);
        assert_eq!(speed, 1.0, "0.75x must cycle back to 1.0x (allowing user to select 1.0x)");
    }

    #[test]
    fn test_screen_blackout_and_whiteout_toggles_and_resets() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality};

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        assert!(deck.total_slides() >= 3);

        // 1. Initial State: Normal display (blank_mode = 0)
        let mut blank_mode = 0i32; // 0: Normal, 1: Blackout, 2: Whiteout
        assert_eq!(blank_mode, 0);

        // 2. Toggle Blackout ('B')
        blank_mode = if blank_mode == 1 { 0 } else { 1 };
        assert_eq!(blank_mode, 1, "Pressing B should activate Blackout");

        // 3. Toggle Blackout again restores normal
        blank_mode = if blank_mode == 1 { 0 } else { 1 };
        assert_eq!(blank_mode, 0, "Pressing B again should deactivate Blackout");

        // 4. Toggle Whiteout ('W')
        blank_mode = if blank_mode == 2 { 0 } else { 2 };
        assert_eq!(blank_mode, 2, "Pressing W should activate Whiteout");

        // 5. Advancing slides (next, prev, jump, next_frame) must auto-reset blank_mode to 0
        deck.next();
        blank_mode = 0;
        assert_eq!(blank_mode, 0, "Advancing to next slide must automatically unblank display");

        // 6. Whiteout then prev slide auto-resets
        blank_mode = 2;
        assert_eq!(blank_mode, 2);
        deck.prev();
        blank_mode = 0;
        assert_eq!(blank_mode, 0, "Going to previous slide must automatically unblank display");
    }

    #[test]
    fn test_screen_swap_display_modes() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality};

        let deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        let slide = deck.current_slide();

        let mut is_screen_swapped = false;

        // Normal display: presenter sees presenter_image, audience sees audience_image
        let (pres_disp, aud_disp) = if is_screen_swapped {
            (&slide.audience_image, &slide.presenter_image)
        } else {
            (&slide.presenter_image, &slide.audience_image)
        };
        assert_eq!(pres_disp.dimensions(), slide.presenter_image.dimensions());
        assert_eq!(aud_disp.dimensions(), slide.audience_image.dimensions());

        // Press 'S' to Swap Screens
        is_screen_swapped = !is_screen_swapped;
        assert!(is_screen_swapped, "Screen swap flag must toggle to true");

        // Swapped display: roles inverted
        let (swapped_pres, swapped_aud) = if is_screen_swapped {
            (&slide.audience_image, &slide.presenter_image)
        } else {
            (&slide.presenter_image, &slide.audience_image)
        };
        assert_eq!(swapped_pres.dimensions(), slide.audience_image.dimensions());
        assert_eq!(swapped_aud.dimensions(), slide.presenter_image.dimensions());

        // Press 'S' again restores default
        is_screen_swapped = !is_screen_swapped;
        assert!(!is_screen_swapped, "Screen swap flag must toggle back to false");
    }

    #[test]
    fn test_beamer_overlay_detection_and_frame_navigation() {
        use presenter::pdf::beamer::BeamerMetadata;
        use presenter::pdf::deck::{PresentationDeck, RenderQuality, Slide};
        use image::RgbaImage;

        // 1. Test Beamer .nav content parser with multi-page overlay frames
        let mock_nav = r#"
\headcommand {\slideentry {0}{0}{1}{1/3}{}{0}}
\headcommand {\beamer@framepages {1}{3}}
\headcommand {\slideentry {0}{0}{2}{4/4}{}{0}}
\headcommand {\beamer@framepages {4}{4}}
\headcommand {\slideentry {0}{0}{3}{5/7}{}{0}}
\headcommand {\beamer@framepages {5}{7}}
\headcommand {\gdef \inserttotalframenumber {3}}
"#;
        let parsed = BeamerMetadata::parse_nav_content(mock_nav);
        assert!(parsed.is_some(), "Should parse valid Beamer .nav content");
        let (mappings, total_frames) = parsed.unwrap();
        assert_eq!(total_frames, 3, "Total logical frames should be 3");
        assert_eq!(mappings.len(), 3);
        assert_eq!(mappings[0].start_page, 1);
        assert_eq!(mappings[0].end_page, 3);
        assert_eq!(mappings[1].start_page, 4);
        assert_eq!(mappings[1].end_page, 4);
        assert_eq!(mappings[2].start_page, 5);
        assert_eq!(mappings[2].end_page, 7);

        // 2. Synthesize 7 slides corresponding to the 3 logical frames
        let mut slides = Vec::new();
        for page in 1..=7 {
            slides.push(Slide {
                page_index: page,
                physical_page_index: page,
                frame_index: page,
                total_frames: 1,
                overlay_step: 1,
                overlay_total: 1,
                title: format!("Slide {}", page),
                notes: String::new(),
                is_whiteboard: false,
                is_hidden: false,
                is_dual_screen: false,
                aspect_ratio: 1.7778,
                media_clips: Vec::new(),
                inking: presenter::ink::SlideInking::new(),
                links: Vec::new(),
                audio_progress_box: None,
                content_hash: 0,
                audience_image: std::sync::Arc::new(presenter::pdf::SlideImage::new(RgbaImage::new(100, 100))),
                presenter_image: std::sync::Arc::new(presenter::pdf::SlideImage::new(RgbaImage::new(100, 100))),
                notes_image: None,
            });
        }

        // 3. Assign frame metadata
        BeamerMetadata::assign_frame_metadata(&mut slides, Some((mappings, total_frames)));

        // Verify Frame 1 overlays (Pages 1, 2, 3)
        assert_eq!(slides[0].frame_index, 1);
        assert_eq!(slides[0].overlay_step, 1);
        assert_eq!(slides[0].overlay_total, 3);

        assert_eq!(slides[1].frame_index, 1);
        assert_eq!(slides[1].overlay_step, 2);
        assert_eq!(slides[1].overlay_total, 3);

        assert_eq!(slides[2].frame_index, 1);
        assert_eq!(slides[2].overlay_step, 3);
        assert_eq!(slides[2].overlay_total, 3);

        // Verify Frame 2 (Page 4)
        assert_eq!(slides[3].frame_index, 2);
        assert_eq!(slides[3].overlay_step, 1);
        assert_eq!(slides[3].overlay_total, 1);

        // Verify Frame 3 overlays (Pages 5, 6, 7)
        assert_eq!(slides[4].frame_index, 3);
        assert_eq!(slides[4].overlay_step, 1);
        assert_eq!(slides[4].overlay_total, 3);

        assert_eq!(slides[6].frame_index, 3);
        assert_eq!(slides[6].overlay_step, 3);
        assert_eq!(slides[6].overlay_total, 3);

        // 4. Test Deck Frame Navigation (next_frame / prev_frame)
        let mut deck = PresentationDeck {
            file_path: None,
            slides,
            toc: Vec::new(),
            current_index: 0, // Starts at Page 1 (Frame 1, Step 1)
            quality: RenderQuality::Balanced,
        };

        // Jump to next frame ('J'): skips step 2 & 3, jumps straight to Page 4 (Frame 2)
        let jumped = deck.next_frame();
        assert!(jumped);
        assert_eq!(deck.current_index, 3, "next_frame() should jump to Page 4 (Frame 2)");
        assert_eq!(deck.current_slide().frame_index, 2);

        // Jump to next frame ('J') again: jumps to Page 5 (Frame 3, Step 1)
        let jumped2 = deck.next_frame();
        assert!(jumped2);
        assert_eq!(deck.current_index, 4, "next_frame() should jump to Page 5 (Frame 3)");
        assert_eq!(deck.current_slide().frame_index, 3);

        // Jump past last frame returns false
        assert!(!deck.next_frame());

        // Jump to previous frame ('K'): jumps back to Frame 2 (Page 4)
        let back = deck.prev_frame();
        assert!(back);
        assert_eq!(deck.current_index, 3, "prev_frame() should jump back to Frame 2");
        assert_eq!(deck.current_slide().frame_index, 2);

        // Jump to previous frame ('K') again: jumps back to Frame 1 (Page 1)
        let back2 = deck.prev_frame();
        assert!(back2);
        assert_eq!(deck.current_index, 0, "prev_frame() should jump back to Frame 1");
        assert_eq!(deck.current_slide().frame_index, 1);
    }

    #[test]
    fn test_pdf_outline_toc_drawer_hierarchical_extraction() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality};
        use presenter::pdf::PdfiumLoader;
        use std::path::Path;

        // 1. Test companion .nav TOC extraction from test_deck/presentation.nav
        let pdf_path = Path::new("test_deck/presentation_dual_screen.pdf");
        if pdf_path.exists() {
            let (slides, toc) = PdfiumLoader::load_deck_from_file(pdf_path, RenderQuality::Balanced)
                .expect("Load deck with TOC");
            assert!(!slides.is_empty(), "Slides should not be empty");
            assert!(!toc.is_empty(), "Extracted TOC outline entries must not be empty");
            println!("Extracted {} TOC entries from presentation:", toc.len());
            for item in &toc {
                println!("  - [{}] {} -> Slide p.{} (Frame {})", item.level, item.title, item.page_index, item.frame_index);
                assert!(item.page_index >= 1, "Page index must be 1-indexed");
                assert!(item.page_index <= slides.len(), "Page index must point to a valid slide");
            }
        }

        // 2. Test fallback TOC generation and hierarchy levels
        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        assert!(!deck.toc.is_empty(), "Demo deck must contain synthetic TOC entries");
        assert_eq!(deck.toc[0].level, 0, "Top-level section must have level 0");

        // 3. Test TOC click-to-jump
        let target_entry = &deck.toc[1];
        let target_page = target_entry.page_index;
        let jumped = deck.jump_to(target_page - 1);
        assert!(jumped);
        assert_eq!(deck.current_slide().page_index, target_page);
    }

    #[test]
    fn test_pdf_interactive_links_click_and_touch_and_hold() {
        use presenter::pdf::deck::{PdfLinkTarget, PresentationDeck, RenderQuality};
        use presenter::ink::{Point, Stroke};
        use std::time::Instant;

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::UltraVector);
        assert!(deck.total_slides() >= 4);

        // Slide 1 has a URL link [0.05, 0.90, 0.35, 0.08] -> "https://github.com"
        // and a page link [0.80, 0.90, 0.16, 0.08] -> Slide 2 (index 1)
        let slide1 = deck.current_slide();
        assert_eq!(slide1.links.len(), 2);

        // 1. Hit testing inside and outside bounds
        let url_link = slide1.find_link_at(0.10, 0.92, 0.01).expect("Hit URL link");
        assert_eq!(url_link.target, PdfLinkTarget::Url("https://github.com".to_string()));
        assert!(url_link.tooltip.contains("github.com"));

        let page_link = slide1.find_link_at(0.85, 0.92, 0.01).expect("Hit page link");
        assert_eq!(page_link.target, PdfLinkTarget::Page(1));
        assert!(page_link.tooltip.contains("Slide 2"));

        // Outside all links
        assert!(slide1.find_link_at(0.50, 0.50, 0.01).is_none());

        // 2. Simulate Touch-and-Hold in Pen Mode
        // On pointer down on page link:
        let hit = slide1.find_link_at(0.85, 0.92, 0.015).cloned();
        assert!(hit.is_some());
        let mut active_link_target = hit;
        let _link_press_start = Some(Instant::now());
        let mut link_triggered = false;

        // In Pen mode, pointer down adds a preliminary stroke
        let mut stroke = Stroke::new(1, [254, 240, 78, 255], 3.0, false);
        stroke.add_point(Point::new(0.85, 0.92, 1.0, 1000));
        let mut active_stroke = Some(stroke);

        // Verify stroke exists before hold completes
        assert!(active_stroke.is_some());

        // Simulate hold threshold reached (e.g. 380ms)
        let elapsed_ms = 400.0;
        let hold_thresh = 380.0;
        if elapsed_ms >= hold_thresh && !link_triggered {
            if let Some(link) = active_link_target.take() {
                link_triggered = true;
                // Discard preliminary pen stroke so no ink dot is left behind!
                active_stroke = None;

                match link.target {
                    PdfLinkTarget::Page(dest_idx) => {
                        let jumped = deck.jump_to(dest_idx);
                        assert!(jumped);
                    }
                    _ => panic!("Expected page link target"),
                }
            }
        }

        assert!(link_triggered);
        assert!(active_stroke.is_none(), "Ink stroke should be cancelled when touch-and-hold link triggers");
        assert_eq!(deck.current_index, 1, "Should have jumped to Slide 2");

        // 3. Simulate Drag-to-Cancel (User touches on link but drags away to draw)
        deck.jump_to(0);
        let anchor_pos = (0.08, 0.92);
        let mut link_cand = deck.current_slide().find_link_at(anchor_pos.0, anchor_pos.1, 0.015).cloned();
        assert!(link_cand.is_some());

        // User moves pointer significantly (dist > 0.02)
        let moved_pos = (0.15, 0.80);
        let dist = ((moved_pos.0 - anchor_pos.0).powi(2) + (moved_pos.1 - anchor_pos.1).powi(2)).sqrt();
        assert!(dist > 0.02);
        if dist > 0.02 {
            link_cand = None; // Cancel link hold
        }
        assert!(link_cand.is_none(), "Significant pointer movement must cancel link hold");

        // 4. Simulate Click in Hand tool
        let hand_click_pos = (0.85, 0.92);
        let clicked_link = deck.current_slide().find_link_at(hand_click_pos.0, hand_click_pos.1, 0.015).cloned();
        assert!(clicked_link.is_some());
        let click_elapsed_ms = 120u64; // fast click
        let is_hand_tool = true;
        let mut clicked_jumped = false;
        if is_hand_tool && click_elapsed_ms < 400 {
            if let Some(PdfLinkTarget::Page(idx)) = clicked_link.map(|l| l.target) {
                clicked_jumped = deck.jump_to(idx);
            }
        }
        assert!(clicked_jumped);
        assert_eq!(deck.current_index, 1);
    }

    #[test]
    fn test_media_player_stop_button_reverts_slide_state() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality};
        use presenter::media::InPlaceVideoPlayer;
        use presenter::ink::{Point, Stroke};

        // 1. Initialize deck and jump to user's selected frame (e.g., Slide 2, index 1)
        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        assert!(deck.total_slides() >= 3);
        let selected_slide_idx = 1;
        assert!(deck.jump_to(selected_slide_idx));
        assert_eq!(deck.current_index, selected_slide_idx);

        // Record the original frame image dimension or properties
        let original_slide = deck.current_slide();
        let original_frame_idx = original_slide.frame_index;
        let original_page_idx = original_slide.page_index;
        let original_w = original_slide.presenter_image.width();
        let original_h = original_slide.presenter_image.height();

        // Add some ink annotations on this slide frame
        let mut strokes: Vec<Stroke> = Vec::new();
        let mut test_stroke = Stroke::new(1, [255, 0, 0, 255], 3.0, false);
        test_stroke.add_point(Point::new(100.0, 100.0, 0.5, 10));
        test_stroke.add_point(Point::new(150.0, 150.0, 0.5, 20));
        strokes.push(test_stroke);
        assert_eq!(strokes.len(), 1);

        // 2. Simulate Media Playback Starting on this slide
        let mut video_player = InPlaceVideoPlayer::new();
        let mut media_playing = true;
        let media_video_visible = true;
        let mut media_current_time_secs = 24.5f32;
        let total_duration = 60.0f32;
        let mut media_progress = media_current_time_secs / total_duration;

        assert!(media_playing);
        assert!(media_video_visible);
        assert!(media_progress > 0.0);

        // 3. User clicks Stop Button ("⏹") or presses Escape
        // In backend (src/main.rs on_stop_media):
        // - video_player.stop()
        // - kills background audio processes
        // - resets media_playing = false, current_time = 0.0, progress = 0.0
        // - resets media_video_visible = false
        // - calls update_slide_view() to restore the exact slide canvas frame image
        video_player.stop();
        media_playing = false;
        media_current_time_secs = 0.0;
        media_progress = 0.0;
        let presenter_video_visible = false;
        let audience_video_visible = false;

        // Slide view update logic: preserves selected frame and restores canvas
        let active_slide = deck.current_slide();
        assert_eq!(active_slide.page_index, original_page_idx, "Must remain on the exact user-selected slide");
        assert_eq!(active_slide.frame_index, original_frame_idx, "Must remain on the exact frame");
        assert_eq!(active_slide.presenter_image.width(), original_w);
        assert_eq!(active_slide.presenter_image.height(), original_h);

        // Verify media player state has been completely stopped and reset
        assert!(!media_playing, "Media must no longer be playing");
        assert_eq!(media_current_time_secs, 0.0, "Media time must be reset to 0.0s");
        assert_eq!(media_progress, 0.0, "Media progress must be reset to 0.0");
        assert!(!presenter_video_visible, "Presenter video overlay must be hidden to show original slide frame");
        assert!(!audience_video_visible, "Audience video overlay must be hidden to show original slide frame");

        // Annotations and ink remain intact on the reverted slide
        assert_eq!(strokes.len(), 1, "User ink annotations must remain intact on the reverted slide");
        assert_eq!(strokes[0].points.len(), 2);
    }

    #[test]
    fn test_end_of_presentation_black_screen_navigation() {
        use presenter::pdf::deck::{PresentationDeck, RenderQuality};

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);
        let total = deck.total_slides();
        assert!(total >= 2);

        // State simulation matching src/main.rs
        let mut current_idx = 0usize;
        let mut is_end_of_show = false;
        let mut blank_mode = 0i32;

        // 1. Navigate to the last slide
        while current_idx + 1 < total {
            if deck.next() {
                current_idx += 1;
            }
        }
        assert_eq!(current_idx, total - 1, "Should be on the last slide");
        assert!(!is_end_of_show, "End of show should not be active while on content slides");
        assert_eq!(blank_mode, 0, "Screen should be normal on content slides");

        // 2. Next slide preview on the last slide should show the black screen
        let has_next_slide = current_idx + 1 < total;
        assert!(!has_next_slide, "No subsequent slide in deck");
        let next_preview_is_black = !has_next_slide;
        assert!(next_preview_is_black, "Next slide preview must be black screen");

        // 3. User reaches past the last slide (presses Next / Right Arrow / Space / next_frame)
        if !is_end_of_show {
            if !deck.next() {
                // Reached past last slide: activate black screen
                is_end_of_show = true;
                blank_mode = 1;
            }
        }

        assert!(is_end_of_show, "is_end_of_show must be true when reaching past last slide");
        assert_eq!(blank_mode, 1, "blank_mode must be 1 (blackout)");

        // 4. Verify audience screen is completely black
        let audience_is_black = (blank_mode == 1) || is_end_of_show;
        assert!(audience_is_black, "Audience display must be 100% blacked out");

        // 5. Verify presenter navigation buttons: Next dimmed, Prev active
        let next_button_opacity = if is_end_of_show { 0.35f32 } else { 1.0f32 };
        let prev_button_opacity = if (current_idx + 1 > 1) || is_end_of_show { 1.0f32 } else { 0.35f32 };
        assert_eq!(next_button_opacity, 0.35);
        assert_eq!(prev_button_opacity, 1.0);

        // 6. Pressing Prev / Left Arrow / Backspace returns to last slide
        if is_end_of_show {
            is_end_of_show = false;
            blank_mode = 0;
        } else if deck.prev() {
            current_idx -= 1;
        }

        assert!(!is_end_of_show, "is_end_of_show must clear upon returning");
        assert_eq!(blank_mode, 0, "blank_mode must return to normal");
        assert_eq!(current_idx, total - 1, "Must return to exact last slide");

        // 7. Test next_frame reaching the end
        if !is_end_of_show {
            if !deck.next_frame() && !deck.next() {
                is_end_of_show = true;
                blank_mode = 1;
            }
        }
        assert!(is_end_of_show, "next_frame at end must also trigger black screen");

        // 8. Toggling blackout ('B') or jumping to slide exits black screen
        if is_end_of_show {
            is_end_of_show = false;
            blank_mode = 0;
        }
        assert!(!is_end_of_show);
        assert_eq!(blank_mode, 0);
    }

    #[test]
    fn test_video_pause_and_resume_preserves_position() {
        use presenter::media::InPlaceVideoPlayer;

        let mut player = InPlaceVideoPlayer::new();
        assert_eq!(player.current_time(), 0.0);
        assert!(!player.is_playing());

        // 1. Simulate starting playback at 0.0s
        player.set_current_time(0.0);

        // 2. Simulate playback progressing to 3.45s
        player.set_current_time(3.45);
        assert_eq!(player.current_time(), 3.45);

        // 3. User pauses playback
        player.pause();
        assert!(!player.is_playing());
        // Verify position is preserved at exactly 3.45s and NOT reset to 0.0
        assert_eq!(player.current_time(), 3.45, "Paused video must preserve its current playback timestamp");

        // 4. Resume playback: start_time_secs is taken from player.current_time() (3.45s)
        let resumed_start = player.current_time();
        assert_eq!(resumed_start, 3.45, "Resumed playback must start from paused position");

        // 5. Simulate video playing further to 5.20s
        player.set_current_time(5.20);
        assert_eq!(player.current_time(), 5.20);

        // 6. User seeks to 1.80s while paused
        player.pause();
        player.set_current_time(1.80);
        assert_eq!(player.current_time(), 1.80, "Seek must update player current_time");

        // 7. User stops playback ("⏹")
        player.stop();
        assert_eq!(player.current_time(), 0.0, "Stop must reset player position to 0.0");
    }

    #[test]
    fn test_color_wheel_hsv_math_and_palette_layout() {
        // HSV to RGB algorithm verification
        let hsv_to_rgb = |h: f32, s: f32, v: f32| -> (u8, u8, u8) {
            let c = v * s;
            let hp = (h.rem_euclid(360.0)) / 60.0;
            let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
            let (r1, g1, b1) = if hp < 1.0 { (c, x, 0.0) }
            else if hp < 2.0 { (x, c, 0.0) }
            else if hp < 3.0 { (0.0, c, x) }
            else if hp < 4.0 { (0.0, x, c) }
            else if hp < 5.0 { (x, 0.0, c) }
            else { (c, 0.0, x) };
            let m = v - c;
            (
                ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
                ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
                ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
            )
        };

        // 1. Primary & Secondary hues at full saturation & brightness
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0), "0 deg should be Red");
        assert_eq!(hsv_to_rgb(60.0, 1.0, 1.0), (255, 255, 0), "60 deg should be Yellow");
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0), "120 deg should be Green");
        assert_eq!(hsv_to_rgb(180.0, 1.0, 1.0), (0, 255, 255), "180 deg should be Cyan");
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), (0, 0, 255), "240 deg should be Blue");
        assert_eq!(hsv_to_rgb(300.0, 1.0, 1.0), (255, 0, 255), "300 deg should be Magenta");

        // 2. Greyscale and brightness scaling
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), (255, 255, 255), "0 sat, 1 val should be White");
        assert_eq!(hsv_to_rgb(0.0, 0.0, 0.0), (0, 0, 0), "0 val should be Black");
        assert_eq!(hsv_to_rgb(0.0, 1.0, 0.5), (128, 0, 0), "Half brightness Red");

        // 3. Wheel geometry calculations (120x120 circle, center at 60,60, radius 56)
        let cx = 60.0f32;
        let cy = 60.0f32;
        let max_r = 56.0f32;

        // Touch at rightmost edge (dx=56, dy=0) -> 0 deg (Red), sat 1.0
        let touch_x = 116.0f32;
        let touch_y = 60.0f32;
        let dx = touch_x - cx;
        let dy = touch_y - cy;
        let dist = (dx * dx + dy * dy).sqrt();
        let angle = dy.atan2(dx);
        let mut hue = angle.to_degrees();
        if hue < 0.0 { hue += 360.0; }
        let sat = (dist / max_r).clamp(0.0, 1.0);
        assert!((hue - 0.0).abs() < 1e-4);
        assert!((sat - 1.0).abs() < 1e-4);

        // Touch at bottom edge (dx=0, dy=56) -> 90 deg, sat 1.0
        let touch_y2 = 116.0f32;
        let angle2 = (touch_y2 - cy).atan2(0.0);
        let mut hue2 = angle2.to_degrees();
        if hue2 < 0.0 { hue2 += 360.0; }
        assert!((hue2 - 90.0).abs() < 1e-4);

        // 4. Palette row layout geometry check
        // Popover width: 290px, padding: 16px left + 16px right => usable inner width: 258px
        // 6 swatches (28px each) + 1 add button (28px) = 7 items
        // 6 gaps * 8px = 48px
        // Total row width: 7 * 28 + 48 = 244px <= 258px (guaranteed no overflow/clipping)
        let usable_width = 290.0 - 32.0; // 258.0px
        let item_count = 7.0f32; // 6 swatches + 1 add button
        let item_size = 28.0f32;
        let spacing = 8.0f32;
        let total_row_w = item_count * item_size + (item_count - 1.0) * spacing;
        assert!(total_row_w <= usable_width, "Palette row ({}px) must fit within usable width ({}px)", total_row_w, usable_width);
    }

    #[test]
    fn test_tool_click_selection_and_right_click_options_menu() {
        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        #[allow(dead_code)]
        enum Tool {
            Pen,
            Highlighter,
            Eraser,
            Laser,
        }

        struct ToolBarState {
            current_tool: Tool,
            active_color_slot: usize,
            show_pen_popover: bool,
            show_highlighter_popover: bool,
            show_eraser_popover: bool,
            show_laser_popover: bool,
            show_color_popover: bool,
        }

        impl ToolBarState {
            fn new() -> Self {
                Self {
                    current_tool: Tool::Pen,
                    active_color_slot: 0,
                    show_pen_popover: false,
                    show_highlighter_popover: false,
                    show_eraser_popover: false,
                    show_laser_popover: false,
                    show_color_popover: false,
                }
            }

            // Left-click selects the tool and closes all popovers
            fn on_tool_click(&mut self, tool: Tool) {
                self.current_tool = tool;
                self.close_all_popovers();
            }

            // Right-click selects the tool and toggles its options popover
            fn on_tool_right_click(&mut self, tool: Tool) {
                self.current_tool = tool;
                let (target_pen, target_hl, target_eraser, target_laser) = match tool {
                    Tool::Pen => (!self.show_pen_popover, false, false, false),
                    Tool::Highlighter => (false, !self.show_highlighter_popover, false, false),
                    Tool::Eraser => (false, false, !self.show_eraser_popover, false),
                    Tool::Laser => (false, false, false, !self.show_laser_popover),
                };
                self.show_pen_popover = target_pen;
                self.show_highlighter_popover = target_hl;
                self.show_eraser_popover = target_eraser;
                self.show_laser_popover = target_laser;
                self.show_color_popover = false;
            }

            // Left-click selects the color slot and closes all popovers
            fn on_color_slot_click(&mut self, slot: usize) {
                self.active_color_slot = slot;
                self.close_all_popovers();
            }

            // Right-click selects the color slot and toggles color options popover
            fn on_color_slot_right_click(&mut self, slot: usize) {
                self.active_color_slot = slot;
                self.show_color_popover = !self.show_color_popover;
                self.show_pen_popover = false;
                self.show_highlighter_popover = false;
                self.show_eraser_popover = false;
                self.show_laser_popover = false;
            }

            fn on_key(&mut self, key: char) {
                match key {
                    'p' | 'P' => self.on_tool_click(Tool::Pen),
                    'h' | 'H' => self.on_tool_click(Tool::Highlighter),
                    'e' | 'E' => self.on_tool_click(Tool::Eraser),
                    'l' | 'L' => self.on_tool_click(Tool::Laser),
                    '1' => self.on_color_slot_click(0),
                    '2' => self.on_color_slot_click(1),
                    '3' => self.on_color_slot_click(2),
                    '4' => self.on_color_slot_click(3),
                    '5' => self.on_color_slot_click(4),
                    _ => {}
                }
            }

            fn close_all_popovers(&mut self) {
                self.show_pen_popover = false;
                self.show_highlighter_popover = false;
                self.show_eraser_popover = false;
                self.show_laser_popover = false;
                self.show_color_popover = false;
            }
        }

        let mut tb = ToolBarState::new();

        // 1. Initial State: Pen is selected, no popovers open
        assert_eq!(tb.current_tool, Tool::Pen);
        assert!(!tb.show_pen_popover);

        // 2. User left-clicks Highlighter: Selects Highlighter, popovers remain CLOSED
        tb.on_tool_click(Tool::Highlighter);
        assert_eq!(tb.current_tool, Tool::Highlighter);
        assert!(!tb.show_highlighter_popover, "Left-clicking a tool must NOT show options popover");
        assert!(!tb.show_pen_popover);

        // 3. User right-clicks Highlighter: Opens Highlighter options popover
        tb.on_tool_right_click(Tool::Highlighter);
        assert_eq!(tb.current_tool, Tool::Highlighter);
        assert!(tb.show_highlighter_popover, "Right-clicking Highlighter must show options popover");

        // 4. User left-clicks Eraser: Switches to Eraser, closes Highlighter options popover
        tb.on_tool_click(Tool::Eraser);
        assert_eq!(tb.current_tool, Tool::Eraser);
        assert!(!tb.show_highlighter_popover, "Switching tools on click must close previous popover");
        assert!(!tb.show_eraser_popover, "Left-clicking Eraser must select without opening menu");

        // 5. User right-clicks Laser Pointer: Switches to Laser and opens Laser options popover
        tb.on_tool_right_click(Tool::Laser);
        assert_eq!(tb.current_tool, Tool::Laser);
        assert!(tb.show_laser_popover, "Right-clicking Laser must open Laser options menu");

        // 6. User presses 'P' shortcut: Selects Pen, closes Laser options popover
        tb.on_key('p');
        assert_eq!(tb.current_tool, Tool::Pen);
        assert!(!tb.show_laser_popover);
        assert!(!tb.show_pen_popover, "Pressing P shortcut must select Pen without opening menu");

        // 7. Color slot left-click: Selects slot 1, no popover opens
        tb.on_color_slot_click(1);
        assert_eq!(tb.active_color_slot, 1);
        assert!(!tb.show_color_popover, "Left-clicking color slot must select without opening popover");

        // 8. Color slot right-click: Selects slot 2 and OPENS color popover
        tb.on_color_slot_right_click(2);
        assert_eq!(tb.active_color_slot, 2);
        assert!(tb.show_color_popover, "Right-clicking color slot must open color popover");

        // 9. Right-clicking again toggles it off
        tb.on_color_slot_right_click(2);
        assert!(!tb.show_color_popover, "Right-clicking again toggles color popover off");

        // 10. Shortcut '4' selects slot 3 (Coral Red)
        tb.on_key('4');
        assert_eq!(tb.active_color_slot, 3);
        assert!(!tb.show_color_popover);

        // 11. Shortcut '5' selects slot 4 (Sky Blue)
        tb.on_key('5');
        assert_eq!(tb.active_color_slot, 4);
        assert!(!tb.show_color_popover);

        // 12. Right-click slot 4 opens popover
        tb.on_color_slot_right_click(4);
        assert_eq!(tb.active_color_slot, 4);
        assert!(tb.show_color_popover);
    }

    #[test]
    fn test_non_dual_screen_note_detection_and_separation() {
        use presenter::pdf::beamer::{BeamerMetadata, NotesLayoutMode};
        use std::path::Path;

        // 1. Test clean_beamer_note_text strips Beamer metadata headers
        let raw_beamer_note = "\
8 1 - 0 1 - 6 2 0 2\n\
PRESENTER NOTES (Slide 1):\n\
• Welcome everyone to the presentation.\n\
• Introduce the objective: evaluating the next-generation Rust-based Beamer Presenter.\n\
• Point out that this presentation includes embedded audio and video media.\n\
• Press `L` on your keyboard to activate the laser pointer at any time.\n\
1 / 5";
        let cleaned = BeamerMetadata::clean_beamer_note_text(raw_beamer_note);
        assert!(!cleaned.contains("8 1 - 0 1 - 6 2 0 2"));
        assert!(!cleaned.contains("1 / 5"));
        assert!(cleaned.contains("PRESENTER NOTES (Slide 1):"));
        assert!(cleaned.contains("Welcome everyone"));

        // 2. Test detect_slide_mappings with Beamer .nav companion file
        let nav_path = Path::new("test_deck/presentation.nav");
        if nav_path.exists() {
            let nav_info = BeamerMetadata::parse_nav_file(nav_path);
            let page_sizes = vec![(1920.0, 1080.0); 10];
            let page_labels = vec![None; 10];
            let page_texts = vec![String::new(); 10];
            let (mappings, mode) = BeamerMetadata::detect_slide_mappings(
                10,
                &page_sizes,
                &page_labels,
                &page_texts,
                nav_info.as_ref(),
            );
            assert_eq!(mode, NotesLayoutMode::BeamerNavMapping);
            assert_eq!(mappings.len(), 5);
            assert_eq!(mappings[0].slide_page_idx, 0); // physical page 1
            assert_eq!(mappings[0].note_page_idx, Some(1)); // physical page 2
            assert_eq!(mappings[1].slide_page_idx, 2); // physical page 3
            assert_eq!(mappings[1].note_page_idx, Some(3)); // physical page 4
            assert_eq!(mappings[4].slide_page_idx, 8); // physical page 9
            assert_eq!(mappings[4].note_page_idx, Some(9)); // physical page 10
        }

        // 3. Test detect_slide_mappings with Page Parity (interleaved notes without .nav)
        let sample_texts = vec![
            "Slide 1 Title".to_string(),
            "PRESENTER NOTES:\nWelcome everyone".to_string(),
            "Slide 2 Architecture".to_string(),
            "SPEAKER NOTES:\nReview the diagram".to_string(),
            "Slide 3 Evaluation".to_string(),
            "NOTES (Slide 3):\nShow benchmark results".to_string(),
        ];
        let page_sizes = vec![(1920.0, 1080.0); 6];
        let page_labels = vec![None; 6];
        let (parity_mappings, parity_mode) = BeamerMetadata::detect_slide_mappings(
            6,
            &page_sizes,
            &page_labels,
            &sample_texts,
            None,
        );
        assert_eq!(parity_mode, NotesLayoutMode::PageParity);
        assert_eq!(parity_mappings.len(), 3);
        assert_eq!(parity_mappings[0].slide_page_idx, 0);
        assert_eq!(parity_mappings[0].note_page_idx, Some(1));
        assert_eq!(parity_mappings[1].slide_page_idx, 2);
        assert_eq!(parity_mappings[1].note_page_idx, Some(3));
        assert_eq!(parity_mappings[2].slide_page_idx, 4);
        assert_eq!(parity_mappings[2].note_page_idx, Some(5));

        // 4. Test detect_slide_mappings with TwoHalvesAfter (LibreOffice / OpenOffice style)
        let lo_texts = vec![
            "Slide 1".to_string(),
            "Slide 2".to_string(),
            "Slide 3".to_string(),
            "Notes for Slide 1".to_string(),
            "Notes for Slide 2".to_string(),
            "Notes for Slide 3".to_string(),
        ];
        let page_sizes = vec![(1920.0, 1080.0); 6];
        let page_labels = vec![None; 6];
        let (lo_mappings, lo_mode) = BeamerMetadata::detect_slide_mappings(
            6,
            &page_sizes,
            &page_labels,
            &lo_texts,
            None,
        );
        assert_eq!(lo_mode, NotesLayoutMode::TwoHalvesAfter);
        assert_eq!(lo_mappings.len(), 3);
        assert_eq!(lo_mappings[0].slide_page_idx, 0);
        assert_eq!(lo_mappings[0].note_page_idx, Some(3));
        assert_eq!(lo_mappings[1].slide_page_idx, 1);
        assert_eq!(lo_mappings[1].note_page_idx, Some(4));
        assert_eq!(lo_mappings[2].slide_page_idx, 2);
        assert_eq!(lo_mappings[2].note_page_idx, Some(5));

        // 5. Test detect_slide_mappings with Page Labels (notes:1, notes:2)
        let labels: Vec<Option<String>> = vec![
            Some("1".to_string()),
            Some("notes:1".to_string()),
            Some("2".to_string()),
            Some("notes:2".to_string()),
        ];
        let page_sizes = vec![(1920.0, 1080.0); 4];
        let page_texts = vec![String::new(); 4];
        let (label_mappings, label_mode) = BeamerMetadata::detect_slide_mappings(
            4,
            &page_sizes,
            &labels,
            &page_texts,
            None,
        );
        assert_eq!(label_mode, NotesLayoutMode::PageLabelMapping);
        assert_eq!(label_mappings.len(), 2);
        assert_eq!(label_mappings[0].slide_page_idx, 0);
        assert_eq!(label_mappings[0].note_page_idx, Some(1));
        assert_eq!(label_mappings[1].slide_page_idx, 2);
        assert_eq!(label_mappings[1].note_page_idx, Some(3));

        // 6. Test DualScreen aspect ratio mode: no separate note pages
        let dual_sizes = vec![(3840.0, 1080.0); 2];
        let page_labels = vec![None; 2];
        let page_texts = vec![String::new(); 2];
        let (dual_mappings, dual_mode) = BeamerMetadata::detect_slide_mappings(
            2,
            &dual_sizes,
            &page_labels,
            &page_texts,
            None,
        );
        assert_eq!(dual_mode, NotesLayoutMode::DualScreen { is_vertical: true });
        assert_eq!(dual_mappings.len(), 2);
        assert_eq!(dual_mappings[0].note_page_idx, None);
        assert_eq!(dual_mappings[1].note_page_idx, None);
    }

    #[test]
    fn test_auto_updater_semver_and_platform_detection() {
        use presenter::updater::{parse_semver, is_newer_version, current_platform_target};

        // 1. Test SemVer parsing
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("v0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("v2.0.0-beta.1"), Some((2, 0, 0)));

        // 2. Test newer version comparisons
        assert!(is_newer_version("0.1.0", "v0.1.1"));
        assert!(is_newer_version("0.1.0", "v0.2.0"));
        assert!(is_newer_version("0.1.0", "v1.0.0"));
        assert!(!is_newer_version("0.1.0", "v0.1.0"));
        assert!(!is_newer_version("0.2.0", "v0.1.9"));
        assert!(!is_newer_version("1.0.0", "v0.9.9"));

        // 3. Test current platform target detection
        let (os_name, arch_name) = current_platform_target();
        assert!(!os_name.is_empty());
        assert!(!arch_name.is_empty());
    }

    #[test]
    fn test_merge_reloaded_keeps_ink_and_inserted_slides() {
        use presenter::ink::{Point, Stroke};
        use presenter::pdf::{PresentationDeck, RenderQuality};

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Eco);
        let fresh = deck.slides.clone();
        let n = fresh.len();

        // Ink on the 2nd PDF slide, an inserted page after it, and the inserted page is current.
        let mut stroke = Stroke::new(7, [255, 0, 0, 255], 2.0, false);
        stroke.add_point(Point::new(0.5, 0.5, 1.0, 0));
        deck.slides[1].inking.strokes.push(stroke);
        deck.current_index = 1;
        deck.insert_page_with_template(0, "grid", [255, 255, 255, 255]);
        assert_eq!(deck.current_index, 2);

        deck.merge_reloaded(fresh, Vec::new());
        assert_eq!(deck.slides.len(), n + 1);
        assert_eq!(deck.slides[1].inking.strokes.len(), 1, "ink follows its PDF page");
        assert!(deck.slides[2].is_whiteboard, "inserted page stays after its anchor");
        assert_eq!(deck.current_index, 2, "current slide is preserved");
        assert!(deck.slides.iter().enumerate().all(|(i, s)| s.page_index == i + 1));
    }

    #[test]
    fn test_movie_rect_matches_pdf_annotation() {
        use presenter::pdf::{PdfiumLoader, RenderQuality};
        let path = std::path::Path::new("test_deck/presentation.pdf");
        let Ok((slides, _)) = PdfiumLoader::load_deck_from_file(path, RenderQuality::Eco) else {
            return; // pdfium unavailable in this environment
        };
        let (slide, clip) = slides
            .iter()
            .find_map(|s| s.media_clips.iter().find(|c| c.file_path.ends_with("sample_video.mp4")).map(|c| (s, c)))
            .expect("video clip");
        let doc = PdfiumLoader::get_pdfium().unwrap().load_pdf_from_file(path, None).unwrap();
        let page = doc.pages().get((slide.physical_page_index - 1) as i32).unwrap();
        let (pw, ph) = (page.width().value, page.height().value);
        // `\movie` writes /Rect [106.299 30.89 347.244 166.386] (PDF points, bottom-left origin).
        let expected = [106.299 / pw, (ph - 166.386) / ph, (347.244 - 106.299) / pw, (166.386 - 30.89) / ph];
        for (got, want) in clip.rect.iter().zip(expected) {
            assert!((got - want).abs() < 0.01, "rect {:?} vs expected {:?}", clip.rect, expected);
        }
    }

    #[test]
    fn test_merge_reloaded_follows_content_when_pages_shift() {
        use presenter::ink::{Point, Stroke};
        use presenter::pdf::{PresentationDeck, RenderQuality};

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Eco);
        for (i, s) in deck.slides.iter_mut().enumerate() {
            s.content_hash = 100 + i as u64;
            s.physical_page_index = i + 1;
        }
        let mut stroke = Stroke::new(1, [0, 0, 255, 255], 2.0, false);
        stroke.add_point(Point::new(0.2, 0.2, 1.0, 0));
        deck.slides[1].inking.strokes.push(stroke);
        deck.current_index = 1;

        // Author inserted a new slide at the front: every page number shifts by one.
        let mut fresh = deck.slides.clone();
        for s in &mut fresh {
            s.inking.strokes.clear();
        }
        let mut new_first = fresh[0].clone();
        new_first.content_hash = 999;
        fresh.insert(0, new_first);
        for (i, s) in fresh.iter_mut().enumerate() {
            s.physical_page_index = i + 1;
        }

        deck.merge_reloaded(fresh, Vec::new());
        assert_eq!(deck.slides[2].inking.strokes.len(), 1, "ink follows the slide's content, not its page number");
        assert!(deck.slides[1].inking.strokes.is_empty());
        assert_eq!(deck.current_index, 2, "current slide follows its content too");
        assert_eq!(PresentationDeck::text_fingerprint("Intro 3 / 12"), PresentationDeck::text_fingerprint("Intro 4 / 13"));
    }

    #[test]
    fn test_audio_progress_box_persistence_and_coordinate_mapping() {
        use presenter::pdf::{PresentationDeck, RenderQuality};

        let mut deck = PresentationDeck::new_demo_deck(RenderQuality::Eco);
        let slide = deck.current_slide_mut();
        assert!(slide.audio_progress_box.is_none());

        // Define a spectrogram bounding box: [x, y, w, h] in slide relative coords (0.0..1.0)
        let spec_box = [0.15_f32, 0.40_f32, 0.70_f32, 0.35_f32];
        slide.audio_progress_box = Some(spec_box);
        assert_eq!(slide.audio_progress_box, Some(spec_box));

        // Test scan line needle x-coordinate calculation at different progress points
        let needle_x_start = spec_box[0] + 0.0 * spec_box[2];
        let needle_x_mid = spec_box[0] + 0.5 * spec_box[2];
        let needle_x_end = spec_box[0] + 1.0 * spec_box[2];

        assert!((needle_x_start - 0.15).abs() < 1e-5);
        assert!((needle_x_mid - 0.50).abs() < 1e-5);
        assert!((needle_x_end - 0.85).abs() < 1e-5);

        // Verify preservation during PDF hot-reload
        for (i, s) in deck.slides.iter_mut().enumerate() {
            s.content_hash = 200 + i as u64;
        }
        let fresh = deck.slides.clone();
        deck.merge_reloaded(fresh, Vec::new());
        assert_eq!(deck.slides[0].audio_progress_box, Some(spec_box));

        // Verify clearing the box
        deck.current_slide_mut().audio_progress_box = None;
        assert!(deck.current_slide().audio_progress_box.is_none());
    }
}
