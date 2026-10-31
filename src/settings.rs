//! User preferences persisted between launches (tool options, colors, laser, timer, sync, quality).
//!
//! Every field is optional so older or hand-edited files load fine: anything missing keeps
//! the built-in default.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub notes_font_size: Option<i32>,
    pub sync_with_source: Option<bool>,
    pub render_quality: Option<String>,

    pub stroke_width: Option<f32>,
    pub auto_shape_enabled: Option<bool>,
    pub scribble_to_erase: Option<bool>,
    pub hold_delay_ms: Option<u64>,

    pub highlighter_mode: Option<i32>,
    pub highlighter_straight_lines: Option<bool>,
    pub highlighter_size_mm: Option<String>,

    pub eraser_size_index: Option<i32>,
    pub eraser_radius: Option<f32>,
    pub erase_whole_line: Option<bool>,
    pub erase_highlight_only: Option<bool>,
    pub auto_select_previous_tool: Option<bool>,
    pub erase_shapes: Option<bool>,
    pub erase_tapes: Option<bool>,

    pub laser_color_index: Option<i32>,
    pub laser_color_rgba: Option<[u8; 4]>,
    pub laser_draw_lines: Option<bool>,
    pub laser_fade_duration_s: Option<f32>,

    pub active_color_slot: Option<i32>,
    pub color_slots: Option<[[u8; 4]; 5]>,
    pub custom_palette: Option<Vec<[u8; 4]>>,

    pub media_speech_active: Option<bool>,

    pub timer_countdown: Option<bool>,
    pub timer_duration_min: Option<i32>,
    pub timer_warn_min: Option<i32>,
    pub timer_flash_overtime: Option<bool>,
    pub timer_show_clock: Option<bool>,
    pub timer_size: Option<i32>,
    pub timer_start_on_advance: Option<bool>,

    pub media_track_fps_mode: Option<i32>, // 0: Auto (Linked to quality), 1: 15 fps, 2: 30 fps, 3: 60 fps
    pub media_track_show_audience_box: Option<bool>,
    pub media_track_needle_width: Option<f32>, // 2.0, 3.0, 4.0
    pub media_track_needle_glow: Option<bool>,
    pub media_track_loop: Option<bool>,
}

static WRITER: std::sync::OnceLock<crate::work::LatestWorker<Settings>> =
    std::sync::OnceLock::new();

impl Settings {
    /// Platform config location, e.g. `~/Library/Application Support/PowerBeamer/settings.json`.
    pub fn path() -> Option<PathBuf> {
        let base = if cfg!(target_os = "macos") {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        }?;
        Some(base.join("PowerBeamer").join("settings.json"))
    }

    /// Loads saved settings; a missing or unreadable file yields defaults.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Latest-value worker serializes writes and never performs disk I/O on the UI thread.
    pub fn save_background(&self) {
        WRITER
            .get_or_init(|| {
                crate::work::LatestWorker::new(|_, settings: Settings, _| {
                    if let Err(e) = settings.save() {
                        eprintln!("Couldn't save settings: {e}");
                    }
                })
            })
            .submit(self.clone());
    }

    pub fn flush_saves() {
        if let Some(writer) = WRITER.get() {
            writer.flush();
        }
    }

    /// Writes atomically (temp file + rename) so a crash never leaves a truncated file.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no config directory")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }
}
