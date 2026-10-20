use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use chrono::Local;
use image::RgbaImage;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, SharedString};

mod ink;
mod media;
mod pdf;
mod timer;
mod updater;

use ink::{InkingRasterizer, LaserTrail, Point, Stroke};
use media::{InPlaceVideoPlayer, probe_video_duration};
use pdf::{PdfExporter, PdfLinkInfo, PdfLinkTarget, PresentationDeck, RenderQuality};
use timer::PresentationTimer;
use updater::UpdateInfo;

slint::include_modules!();

pub fn rgba_to_slint_image(img: &RgbaImage) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(img.width(), img.height());
    buffer.make_mut_bytes().copy_from_slice(img.as_raw());
    Image::from_rgba8(buffer)
}

/// Returns the cached slint image, converting (and caching) it on first use.
fn cached_slint_image(cell: &RefCell<Option<Image>>, img: &RgbaImage) -> Image {
    if let Some(i) = cell.borrow().as_ref() {
        return i.clone();
    }
    let i = rgba_to_slint_image(img);
    *cell.borrow_mut() = Some(i.clone());
    i
}

pub fn update_slide_thumbnails_model(deck: &PresentationDeck, presenter: &PresenterWindow) {
    let thumbs: Vec<SlideThumbnail> = deck.slides.iter().enumerate().map(|(i, slide)| {
        let aspect = slide.audience_image.height() as f32 / slide.audience_image.width() as f32;
        let target_w = 320u32;
        let target_h = (target_w as f32 * aspect).max(1.0) as u32;
        let thumb_img = image::imageops::thumbnail(&slide.audience_image, target_w, target_h);
        SlideThumbnail {
            index: (i + 1) as i32,
            image: rgba_to_slint_image(&thumb_img),
        }
    }).collect();
    presenter.set_slide_thumbnails(std::rc::Rc::new(slint::VecModel::from(thumbs)).into());
}

struct AppState {
    deck: PresentationDeck,
    timer: PresentationTimer,
    active_stroke: Option<Stroke>,
    laser_active: bool,
    active_color: [u8; 4],
    stroke_width: f32,
    current_tool: ActiveTool,
    stroke_counter: u64,

    // Pen & Draw-and-hold settings
    auto_shape_enabled: bool,
    scribble_to_erase: bool,
    hold_delay_ms: u64,
    enhanced_sensitivity: bool,
    draw_on_hover: bool,
    pen_type: i32,

    // Highlighter settings
    highlighter_mode: i32, // 0: Highlighter, 1: Tape
    highlighter_straight_lines: bool,
    highlighter_size_mm: String,

    // Eraser settings
    eraser_size_index: i32,
    eraser_radius: f32,
    erase_whole_line: bool,
    erase_highlight_only: bool,
    auto_select_previous_tool: bool,
    erase_shapes: bool,
    erase_tapes: bool,
    previous_tool: ActiveTool,

    // Laser Pointer & Draw Lines settings
    laser_color_index: i32,
    laser_color_rgba: [u8; 4],
    laser_draw_lines: bool,
    laser_fade_duration_s: f32,
    laser_trail: Option<LaserTrail>,
    last_laser_update: std::time::Instant,

    // High-performance base inking overlay cache
    cached_ink_overlay: Option<RgbaImage>,
    cached_ink_slide_idx: usize,
    cached_ink_stroke_count: usize,

    // Color Slots & Custom Palette
    active_color_slot: i32,
    color_slot_1: [u8; 4],
    color_slot_2: [u8; 4],
    color_slot_3: [u8; 4],
    color_slot_4: [u8; 4],
    color_slot_5: [u8; 4],
    custom_palette: Vec<[u8; 4]>,

    // Slide Media Player Toolbar state
    media_playing: bool,
    media_progress: f32,
    media_duration_secs: f32,
    media_current_time_secs: f32,
    media_speed: f32,
    media_speech_active: bool,

    // In-flight gesture & hold detection
    pointer_is_down: bool,
    last_pointer_move_time: std::time::Instant,
    hold_anchor_pos: (f32, f32),
    is_shape_snapped: bool,
    raw_stroke_backup: Vec<Point>,

    // PDF Interactive Links Press & Hold State
    active_link_target: Option<PdfLinkInfo>,
    link_press_start: Option<std::time::Instant>,
    link_triggered: bool,

    // Presentation Mode & Display Swapping
    blank_mode: i32, // 0: Normal, 1: Blackout, 2: Whiteout
    is_screen_swapped: bool,
    is_end_of_show: bool,
}

pub fn open_external_url(url: &str) {
    let clean = url.trim();
    let url_to_open = if clean.starts_with("www.") {
        format!("https://{}", clean)
    } else {
        clean.to_string()
    };

    println!("Opening external URL: {}", url_to_open);
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&url_to_open).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd").args(["/C", "start", "", &url_to_open]).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url_to_open).spawn();
    }
}

impl AppState {
    fn format_media_time(&self) -> String {
        let cur = self.media_current_time_secs as u32;
        let tot = self.media_duration_secs as u32;
        format!("{:02}:{:02} / {:02}:{:02}", cur / 60, cur % 60, tot / 60, tot % 60)
    }

    fn format_media_speed(&self) -> String {
        if (self.media_speed - 1.0).abs() < 0.01 {
            "1.0x".to_string()
        } else if (self.media_speed - 1.25).abs() < 0.01 {
            "1.25x".to_string()
        } else if (self.media_speed - 1.5).abs() < 0.01 {
            "1.5x".to_string()
        } else if (self.media_speed - 2.0).abs() < 0.01 {
            "2.0x".to_string()
        } else if (self.media_speed - 0.75).abs() < 0.01 {
            "0.75x".to_string()
        } else {
            format!("{:.1}x", self.media_speed)
        }
    }

    fn new() -> Self {
        Self {
            deck: PresentationDeck::new_demo_deck(RenderQuality::Balanced),
            timer: PresentationTimer::new(45),
            active_stroke: None,
            laser_active: false,
            active_color: [254, 240, 78, 255], // Yellow #fef04e
            stroke_width: 3.5,
            current_tool: ActiveTool::Pen,
            stroke_counter: 0,

            // Media Player defaults
            media_playing: false,
            media_progress: 0.0,
            media_duration_secs: 90.0,
            media_current_time_secs: 0.0,
            media_speed: 1.0,
            media_speech_active: true,

            blank_mode: 0,
            is_screen_swapped: false,
            is_end_of_show: false,

            auto_shape_enabled: true,
            scribble_to_erase: true,
            hold_delay_ms: 500,
            enhanced_sensitivity: true,
            draw_on_hover: true,
            pen_type: 1,

            highlighter_mode: 0,
            highlighter_straight_lines: false,
            highlighter_size_mm: "4.6mm".to_string(),

            eraser_size_index: 2,
            eraser_radius: 0.04,
            erase_whole_line: false,
            erase_highlight_only: false,
            auto_select_previous_tool: false,
            erase_shapes: true,
            erase_tapes: false,
            previous_tool: ActiveTool::Pen,

            laser_color_index: 0,
            laser_color_rgba: [254, 240, 78, 255], // Initialized to elected color (Yellow #fef04e)
            laser_draw_lines: true,
            laser_fade_duration_s: 1.8,
            laser_trail: None,
            last_laser_update: std::time::Instant::now(),
            cached_ink_overlay: None,
            cached_ink_slide_idx: usize::MAX,
            cached_ink_stroke_count: usize::MAX,

            active_color_slot: 0,
            color_slot_1: [254, 240, 78, 255], // Yellow #fef04e
            color_slot_2: [93, 166, 56, 255],  // Green #5da638
            color_slot_3: [94, 53, 247, 255],  // Purple #5e35f7
            color_slot_4: [239, 68, 68, 255],  // Coral Red #ef4444
            color_slot_5: [0, 197, 255, 255],  // Sky Blue #00c5ff
            custom_palette: vec![
                [254, 240, 78, 255], // Yellow #fef04e
                [93, 166, 56, 255],  // Green #5da638
                [94, 53, 247, 255],  // Purple #5e35f7
                [239, 68, 68, 255],  // Coral Red #ef4444
                [0, 197, 255, 255],  // Sky Blue #00c5ff
            ],

            pointer_is_down: false,
            last_pointer_move_time: std::time::Instant::now(),
            hold_anchor_pos: (0.0, 0.0),
            is_shape_snapped: false,
            raw_stroke_backup: Vec::new(),

            active_link_target: None,
            link_press_start: None,
            link_triggered: false,
        }
    }
}

#[cfg(target_os = "macos")]
fn setup_macos_app_icon() {
    use std::ffi::{c_char, c_void};

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    let icon_bytes = include_bytes!("../ui/icons/app_icon.png");

    unsafe {
        let cls_nsapp = objc_getClass(b"NSApplication\0".as_ptr() as *const c_char);
        let sel_shared_app = sel_registerName(b"sharedApplication\0".as_ptr() as *const c_char);
        let get_shared_app: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let app = get_shared_app(cls_nsapp, sel_shared_app);

        let cls_nsdata = objc_getClass(b"NSData\0".as_ptr() as *const c_char);
        let sel_data_with_bytes = sel_registerName(b"dataWithBytes:length:\0".as_ptr() as *const c_char);
        let make_data: unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, usize) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let data = make_data(cls_nsdata, sel_data_with_bytes, icon_bytes.as_ptr(), icon_bytes.len());

        let cls_nsimage = objc_getClass(b"NSImage\0".as_ptr() as *const c_char);
        let sel_alloc = sel_registerName(b"alloc\0".as_ptr() as *const c_char);
        let alloc_img: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let img_alloc = alloc_img(cls_nsimage, sel_alloc);

        let sel_init_with_data = sel_registerName(b"initWithData:\0".as_ptr() as *const c_char);
        let init_img: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let image = init_img(img_alloc, sel_init_with_data, data);

        if !image.is_null() && !app.is_null() {
            let sel_set_icon = sel_registerName(b"setApplicationIconImage:\0".as_ptr() as *const c_char);
            let set_icon: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            set_icon(app, sel_set_icon, image);
            println!("macOS Dock icon successfully assigned to Presenter app icon.");
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing Beamer LaTeX PDF Presenter...");

    let args: Vec<String> = std::env::args().collect();
    let mut initial_deck = PresentationDeck::new_demo_deck(RenderQuality::Balanced);

    if args.len() > 1 && !args[1].starts_with('-') {
        let path = std::path::Path::new(&args[1]);
        if path.exists() {
            println!("Loading presentation from CLI argument: {}", path.display());
            match pdf::PdfiumLoader::load_deck_from_file(path, RenderQuality::Balanced) {
                Ok((slides, toc)) => {
                    println!("Loaded {} slides and {} TOC entries from {}", slides.len(), toc.len(), path.display());
                    initial_deck.slides = slides;
                    initial_deck.toc = toc;
                    initial_deck.current_index = 0;
                    initial_deck.file_path = Some(path.to_path_buf());
                }
                Err(e) => {
                    eprintln!("Notice: {}. Using interactive demo deck.", e);
                }
            }
        }
    }

    let mut initial_state = AppState::new();
    initial_state.deck = initial_deck;
    let state = Rc::new(RefCell::new(initial_state));

    // Create main Presenter Window
    let presenter_window = PresenterWindow::new()?;

    // Create Audience Window (can be shown on external monitor / projector)
    let audience_window = AudienceWindow::new()?;

    // Create Detached Notes Window (can be shown on 3rd screen or tablet)
    let notes_window = NotesWindow::new()?;

    // Dynamic macOS Dock Icon Setup
    #[cfg(target_os = "macos")]
    setup_macos_app_icon();

    // Helper to update only the inking overlay (called on draw, erase, undo, redo)
    let update_ink_overlay = {
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        let state = state.clone();

        Rc::new(move || {
            let mut st = state.borrow_mut();
            let current_idx = st.deck.current_index;
            let current_slide = &st.deck.slides[current_idx];
            let aspect = current_slide.aspect_ratio.max(0.5f32);
            let overlay_w: u32 = 1600;
            let overlay_h: u32 = (1600.0f32 / aspect).round() as u32;

            let stroke_count = current_slide.inking.strokes.len();
            if stroke_count == 0 && st.active_stroke.is_none() && st.laser_trail.is_none() {
                // Nothing to draw: use a tiny transparent image instead of a full-size overlay.
                st.cached_ink_overlay = None;
                drop(st);
                let empty = rgba_to_slint_image(&RgbaImage::new(1, 1));
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_ink_overlay_image(empty.clone());
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_ink_image(empty);
                }
                return;
            }
            let cache_valid = st.cached_ink_overlay.is_some()
                && st.cached_ink_slide_idx == current_idx
                && st.cached_ink_stroke_count == stroke_count;

            if !cache_valid {
                let fresh_base = InkingRasterizer::render_strokes(
                    overlay_w,
                    overlay_h,
                    &current_slide.inking.strokes,
                    None,
                    None,
                );
                st.cached_ink_overlay = Some(fresh_base);
                st.cached_ink_slide_idx = current_idx;
                st.cached_ink_stroke_count = stroke_count;
            }

            let base_img = st.cached_ink_overlay.as_ref().unwrap();
            let ink_img = if st.active_stroke.is_some() || st.laser_trail.is_some() {
                let mut composite = base_img.clone();
                if let Some(active) = st.active_stroke.as_ref() {
                    InkingRasterizer::draw_stroke(&mut composite, active);
                }
                if let Some(laser) = st.laser_trail.as_ref() {
                    InkingRasterizer::draw_laser_trail(&mut composite, laser);
                }
                composite
            } else {
                base_img.clone()
            };
            drop(st);

            let slint_ink = rgba_to_slint_image(&ink_img);

            if let Some(p) = presenter_handle.upgrade() {
                p.set_ink_overlay_image(slint_ink.clone());
            }
            if let Some(a) = audience_handle.upgrade() {
                a.set_ink_image(slint_ink);
            }
        })
    };

    // In-place Video Player Engine (Decodes frames directly into Slint Image without external popup windows)
    let video_player = std::sync::Arc::new(std::sync::Mutex::new(InPlaceVideoPlayer::new()));

    // Helper to update slide background, metadata, notes, and triggers ink update
    let update_slide_view = {
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        let notes_handle = notes_window.as_weak();
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let video_player = video_player.clone();

        Rc::new(move || {
            video_player.lock().unwrap().stop();
            let mut st = state.borrow_mut();
            st.cached_ink_overlay = None;
            st.media_playing = false;
            st.media_progress = 0.0;
            st.media_current_time_secs = 0.0;
            st.active_link_target = None;
            st.link_press_start = None;
            st.link_triggered = false;

            let current_idx = st.deck.current_index;
            let total = st.deck.total_slides();
            let current_slide = &st.deck.slides[current_idx];

            let slint_current = cached_slint_image(&current_slide.gpu_cache.presenter, &current_slide.presenter_image);
            let aud_slide = cached_slint_image(&current_slide.gpu_cache.audience, &current_slide.audience_image);

            let (disp_presenter, disp_audience) = if st.is_screen_swapped {
                (aud_slide.clone(), slint_current.clone())
            } else {
                (slint_current.clone(), aud_slide.clone())
            };

            let cur_frame = current_slide.frame_index;
            let tot_frames = current_slide.total_frames;
            let over_step = current_slide.overlay_step;
            let over_tot = current_slide.overlay_total;
            let frame_info = if tot_frames > 0 && over_tot > 1 {
                format!("Frame {}/{} • Step {}/{}", cur_frame, tot_frames, over_step, over_tot)
            } else if tot_frames > 0 {
                format!("Frame {}/{}", cur_frame, tot_frames)
            } else {
                String::new()
            };

            let toc_models: Vec<TocItemModel> = st.deck.toc.iter().map(|entry| {
                let is_act = (entry.page_index == current_idx + 1)
                    || (entry.frame_index == cur_frame && entry.page_index <= current_idx + 1);
                TocItemModel {
                    title: SharedString::from(&entry.title),
                    page_index: entry.page_index as i32,
                    level: entry.level as i32,
                    frame_index: entry.frame_index as i32,
                    is_active: is_act,
                }
            }).collect();
            let toc_model_rc = Rc::new(slint::VecModel::from(toc_models));

            let next_img = if current_idx + 1 < total {
                let nx = &st.deck.slides[current_idx + 1];
                cached_slint_image(&nx.gpu_cache.presenter, &nx.presenter_image)
            } else {
                let (w, h) = (1600, 900);
                let mut black_img = RgbaImage::new(w, h);
                for p in black_img.pixels_mut() {
                    *p = image::Rgba([15, 15, 18, 255]);
                }
                rgba_to_slint_image(&black_img)
            };

            let has_media = !current_slide.media_clips.is_empty();
            let first_clip = current_slide.media_clips.first().cloned();
            let slide_aspect = current_slide.aspect_ratio;
            let is_dual_screen = current_slide.is_dual_screen;
            let slide_notes = current_slide.notes.clone();
            let (has_notes_img, slint_notes_img) = if let Some(notes_img) = &current_slide.notes_image {
                (true, cached_slint_image(&current_slide.gpu_cache.notes, notes_img))
            } else {
                (false, slint::Image::default())
            };

            let mut media_rect = [0.15f32, 0.26f32, 0.70f32, 0.50f32];
            let media_label = if let Some(first) = first_clip {
                media_rect = first.rect;
                if first.is_resolved {
                    st.media_duration_secs = probe_video_duration(&first.file_path);
                    first.display_path
                } else {
                    format!("{} (Not Found)", first.display_path)
                }
            } else {
                String::new()
            };

            let doc_title = if let Some(path) = &st.deck.file_path {
                path.file_name().unwrap_or_default().to_string_lossy().to_string()
            } else {
                "Interactive Beamer Presentation.pdf".to_string()
            };

            // Update Presenter Window
            if let Some(p) = presenter_handle.upgrade() {
                p.set_slide_aspect(slide_aspect);
                p.set_document_title(SharedString::from(&doc_title));
                p.set_current_slide((current_idx + 1) as i32);
                p.set_total_slides(total as i32);
                p.set_current_slide_image(disp_presenter.clone());
                p.set_next_slide_image(next_img);
                p.set_has_notes_image(has_notes_img);
                p.set_is_dual_screen(is_dual_screen);
                p.set_notes_slide_image(slint_notes_img.clone());
                p.set_notes_text(SharedString::from(&slide_notes));
                p.set_render_quality(SharedString::from(st.deck.quality.label()));
                p.set_current_has_media(has_media);
                p.set_media_status_text(SharedString::from(&media_label));
                p.set_media_playing(false);
                p.set_media_video_visible(false);
                p.set_media_rect_x(media_rect[0]);
                p.set_media_rect_y(media_rect[1]);
                p.set_media_rect_w(media_rect[2]);
                p.set_media_rect_h(media_rect[3]);
                p.set_media_progress(0.0);
                p.set_media_time_text(SharedString::from(&st.format_media_time()));
                p.set_media_speed_text(SharedString::from(&st.format_media_speed()));
                p.set_media_speech_active(st.media_speech_active);

                p.set_blank_mode(st.blank_mode);
                p.set_is_end_of_show(st.is_end_of_show);
                p.set_is_screen_swapped(st.is_screen_swapped);
                p.set_laser_fade_duration_s(st.laser_fade_duration_s);
                p.set_laser_fade_duration_text(SharedString::from(format!("{:.1}s", st.laser_fade_duration_s)));
                p.set_current_frame(cur_frame as i32);
                p.set_total_frames(tot_frames as i32);
                p.set_overlay_step(over_step as i32);
                p.set_overlay_total(over_tot as i32);
                p.set_frame_info_text(SharedString::from(&frame_info));
                p.set_toc_items(slint::ModelRc::from(toc_model_rc));

                p.set_hovered_on_link(false);
                p.set_hovered_link_tooltip(SharedString::new());
                p.set_link_holding(false);
                p.set_link_hold_progress(0.0);
            }

            // Update Audience Window
            if let Some(a) = audience_handle.upgrade() {
                a.set_slide_aspect(slide_aspect);
                a.set_slide_image(disp_audience);
                a.set_blank_mode(st.blank_mode);
                a.set_is_end_of_show(st.is_end_of_show);
                a.set_media_video_visible(false);
                a.set_media_rect_x(media_rect[0]);
                a.set_media_rect_y(media_rect[1]);
                a.set_media_rect_w(media_rect[2]);
                a.set_media_rect_h(media_rect[3]);
                a.set_laser_color(slint::Color::from_argb_u8(st.laser_color_rgba[3], st.laser_color_rgba[0], st.laser_color_rgba[1], st.laser_color_rgba[2]));
            }

            // Update Notes Window
            if let Some(n) = notes_handle.upgrade() {
                n.set_slide_num((current_idx + 1) as i32);
                n.set_total_slides(total as i32);
                n.set_has_notes_image(has_notes_img);
                n.set_is_dual_screen(is_dual_screen);
                n.set_notes_slide_image(slint_notes_img);
                n.set_notes_body(SharedString::from(&slide_notes));
                n.set_mini_slide(slint_current);
            }

            drop(st);
            update_ink();

            // Prewarm neighbouring slides' GPU images while idle so the next step is instant.
            let state_pw = state.clone();
            slint::Timer::single_shot(Duration::from_millis(30), move || {
                let st = state_pw.borrow();
                let idx = st.deck.current_index;
                for i in [idx + 1, idx.wrapping_sub(1), idx + 2] {
                    if let Some(s) = st.deck.slides.get(i) {
                        cached_slint_image(&s.gpu_cache.presenter, &s.presenter_image);
                        cached_slint_image(&s.gpu_cache.audience, &s.audience_image);
                        if let Some(n) = &s.notes_image {
                            cached_slint_image(&s.gpu_cache.notes, n);
                        }
                    }
                }
            });
        })
    };

    // Initial sync
    update_slide_view();
    update_slide_thumbnails_model(&state.borrow().deck, &presenter_window);

    // 1-second presentation timer tick
    let timer_tick = slint::Timer::default();
    {
        let presenter_handle = presenter_window.as_weak();
        let notes_handle = notes_window.as_weak();
        let state = state.clone();

        timer_tick.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(500),
            move || {
                let mut st = state.borrow_mut();
                let elapsed = st.timer.elapsed_str();
                let remaining = st.timer.remaining_str();
                let now = Local::now().format("%I:%M %p").to_string();

                let mut media_update = None;
                let is_video_clip = {
                    let slide = st.deck.current_slide();
                    if let Some(clip) = slide.media_clips.first() {
                        let path = &clip.file_path;
                        !(path.ends_with(".mp3")
                            || path.ends_with(".wav")
                            || path.ends_with(".ogg")
                            || path.ends_with(".flac")
                            || path.ends_with(".m4a")
                            || path.ends_with(".aac"))
                    } else {
                        false
                    }
                };
                if st.media_playing && !is_video_clip {
                    st.media_current_time_secs += 0.5 * st.media_speed;
                    if st.media_current_time_secs >= st.media_duration_secs {
                        st.media_current_time_secs = 0.0;
                        st.media_playing = false;
                    }
                    st.media_progress = (st.media_current_time_secs / st.media_duration_secs).clamp(0.0, 1.0);
                    media_update = Some((st.media_playing, st.media_progress, st.format_media_time()));
                }

                drop(st);

                if let Some(p) = presenter_handle.upgrade() {
                    p.set_elapsed_time(SharedString::from(&elapsed));
                    p.set_remaining_time(SharedString::from(&remaining));
                    p.set_wall_clock(SharedString::from(&now));

                    if let Some((playing, progress, time_text)) = media_update {
                        p.set_media_playing(playing);
                        p.set_media_progress(progress);
                        p.set_media_time_text(SharedString::from(&time_text));
                    }
                }
                if let Some(n) = notes_handle.upgrade() {
                    n.set_timer_str(SharedString::from(&elapsed));
                }
            },
        );
    }

    // --- SLIDE NAVIGATION CALLBACKS ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_next_slide(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                return;
            }
            st.blank_mode = 0;
            if st.deck.next() {
                drop(st);
                update();
            } else {
                // Reached past the last slide: display black screen
                st.is_end_of_show = true;
                st.blank_mode = 1;
                drop(st);
                update();
                println!("Reached end of presentation: displaying black screen.");
            }
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_prev_slide(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                st.is_end_of_show = false;
                st.blank_mode = 0;
                drop(st);
                update();
                println!("Returned from end-of-show black screen to last slide.");
                return;
            }
            st.blank_mode = 0;
            if st.deck.prev() {
                drop(st);
                update();
            }
        });
    }

    // --- BEAMER FRAME NAVIGATION CALLBACKS (\pause overlay jumps) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_next_frame(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                return;
            }
            st.blank_mode = 0;
            if st.deck.next_frame() {
                drop(st);
                update();
            } else if st.deck.next() {
                drop(st);
                update();
            } else {
                // Reached past the last frame: display black screen
                st.is_end_of_show = true;
                st.blank_mode = 1;
                drop(st);
                update();
                println!("Reached end of frames: displaying black screen.");
            }
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_prev_frame(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                st.is_end_of_show = false;
                st.blank_mode = 0;
                drop(st);
                update();
                return;
            }
            st.blank_mode = 0;
            if st.deck.prev_frame() {
                drop(st);
                update();
            }
        });
    }

    // --- SCREEN BLACKOUT / WHITEOUT & SCREEN SWAP CALLBACKS ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_toggle_blackout(move || {
            let mut st = state.borrow_mut();
            if st.is_end_of_show {
                st.is_end_of_show = false;
                st.blank_mode = 0;
            } else {
                st.blank_mode = if st.blank_mode == 1 { 0 } else { 1 };
            }
            drop(st);
            update();
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_toggle_whiteout(move || {
            let mut st = state.borrow_mut();
            st.blank_mode = if st.blank_mode == 2 { 0 } else { 2 };
            drop(st);
            update();
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_swap_screens(move || {
            let mut st = state.borrow_mut();
            st.is_screen_swapped = !st.is_screen_swapped;
            drop(st);
            update();
        });
    }

    // --- TOC OUTLINE DRAWER CALLBACKS ---
    {
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_toggle_toc(move || {
            if let Some(p) = presenter_handle.upgrade() {
                let cur = p.get_show_toc_drawer();
                p.set_show_toc_drawer(!cur);
            }
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_jump_to_toc(move |page_index| {
            let mut st = state.borrow_mut();
            st.is_end_of_show = false;
            st.blank_mode = 0;
            let target = if page_index > 0 { (page_index - 1) as usize } else { 0 };
            if st.deck.jump_to(target) {
                drop(st);
                update();
            }
        });
    }

    // --- JUMP TO SLIDE (Thumbnails overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_jump_to_slide(move |slide_num| {
            let mut st = state.borrow_mut();
            st.is_end_of_show = false;
            st.blank_mode = 0;
            if slide_num >= 1 && (slide_num as usize) <= st.deck.total_slides() {
                st.deck.current_index = (slide_num - 1) as usize;
                drop(st);
                update();
            }
        });
    }

    // --- STROKE WIDTH CHANGED ---
    {
        let state = state.clone();
        presenter_window.on_width_changed(move |len| {
            let mut st = state.borrow_mut();
            st.stroke_width = (len / 1.0).max(1.0);
            println!("Updated stroke width: {}px", st.stroke_width);
        });
    }

    // --- ADD WHITEBOARD PAGE (+) / (N) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_add_whiteboard_page(move || {
            let mut st = state.borrow_mut();
            st.is_end_of_show = false;
            st.blank_mode = 0;
            st.deck.insert_whiteboard_slide(true); // Blackboard with grid dots
            println!("Inserted new whiteboard scratchpad page at index {}", st.deck.current_index + 1);
            drop(st);
            update();
            if let Some(p) = presenter_handle.upgrade() {
                let st = state.borrow();
                update_slide_thumbnails_model(&st.deck, &p);
            }
        });
    }

    // --- DELETE SLIDE (Trash icon in Page Overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_delete_slide(move |idx| {
            let mut st = state.borrow_mut();
            if st.deck.slides.len() > 1 {
                let remove_idx = (idx - 1).clamp(0, st.deck.slides.len() as i32 - 1) as usize;
                st.deck.slides.remove(remove_idx);
                // Re-index slides
                for (i, s) in st.deck.slides.iter_mut().enumerate() {
                    s.page_index = i + 1;
                }
                if st.deck.current_index >= st.deck.slides.len() {
                    st.deck.current_index = st.deck.slides.len() - 1;
                }
                drop(st);
                update();
                if let Some(p) = presenter_handle.upgrade() {
                    let st = state.borrow();
                    update_slide_thumbnails_model(&st.deck, &p);
                }
            }
        });
    }

    // --- DUPLICATE / COPY SLIDE (Copy icon in Page Overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_duplicate_slide(move |idx| {
            let mut st = state.borrow_mut();
            let copy_idx = (idx - 1).clamp(0, st.deck.slides.len() as i32 - 1) as usize;
            let mut cloned = st.deck.slides[copy_idx].clone();
            cloned.title = format!("{} (Copy)", cloned.title);
            let insert_idx = copy_idx + 1;
            st.deck.slides.insert(insert_idx, cloned);
            for (i, s) in st.deck.slides.iter_mut().enumerate() {
                s.page_index = i + 1;
            }
            st.deck.current_index = insert_idx;
            drop(st);
            update();
            if let Some(p) = presenter_handle.upgrade() {
                let st = state.borrow();
                update_slide_thumbnails_model(&st.deck, &p);
            }
        });
    }

    // --- ROTATE SLIDE (Rotate icon in Page Overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_rotate_slide(move |idx| {
            let mut st = state.borrow_mut();
            let target_idx = (idx - 1).clamp(0, st.deck.slides.len() as i32 - 1) as usize;
            let slide = &mut st.deck.slides[target_idx];
            slide.audience_image = image::imageops::rotate90(&slide.audience_image);
            slide.presenter_image = image::imageops::rotate90(&slide.presenter_image);
            slide.gpu_cache.reset();
            slide.aspect_ratio = slide.audience_image.width() as f32 / slide.audience_image.height() as f32;
            drop(st);
            update();
            if let Some(p) = presenter_handle.upgrade() {
                let st = state.borrow();
                update_slide_thumbnails_model(&st.deck, &p);
            }
        });
    }

    // --- INSERT CONFIGURED PAGE (From Insert Page Modal) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_insert_page_configured(move |pos_mode, template_kind, bg_color| {
            let r = bg_color.red();
            let g = bg_color.green();
            let b = bg_color.blue();
            let a = bg_color.alpha();
            let bg_rgba = [r, g, b, a];

            let mut st = state.borrow_mut();
            let new_idx = st.deck.insert_page_with_template(pos_mode as usize, template_kind.as_str(), bg_rgba);
            println!(
                "Inserted new page (template: '{}', bg: {:?}, pos_mode: {}) at index {}",
                template_kind, bg_rgba, pos_mode, new_idx + 1
            );
            drop(st);
            update();
            if let Some(p) = presenter_handle.upgrade() {
                let st = state.borrow();
                update_slide_thumbnails_model(&st.deck, &p);
            }
        });
    }

    // --- REORDER / MOVE SLIDE (Move Left / Move Right in Page Overview) ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_move_slide(move |from_slide, to_slide| {
            let mut st = state.borrow_mut();
            let total = st.deck.slides.len() as i32;
            if from_slide >= 1 && from_slide <= total && to_slide >= 1 && to_slide <= total && from_slide != to_slide {
                let from_idx = (from_slide - 1) as usize;
                let to_idx = (to_slide - 1) as usize;
                if st.deck.move_slide(from_idx, to_idx) {
                    println!("Reordered slide {} -> {}", from_slide, to_slide);
                    drop(st);
                    update();
                    if let Some(p) = presenter_handle.upgrade() {
                        let st = state.borrow();
                        update_slide_thumbnails_model(&st.deck, &p);
                    }
                }
            }
        });
    }

    // --- MEDIA PLAYER TOOLBAR (IN-PLACE NATIVE PLAYBACK, STOP, SPEED, SCRUBBING) ---
    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        let video_player = video_player.clone();
        let update_slide_view = update_slide_view.clone();

        let start_playback_fn = {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let video_player = video_player.clone();

            Rc::new(move || {
                let (clip_path, is_audio, cur_time, speed, duration, muted, rect) = {
                    let mut st = state.borrow_mut();
                    let clip_opt = st.deck.current_slide().media_clips.first().cloned();
                    if let Some(clip) = clip_opt {
                        if !clip.is_resolved {
                            println!("Embedded media clip not resolved on disk: {}", clip.file_path);
                            return;
                        }
                        // If already at or past the end, restart from beginning
                        if st.media_duration_secs > 0.0 && (st.media_current_time_secs >= st.media_duration_secs - 0.1 || st.media_progress >= 0.99) {
                            st.media_current_time_secs = 0.0;
                            st.media_progress = 0.0;
                        }
                        let path = clip.file_path;
                        let is_aud = path.ends_with(".mp3")
                            || path.ends_with(".wav")
                            || path.ends_with(".ogg")
                            || path.ends_with(".flac")
                            || path.ends_with(".m4a")
                            || path.ends_with(".aac");
                        (
                            path,
                            is_aud,
                            st.media_current_time_secs,
                            st.media_speed,
                            st.media_duration_secs,
                            !st.media_speech_active,
                            clip.rect,
                        )
                    } else {
                        return;
                    }
                };

                println!(
                    "Starting in-place native playback of media: {} at pos {:.2}s (rect: {:?})",
                    clip_path, cur_time, rect
                );

                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_rect_x(rect[0]);
                    p.set_media_rect_y(rect[1]);
                    p.set_media_rect_w(rect[2]);
                    p.set_media_rect_h(rect[3]);
                    p.set_media_video_visible(true);
                    p.set_media_playing(true);
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_rect_x(rect[0]);
                    a.set_media_rect_y(rect[1]);
                    a.set_media_rect_w(rect[2]);
                    a.set_media_rect_h(rect[3]);
                    a.set_media_video_visible(true);
                }

                if is_audio {
                    let base_dir = std::path::Path::new(&clip_path)
                        .parent()
                        .unwrap_or(std::path::Path::new("."));
                    let mut cmd = std::process::Command::new("afplay");
                    cmd.arg(&clip_path).current_dir(base_dir);
                    if cmd.spawn().is_err() {
                        let mut ffp = std::process::Command::new("ffplay");
                        ffp.args(["-nodisp", "-autoexit", "-loglevel", "quiet", "-ss", &format!("{:.2}", cur_time), &clip_path])
                            .current_dir(base_dir);
                        let _ = ffp.spawn();
                    }
                    return;
                }

                let p_weak = presenter_handle.clone();
                let a_weak = audience_handle.clone();
                let p_weak_eof = presenter_handle.clone();
                let duration_secs = duration;

                let mut vp = video_player.lock().unwrap();
                let started = vp.play(
                    &clip_path,
                    cur_time,
                    speed,
                    duration,
                    muted,
                    move |pixel_buf, time_secs, progress| {
                        let p_weak = p_weak.clone();
                        let a_weak = a_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            let frame = rgba_to_slint_image(&pixel_buf);
                            let cur_u = time_secs as u32;
                            let dur_u = duration_secs as u32;
                            let time_text = format!("{:02}:{:02} / {:02}:{:02}", cur_u / 60, cur_u % 60, dur_u / 60, dur_u % 60);

                            if let Some(p) = p_weak.upgrade() {
                                p.set_video_frame_image(frame.clone());
                                p.set_media_progress(progress);
                                p.set_media_time_text(SharedString::from(&time_text));
                            }
                            if let Some(a) = a_weak.upgrade() {
                                a.set_video_frame_image(frame);
                            }
                        });
                    },
                    move || {
                        let p_weak = p_weak_eof.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(p) = p_weak.upgrade() {
                                p.set_media_playing(false);
                                p.set_media_progress(1.0);
                            }
                        });
                    },
                );

                if !started {
                    eprintln!("Failed to stream in-place video frames for {}", clip_path);
                }
            })
        };

        // 1. Play Media
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_play_media(move || {
                let is_at_end = presenter_handle.upgrade().map(|p| p.get_media_progress() >= 0.99).unwrap_or(false);
                {
                    let mut st = state.borrow_mut();
                    if is_at_end {
                        st.media_current_time_secs = 0.0;
                        st.media_progress = 0.0;
                    }
                    st.media_playing = true;
                }
                start_playback();
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(true);
                }
            });
        }

        // 2. Pause Media
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let video_player = video_player.clone();
            presenter_window.on_pause_media(move || {
                let cur_t = {
                    let mut vp = video_player.lock().unwrap();
                    vp.pause();
                    vp.current_time()
                };
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("killall").arg("afplay").output();
                }
                let _ = std::process::Command::new("killall").arg("ffplay").output();
                {
                    let mut st = state.borrow_mut();
                    st.media_playing = false;
                    st.media_current_time_secs = cur_t;
                    if st.media_duration_secs > 0.0 {
                        st.media_progress = (cur_t / st.media_duration_secs).clamp(0.0, 1.0);
                    }
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(false);
                }
            });
        }

        // 3. Stop Media (reverts slide back to original state / selected frame)
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let video_player = video_player.clone();
            let update_slide_view = update_slide_view.clone();
            presenter_window.on_stop_media(move || {
                video_player.lock().unwrap().stop();
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("killall").arg("afplay").output();
                }
                let _ = std::process::Command::new("killall").arg("ffplay").output();

                {
                    let mut st = state.borrow_mut();
                    st.media_playing = false;
                    st.media_current_time_secs = 0.0;
                    st.media_progress = 0.0;
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(false);
                    p.set_media_video_visible(false);
                    p.set_media_progress(0.0);
                }
                if let Some(a) = audience_handle.upgrade() {
                    a.set_media_video_visible(false);
                }

                // Explicitly revert slide back to original state (user's selected frame)
                update_slide_view();
                println!("Media stopped: reverted slide to original frame state.");
            });
        }

        // 4. Toggle Playback
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_toggle_media_playback(move || {
                let is_at_end = presenter_handle.upgrade().map(|p| p.get_media_progress() >= 0.99).unwrap_or(false);
                let will_play = {
                    let mut st = state.borrow_mut();
                    if is_at_end {
                        st.media_current_time_secs = 0.0;
                        st.media_progress = 0.0;
                    }
                    st.media_playing = !st.media_playing;
                    st.media_playing
                };
                if will_play {
                    start_playback();
                } else {
                    let cur_t = {
                        let mut vp = video_player.lock().unwrap();
                        vp.pause();
                        vp.current_time()
                    };
                    #[cfg(target_os = "macos")]
                    {
                        let _ = std::process::Command::new("killall").arg("afplay").output();
                    }
                    let _ = std::process::Command::new("killall").arg("ffplay").output();
                    {
                        let mut st = state.borrow_mut();
                        st.media_current_time_secs = cur_t;
                        if st.media_duration_secs > 0.0 {
                            st.media_progress = (cur_t / st.media_duration_secs).clamp(0.0, 1.0);
                        }
                    }
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_playing(will_play);
                }
            });
        }

        // 5. Seek Media
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let audience_handle = audience_handle.clone();
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_seek_media(move |pos| {
                let (p_clamped, is_playing, seek_time, clip_opt) = {
                    let mut st = state.borrow_mut();
                    let p_clamped = pos.clamp(0.0, 1.0);
                    st.media_progress = p_clamped;
                    st.media_current_time_secs = p_clamped * st.media_duration_secs;
                    let playing = st.media_playing;
                    let seek_t = st.media_current_time_secs;
                    let clip = st.deck.current_slide().media_clips.first().cloned();
                    (p_clamped, playing, seek_t, clip)
                };
                video_player.lock().unwrap().set_current_time(seek_time);
                let time_str = {
                    let st = state.borrow();
                    st.format_media_time()
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_progress(p_clamped);
                    p.set_media_time_text(SharedString::from(&time_str));
                }
                if is_playing {
                    start_playback();
                } else if let Some(clip) = clip_opt {
                    if clip.is_resolved {
                        if let Some(pixel_buf) = InPlaceVideoPlayer::extract_single_frame(&clip.file_path, seek_time) {
                            let frame = rgba_to_slint_image(&pixel_buf);
                            if let Some(p) = presenter_handle.upgrade() {
                                p.set_video_frame_image(frame.clone());
                                p.set_media_video_visible(true);
                            }
                            if let Some(a) = audience_handle.upgrade() {
                                a.set_video_frame_image(frame);
                                a.set_media_video_visible(true);
                            }
                        }
                    }
                }
            });
        }

        // 6. Toggle Media Speed
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_toggle_media_speed(move || {
                let (speed_label, is_playing) = {
                    let mut st = state.borrow_mut();
                    if st.media_playing {
                        let cur_t = video_player.lock().unwrap().current_time();
                        st.media_current_time_secs = cur_t;
                    }
                    const SPEED_OPTIONS: [f32; 5] = [1.0, 1.25, 1.5, 2.0, 0.75];
                    let cur = st.media_speed;
                    let idx = SPEED_OPTIONS
                        .iter()
                        .position(|&s| (s - cur).abs() < 0.05)
                        .unwrap_or(0);
                    st.media_speed = SPEED_OPTIONS[(idx + 1) % SPEED_OPTIONS.len()];
                    (st.format_media_speed(), st.media_playing)
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_speed_text(SharedString::from(&speed_label));
                }
                if is_playing {
                    start_playback();
                }
            });
        }

        // 7. Toggle Media Speech / Audio Mute
        {
            let state = state.clone();
            let presenter_handle = presenter_handle.clone();
            let video_player = video_player.clone();
            let start_playback = start_playback_fn.clone();
            presenter_window.on_toggle_media_speech(move || {
                let (active, is_playing) = {
                    let mut st = state.borrow_mut();
                    if st.media_playing {
                        let cur_t = video_player.lock().unwrap().current_time();
                        st.media_current_time_secs = cur_t;
                    }
                    st.media_speech_active = !st.media_speech_active;
                    (st.media_speech_active, st.media_playing)
                };
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_media_speech_active(active);
                }
                if is_playing {
                    start_playback();
                }
            });
        }
    }

    // --- TOOL SELECTION ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_tool_selected(move |tool| {
            let mut st = state.borrow_mut();
            if st.current_tool != ActiveTool::Eraser && tool == ActiveTool::Eraser {
                st.previous_tool = st.current_tool;
            }
            st.current_tool = tool;
            if tool == ActiveTool::Laser {
                let color = st.active_color;
                st.laser_color_rgba = color;
                if let Some(trail) = st.laser_trail.as_mut() {
                    trail.color_rgba = color;
                }
                let c = slint::Color::from_argb_u8(color[3], color[0], color[1], color[2]);
                if let Some(p) = pres_handle.upgrade() {
                    p.set_laser_color(c);
                }
                if let Some(aud) = audience_handle.upgrade() {
                    aud.set_laser_color(c);
                }
            }
            println!("Selected tool: {:?}", tool);
        });
    }

    // --- COLOR PICKER & COLOR WHEEL ---
    fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
        if s <= 0.0 {
            let val = (v * 255.0).round().clamp(0.0, 255.0) as u8;
            return (val, val, val);
        }
        let h_norm = ((h % 360.0) + 360.0) % 360.0 / 60.0;
        let i = h_norm.floor() as i32;
        let f = h_norm - i as f32;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        let (r, g, b) = match i % 6 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        (
            (r * 255.0).round().clamp(0.0, 255.0) as u8,
            (g * 255.0).round().clamp(0.0, 255.0) as u8,
            (b * 255.0).round().clamp(0.0, 255.0) as u8,
        )
    }

    fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
        let rf = r as f32 / 255.0;
        let gf = g as f32 / 255.0;
        let bf = b as f32 / 255.0;
        let max = rf.max(gf).max(bf);
        let min = rf.min(gf).min(bf);
        let delta = max - min;
        let v = max;
        let s = if max > 0.0 { delta / max } else { 0.0 };
        let mut h = if delta == 0.0 {
            0.0
        } else if (max - rf).abs() < 1e-4 {
            60.0 * (((gf - bf) / delta) % 6.0)
        } else if (max - gf).abs() < 1e-4 {
            60.0 * (((bf - rf) / delta) + 2.0)
        } else {
            60.0 * (((rf - gf) / delta) + 4.0)
        };
        if h < 0.0 {
            h += 360.0;
        }
        (h, s, v)
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_color_changed(move |c| {
            let mut st = state.borrow_mut();
            let rgba = [c.red(), c.green(), c.blue(), c.alpha()];
            st.active_color = rgba;
            st.laser_color_rgba = rgba;
            match st.active_color_slot {
                0 => st.color_slot_1 = rgba,
                1 => st.color_slot_2 = rgba,
                2 => st.color_slot_3 = rgba,
                3 => st.color_slot_4 = rgba,
                4 => st.color_slot_5 = rgba,
                _ => {}
            }
            if let Some(trail) = st.laser_trail.as_mut() {
                trail.color_rgba = rgba;
            }
            let (h, s, v) = rgb_to_hsv(c.red(), c.green(), c.blue());
            let (pr, pg, pb) = hsv_to_rgb(h, s, 1.0);
            let cx = 60.0f32;
            let cy = 60.0f32;
            let max_r = 56.0f32;
            let rad = h.to_radians();
            let tx = cx + rad.cos() * s * max_r;
            let ty = cy + rad.sin() * s * max_r;

            if let Some(p) = pres_handle.upgrade() {
                p.set_active_color(c);
                p.set_laser_color(c);
                p.set_wheel_hue(h);
                p.set_wheel_sat(s);
                p.set_wheel_val(v);
                p.set_wheel_pure_color(slint::Color::from_rgb_u8(pr, pg, pb));
                p.set_wheel_thumb_x(tx * 1.0);
                p.set_wheel_thumb_y(ty * 1.0);
            }
            if let Some(aud) = audience_handle.upgrade() {
                aud.set_laser_color(c);
            }
            println!("Updated stroke and laser color: RGBA({}, {}, {}, {})", c.red(), c.green(), c.blue(), c.alpha());
        });
    }

    // --- COLOR WHEEL TOUCHED ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_color_wheel_touched(move |mx, my| {
            let cx = 60.0f32;
            let cy = 60.0f32;
            let max_r = 56.0f32;
            let dx = (mx / 1.0) - cx;
            let dy = (my / 1.0) - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let clamped_dist = dist.min(max_r);
            let angle = dy.atan2(dx);
            let mut hue = angle.to_degrees();
            if hue < 0.0 { hue += 360.0; }
            let sat = (clamped_dist / max_r).clamp(0.0, 1.0);

            if let Some(p) = pres_handle.upgrade() {
                let val = p.get_wheel_val();
                let (r, g, b) = hsv_to_rgb(hue, sat, val);
                let (pr, pg, pb) = hsv_to_rgb(hue, sat, 1.0);
                let selected_color = slint::Color::from_rgb_u8(r, g, b);
                let pure_color = slint::Color::from_rgb_u8(pr, pg, pb);

                let thumb_x = cx + angle.cos() * clamped_dist;
                let thumb_y = cy + angle.sin() * clamped_dist;

                p.set_wheel_hue(hue);
                p.set_wheel_sat(sat);
                p.set_wheel_pure_color(pure_color);
                p.set_wheel_thumb_x(thumb_x * 1.0);
                p.set_wheel_thumb_y(thumb_y * 1.0);
                p.set_active_color(selected_color);
                p.set_laser_color(selected_color);

                let mut st = state.borrow_mut();
                let rgba = [r, g, b, 255];
                st.active_color = rgba;
                st.laser_color_rgba = rgba;
                match st.active_color_slot {
                    0 => { st.color_slot_1 = rgba; p.set_color_slot_1(selected_color); }
                    1 => { st.color_slot_2 = rgba; p.set_color_slot_2(selected_color); }
                    2 => { st.color_slot_3 = rgba; p.set_color_slot_3(selected_color); }
                    3 => { st.color_slot_4 = rgba; p.set_color_slot_4(selected_color); }
                    4 => { st.color_slot_5 = rgba; p.set_color_slot_5(selected_color); }
                    _ => {}
                }
                if let Some(trail) = st.laser_trail.as_mut() {
                    trail.color_rgba = rgba;
                }
            }
            if let Some(aud) = audience_handle.upgrade() {
                if let Some(p) = pres_handle.upgrade() {
                    aud.set_laser_color(p.get_active_color());
                }
            }
        });
    }

    // --- COLOR BRIGHTNESS SLIDER TOUCHED ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_color_brightness_touched(move |val| {
            let val_clamped = val.clamp(0.0, 1.0);
            if let Some(p) = pres_handle.upgrade() {
                let hue = p.get_wheel_hue();
                let sat = p.get_wheel_sat();
                let (r, g, b) = hsv_to_rgb(hue, sat, val_clamped);
                let selected_color = slint::Color::from_rgb_u8(r, g, b);

                p.set_wheel_val(val_clamped);
                p.set_active_color(selected_color);
                p.set_laser_color(selected_color);

                let mut st = state.borrow_mut();
                let rgba = [r, g, b, 255];
                st.active_color = rgba;
                st.laser_color_rgba = rgba;
                match st.active_color_slot {
                    0 => { st.color_slot_1 = rgba; p.set_color_slot_1(selected_color); }
                    1 => { st.color_slot_2 = rgba; p.set_color_slot_2(selected_color); }
                    2 => { st.color_slot_3 = rgba; p.set_color_slot_3(selected_color); }
                    3 => { st.color_slot_4 = rgba; p.set_color_slot_4(selected_color); }
                    4 => { st.color_slot_5 = rgba; p.set_color_slot_5(selected_color); }
                    _ => {}
                }
                if let Some(trail) = st.laser_trail.as_mut() {
                    trail.color_rgba = rgba;
                }
            }
            if let Some(aud) = audience_handle.upgrade() {
                if let Some(p) = pres_handle.upgrade() {
                    aud.set_laser_color(p.get_active_color());
                }
            }
        });
    }

    // --- ADD CUSTOM COLOR TO PALETTE ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_add_custom_color(move |c| {
            let mut st = state.borrow_mut();
            let new_rgba = [c.red(), c.green(), c.blue(), c.alpha()];
            if !st.custom_palette.iter().any(|&col| col == new_rgba) {
                st.custom_palette.push(new_rgba);
                println!("Added custom color to palette: RGBA({}, {}, {}, {})", c.red(), c.green(), c.blue(), c.alpha());
            }
            if let Some(p) = pres_handle.upgrade() {
                let slint_colors: Vec<slint::Color> = st.custom_palette.iter().map(|&rgba| {
                    slint::Color::from_argb_u8(rgba[3], rgba[0], rgba[1], rgba[2])
                }).collect();
                p.set_custom_palette(std::rc::Rc::new(slint::VecModel::from(slint_colors)).into());
            }
        });
    }

    // --- UNDO & REDO ---
    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        presenter_window.on_undo(move || {
            let mut st = state.borrow_mut();
            if st.deck.current_slide_mut().inking.undo() {
                st.cached_ink_overlay = None;
                drop(st);
                update_ink();
            }
        });
    }

    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        presenter_window.on_redo(move || {
            let mut st = state.borrow_mut();
            if st.deck.current_slide_mut().inking.redo() {
                st.cached_ink_overlay = None;
                drop(st);
                update_ink();
            }
        });
    }

    // --- PEN TOOL SETTINGS CALLBACKS ---
    {
        let state = state.clone();
        presenter_window.on_toggle_auto_shape(move |enabled| {
            let mut st = state.borrow_mut();
            st.auto_shape_enabled = enabled;
            println!("Auto Shape toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_scribble_to_erase(move |enabled| {
            let mut st = state.borrow_mut();
            st.scribble_to_erase = enabled;
            println!("Scribble to Erase toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_change_hold_delay(move |delay_s| {
            let mut st = state.borrow_mut();
            st.hold_delay_ms = (delay_s * 1000.0).round() as u64;
            if let Some(p) = pres_handle.upgrade() {
                p.set_hold_delay_text(format!("{:.1}s", delay_s).into());
            }
            println!("Draw and hold delay updated: {}ms", st.hold_delay_ms);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_enhanced_sensitivity(move |enabled| {
            let mut st = state.borrow_mut();
            st.enhanced_sensitivity = enabled;
            println!("Enhanced Sensitivity toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_draw_on_hover(move |enabled| {
            let mut st = state.borrow_mut();
            st.draw_on_hover = enabled;
            println!("Draw on Hover toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_select_pen_type(move |ptype| {
            let mut st = state.borrow_mut();
            st.pen_type = ptype;
            println!("Selected Pen Type: {}", match ptype {
                0 => "Fineliner",
                1 => "Fountain Pen",
                2 => "Marker",
                _ => "Custom",
            });
        });
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_select_pen_size(move |size_val, label| {
            let mut st = state.borrow_mut();
            let px = (size_val * 4.2).max(1.0);
            st.stroke_width = px;
            if let Some(p) = pres_handle.upgrade() {
                let lbl = if label.is_empty() {
                    format!("{:.1}mm", size_val)
                } else {
                    label.to_string()
                };
                p.set_pen_size_mm(lbl.into());
                p.set_stroke_width(px * 1.0);
            }
            println!("Pen size selected: {:.2}mm ({}px)", size_val, px);
        });
    }

    // --- HIGHLIGHTER TOOL SETTINGS CALLBACKS ---
    {
        let state = state.clone();
        presenter_window.on_select_highlighter_mode(move |mode| {
            let mut st = state.borrow_mut();
            st.highlighter_mode = mode;
            println!("Highlighter mode selected: {}", if mode == 1 { "Tape" } else { "Highlighter" });
        });
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_select_highlighter_size(move |size_val, label| {
            let mut st = state.borrow_mut();
            let px = (size_val * 4.2).max(2.0);
            st.stroke_width = px;
            if let Some(p) = pres_handle.upgrade() {
                let lbl = if label.is_empty() {
                    if (size_val - 1.9).abs() < 0.15 {
                        "1.9mm".to_string()
                    } else if (size_val - 2.7).abs() < 0.15 {
                        "2.7mm".to_string()
                    } else if (size_val - 4.6).abs() < 0.15 {
                        "4.6mm".to_string()
                    } else if (size_val - 5.4).abs() < 0.15 {
                        "5.4mm".to_string()
                    } else if (size_val - 6.2).abs() < 0.15 {
                        "6.2mm".to_string()
                    } else {
                        format!("{:.1}mm", size_val)
                    }
                } else {
                    label.to_string()
                };
                st.highlighter_size_mm = lbl.clone();
                p.set_highlighter_size_mm(lbl.into());
                p.set_highlighter_size_value(size_val);
                p.set_stroke_width(px * 1.0);
            }
            println!("Highlighter size selected: {:.2}mm ({}px)", size_val, px);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_highlighter_straight_lines(move |enabled| {
            let mut st = state.borrow_mut();
            st.highlighter_straight_lines = enabled;
            println!("Highlighter Draw Straight Lines toggled: {}", enabled);
        });
    }

    // --- ERASER TOOL SETTINGS CALLBACKS ---
    {
        let state = state.clone();
        presenter_window.on_select_eraser_size(move |index, radius| {
            let mut st = state.borrow_mut();
            st.eraser_size_index = index;
            st.eraser_radius = radius;
            println!("Eraser size selected: slot {} (radius: {:.3})", index, radius);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_erase_whole_line(move |enabled| {
            let mut st = state.borrow_mut();
            st.erase_whole_line = enabled;
            println!("Erase Whole Line toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_erase_highlight_only(move |enabled| {
            let mut st = state.borrow_mut();
            st.erase_highlight_only = enabled;
            println!("Erase Highlight Only toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_auto_select_previous_tool(move |enabled| {
            let mut st = state.borrow_mut();
            st.auto_select_previous_tool = enabled;
            println!("Auto-select Previous Tool toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_erase_shapes(move |enabled| {
            let mut st = state.borrow_mut();
            st.erase_shapes = enabled;
            println!("Erase Shapes toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_erase_tapes(move |enabled| {
            let mut st = state.borrow_mut();
            st.erase_tapes = enabled;
            println!("Erase Tapes toggled: {}", enabled);
        });
    }

    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_reset_eraser_settings(move || {
            let mut st = state.borrow_mut();
            st.eraser_size_index = 2;
            st.eraser_radius = 0.04;
            st.erase_whole_line = false;
            st.erase_highlight_only = false;
            st.auto_select_previous_tool = false;
            st.erase_shapes = true;
            st.erase_tapes = false;
            if let Some(p) = pres_handle.upgrade() {
                p.set_eraser_size_index(2);
                p.set_eraser_radius(0.04);
                p.set_erase_whole_line(false);
                p.set_erase_highlight_only(false);
                p.set_auto_select_previous_tool(false);
                p.set_erase_shapes(true);
                p.set_erase_tapes(false);
            }
            println!("Eraser settings reset to defaults.");
        });
    }

    // --- LASER POINTER & DRAW LINES SETTINGS ---
    {
        let state = state.clone();
        let audience_handle = audience_window.as_weak();
        presenter_window.on_select_laser_color(move |idx, _color| {
            let color = match idx {
                0 => [255, 59, 48, 255],   // Red #ff3b30
                1 => [0, 85, 255, 255],    // Blue #0055ff
                2 => [0, 229, 58, 255],    // Green #00e53a
                3 => [184, 0, 255, 255],   // Purple #b800ff
                _ => [255, 59, 48, 255],
            };
            let mut st = state.borrow_mut();
            st.laser_color_index = idx;
            st.laser_color_rgba = color;
            if let Some(trail) = st.laser_trail.as_mut() {
                trail.color_rgba = color;
            }
            if let Some(aud) = audience_handle.upgrade() {
                aud.set_laser_color(slint::Color::from_argb_u8(color[3], color[0], color[1], color[2]));
            }
            println!("Selected laser color index: {}, rgba: {:?}", idx, color);
        });
    }

    {
        let state = state.clone();
        presenter_window.on_toggle_laser_draw_lines(move |val| {
            let mut st = state.borrow_mut();
            st.laser_draw_lines = val;
            let duration = if val {
                (st.laser_fade_duration_s * 1000.0).round() as u64
            } else {
                (st.laser_fade_duration_s * 1000.0).round().min(350.0) as u64
            };
            if let Some(trail) = st.laser_trail.as_mut() {
                trail.draw_lines = val;
                trail.duration_ms = duration;
            }
            println!("Laser draw lines toggled: {}", val);
        });
    }

    // --- LASER FADE DURATION CONTROLLER (0.0s to 5.0s with decimals) ---
    {
        let state = state.clone();
        let pres_handle = presenter_window.as_weak();
        presenter_window.on_change_laser_fade_duration(move |duration| {
            let mut st = state.borrow_mut();
            let clamped = duration.clamp(0.0, 5.0);
            st.laser_fade_duration_s = clamped;
            let duration_ms = (clamped * 1000.0).round() as u64;
            if let Some(trail) = st.laser_trail.as_mut() {
                trail.duration_ms = duration_ms;
            }
            if let Some(p) = pres_handle.upgrade() {
                p.set_laser_fade_duration_s(clamped);
                p.set_laser_fade_duration_text(SharedString::from(format!("{:.1}s", clamped)));
            }
            println!("Updated laser fade duration: {:.1}s ({}ms)", clamped, duration_ms);
        });
    }

    // --- LASER TRAIL DECAY TIMER (16ms / 60 FPS) ---
    let laser_decay_timer = slint::Timer::default();
    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        laser_decay_timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
            let mut st = state.borrow_mut();
            if let Some(trail) = st.laser_trail.as_mut() {
                if !trail.positions.is_empty() {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    trail.prune(now_ms);
                    if trail.positions.is_empty() {
                        st.laser_trail = None;
                    }
                    drop(st);
                    update_ink();
                }
            }
        });
    }

    // --- DRAW AND HOLD AUTO SHAPE & LINK INTERACTIVE TIMER (25ms) ---
    let shape_check_timer = slint::Timer::default();
    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let update_view = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        shape_check_timer.start(slint::TimerMode::Repeated, Duration::from_millis(25), move || {
            let mut st = state.borrow_mut();

            // Check interactive PDF link touch & hold (activates on ~380ms hold)
            if st.pointer_is_down && !st.link_triggered {
                if let Some(link) = st.active_link_target.clone() {
                    if let Some(start_time) = st.link_press_start {
                        let elapsed_ms = start_time.elapsed().as_millis() as f32;
                        let hold_thresh = 380.0;
                        let progress = (elapsed_ms / hold_thresh).min(1.0);

                        if let Some(p) = presenter_handle.upgrade() {
                            p.set_link_hold_progress(progress);
                            p.set_link_holding(true);
                        }

                        if elapsed_ms >= hold_thresh {
                            st.link_triggered = true;
                            st.link_press_start = None;
                            st.active_stroke = None;
                            st.raw_stroke_backup.clear();

                            if let Some(p) = presenter_handle.upgrade() {
                                p.set_link_holding(false);
                                p.set_link_hold_progress(0.0);
                            }

                            match link.target {
                                PdfLinkTarget::Url(url) => {
                                    println!("Touch & Hold triggered PDF link: {}", url);
                                    open_external_url(&url);
                                    drop(st);
                                    update_ink();
                                    return;
                                }
                                PdfLinkTarget::Page(dest_idx) => {
                                    println!("Touch & Hold triggered PDF destination: jump to slide {}", dest_idx + 1);
                                    if st.deck.jump_to(dest_idx) {
                                        drop(st);
                                        update_view();
                                        update_ink();
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !st.pointer_is_down || st.is_shape_snapped || !st.auto_shape_enabled {
                return;
            }
            let elapsed_ms = st.last_pointer_move_time.elapsed().as_millis() as u64;
            if elapsed_ms < st.hold_delay_ms {
                return;
            }

            // Check scribble to erase first if enabled
            if st.scribble_to_erase && ink::ShapeRecognizer::is_scribble(&st.raw_stroke_backup) {
                println!("Scribble-to-erase triggered on hold!");
                let (min_x, min_y, max_x, max_y) = ink::ShapeRecognizer::bounding_box(&st.raw_stroke_backup);
                let cx = (min_x + max_x) * 0.5;
                let cy = (min_y + max_y) * 0.5;
                let r = ((max_x - min_x).max(max_y - min_y) * 0.7).max(0.04);
                st.deck.current_slide_mut().inking.erase_at(cx, cy, r);
                st.active_stroke = None;
                st.is_shape_snapped = true;
                drop(st);
                update_ink();
                return;
            }

            // Auto shape recognition: snap to regular shape while pointer is held down!
            if let Some(stroke) = st.active_stroke.as_mut() {
                if stroke.points.len() >= 3 {
                    if let Some(recognized) = ink::ShapeRecognizer::recognize(&stroke.points, stroke.is_highlighter) {
                        println!("Auto Shape recognized while holding: {:?}", recognized.kind);
                        stroke.points = recognized.points;
                        stroke.is_shape = true;
                        stroke.recompute_bounding_box();
                        st.is_shape_snapped = true;
                        drop(st);
                        update_ink();
                    }
                }
            }
        });
    }

    // --- INKING & LASER CANVAS POINTER HANDLERS ---
    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();

        presenter_window.on_canvas_pointer_down(move |x, y, pressure| {
            let mut st = state.borrow_mut();
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            st.pointer_is_down = true;
            st.last_pointer_move_time = std::time::Instant::now();
            st.hold_anchor_pos = (x, y);
            st.is_shape_snapped = false;
            st.raw_stroke_backup.clear();

            // Detect interactive PDF link on pointer down
            let hit_link = st.deck.current_slide().find_link_at(x, y, 0.015).cloned();
            st.active_link_target = hit_link.clone();
            st.link_press_start = if hit_link.is_some() { Some(std::time::Instant::now()) } else { None };
            st.link_triggered = false;

            if let Some(ref link) = hit_link {
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_link_holding(true);
                    p.set_link_hold_progress(0.0);
                    p.set_hovered_link_tooltip(slint::SharedString::from(&link.tooltip));
                }
            }

            match st.current_tool {
                ActiveTool::Pen => {
                    st.stroke_counter += 1;
                    let mut stroke = Stroke::new(st.stroke_counter, st.active_color, st.stroke_width, false);
                    let pt = Point::new(x, y, pressure, now_ms);
                    stroke.add_point(pt);
                    st.raw_stroke_backup.push(pt);
                    st.active_stroke = Some(stroke);
                    drop(st);
                    update_ink();
                }
                ActiveTool::Highlighter => {
                    st.stroke_counter += 1;
                    let mut stroke = Stroke::new(st.stroke_counter, st.active_color, st.stroke_width, true);
                    stroke.is_tape = st.highlighter_mode == 1;
                    if st.highlighter_straight_lines {
                        stroke.is_shape = true;
                    }
                    let pt = Point::new(x, y, pressure, now_ms);
                    stroke.add_point(pt);
                    st.raw_stroke_backup.push(pt);
                    st.active_stroke = Some(stroke);
                    drop(st);
                    update_ink();
                }

                ActiveTool::Eraser => {
                    let opts = ink::EraserOptions {
                        radius: st.eraser_radius,
                        erase_whole_line: st.erase_whole_line,
                        erase_highlight_only: st.erase_highlight_only,
                        erase_shapes: st.erase_shapes,
                        erase_tapes: st.erase_tapes,
                    };
                    let erased = st.deck.current_slide_mut().inking.erase_at_with_options(x, y, &opts);
                    if erased {
                        drop(st);
                        update_ink();
                    }
                }
                ActiveTool::Laser => {
                    // Check if tapping on any Tape stroke to toggle reveal / cover
                    let mut toggled_tape = false;
                    for stroke in &mut st.deck.current_slide_mut().inking.strokes {
                        if stroke.is_tape && stroke.intersects_circle(x, y, 0.03) {
                            stroke.is_tape_revealed = !stroke.is_tape_revealed;
                            toggled_tape = true;
                            println!("Tape #{} toggled: revealed = {}", stroke.id, stroke.is_tape_revealed);
                            break;
                        }
                    }
                    if toggled_tape {
                        drop(st);
                        update_ink();
                        return;
                    }

                    st.laser_active = true;
                    let duration = if st.laser_draw_lines {
                        (st.laser_fade_duration_s * 1000.0).round() as u64
                    } else {
                        (st.laser_fade_duration_s * 1000.0).round().min(350.0) as u64
                    };
                    let mut trail = st.laser_trail.take().unwrap_or_else(|| {
                        LaserTrail::new(duration, st.laser_color_rgba, st.laser_draw_lines)
                    });
                    trail.duration_ms = duration;
                    trail.color_rgba = st.laser_color_rgba;
                    trail.draw_lines = st.laser_draw_lines;
                    trail.start_stroke();
                    trail.add(x, y, now_ms);
                    st.laser_trail = Some(trail);

                    if let Some(p) = presenter_handle.upgrade() {
                        p.set_laser_active(true);
                        p.set_laser_x(x);
                        p.set_laser_y(y);
                    }
                    if let Some(aud) = audience_handle.upgrade() {
                        aud.set_laser_visible(true);
                        aud.set_laser_x(x);
                        aud.set_laser_y(y);
                    }
                    drop(st);
                    update_ink();
                }
                _ => {}
            }
        });
    }

    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();

        presenter_window.on_canvas_pointer_move(move |x, y, pressure| {
            let mut st = state.borrow_mut();
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            if !st.pointer_is_down {
                let hover_link = st.deck.current_slide().find_link_at(x, y, 0.015);
                if let Some(p) = presenter_handle.upgrade() {
                    if let Some(link) = hover_link {
                        p.set_hovered_on_link(true);
                        let hint = if st.current_tool == ActiveTool::Laser { "Click to open" } else { "Touch & hold to open" };
                        p.set_hovered_link_tooltip(slint::SharedString::from(format!("{} ({})", link.tooltip, hint)));
                    } else {
                        p.set_hovered_on_link(false);
                        p.set_hovered_link_tooltip(slint::SharedString::new());
                    }
                }
            } else if st.active_link_target.is_some() {
                let dist = ((x - st.hold_anchor_pos.0).powi(2) + (y - st.hold_anchor_pos.1).powi(2)).sqrt();
                if dist > 0.02 {
                    st.active_link_target = None;
                    st.link_press_start = None;
                    if let Some(p) = presenter_handle.upgrade() {
                        p.set_link_holding(false);
                        p.set_link_hold_progress(0.0);
                    }
                }
            }

            if st.active_stroke.is_none() {
                if st.current_tool == ActiveTool::Eraser && pressure > 0.0 {
                    let opts = ink::EraserOptions {
                        radius: st.eraser_radius,
                        erase_whole_line: st.erase_whole_line,
                        erase_highlight_only: st.erase_highlight_only,
                        erase_shapes: st.erase_shapes,
                        erase_tapes: st.erase_tapes,
                    };
                    let erased = st.deck.current_slide_mut().inking.erase_at_with_options(x, y, &opts);
                    if erased {
                        st.cached_ink_overlay = None;
                        drop(st);
                        update_ink();
                    }
                } else if st.current_tool == ActiveTool::Laser {
                    if st.pointer_is_down && pressure > 0.0 {
                        let duration = if st.laser_draw_lines {
                            (st.laser_fade_duration_s * 1000.0).round() as u64
                        } else {
                            (st.laser_fade_duration_s * 1000.0).round().min(350.0) as u64
                        };
                        let mut trail = st.laser_trail.take().unwrap_or_else(|| {
                            LaserTrail::new(duration, st.laser_color_rgba, st.laser_draw_lines)
                        });
                        trail.duration_ms = duration;
                        trail.color_rgba = st.laser_color_rgba;
                        trail.draw_lines = st.laser_draw_lines;
                        trail.add(x, y, now_ms);
                        st.laser_trail = Some(trail);

                        if let Some(aud) = audience_handle.upgrade() {
                            aud.set_laser_visible(true);
                            aud.set_laser_x(x);
                            aud.set_laser_y(y);
                        }

                        // Responsive frame pacing: render at ~60 FPS (16ms) during active mouse dragging
                        // This keeps the mouse event thread completely unblocked with zero latency
                        if st.last_laser_update.elapsed().as_millis() >= 16 {
                            st.last_laser_update = std::time::Instant::now();
                            drop(st);
                            update_ink();
                        }
                    } else {
                        if let Some(aud) = audience_handle.upgrade() {
                            aud.set_laser_visible(false);
                        }
                    }
                    return;
                }
                return;
            }

            // Direct "Draw Straight Lines" mode for Highlighter and Tape
            if st.current_tool == ActiveTool::Highlighter && st.highlighter_straight_lines {
                if let Some(stroke) = st.active_stroke.as_mut() {
                    let start_p = stroke.points[0];
                    let dx = (x - start_p.x).abs();
                    let dy = (y - start_p.y).abs();
                    // Text line horizontal snap: lock end_y to start_p.y when moving roughly horizontally
                    let end_y = if dy < 0.04 || (dx > 0.02 && dy < dx * 0.35) {
                        start_p.y
                    } else {
                        y
                    };
                    let end_pt = Point::new(x, end_y, pressure, now_ms);
                    stroke.points = vec![start_p, end_pt];
                    stroke.is_shape = true;
                    stroke.recompute_bounding_box();
                    drop(st);
                    update_ink();
                    return;
                }
            }

            if st.is_shape_snapped {
                let stroke = st.active_stroke.as_mut().unwrap();
                if stroke.points.len() == 2 {
                    stroke.points[1].x = x;
                    if stroke.is_highlighter {
                        let dy = (y - stroke.points[0].y).abs();
                        let dx = (x - stroke.points[0].x).abs();
                        if dy < 0.04 || dy < dx * 0.35 {
                            stroke.points[1].y = stroke.points[0].y;
                        } else {
                            stroke.points[1].y = y;
                        }
                    } else {
                        stroke.points[1].y = y;
                    }
                    stroke.recompute_bounding_box();
                    drop(st);
                    update_ink();
                }
                return;
            }

            let dist = ((x - st.hold_anchor_pos.0).powi(2) + (y - st.hold_anchor_pos.1).powi(2)).sqrt();
            if dist > 0.012 {
                st.hold_anchor_pos = (x, y);
                st.last_pointer_move_time = std::time::Instant::now();
            }

            // Straight highlighting assistance across text lines
            let mut pt_y = y;
            if st.current_tool == ActiveTool::Highlighter && !st.raw_stroke_backup.is_empty() {
                let start_p = st.raw_stroke_backup[0];
                let dx = (x - start_p.x).abs();
                let dy = (y - start_p.y).abs();
                if dx > 0.025 && dy < 0.035 {
                    pt_y = start_p.y;
                }
            }

            let p = Point::new(x, pt_y, pressure, now_ms);
            st.raw_stroke_backup.push(p);
            st.active_stroke.as_mut().unwrap().add_point(p);
            drop(st);
            update_ink();
        });
    }

    {
        let state = state.clone();
        let update_ink = update_ink_overlay.clone();
        let update_view = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        let audience_handle = audience_window.as_weak();

        presenter_window.on_canvas_pointer_up(move || {
            let mut st = state.borrow_mut();
            st.pointer_is_down = false;

            if let Some(p) = presenter_handle.upgrade() {
                p.set_link_holding(false);
                p.set_link_hold_progress(0.0);
            }

            // Check if pointer up activates an interactive link (click or tap on link)
            if !st.link_triggered {
                if let Some(link) = st.active_link_target.take() {
                    let elapsed_ms = st.last_pointer_move_time.elapsed().as_millis() as u64;
                    let is_click = elapsed_ms < 400;
                    let is_click_or_laser = is_click || st.current_tool == ActiveTool::Laser;
                    if is_click_or_laser {
                        st.active_stroke = None;
                        st.raw_stroke_backup.clear();
                        st.link_triggered = true;

                        match link.target {
                            PdfLinkTarget::Url(url) => {
                                println!("Clicked PDF link: {}", url);
                                open_external_url(&url);
                            }
                            PdfLinkTarget::Page(dest_idx) => {
                                println!("Clicked PDF destination: jump to slide {}", dest_idx + 1);
                                if st.deck.jump_to(dest_idx) {
                                    drop(st);
                                    update_view();
                                    update_ink();
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            st.active_link_target = None;

            let elapsed_ms = st.last_pointer_move_time.elapsed().as_millis() as u64;
            let should_snap = !st.is_shape_snapped && st.auto_shape_enabled && elapsed_ms >= st.hold_delay_ms;

            if should_snap {
                if let Some(stroke) = st.active_stroke.as_mut() {
                    if stroke.points.len() >= 3 {
                        if let Some(recognized) = ink::ShapeRecognizer::recognize(&stroke.points, stroke.is_highlighter) {
                            stroke.points = recognized.points;
                            stroke.is_shape = true;
                            stroke.recompute_bounding_box();
                        }
                    }
                }
            }

            if st.current_tool == ActiveTool::Highlighter && st.highlighter_straight_lines {
                if let Some(stroke) = st.active_stroke.as_mut() {
                    stroke.is_shape = true;
                    stroke.recompute_bounding_box();
                }
            }

            let has_stroke = if let Some(stroke) = st.active_stroke.take() {
                st.deck.current_slide_mut().inking.add_stroke(stroke);
                true
            } else {
                false
            };
            st.is_shape_snapped = false;
            st.raw_stroke_backup.clear();

            if st.current_tool == ActiveTool::Laser {
                st.laser_active = false;
                if let Some(trail) = st.laser_trail.as_mut() {
                    trail.end_stroke();
                }
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_laser_active(false);
                }
                if let Some(aud) = audience_handle.upgrade() {
                    aud.set_laser_visible(false);
                }
                drop(st);
                update_ink();
                return;
            }

            if st.current_tool == ActiveTool::Eraser && st.auto_select_previous_tool {
                let prev = st.previous_tool;
                st.current_tool = prev;
                if let Some(p) = presenter_handle.upgrade() {
                    p.set_current_tool(prev);
                }
                println!("Auto-selected previous tool: {:?}", prev);
            }
            drop(st);
            if has_stroke {
                update_ink();
            }
        });
    }

    // --- MULTI-WINDOW POP-OUT TOGGLES ---
    {
        let audience_win = audience_window.as_weak();
        let presenter_win = presenter_window.as_weak();
        let mut is_open = false;

        presenter_window.on_toggle_audience_window(move || {
            if let Some(aud) = audience_win.upgrade() {
                is_open = !is_open;
                if is_open {
                    let _ = aud.show();
                    println!("Audience Window popped out to separate monitor / display.");
                } else {
                    let _ = aud.hide();
                    println!("Audience Window docked / hidden.");
                }
                if let Some(pres) = presenter_win.upgrade() {
                    pres.set_audience_window_open(is_open);
                }
            }
        });
    }

    {
        let notes_win = notes_window.as_weak();
        let presenter_win = presenter_window.as_weak();
        let update_view = update_slide_view.clone();
        let mut is_open = false;

        presenter_window.on_toggle_notes_window(move || {
            if let Some(notes) = notes_win.upgrade() {
                is_open = !is_open;
                if is_open {
                    update_view();
                    let _ = notes.show();
                    println!("Speaker Notes window popped out to floating monitor.");
                } else {
                    let _ = notes.hide();
                    println!("Speaker Notes window closed.");
                }
                if let Some(pres) = presenter_win.upgrade() {
                    pres.set_notes_window_open(is_open);
                }
            }
        });
    }

    // --- SPEAKER COMMENTS & NOTES REAL-TIME SYNC ---
    {
        let state = state.clone();
        let notes_handle = notes_window.as_weak();
        presenter_window.on_update_slide_notes(move |slide_num, new_notes| {
            let mut st = state.borrow_mut();
            let idx = (slide_num - 1) as usize;
            if idx < st.deck.slides.len() {
                st.deck.slides[idx].notes = new_notes.to_string();
                if let Some(n) = notes_handle.upgrade() {
                    if idx == st.deck.current_index {
                        n.set_notes_body(new_notes);
                    }
                }
            }
        });
    }

    {
        let state = state.clone();
        let presenter_handle = presenter_window.as_weak();
        notes_window.on_update_slide_notes(move |slide_num, new_notes| {
            let mut st = state.borrow_mut();
            let idx = (slide_num - 1) as usize;
            if idx < st.deck.slides.len() {
                st.deck.slides[idx].notes = new_notes.to_string();
                if let Some(p) = presenter_handle.upgrade() {
                    if idx == st.deck.current_index {
                        p.set_notes_text(new_notes);
                    }
                }
            }
        });
    }

    // --- OPEN PDF & EXPORT PDF ---
    {
        let state = state.clone();
        let update = update_slide_view.clone();
        let presenter_handle = presenter_window.as_weak();
        presenter_window.on_open_pdf(move || {
            let picked = rfd::FileDialog::new()
                .add_filter("PDF Presentations", &["pdf"])
                .set_title("Open Beamer LaTeX PDF Presentation")
                .pick_file();

            if let Some(path) = picked {
                println!("Selected PDF: {}", path.display());
                let mut st = state.borrow_mut();
                match pdf::PdfiumLoader::load_deck_from_file(&path, st.deck.quality) {
                    Ok((slides, toc)) => {
                        println!("Successfully loaded {} slides and {} TOC entries from Beamer PDF!", slides.len(), toc.len());
                        st.deck.slides = slides;
                        st.deck.toc = toc;
                        st.deck.current_index = 0;
                        st.deck.file_path = Some(path);
                        drop(st);
                        update();
                        if let Some(p) = presenter_handle.upgrade() {
                            let st = state.borrow();
                            update_slide_thumbnails_model(&st.deck, &p);
                        }
                    }
                    Err(err) => {
                        println!("Notice: {}; keeping current interactive deck.", err);
                    }
                }
            }
        });
    }

    {
        let state = state.clone();
        presenter_window.on_export_pdf(move || {
            let dest = rfd::FileDialog::new()
                .add_filter("PDF Presentation", &["pdf"])
                .set_file_name("presentation_annotated.pdf")
                .set_title("Save Presentation Annotations")
                .save_file();

            if let Some(path) = dest {
                let st = state.borrow();
                match PdfExporter::export_annotated_presentation(&st.deck, &path) {
                    Ok(msg) => println!("Export successful: {}", msg),
                    Err(err) => eprintln!("Export failed: {}", err),
                }
            }
        });
    }

    // --- TIMER TOGGLE & RESET ---
    {
        let state = state.clone();
        presenter_window.on_toggle_timer(move || {
            let mut st = state.borrow_mut();
            st.timer.toggle();
        });
    }

    {
        let state = state.clone();
        presenter_window.on_reset_timer(move || {
            let mut st = state.borrow_mut();
            st.timer.reset();
            println!("Presentation timer reset to 00:00");
        });
    }

    {
        let state = state.clone();
        let update = update_slide_view.clone();
        presenter_window.on_quality_changed(move |quality_str| {
            let mut st = state.borrow_mut();
            if quality_str.contains("600") {
                st.deck.quality = RenderQuality::UltraVector;
            } else if quality_str.contains("300") {
                st.deck.quality = RenderQuality::Balanced;
            } else {
                st.deck.quality = RenderQuality::Eco;
            }
            println!("Rendering quality set to: {:?}", st.deck.quality);
            drop(st);
            update();
        });
    }

    // --- GITHUB AUTO-UPDATER & OPTIONS INTEGRATION ---
    presenter_window.set_app_version(SharedString::from(env!("CARGO_PKG_VERSION")));
    let update_info_cell = std::sync::Arc::new(std::sync::Mutex::new(None::<UpdateInfo>));

    {
        let p_weak = presenter_window.as_weak();
        let info_cell = update_info_cell.clone();
        presenter_window.on_check_for_updates(move || {
            if let Some(p) = p_weak.upgrade() {
                p.set_update_status(SharedString::from("checking"));
                p.set_update_status_message(SharedString::new());
            }
            let p_weak = p_weak.clone();
            let info_cell = info_cell.clone();
            std::thread::spawn(move || {
                let res = updater::check_for_update(updater::DEFAULT_REPO);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(p) = p_weak.upgrade() {
                        match res {
                            Ok(Some(info)) => {
                                p.set_update_status(SharedString::from("available"));
                                p.set_latest_version(SharedString::from(&info.latest_version));
                                p.set_release_notes(SharedString::from(&info.release_notes));
                                p.set_release_url(SharedString::from(&info.release_url));
                                p.set_update_available_badge(true);
                                *info_cell.lock().unwrap() = Some(info);
                            }
                            Ok(None) => {
                                p.set_update_status(SharedString::from("up_to_date"));
                                p.set_update_available_badge(false);
                            }
                            Err(err) => {
                                p.set_update_status(SharedString::from("error"));
                                p.set_update_status_message(SharedString::from(err));
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let p_weak = presenter_window.as_weak();
        let info_cell = update_info_cell.clone();
        presenter_window.on_install_update(move || {
            let maybe_info = info_cell.lock().unwrap().clone();
            if let Some(info) = maybe_info {
                if let Some(p) = p_weak.upgrade() {
                    p.set_update_status(SharedString::from("downloading"));
                    p.set_update_progress(0.0);
                }
                let p_weak = p_weak.clone();
                std::thread::spawn(move || {
                    let p_weak_progress = p_weak.clone();
                    let res = updater::download_and_install_update(&info.download_url, move |prog| {
                        let p_weak_progress = p_weak_progress.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(p) = p_weak_progress.upgrade() {
                                p.set_update_progress(prog);
                            }
                        });
                    });

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(p) = p_weak.upgrade() {
                            match res {
                                Ok(()) => {
                                    p.set_update_status(SharedString::from("ready"));
                                }
                                Err(err) => {
                                    p.set_update_status(SharedString::from("error"));
                                    p.set_update_status_message(SharedString::from(err));
                                }
                            }
                        }
                    });
                });
            }
        });
    }

    presenter_window.on_restart_after_update(move || {
        updater::restart_application();
    });

    presenter_window.on_open_external_url(move |url| {
        open_external_url(url.as_str());
    });

    // Background non-blocking update check on startup
    {
        let p_weak = presenter_window.as_weak();
        let info_cell = update_info_cell.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Ok(Some(info)) = updater::check_for_update(updater::DEFAULT_REPO) {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(p) = p_weak.upgrade() {
                        p.set_update_status(SharedString::from("available"));
                        p.set_latest_version(SharedString::from(&info.latest_version));
                        p.set_release_notes(SharedString::from(&info.release_notes));
                        p.set_release_url(SharedString::from(&info.release_url));
                        p.set_update_available_badge(true);
                        *info_cell.lock().unwrap() = Some(info);
                    }
                });
            }
        });
    }

    println!("Starting Beamer Presenter main window loop...");
    presenter_window.run()?;

    Ok(())
}

