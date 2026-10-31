//! Application state shared by all UI callbacks, plus deck loading and preferences.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use image::RgbaImage;
use slint::{Image, SharedString};

use super::ui::{lock, to_slint_color};
use crate::ink::{LaserTrail, Point, Stroke};
use crate::media::probe_video_duration;
use crate::pdf::{self, PdfLinkInfo, PresentationDeck, RenderQuality, Slide, TocEntry};
use crate::settings::Settings;
use crate::timer::PresentationTimer;
use crate::{ActiveTool, PresenterWindow, TocItemModel};

/// Probes durations of all resolved media clips on a background thread so `ffprobe`
/// never runs on the UI thread.
pub(crate) fn prefetch_media_durations(slides: &[Slide], cache: &Arc<Mutex<HashMap<String, f32>>>) {
    let paths: Vec<String> = {
        let known = lock(&cache);
        slides
            .iter()
            .flat_map(|s| s.media_clips.iter())
            .filter(|c| c.is_resolved && !known.contains_key(&c.file_path))
            .map(|c| c.file_path.clone())
            .collect()
    };
    probe_media_durations(paths, cache);
}

pub(crate) fn probe_media_durations(paths: Vec<String>, cache: &Arc<Mutex<HashMap<String, f32>>>) {
    if paths.is_empty() {
        return;
    }
    use std::collections::HashSet;
    use std::sync::OnceLock;
    type Probe = (String, Arc<Mutex<HashMap<String, f32>>>);
    static PENDING: OnceLock<Mutex<HashSet<(usize, String)>>> = OnceLock::new();
    static PROBES: OnceLock<std::sync::mpsc::Sender<Probe>> = OnceLock::new();
    let pending = PENDING.get_or_init(Default::default);
    let sender = PROBES.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Probe>();
        std::thread::spawn(move || {
            while let Ok((path, cache)) = rx.recv() {
                #[cfg(test)]
                MEDIA_PROBE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let duration = probe_video_duration(&path);
                lock(&cache).insert(path.clone(), duration);
                lock(PENDING.get().unwrap()).remove(&(Arc::as_ptr(&cache) as usize, path));
            }
        });
        tx
    });
    for path in paths {
        if lock(&cache).contains_key(&path) {
            continue;
        }
        let key = (Arc::as_ptr(cache) as usize, path.clone());
        if lock(pending).insert(key) {
            let _ = sender.send((path, cache.clone()));
        }
    }
}

/// Cheap content fingerprint of committed strokes, used to validate the ink cache.
/// Catches changes that keep the stroke count equal (tape reveal, undo/redo swaps, edits).
pub(crate) fn ink_fingerprint(strokes: &[Stroke]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    strokes.len().hash(&mut h);
    for s in strokes {
        s.id.hash(&mut h);
        s.points.len().hash(&mut h);
        s.color_rgba.hash(&mut h);
        s.width.to_bits().hash(&mut h);
        (s.is_highlighter, s.is_shape, s.is_tape, s.is_tape_revealed).hash(&mut h);
        if let (Some(a), Some(b)) = (s.points.first(), s.points.last()) {
            (a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits()).hash(&mut h);
        }
    }
    h.finish()
}

/// Persistent raster of (committed ink + finalized part of the in-progress stroke).
pub(crate) struct ActiveInkLayer {
    pub(crate) img: RgbaImage,
    pub(crate) base_generation: u64,
    pub(crate) stroke_id: u64,
    pub(crate) committed_segs: usize,
    /// Last raw point included in `committed_segs`, to detect stroke rewrites (shape snap, undo).
    pub(crate) anchor: Option<Point>,
}

pub(crate) struct AppState {
    pub(crate) deck: PresentationDeck,
    pub(crate) timer: PresentationTimer,
    pub(crate) active_stroke: Option<Stroke>,
    pub(crate) laser_active: bool,
    pub(crate) active_color: [u8; 4],
    pub(crate) stroke_width: f32,
    pub(crate) pen_stroke_width: f32,
    pub(crate) current_tool: ActiveTool,
    pub(crate) stroke_counter: u64,

    // Pen & Draw-and-hold settings
    pub(crate) auto_shape_enabled: bool,
    pub(crate) scribble_to_erase: bool,
    pub(crate) hold_delay_ms: u64,

    // Highlighter settings
    pub(crate) highlighter_mode: i32, // 0: Highlighter, 1: Tape
    pub(crate) highlighter_straight_lines: bool,
    pub(crate) highlighter_size_mm: String,

    // Eraser settings
    pub(crate) eraser_size_index: i32,
    pub(crate) eraser_radius: f32,
    pub(crate) erase_whole_line: bool,
    pub(crate) erase_highlight_only: bool,
    pub(crate) auto_select_previous_tool: bool,
    pub(crate) erase_shapes: bool,
    pub(crate) erase_tapes: bool,
    pub(crate) previous_tool: ActiveTool,

    // Laser Pointer & Draw Lines settings
    pub(crate) laser_color_index: i32,
    pub(crate) laser_color_rgba: [u8; 4],
    pub(crate) laser_draw_lines: bool,
    pub(crate) laser_fade_duration_s: f32,
    pub(crate) laser_trail: Option<LaserTrail>,
    pub(crate) last_laser_update: std::time::Instant,

    // High-performance base inking overlay cache
    pub(crate) cached_ink_overlay: Option<RgbaImage>,
    pub(crate) cached_ink_slint: Option<Image>,
    pub(crate) cached_ink_slide_idx: usize,
    pub(crate) cached_ink_fingerprint: u64,
    pub(crate) ink_base_generation: u64,
    pub(crate) active_ink_layer: Option<ActiveInkLayer>,

    // Color Slots & Custom Palette
    pub(crate) active_color_slot: i32,
    pub(crate) color_slot_1: [u8; 4],
    pub(crate) color_slot_2: [u8; 4],
    pub(crate) color_slot_3: [u8; 4],
    pub(crate) color_slot_4: [u8; 4],
    pub(crate) color_slot_5: [u8; 4],
    pub(crate) custom_palette: Vec<[u8; 4]>,

    // Presentation timer options (see the timer menu)
    pub(crate) timer_countdown: bool,
    pub(crate) timer_duration_min: i32,
    pub(crate) timer_warn_min: i32,
    pub(crate) timer_flash_overtime: bool,
    pub(crate) timer_show_clock: bool,
    pub(crate) notes_font_size: i32,
    pub(crate) timer_size: i32, // 0: S, 1: M, 2: L
    pub(crate) timer_start_on_advance: bool,
    // Slide the timer is waiting to leave before it starts (start-on-first-advance)
    pub(crate) timer_armed_at_slide: Option<usize>,

    // Slide Media Player Toolbar state
    pub(crate) media_playing: bool,
    pub(crate) media_progress: f32,
    pub(crate) media_duration_secs: f32,
    pub(crate) media_current_time_secs: f32,
    pub(crate) media_speed: f32,
    pub(crate) media_speech_active: bool,
    pub(crate) media_playback_started: Option<std::time::Instant>,
    pub(crate) media_playback_start_offset: f32,
    // Clip the toolbar drives on a slide with several clips, as (slide index, clip index)
    pub(crate) active_media_clip: (usize, usize),

    // In-flight gesture & hold detection
    pub(crate) pointer_is_down: bool,
    pub(crate) last_pointer_move_time: std::time::Instant,
    pub(crate) hold_anchor_pos: (f32, f32),
    pub(crate) is_shape_snapped: bool,
    pub(crate) raw_stroke_backup: Vec<Point>,

    // PDF Interactive Links Press & Hold State
    pub(crate) active_link_target: Option<PdfLinkInfo>,
    pub(crate) link_press_start: Option<std::time::Instant>,
    pub(crate) link_triggered: bool,

    // Media clip target clicked
    pub(crate) active_media_target: Option<usize>,

    // Presentation Mode & Display Swapping
    pub(crate) blank_mode: i32, // 0: Normal, 1: Blackout, 2: Whiteout
    pub(crate) is_screen_swapped: bool,
    pub(crate) is_end_of_show: bool,

    // Media durations probed off the UI thread, keyed by file path
    pub(crate) media_durations: Arc<Mutex<HashMap<String, f32>>>,
    pub(crate) media_duration_pending: Option<String>,

    // TOC model is built once per deck; slide changes only flip `is_active` on rows
    pub(crate) toc_model: Option<Rc<slint::VecModel<TocItemModel>>>,

    // PDF currently being rendered on a worker thread, if any
    pub(crate) deck_load: Option<DeckLoadJob>,

    // Source PDF sync: reload when the original file changes on disk
    pub(crate) sync_with_source: bool,
    pub(crate) source_stamp: Option<FileStamp>,
    pub(crate) pending_source_stamp: Option<FileStamp>,

    // Zoom & Pan state
    pub(crate) zoom_level: f32,
    pub(crate) zoom_pan_x: f32,
    pub(crate) zoom_pan_y: f32,
    pub(crate) selection_box: Option<(f32, f32, f32, f32)>,

    // Spectrogram/Waveform Audio Timeline Progress Box Tool
    pub(crate) media_tracking_tool_active: bool,
    pub(crate) drawing_track_box: Option<(f32, f32, f32, f32)>,
    pub(crate) media_track_fps_mode: i32, // 0: Auto (Linked to quality), 1: 15 fps, 2: 30 fps, 3: 60 fps
    pub(crate) media_track_show_audience_box: bool,
    pub(crate) media_track_needle_width: f32,
    pub(crate) media_track_needle_glow: bool,
    pub(crate) media_track_loop: bool,
}

impl AppState {
    pub(crate) fn capture_settings(&self) -> Settings {
        Settings {
            sync_with_source: Some(self.sync_with_source),
            render_quality: Some(self.deck.quality.label().to_string()),
            stroke_width: Some(self.pen_stroke_width),
            auto_shape_enabled: Some(self.auto_shape_enabled),
            scribble_to_erase: Some(self.scribble_to_erase),
            hold_delay_ms: Some(self.hold_delay_ms),
            highlighter_mode: Some(self.highlighter_mode),
            highlighter_straight_lines: Some(self.highlighter_straight_lines),
            highlighter_size_mm: Some(self.highlighter_size_mm.clone()),
            eraser_size_index: Some(self.eraser_size_index),
            eraser_radius: Some(self.eraser_radius),
            erase_whole_line: Some(self.erase_whole_line),
            erase_highlight_only: Some(self.erase_highlight_only),
            auto_select_previous_tool: Some(self.auto_select_previous_tool),
            erase_shapes: Some(self.erase_shapes),
            erase_tapes: Some(self.erase_tapes),
            laser_color_index: Some(self.laser_color_index),
            laser_color_rgba: Some(self.laser_color_rgba),
            laser_draw_lines: Some(self.laser_draw_lines),
            laser_fade_duration_s: Some(self.laser_fade_duration_s),
            active_color_slot: Some(self.active_color_slot),
            color_slots: Some([
                self.color_slot_1,
                self.color_slot_2,
                self.color_slot_3,
                self.color_slot_4,
                self.color_slot_5,
            ]),
            custom_palette: Some(self.custom_palette.clone()),
            media_speech_active: Some(self.media_speech_active),
            timer_countdown: Some(self.timer_countdown),
            timer_duration_min: Some(self.timer_duration_min),
            timer_warn_min: Some(self.timer_warn_min),
            timer_flash_overtime: Some(self.timer_flash_overtime),
            timer_show_clock: Some(self.timer_show_clock),
            notes_font_size: Some(self.notes_font_size),
            timer_size: Some(self.timer_size),
            timer_start_on_advance: Some(self.timer_start_on_advance),
            media_track_fps_mode: Some(self.media_track_fps_mode),
            media_track_show_audience_box: Some(self.media_track_show_audience_box),
            media_track_needle_width: Some(self.media_track_needle_width),
            media_track_needle_glow: Some(self.media_track_needle_glow),
            media_track_loop: Some(self.media_track_loop),
        }
    }

    pub(crate) fn apply_settings(&mut self, s: &Settings) {
        if let Some(width) = s.stroke_width {
            self.pen_stroke_width = width;
        }
        macro_rules! set {
            ($($field:ident),*) => { $( if let Some(v) = s.$field.clone() { self.$field = v; } )* };
        }
        set!(
            sync_with_source,
            stroke_width,
            auto_shape_enabled,
            scribble_to_erase,
            hold_delay_ms,
            highlighter_mode,
            highlighter_straight_lines,
            highlighter_size_mm,
            eraser_size_index,
            eraser_radius,
            erase_whole_line,
            erase_highlight_only,
            auto_select_previous_tool,
            erase_shapes,
            erase_tapes,
            laser_color_index,
            laser_color_rgba,
            laser_draw_lines,
            laser_fade_duration_s,
            active_color_slot,
            custom_palette,
            media_speech_active,
            timer_countdown,
            timer_duration_min,
            timer_warn_min,
            timer_flash_overtime,
            timer_show_clock,
            notes_font_size,
            timer_size,
            timer_start_on_advance,
            media_track_fps_mode,
            media_track_show_audience_box,
            media_track_needle_width,
            media_track_needle_glow,
            media_track_loop
        );
        self.timer
            .set_target_minutes(self.timer_duration_min.max(1) as u64);
        if self.timer_start_on_advance {
            self.arm_timer();
        }
        if let Some(q) = &s.render_quality {
            self.deck.quality = RenderQuality::from_label(q);
        }
        if let Some([c1, c2, c3, c4, c5]) = s.color_slots {
            (
                self.color_slot_1,
                self.color_slot_2,
                self.color_slot_3,
                self.color_slot_4,
                self.color_slot_5,
            ) = (c1, c2, c3, c4, c5);
        }
        let slots = [
            self.color_slot_1,
            self.color_slot_2,
            self.color_slot_3,
            self.color_slot_4,
            self.color_slot_5,
        ];
        if let Some(c) = slots.get(self.active_color_slot.clamp(0, 4) as usize) {
            self.active_color = *c;
        }
    }

    /// Reflects persisted preferences in the presenter UI after startup.
    pub(crate) fn push_settings_to_ui(&self, p: &PresenterWindow) {
        p.set_sync_with_source(self.sync_with_source);
        p.set_render_quality(SharedString::from(self.deck.quality.label()));
        p.set_stroke_width(self.stroke_width);
        let pen_mm = self.pen_stroke_width / 4.2;
        p.set_pen_size_value(pen_mm);
        p.set_pen_size_mm(SharedString::from(format!("{:.1}mm", pen_mm)));
        p.set_auto_shape_enabled(self.auto_shape_enabled);
        p.set_scribble_to_erase(self.scribble_to_erase);
        let hold_s = self.hold_delay_ms as f32 / 1000.0;
        p.set_hold_delay_s(hold_s);
        p.set_hold_delay_text(SharedString::from(format!("{:.1}s", hold_s)));
        p.set_highlighter_mode(self.highlighter_mode);
        p.set_highlighter_straight_lines(self.highlighter_straight_lines);
        p.set_highlighter_size_mm(SharedString::from(&self.highlighter_size_mm));
        if let Ok(v) = self
            .highlighter_size_mm
            .trim_end_matches("mm")
            .parse::<f32>()
        {
            p.set_highlighter_size_value(v);
        }
        p.set_eraser_size_index(self.eraser_size_index);
        p.set_eraser_radius(self.eraser_radius);
        p.set_erase_whole_line(self.erase_whole_line);
        p.set_erase_highlight_only(self.erase_highlight_only);
        p.set_auto_select_previous_tool(self.auto_select_previous_tool);
        p.set_erase_shapes(self.erase_shapes);
        p.set_erase_tapes(self.erase_tapes);
        p.set_laser_color_index(self.laser_color_index);
        p.set_laser_color(to_slint_color(self.laser_color_rgba));
        p.set_laser_draw_lines(self.laser_draw_lines);
        p.set_laser_fade_duration_s(self.laser_fade_duration_s);
        p.set_laser_fade_duration_text(SharedString::from(format!(
            "{:.1}s",
            self.laser_fade_duration_s
        )));
        p.set_active_color_slot(self.active_color_slot);
        p.set_active_color(to_slint_color(self.active_color));
        p.set_color_slot_1(to_slint_color(self.color_slot_1));
        p.set_color_slot_2(to_slint_color(self.color_slot_2));
        p.set_color_slot_3(to_slint_color(self.color_slot_3));
        p.set_color_slot_4(to_slint_color(self.color_slot_4));
        p.set_color_slot_5(to_slint_color(self.color_slot_5));
        let palette: Vec<slint::Color> = self
            .custom_palette
            .iter()
            .map(|c| to_slint_color(*c))
            .collect();
        p.set_custom_palette(Rc::new(slint::VecModel::from(palette)).into());
        p.set_media_speech_active(self.media_speech_active);
        p.set_timer_countdown(self.timer_countdown);
        p.set_timer_duration_min(self.timer_duration_min);
        p.set_timer_warn_min(self.timer_warn_min);
        p.set_timer_flash_overtime(self.timer_flash_overtime);
        p.set_timer_show_clock(self.timer_show_clock);
        p.set_notes_font_size(self.notes_font_size.clamp(8, 48));
        p.set_timer_size(self.timer_size);
        p.set_timer_start_on_advance(self.timer_start_on_advance);
        p.set_media_track_fps_mode(self.media_track_fps_mode);
        p.set_media_track_show_audience_box(self.media_track_show_audience_box);
        p.set_media_track_needle_width(self.media_track_needle_width);
        p.set_media_track_needle_glow(self.media_track_needle_glow);
        p.set_media_track_loop(self.media_track_loop);
    }

    /// Stops the timer at zero until the presenter leaves the current slide.
    pub(crate) fn arm_timer(&mut self) {
        self.timer.pause();
        self.timer.reset();
        self.timer_armed_at_slide = Some(self.deck.current_index);
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DeckLoadKind {
    /// Replace the deck (File > Open, command line).
    Open,
    /// Merge a rebuilt source PDF into the current deck, keeping ink and inserted pages.
    Sync,
    /// Re-render the same PDF at a new quality, merged like `Sync`.
    Requality,
}

pub(crate) type DeckLoadResult = Result<(Vec<Slide>, Vec<TocEntry>), String>;

pub(crate) struct DeckLoadJob {
    pub(crate) id: u64,
    pub(crate) path: std::path::PathBuf,
    pub(crate) kind: DeckLoadKind,
}

impl AppState {
    /// One worker renders at a time. A newer request replaces pending work and
    /// cancels an active PDF render at the next page boundary.
    pub(crate) fn start_deck_load(&mut self, path: std::path::PathBuf, kind: DeckLoadKind) {
        use std::sync::{OnceLock, atomic::Ordering};
        type Request = (std::path::PathBuf, RenderQuality);
        static WORKER: OnceLock<crate::work::LatestWorker<Request>> = OnceLock::new();
        let worker = WORKER.get_or_init(|| {
            crate::work::LatestWorker::new(|id, (path, quality): Request, generation| {
                let cancelled = || generation.load(Ordering::Acquire) != id;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if pdf::PptxEngine::is_pptx(&path) {
                        pdf::PptxEngine::load_deck_cancellable(&path, quality, &cancelled)
                    } else {
                        pdf::PdfiumLoader::load_deck_cancellable(&path, quality, &cancelled)
                    }
                }))
                .unwrap_or_else(|_| Err("presentation loader panicked".into()));
                if !cancelled() {
                    if let Err(error) =
                        slint::invoke_from_event_loop(move || super::files::finish_load(id, result))
                    {
                        eprintln!("Couldn't deliver presentation load result: {error}");
                    }
                }
            })
        });
        let id = worker.submit((path.clone(), self.deck.quality));
        self.deck_load = Some(DeckLoadJob { id, path, kind });
    }
}

/// Modification time + size of a file, used to detect rebuilds of the source PDF.
pub(crate) type FileStamp = (std::time::SystemTime, u64);

pub(crate) fn file_stamp(path: &std::path::Path) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl AppState {
    /// Installs a freshly opened deck and resets every cache derived from the previous one.
    pub(crate) fn install_deck(
        &mut self,
        slides: Vec<Slide>,
        toc: Vec<TocEntry>,
        path: std::path::PathBuf,
    ) {
        lock(&self.media_durations).clear();
        self.deck.slides = slides;
        self.deck.toc = toc;
        self.deck.current_index = 0;
        self.source_stamp = file_stamp(&path);
        self.pending_source_stamp = None;
        self.deck.file_path = Some(path);
        self.on_deck_contents_changed();
    }

    pub(crate) fn on_deck_contents_changed(&mut self) {
        self.toc_model = None;
        self.cached_ink_overlay = None;
        self.cached_ink_slint = None;
        self.active_ink_layer = None;
        prefetch_media_durations(&self.deck.slides, &self.media_durations);
    }
}

impl AppState {
    /// Index of the clip the media toolbar drives on the current slide (0 after a slide change).
    pub(crate) fn active_clip_index(&self) -> usize {
        let (slide, idx) = self.active_media_clip;
        if slide == self.deck.current_index && idx < self.deck.current_slide().media_clips.len() {
            idx
        } else {
            0
        }
    }

    pub(crate) fn active_clip(&self) -> Option<pdf::MediaClip> {
        self.deck
            .current_slide()
            .media_clips
            .get(self.active_clip_index())
            .cloned()
    }

    pub(crate) fn format_media_time(&self) -> String {
        let cur = self.media_current_time_secs as u32;
        let tot = self.media_duration_secs as u32;
        format!(
            "{:02}:{:02} / {:02}:{:02}",
            cur / 60,
            cur % 60,
            tot / 60,
            tot % 60
        )
    }

    pub(crate) fn format_media_speed(&self) -> String {
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

    pub(crate) fn new() -> Self {
        Self {
            deck: PresentationDeck::new_demo_deck(RenderQuality::Balanced),
            timer: PresentationTimer::new(45),
            active_stroke: None,
            laser_active: false,
            active_color: [254, 240, 78, 255], // Yellow #fef04e
            stroke_width: 3.5,
            pen_stroke_width: 3.5,
            current_tool: ActiveTool::Laser,
            stroke_counter: 0,

            timer_countdown: false,
            timer_duration_min: 45,
            timer_warn_min: 5,
            timer_flash_overtime: true,
            timer_show_clock: false,
            notes_font_size: 14,
            timer_size: 0,
            timer_start_on_advance: false,
            timer_armed_at_slide: None,

            // Media Player defaults
            media_playing: false,
            media_progress: 0.0,
            media_duration_secs: 90.0,
            media_current_time_secs: 0.0,
            media_speed: 1.0,
            media_speech_active: true,
            media_playback_started: None,
            media_playback_start_offset: 0.0,
            active_media_clip: (0, 0),

            blank_mode: 0,
            is_screen_swapped: false,
            is_end_of_show: false,

            auto_shape_enabled: true,
            scribble_to_erase: true,
            hold_delay_ms: 500,

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
            media_durations: Default::default(),
            media_duration_pending: None,
            toc_model: None,
            deck_load: None,
            sync_with_source: true,
            source_stamp: None,
            pending_source_stamp: None,
            cached_ink_overlay: None,
            cached_ink_slint: None,
            cached_ink_slide_idx: usize::MAX,
            cached_ink_fingerprint: 0,
            ink_base_generation: 0,
            active_ink_layer: None,

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
            active_media_target: None,

            zoom_level: 1.0,
            zoom_pan_x: 0.0,
            zoom_pan_y: 0.0,
            selection_box: None,
            media_tracking_tool_active: false,
            drawing_track_box: None,
            media_track_fps_mode: 0, // Auto
            media_track_show_audience_box: true,
            media_track_needle_width: 3.0,
            media_track_needle_glow: true,
            media_track_loop: false,
        }
    }
}

#[cfg(test)]
static MEDIA_PROBE_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
mod probe_tests {
    use super::*;
    #[test]
    fn duplicate_duration_requests_launch_one_probe() {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let before = MEDIA_PROBE_CALLS.load(std::sync::atomic::Ordering::Relaxed);
        for _ in 0..100 {
            probe_media_durations(vec!["test_deck/sample_video.mp4".into()], &cache);
        }
        let start = std::time::Instant::now();
        while lock(&cache).is_empty() {
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            MEDIA_PROBE_CALLS.load(std::sync::atomic::Ordering::Relaxed) - before,
            1
        );
    }
}
