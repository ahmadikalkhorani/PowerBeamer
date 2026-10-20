//! User preferences persisted between launches (tool options, colors, laser, sync, quality).
//!
//! Every field is optional so older or hand-edited files load fine: anything missing keeps
//! the built-in default.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sync_with_source: Option<bool>,
    pub render_quality: Option<String>,

    pub stroke_width: Option<f32>,
    pub pen_type: Option<i32>,
    pub auto_shape_enabled: Option<bool>,
    pub scribble_to_erase: Option<bool>,
    pub hold_delay_ms: Option<u64>,
    pub enhanced_sensitivity: Option<bool>,
    pub draw_on_hover: Option<bool>,

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
}

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
