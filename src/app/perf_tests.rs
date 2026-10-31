//! Headless UI checks use the real compiled Slint components and software renderer.
use crate::{PresenterWindow, SlideThumbnail};
use slint::platform::{
    Platform, WindowAdapter,
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
};
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

type UiEvent = Box<dyn FnOnce() + Send>;
struct TestProxy(std::sync::mpsc::Sender<UiEvent>);
impl slint::platform::EventLoopProxy for TestProxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        Ok(())
    }
    fn invoke_from_event_loop(&self, event: UiEvent) -> Result<(), slint::EventLoopError> {
        self.0
            .send(event)
            .map_err(|_| slint::EventLoopError::EventLoopTerminated)
    }
}
struct Headless {
    clock: Rc<Cell<Duration>>,
    events: std::sync::mpsc::Sender<UiEvent>,
}
impl Platform for Headless {
    fn new_event_loop_proxy(&self) -> Option<Box<dyn slint::platform::EventLoopProxy>> {
        Some(Box::new(TestProxy(self.events.clone())))
    }
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
    fn duration_since_start(&self) -> Duration {
        self.clock.get()
    }
}
fn image(pixels: &image::RgbaImage) -> slint::Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(pixels.width(), pixels.height());
    buffer.make_mut_bytes().copy_from_slice(pixels.as_raw());
    slint::Image::from_rgba8(buffer)
}
#[test]
fn headless_overview_and_activity_timers() {
    let clock = Rc::new(Cell::new(Duration::ZERO));
    let (event_tx, event_rx) = std::sync::mpsc::channel::<UiEvent>();
    slint::platform::set_platform(Box::new(Headless {
        clock: clock.clone(),
        events: event_tx,
    }))
    .unwrap();
    let base = image::RgbaImage::from_fn(320, 180, |x, y| {
        image::Rgba([(x % 255) as u8, (y % 255) as u8, 180, 255])
    });
    let presenter = PresenterWindow::new().unwrap();
    presenter
        .window()
        .set_size(slint::PhysicalSize::new(1280, 800));
    presenter.set_total_slides(10000);
    presenter.set_show_thumbnails(true);
    let shared_thumb = image(&base);
    presenter.set_slide_thumbnails(
        Rc::new(slint::VecModel::from(
            (1..=10000)
                .map(|i| SlideThumbnail {
                    index: i,
                    image: shared_thumb.clone(),
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    presenter.show().unwrap();
    let snapshot = presenter.window().take_snapshot().unwrap();
    // The overview must actually paint its thumbnail images, including very large decks.
    let colored = snapshot
        .as_slice()
        .iter()
        .filter(|p| p.b > 150 && p.r > 40 && p.g > 40)
        .count();
    assert!(
        colored > 10000,
        "overview did not paint thumbnails: {colored} colored pixels"
    );
    let dir = std::path::Path::new("target/perf-results");
    std::fs::create_dir_all(dir).unwrap();
    image::save_buffer(
        dir.join("overview.png"),
        snapshot.as_bytes(),
        snapshot.width(),
        snapshot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
    let jumped = Rc::new(Cell::new(0));
    let target = jumped.clone();
    presenter.on_jump_to_slide(move |index| target.set(index));
    let click = |x, y| {
        presenter
            .window()
            .dispatch_event(slint::platform::WindowEvent::PointerPressed {
                position: slint::LogicalPosition::new(x, y),
                button: slint::platform::PointerEventButton::Left,
            });
        presenter
            .window()
            .dispatch_event(slint::platform::WindowEvent::PointerReleased {
                position: slint::LogicalPosition::new(x, y),
                button: slint::platform::PointerEventButton::Left,
            });
    };
    click(500., 180.);
    assert_eq!(jumped.get(), 2);
    presenter
        .window()
        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(800., 400.),
            delta_x: 0.,
            delta_y: -5000.,
        });
    let scrolled = presenter.window().take_snapshot().unwrap();
    let colored = scrolled
        .as_slice()
        .iter()
        .filter(|p| p.b > 150 && p.r > 40 && p.g > 40)
        .count();
    assert!(colored > 10000, "scrolled overview lost its thumbnails");
    assert_ne!(snapshot.as_bytes(), scrolled.as_bytes());
    click(500., 240.);
    assert!(
        jumped.get() > 20,
        "virtualized cards lost global slide indices: {}",
        jumped.get()
    );
    // Exercise real pointer callbacks and the full overlay path, including the
    // software backend's coalescing fallback. No settings writer or updater runs.
    presenter.set_show_thumbnails(false);
    let state = Rc::new(RefCell::new(super::state::AppState::new()));
    state.borrow_mut().current_tool = crate::ActiveTool::Pen;
    state.borrow_mut().auto_shape_enabled = false;
    state.borrow_mut().scribble_to_erase = false;
    let audience = crate::AudienceWindow::new().unwrap();
    let notes = crate::NotesWindow::new().unwrap();
    let video = std::sync::Arc::new(std::sync::Mutex::new(
        crate::media::InPlaceVideoPlayer::new(),
    ));
    let ink = super::view::build_ink_overlay(&state, &presenter, &audience);
    let update = super::view::build_slide_view(&state, &presenter, &audience, &notes, &ink, &video);
    let context = super::context::AppContext {
        state: state.clone(),
        presenter_window: presenter.clone_strong(),
        audience_window: audience.clone_strong(),
        notes_window: notes,
        update_ink_overlay: ink,
        video_player: video,
        update_slide_view: update.clone(),
        audio_tick: Rc::new(RefCell::new(None)),
    };
    let _canvas = super::canvas::wire(&context);
    update();
    presenter.invoke_canvas_pointer_down(0.1, 0.4, 1.0);
    for i in 0..120 {
        presenter.invoke_canvas_pointer_move(0.1 + i as f32 / 240., 0.4, 1.0);
        if i % 4 == 3 {
            clock.set(clock.get() + Duration::from_millis(16));
            slint::platform::update_timers_and_animations();
        }
    }
    presenter.invoke_canvas_pointer_up();
    let pixels = presenter.get_ink_overlay_image().to_rgba8().unwrap();
    let st = state.borrow();
    assert_eq!(st.deck.current_slide().inking.strokes[0].points.len(), 121);
    let expected = crate::ink::InkingRasterizer::render_strokes(
        1600,
        900,
        &st.deck.current_slide().inking.strokes,
        None,
        None,
    );
    assert_eq!(pixels.as_bytes(), expected.as_raw());
    drop(st);
    // Keep navigating without letting the idle prewarmer run: eviction must still
    // bound display images, rather than waiting until key repeat finishes.
    let template = state.borrow().deck.slides[0].export_snapshot();
    state.borrow_mut().deck.slides = (0..100)
        .map(|i| {
            let mut slide = template.export_snapshot();
            let pixels = std::sync::Arc::new(crate::pdf::SlideImage::new_resident(
                image::RgbaImage::from_pixel(16, 9, image::Rgba([i, 30, 50, 255])),
            ));
            slide.audience_image = pixels.clone();
            slide.presenter_image = pixels;
            slide.inking = Default::default();
            slide
        })
        .collect();
    for index in 0..100 {
        state.borrow_mut().deck.current_index = index;
        update();
        assert!(
            super::gpu_images::full_cache_len() <= 12,
            "display cache grew during key repeat"
        );
    }
    let active = Rc::new(Cell::new(false));
    let ticks = Rc::new(Cell::new(0));
    let enabled = active.clone();
    let target = ticks.clone();
    let timer = super::scheduling::DemandTimer::new(
        Duration::from_millis(16),
        move || enabled.get(),
        move || target.set(target.get() + 1),
    );
    active.set(true);
    timer.wake();
    let timer_start = clock.get();
    clock.set(timer_start + Duration::from_millis(16));
    slint::platform::update_timers_and_animations();
    assert_eq!(ticks.get(), 1);
    active.set(false);
    clock.set(timer_start + Duration::from_millis(32));
    slint::platform::update_timers_and_animations();
    clock.set(timer_start + Duration::from_millis(1000));
    slint::platform::update_timers_and_animations();
    assert_eq!(ticks.get(), 1);
    let draws = Rc::new(RefCell::new(0));
    let target = draws.clone();
    let render = super::scheduling::coalesce_frame(Rc::new(move || *target.borrow_mut() += 1));
    for _ in 0..1000 {
        render();
    }
    clock.set(timer_start + Duration::from_millis(1016));
    slint::platform::update_timers_and_animations();
    assert_eq!(*draws.borrow(), 1);
    check_controls(&context);
    // Exercise the real asynchronous open path, not just metadata parsing. A
    // converted companion avoids automating the user's running Office session.
    let _files = super::files::wire(&context);
    let temp = tempfile::tempdir().unwrap();
    let pptx = temp.path().join("sample_deck.pptx");
    std::fs::copy("test_deck/sample_deck.pptx", &pptx).unwrap();
    let pdf = pptx.with_extension("pdf");
    {
        use pdfium_render::prelude::*;
        let pdfium = crate::pdf::PdfiumLoader::get_pdfium().unwrap();
        let mut converted = pdfium.create_new_pdf().unwrap();
        for _ in 0..3 {
            converted
                .pages_mut()
                .create_page_at_end(PdfPagePaperSize::Custom(
                    PdfPoints::new(960.0),
                    PdfPoints::new(540.0),
                ))
                .unwrap();
        }
        converted.save_to_file(&pdf).unwrap();
    }
    state.borrow_mut().deck.quality = crate::pdf::RenderQuality::Balanced;
    state
        .borrow_mut()
        .start_deck_load(pptx.clone(), super::state::DeckLoadKind::Open);
    presenter.set_deck_loading(true);
    super::ui::show_toast(&presenter, "Opening sample_deck.pptx…", false);
    // An opening status must remain visible beyond the old four-second toast.
    clock.set(clock.get() + Duration::from_secs(5));
    slint::platform::update_timers_and_animations();
    assert!(presenter.get_deck_loading());
    assert!(
        !presenter.get_toast_visible(),
        "the transient toast timer should have expired"
    );
    let busy = presenter.window().take_snapshot().unwrap();
    let green_status_pixels = busy
        .as_slice()
        .iter()
        .skip((busy.height().saturating_sub(80) * busy.width()) as usize)
        .filter(|pixel| pixel.g > 140 && pixel.r < 80 && pixel.b < 140)
        .count();
    assert!(
        green_status_pixels > 20,
        "loading status disappeared before import completed"
    );
    let start = std::time::Instant::now();
    while state.borrow().deck_load.is_some() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "PPTX import never reached the UI"
        );
        while let Ok(event) = event_rx.try_recv() {
            event();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(state.borrow().deck.file_path.as_ref(), Some(&pptx));
    assert_eq!(state.borrow().deck.slides.len(), 3);
    assert_eq!(
        state.borrow().deck.slides[0].title,
        "Quantum Computing Overview"
    );
    assert!(!presenter.get_deck_loading());
    assert_eq!(presenter.get_total_slides(), 3);
    let size = presenter.get_current_slide_image().size();
    // PDFium uses floating-point page widths; the existing DPI conversion can
    // truncate 3999.999... to 3999. Both widths are full-resolution, not a thumbnail.
    assert!((3999..=4000).contains(&size.width));
    assert_eq!(size.height, 2250);
    presenter.hide().unwrap();
}

fn check_controls(ctx: &super::context::AppContext) {
    use crate::ActiveTool;
    use slint::platform::{Key, WindowEvent};
    let p = &ctx.presenter_window;
    let state = &ctx.state;
    let _tools = super::tools::wire(ctx);
    let _navigation = super::navigation::wire(ctx);
    let _pages = super::pages::wire(ctx);
    let _windows = super::windows::wire(ctx);
    let _media = super::media_controls::wire(ctx);
    drop(super::session::wire(ctx)); // Register controls without running autosave ticks.
    state.borrow().push_settings_to_ui(p);
    let key = |text: &str| {
        assert!(
            p.invoke_handle_key(text.into(), false, false, false),
            "unhandled {text:?}"
        )
    };
    p.set_show_thumbnails(false);
    // Dispatch through Slint's focus/key routing, not just the shortcut function.
    p.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Tab.into(),
    });
    p.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Tab.into(),
    });
    assert!(
        p.get_show_thumbnails(),
        "Tab failed through real keyboard routing"
    );
    key("t");
    assert!(!p.get_show_thumbnails());
    p.set_notes_drawer_open(true);
    key(&slint::SharedString::from(Key::Escape));
    assert!(!p.get_notes_drawer_open());
    for (shortcut, tool) in [
        ("p", ActiveTool::Pen),
        ("h", ActiveTool::Highlighter),
        ("e", ActiveTool::Eraser),
        ("l", ActiveTool::Laser),
        ("z", ActiveTool::Select),
    ] {
        key(shortcut);
        assert_eq!(state.borrow().current_tool, tool);
        assert_eq!(p.get_current_tool(), tool);
    }
    p.invoke_select_pen_size(1.9, "1.9mm".into());
    p.invoke_select_highlighter_size(6.2, "6.2mm".into());
    key("p");
    assert!((state.borrow().stroke_width - 1.9 * 4.2).abs() < 0.001);
    key("h");
    assert!((state.borrow().stroke_width - 6.2 * 4.2).abs() < 0.001);
    assert!((state.borrow().capture_settings().stroke_width.unwrap() - 1.9 * 4.2).abs() < 0.001);
    for slot in 0..5 {
        key(&(slot + 1).to_string());
        assert_eq!(state.borrow().active_color_slot, slot);
    }
    let original_slot1 = state.borrow().color_slot_1;
    p.invoke_color_wheel_touched(100., 60.);
    p.invoke_color_brightness_touched(0.5);
    assert_eq!(state.borrow().color_slot_1, original_slot1);
    assert_eq!(state.borrow().color_slot_5, state.borrow().active_color);
    assert!(!p.invoke_handle_key("p".into(), true, false, false));
    assert_eq!(state.borrow().current_tool, ActiveTool::Highlighter);
    for (name, expected) in [("b", 1), ("b", 0), ("w", 2), ("w", 0)] {
        key(name);
        assert_eq!(state.borrow().blank_mode, expected);
    }
    let index = state.borrow().deck.current_index;
    key("Left");
    assert_eq!(state.borrow().deck.current_index, index - 1);
    key("Right");
    assert_eq!(state.borrow().deck.current_index, index);
    key("k");
    key("j");
    for code in [
        Key::LeftArrow,
        Key::UpArrow,
        Key::PageUp,
        Key::Backspace,
        Key::RightArrow,
        Key::DownArrow,
        Key::PageDown,
        Key::Return,
    ] {
        key(&slint::SharedString::from(code));
    }
    key(" ");
    key("o");
    assert!(p.get_show_toc_drawer());
    key(&slint::SharedString::from(Key::Escape));
    assert!(!p.get_show_toc_drawer());
    key("s");
    key("s");
    key("m");
    assert!(!state.borrow().media_playing, "playing state without media");
    assert!(!p.get_media_playing());
    state.borrow_mut().media_playing = true;
    p.invoke_toggle_audience_window();
    assert!(p.get_audience_window_open());
    assert!(state.borrow().media_playing, "popout interrupted playback");
    ctx.audience_window
        .window()
        .dispatch_event(WindowEvent::CloseRequested);
    assert!(!p.get_audience_window_open());
    p.invoke_toggle_audience_window();
    assert!(p.get_audience_window_open());
    p.invoke_toggle_audience_window();
    assert!(!p.get_audience_window_open());
    p.invoke_toggle_notes_window();
    assert!(p.get_notes_window_open());
    ctx.notes_window
        .window()
        .dispatch_event(WindowEvent::CloseRequested);
    assert!(!p.get_notes_window_open());
    state.borrow_mut().media_playing = false;
    p.invoke_toggle_auto_shape(false);
    assert!(!state.borrow().auto_shape_enabled);
    p.invoke_toggle_scribble_to_erase(false);
    assert!(!state.borrow().scribble_to_erase);
    p.invoke_change_hold_delay(0.8);
    assert_eq!(state.borrow().hold_delay_ms, 800);
    p.invoke_select_highlighter_mode(1);
    assert_eq!(state.borrow().highlighter_mode, 1);
    p.invoke_select_highlighter_mode(0);
    p.invoke_toggle_highlighter_straight_lines(true);
    assert!(state.borrow().highlighter_straight_lines);
    p.invoke_select_eraser_size(1, 0.02);
    assert_eq!(state.borrow().eraser_radius, 0.02);
    p.invoke_toggle_erase_whole_line(true);
    assert!(state.borrow().erase_whole_line);
    p.invoke_toggle_erase_highlight_only(true);
    assert!(state.borrow().erase_highlight_only);
    p.invoke_toggle_erase_shapes(false);
    assert!(!state.borrow().erase_shapes);
    p.invoke_toggle_erase_tapes(true);
    assert!(state.borrow().erase_tapes);
    p.invoke_reset_eraser_settings();
    assert!(!state.borrow().erase_whole_line);
    p.invoke_select_laser_color(1, slint::Color::from_rgb_u8(1, 2, 3));
    assert_eq!(state.borrow().laser_color_rgba, [0, 85, 255, 255]);
    p.invoke_toggle_laser_draw_lines(true);
    assert!(state.borrow().laser_draw_lines);
    p.invoke_change_laser_fade_duration(0.8);
    assert_eq!(state.borrow().laser_fade_duration_s, 0.8);
    p.invoke_set_media_track_fps_mode(2);
    assert_eq!(state.borrow().media_track_fps_mode, 2);
    p.invoke_set_media_track_show_audience_box(true);
    assert!(ctx.audience_window.get_media_track_show_box());
    p.invoke_set_media_track_needle_width(3.0);
    assert_eq!(ctx.audience_window.get_media_track_needle_width(), 3.0);
    p.invoke_set_media_track_needle_glow(false);
    assert!(!ctx.audience_window.get_media_track_needle_glow());
    p.invoke_set_media_track_loop(true);
    assert!(state.borrow().media_track_loop);
    p.invoke_toggle_timer();
    p.invoke_reset_timer();
    p.set_timer_duration_min(20);
    p.set_timer_warn_min(3);
    p.invoke_timer_options_changed();
    assert_eq!(state.borrow().timer_duration_min, 20);
    assert_eq!(state.borrow().timer_warn_min, 3);
    p.invoke_resize_notes_font(20);
    assert_eq!(p.get_notes_font_size(), 20);
    assert_eq!(ctx.notes_window.get_notes_font_size(), 20);
    let saved = state.borrow().capture_settings();
    assert_eq!(saved.notes_font_size, Some(20));
    let mut restored = crate::app::state::AppState::new();
    restored.apply_settings(&saved);
    assert_eq!(restored.notes_font_size, 20);
    ctx.notes_window.invoke_resize_notes_font(100);
    assert_eq!(p.get_notes_font_size(), 48);
    ctx.notes_window.invoke_resize_notes_font(1);
    assert_eq!(p.get_notes_font_size(), 8);
    p.invoke_resize_notes_font(14);
    p.invoke_toggle_preview_window(0);
    assert!(p.get_next_detached());
    assert!(!p.get_beamer_detached());
    p.invoke_toggle_preview_window(1);
    assert!(p.get_beamer_detached());
    p.invoke_toggle_preview_window(0);
    p.invoke_toggle_preview_window(1);
    assert!(!p.get_next_detached());
    assert!(!p.get_beamer_detached());
    let slide = p.get_current_slide();
    p.invoke_update_slide_notes(slide, "edited presenter notes".into());
    assert_eq!(ctx.notes_window.get_notes_body(), "edited presenter notes");
    ctx.notes_window
        .invoke_update_slide_notes(slide, "edited detached notes".into());
    assert_eq!(p.get_notes_text(), "edited detached notes");
    p.set_show_thumbnails(false);
    p.set_notes_drawer_open(true);
    let snapshot = p.window().take_snapshot().unwrap();
    // Find both divider grips in the rendered sidebar and drag them through
    // Slint's real pointer routing. Each drag must leave the third panel alone.
    let grips: Vec<_> = (0..snapshot.height())
        .filter_map(|y| {
            let xs: Vec<_> = (snapshot.width() * 3 / 4..snapshot.width())
                .filter(|&x| {
                    let pixel = snapshot.as_slice()[(y * snapshot.width() + x) as usize];
                    pixel.r == 82 && pixel.g == 82 && pixel.b == 91
                })
                .collect();
            (xs.len() >= 40 && xs.len() <= 44).then(|| (xs[xs.len() / 2], y))
        })
        .collect();
    let mut rows = Vec::new();
    for (x, y) in grips {
        if rows.last().is_none_or(|&(_, previous)| y > previous + 4) {
            rows.push((x, y));
        }
    }
    assert_eq!(rows.len(), 2, "expected two independent resize handles");
    let drag = |x: u32, y: u32, dy: f32| {
        let position = slint::LogicalPosition::new(x as f32, y as f32);
        p.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        p.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        let position = slint::LogicalPosition::new(x as f32, y as f32 + dy);
        p.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        p.window().take_snapshot().unwrap();
        p.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
    };
    let text_weight = p.get_text_panel_weight();
    drag(rows[0].0, rows[0].1, -40.0);
    assert!(p.get_next_panel_weight() < 0.30);
    assert!(p.get_beamer_panel_weight() > 0.30);
    assert_eq!(p.get_text_panel_weight(), text_weight);
    let next_weight = p.get_next_panel_weight();
    drag(rows[1].0, rows[1].1, -40.0);
    assert!(p.get_text_panel_weight() > text_weight);
    assert_eq!(p.get_next_panel_weight(), next_weight);
    // Exercise the menu via pointer events, then render the open menu too.
    let position = slint::LogicalPosition::new(1229.0, 72.0);
    p.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    p.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    assert!(p.get_show_panel_menu());
    p.window().set_size(slint::PhysicalSize::new(1281, 800));
    p.window().set_size(slint::PhysicalSize::new(1280, 800));
    let snapshot = p.window().take_snapshot().unwrap();
    image::save_buffer(
        "target/perf-results/presenter-panels.png",
        snapshot.as_bytes(),
        snapshot.width(),
        snapshot.height(),
        image::ColorType::Rgba8,
    )
    .unwrap();
    p.set_notes_drawer_open(false);

    let count = state.borrow().deck.total_slides();
    p.invoke_insert_page_configured(0, "dotted".into(), slint::Color::from_rgb_u8(255, 255, 255));
    assert_eq!(state.borrow().deck.total_slides(), count + 1);
    let page = p.get_current_slide();
    p.invoke_duplicate_slide(page);
    assert_eq!(state.borrow().deck.total_slides(), count + 2);
    p.invoke_rotate_slide(p.get_current_slide());
    p.invoke_delete_slide(p.get_current_slide());
    assert_eq!(state.borrow().deck.total_slides(), count + 1);
    key("n");
    assert_eq!(state.borrow().deck.total_slides(), count + 2);
    key("p");
    p.invoke_canvas_pointer_down(0.1, 0.4, 1.0);
    p.invoke_canvas_pointer_move(0.4, 0.4, 1.0);
    p.invoke_canvas_pointer_up();
    assert_eq!(state.borrow().deck.current_slide().inking.strokes.len(), 1);
    assert!(p.invoke_handle_key("z".into(), true, false, false));
    assert!(
        state
            .borrow()
            .deck
            .current_slide()
            .inking
            .strokes
            .is_empty()
    );
    assert!(p.invoke_handle_key("Z".into(), false, true, true));
    assert_eq!(state.borrow().deck.current_slide().inking.strokes.len(), 1);
}
