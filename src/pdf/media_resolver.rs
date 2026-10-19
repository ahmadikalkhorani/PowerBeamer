#![allow(dead_code)]
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use image::RgbaImage;
use super::beamer::MediaClip;

/// Common media file extensions encountered in LaTeX Beamer presentations
pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "webm", "mov", "mkv", "avi", "m4v", "ogv",
    "mp3", "wav", "ogg", "flac", "m4a", "aac",
];

/// Common subdirectories where LaTeX authors store presentation media assets
pub const MEDIA_SUBDIRECTORIES: &[&str] = &[
    "",             // Direct in presentation folder
    "media",
    "videos",
    "video",
    "audio",
    "sounds",
    "sound",
    "assets",
    "figures",
    "figs",
    "fig",
    "movies",
    "clips",
    "resources",
    "res",
    "data",
];

/// Common build directory names used by latexmk, TeXstudio, VSCode LaTeX Workshop, etc.
pub const BUILD_DIRECTORIES: &[&str] = &[
    "build",
    "out",
    "output",
    "dist",
    "bin",
    "aux",
    ".aux",
    "_build",
];

#[derive(Clone, Debug)]
pub struct CompanionFrame {
    pub frame_index: usize,
    pub title: String,
    pub media_paths: Vec<String>,
    pub media_rects: HashMap<String, [f32; 4]>,
    pub notes: String,
}

pub struct MediaResolver;

impl MediaResolver {
    /// Normalize a raw media path string extracted from LaTeX or PDF annotations.
    /// Strips URL protocols (run:, file://), quotes, leading ./, unescapes \_, and trims whitespace.
    pub fn clean_raw_path(raw: &str) -> String {
        let mut s = raw.trim();

        // Strip surrounding quotes or braces
        if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
            s = &s[1..s.len() - 1].trim();
        }
        if s.starts_with('{') && s.ends_with('}') {
            s = &s[1..s.len() - 1].trim();
        }

        // Strip URI schemes commonly used in LaTeX hyperref and multimedia
        if let Some(stripped) = s.strip_prefix("run:") {
            s = stripped.trim();
        } else if let Some(stripped) = s.strip_prefix("file://") {
            s = stripped.trim();
        } else if let Some(stripped) = s.strip_prefix("file:") {
            s = stripped.trim();
        }

        // Normalize leading ./
        if let Some(stripped) = s.strip_prefix("./") {
            s = stripped.trim();
        }

        // Unescape LaTeX escaped underscores
        let unescaped = s.replace("\\_", "_");

        // Normalize Windows backslashes to forward slashes
        let mut normalized = unescaped.replace('\\', "/");

        // Trim leading / if it was originally a Windows relative path like \movies\demo.mp4
        if normalized.starts_with('/') && !raw.starts_with('/') {
            normalized = normalized.trim_start_matches('/').to_string();
        }

        normalized
    }

    /// Primary resolution engine: Given a base directory (where the PDF/TeX lives)
    /// and a raw relative/absolute path string, returns the canonical absolute path
    /// on disk if the file exists.
    pub fn resolve_path(base_dir: &Path, raw_path: &str) -> Option<PathBuf> {
        let cleaned = Self::clean_raw_path(raw_path);
        if cleaned.is_empty() {
            return None;
        }

        let input_path = Path::new(&cleaned);

        // 1. If path is already absolute and exists, return canonicalized path
        if input_path.is_absolute() && input_path.exists() {
            return fs::canonicalize(input_path).ok().or_else(|| Some(input_path.to_path_buf()));
        }

        // Determine candidate base directories (including parent if inside a build/ folder)
        let mut candidate_bases: Vec<PathBuf> = Vec::new();
        candidate_bases.push(base_dir.to_path_buf());

        // Check if base_dir is a known build directory (e.g. project/build -> check project/)
        if let Some(folder_name) = base_dir.file_name().and_then(|f| f.to_str()) {
            if BUILD_DIRECTORIES.iter().any(|&b| b.eq_ignore_ascii_case(folder_name)) {
                if let Some(parent) = base_dir.parent() {
                    candidate_bases.push(parent.to_path_buf());
                }
            }
        }
        // Also always include base_dir's parent as secondary fallback
        if let Some(parent) = base_dir.parent() {
            if !candidate_bases.contains(&parent.to_path_buf()) {
                candidate_bases.push(parent.to_path_buf());
            }
        }

        // Relative path variations to check
        let rel_variations = [
            cleaned.clone(),
            cleaned.trim_start_matches("./").to_string(),
        ];

        let file_name = input_path.file_name().and_then(|f| f.to_str()).unwrap_or(&cleaned);

        // 2. Cascade Search: Candidate base dirs × Standard media subdirectories × Rel variations
        for base in &candidate_bases {
            for rel in &rel_variations {
                for subdir in MEDIA_SUBDIRECTORIES {
                    let target = if subdir.is_empty() {
                        base.join(rel)
                    } else {
                        base.join(subdir).join(rel)
                    };

                    if target.exists() && target.is_file() {
                        return fs::canonicalize(&target).ok().or(Some(target));
                    }
                }
            }

            // 3. Fallback: Search by pure filename in candidate media subdirectories
            for subdir in MEDIA_SUBDIRECTORIES {
                let target = if subdir.is_empty() {
                    base.join(file_name)
                } else {
                    base.join(subdir).join(file_name)
                };

                if target.exists() && target.is_file() {
                    return fs::canonicalize(&target).ok().or(Some(target));
                }
            }
        }

        // 4. Shallow recursive search (depth <= 2) in base_dir for filename
        if let Some(found) = Self::find_file_shallow(base_dir, file_name, 2) {
            return Some(found);
        }

        None
    }

    /// Helper: Shallow recursive search within a directory up to max_depth
    fn find_file_shallow(dir: &Path, target_filename: &str, max_depth: usize) -> Option<PathBuf> {
        if max_depth == 0 || !dir.is_dir() {
            return None;
        }

        if let Ok(entries) = fs::read_dir(dir) {
            let mut subdirs = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.eq_ignore_ascii_case(target_filename) {
                            return fs::canonicalize(&path).ok().or(Some(path));
                        }
                    }
                } else if path.is_dir() {
                    if let Some(dirname) = path.file_name().and_then(|n| n.to_str()) {
                        if !dirname.starts_with('.') && dirname != "target" && dirname != "node_modules" {
                            subdirs.push(path);
                        }
                    }
                }
            }

            for subdir in subdirs {
                if let Some(found) = Self::find_file_shallow(&subdir, target_filename, max_depth - 1) {
                    return Some(found);
                }
            }
        }

        None
    }

    /// Detect MIME type from file extension
    pub fn detect_mime_type(path: &Path) -> String {
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "mp4" | "m4v" => "video/mp4".to_string(),
            "webm" => "video/webm".to_string(),
            "mov" => "video/quicktime".to_string(),
            "mkv" => "video/x-matroska".to_string(),
            "avi" => "video/x-msvideo".to_string(),
            "ogv" => "video/ogg".to_string(),
            "mp3" => "audio/mpeg".to_string(),
            "wav" => "audio/wav".to_string(),
            "ogg" => "audio/ogg".to_string(),
            "flac" => "audio/flac".to_string(),
            "m4a" | "aac" => "audio/aac".to_string(),
            _ => "application/octet-stream".to_string(),
        }
    }

    /// Extract media references mentioned in arbitrary text (e.g. PDF text streams, annotations)
    pub fn extract_media_from_text(text: &str) -> Vec<String> {
        let mut results = Vec::new();
        let unescaped = text.replace("\\_", "_");
        let words: Vec<&str> = unescaped.split_whitespace().collect();

        for word in words {
            // Trim surrounding punctuation like ()[]{},:;'
            let clean = word.trim_matches(|c: char| {
                c == '(' || c == ')' || c == '[' || c == ']' || c == '{' || c == '}' ||
                c == '"' || c == '\'' || c == ',' || c == ';' || c == '<' || c == '>'
            });

            for &ext in MEDIA_EXTENSIONS {
                let dot_ext = format!(".{}", ext);
                if clean.to_ascii_lowercase().ends_with(&dot_ext) {
                    let cleaned_path = Self::clean_raw_path(clean);
                    if !cleaned_path.is_empty() && !results.contains(&cleaned_path) {
                        results.push(cleaned_path);
                    }
                    break;
                }
            }
        }

        results
    }

    /// Find companion LaTeX source file (.tex) corresponding to the presentation PDF.
    /// Checks `<pdf_stem>.tex`, `presentation.tex`, `main.tex`, or single .tex in folder.
    pub fn find_companion_tex(pdf_path: &Path) -> Option<PathBuf> {
        let base_dir = pdf_path.parent().unwrap_or(Path::new("."));

        // 1. Direct stem match (presentation.pdf -> presentation.tex)
        let stem_tex = pdf_path.with_extension("tex");
        if stem_tex.exists() {
            return Some(stem_tex);
        }

        // 2. Base directory candidates
        let candidates = ["presentation.tex", "main.tex", "talk.tex", "slides.tex"];
        for candidate in candidates {
            let p = base_dir.join(candidate);
            if p.exists() {
                return Some(p);
            }
        }

        // 3. Parent directory candidates (if compiled in build/)
        if let Some(parent) = base_dir.parent() {
            for candidate in candidates {
                let p = parent.join(candidate);
                if p.exists() {
                    return Some(p);
                }
            }
        }

        // 4. Any .tex file in base_dir if there is exactly one
        if let Ok(entries) = fs::read_dir(base_dir) {
            let tex_files: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map_or(false, |ext| ext == "tex"))
                .collect();
            if tex_files.len() == 1 {
                return Some(tex_files[0].clone());
            }
        }

        None
    }

    /// Parse companion LaTeX source code to associate frame slides with their media macros.
    /// Handles multiline frames, frame titles, and macro parsing.
    pub fn scan_companion_frames(pdf_path: &Path) -> Vec<CompanionFrame> {
        let mut frames: Vec<CompanionFrame> = Vec::new();

        let tex_path = match Self::find_companion_tex(pdf_path) {
            Some(p) => p,
            None => return frames,
        };

        let content = match fs::read_to_string(&tex_path) {
            Ok(c) => c,
            Err(_) => return frames,
        };

        // Split by \begin{frame}
        let frame_delimiter = "\\begin{frame}";
        let parts: Vec<&str> = content.split(frame_delimiter).collect();

        // Part 0 is preamble before first frame
        for (idx, part) in parts.iter().skip(1).enumerate() {
            let frame_index = idx + 1;
            // Frame content is everything up to \end{frame}
            let frame_body = match part.find("\\end{frame}") {
                Some(end_pos) => &part[..end_pos],
                None => *part,
            };

            // Extract title if present: {Frame Title} or \frametitle{Frame Title}
            let title = Self::extract_frame_title(frame_body);

            // Extract media targets within this entire multiline frame
            let (mut media_paths, media_rects) = Self::extract_multiline_media_macros(frame_body);

            // Also check for raw media filenames in the frame text
            let text_media = Self::extract_media_from_text(frame_body);
            for tm in text_media {
                if !media_paths.contains(&tm) {
                    media_paths.push(tm);
                }
            }

            media_paths.sort();
            media_paths.dedup();

            // Extract \note{...} content from frame body
            let notes = Self::extract_frame_notes(frame_body);

            frames.push(CompanionFrame {
                frame_index,
                title,
                media_paths,
                media_rects,
                notes,
            });
        }

        frames
    }

    /// Convenience wrapper: returns a map of frame_index -> Vec<media_paths>
    pub fn scan_companion_tex(pdf_path: &Path) -> HashMap<usize, Vec<String>> {
        let frames = Self::scan_companion_frames(pdf_path);
        let mut map = HashMap::new();
        for frame in frames {
            if !frame.media_paths.is_empty() {
                map.insert(frame.frame_index, frame.media_paths);
            }
        }
        map
    }

    /// Convenience wrapper: returns a map of frame_index -> notes text
    pub fn scan_companion_tex_notes(pdf_path: &Path) -> HashMap<usize, String> {
        let frames = Self::scan_companion_frames(pdf_path);
        let mut map = HashMap::new();
        for frame in frames {
            if !frame.notes.is_empty() {
                map.insert(frame.frame_index, frame.notes);
            }
        }
        map
    }

    /// Extract \note{...} from LaTeX frame body and clean markup
    pub fn extract_frame_notes(body: &str) -> String {
        let note_pattern = "\\note";
        let mut search_from = 0;
        let mut extracted = String::new();

        while let Some(pos) = body[search_from..].find(note_pattern) {
            let start = search_from + pos + note_pattern.len();
            let rem = &body[start..];
            let rem_trimmed = rem.trim_start();
            let after_opt = if rem_trimmed.starts_with('[') {
                if let Some(close_bracket) = rem_trimmed.find(']') {
                    &rem_trimmed[close_bracket + 1..]
                } else {
                    rem_trimmed
                }
            } else {
                rem_trimmed
            };

            let after_opt_trimmed = after_opt.trim_start();
            if after_opt_trimmed.starts_with('{') {
                if let Some(note_content) = Self::extract_first_balanced_brace(after_opt_trimmed) {
                    let cleaned = Self::clean_latex_notes(&note_content);
                    if !cleaned.is_empty() {
                        if !extracted.is_empty() {
                            extracted.push_str("\n\n");
                        }
                        extracted.push_str(&cleaned);
                    }
                }
            }
            search_from = start;
        }

        extracted
    }

    /// Convert LaTeX markup in speaker notes to clean, readable text
    pub fn clean_latex_notes(raw: &str) -> String {
        let mut lines = Vec::new();
        for line in raw.lines() {
            let mut l = line.trim().to_string();
            if l.starts_with("\\begin{itemize}") || l.starts_with("\\end{itemize}")
                || l.starts_with("\\begin{enumerate}") || l.starts_with("\\end{enumerate}") {
                continue;
            }
            if l.starts_with("\\item") {
                l = format!("• {}", l.trim_start_matches("\\item").trim());
            }
            // Strip formatting wrappers
            l = l.replace("\\textbf{", "").replace("\\textit{", "").replace("\\emph{", "");
            l = l.replace("\\\\", "\n");
            l = l.replace("}", "");
            l = l.trim().to_string();
            if !l.is_empty() {
                lines.push(l);
            }
        }
        lines.join("\n")
    }

    /// Extract frame title from frame body ({Title} right at the start or \frametitle{...})
    fn extract_frame_title(body: &str) -> String {
        let trimmed = body.trim();
        // Check for \frametitle{...}
        if let Some(pos) = trimmed.find("\\frametitle") {
            let rem = &trimmed[pos + "\\frametitle".len()..];
            if let Some(title) = Self::extract_first_balanced_brace(rem) {
                return title.trim().to_string();
            }
        }

        // Check if frame begins with {Title}
        if trimmed.starts_with('{') {
            if let Some(title) = Self::extract_first_balanced_brace(trimmed) {
                return title.trim().to_string();
            }
        }

        // Check if frame begins with [<options>]{Title}
        if trimmed.starts_with('[') {
            if let Some(close_bracket) = trimmed.find(']') {
                let after_bracket = trimmed[close_bracket + 1..].trim();
                if after_bracket.starts_with('{') {
                    if let Some(title) = Self::extract_first_balanced_brace(after_bracket) {
                        return title.trim().to_string();
                    }
                }
            }
        }

        String::new()
    }

    /// Extract media targets and their bounding rectangles from multiline LaTeX frame body
    fn extract_multiline_media_macros(body: &str) -> (Vec<String>, HashMap<String, [f32; 4]>) {
        let mut targets = Vec::new();
        let mut rects = HashMap::new();

        // 1. \movie[options]{poster}{target}
        // 2. \sound[options]{text}{target}
        // 3. \includemedia[options]{poster}{target}
        for macro_name in &["\\movie", "\\sound", "\\includemedia"] {
            let mut search_from = 0;
            while let Some(pos) = body[search_from..].find(macro_name) {
                let start = search_from + pos + macro_name.len();
                let rem = &body[start..];

                let mut parsed_rect = [0.15, 0.26, 0.70, 0.50];

                // Parse optional [options]
                let rem_after_opt = if rem.trim_start().starts_with('[') {
                    let trimmed = rem.trim_start();
                    if let Some(close_bracket) = trimmed.find(']') {
                        let opts = &trimmed[1..close_bracket];
                        parsed_rect = Self::parse_macro_rect(opts);
                        &trimmed[close_bracket + 1..]
                    } else {
                        rem
                    }
                } else {
                    rem
                };

                // Skip first argument (poster / label)
                let rem_after_first = rem_after_opt.trim_start();
                if rem_after_first.starts_with('{') {
                    if let Some((_, end_idx)) = Self::find_balanced_brace_span(rem_after_first) {
                        let rem_after_poster = rem_after_first[end_idx..].trim_start();
                        // Next balanced brace is the target filename!
                        if rem_after_poster.starts_with('{') {
                            if let Some(target) = Self::extract_first_balanced_brace(rem_after_poster) {
                                let cleaned = Self::clean_raw_path(&target);
                                if !cleaned.is_empty() {
                                    if !targets.contains(&cleaned) {
                                        targets.push(cleaned.clone());
                                    }
                                    rects.insert(cleaned, parsed_rect);
                                }
                            }
                        }
                    }
                }

                search_from = start;
            }
        }

        // 4. \href{run:target}{text}
        let href_pattern = "\\href{run:";
        let mut search_from = 0;
        while let Some(pos) = body[search_from..].find(href_pattern) {
            let start = search_from + pos + href_pattern.len();
            if let Some(close_brace) = body[start..].find('}') {
                let target = &body[start..start + close_brace];
                let cleaned = Self::clean_raw_path(target);
                if !cleaned.is_empty() {
                    if !targets.contains(&cleaned) {
                        targets.push(cleaned.clone());
                    }
                    rects.insert(cleaned, [0.15, 0.26, 0.70, 0.50]);
                }
            }
            search_from = start;
        }

        (targets, rects)
    }

    /// Parse normalized [x, y, w, h] from options like width=7cm,height=3.9cm
    pub fn parse_macro_rect(options: &str) -> [f32; 4] {
        let mut w_norm = 0.70_f32;
        let mut h_norm = 0.50_f32;

        fn parse_dim_cm(opts: &str, key: &str) -> Option<f32> {
            let key_idx = opts.find(key)?;
            let rest = &opts[key_idx + key.len()..];
            let val_str = rest.trim_start_matches(|c| c == '=' || c == ' ');
            let num_end = val_str.find(|c: char| !c.is_numeric() && c != '.').unwrap_or(val_str.len());
            let num: f32 = val_str[..num_end].parse().ok()?;
            let unit = &val_str[num_end..];
            if unit.starts_with("cm") {
                Some(num)
            } else if unit.starts_with("mm") {
                Some(num / 10.0)
            } else if unit.starts_with("in") {
                Some(num * 2.54)
            } else if unit.starts_with("pt") {
                Some(num * 0.0352778)
            } else {
                Some(num)
            }
        }

        if let Some(w_cm) = parse_dim_cm(options, "width") {
            // Standard 16:9 Beamer slide width is 16.0cm
            w_norm = (w_cm / 16.0).clamp(0.20, 0.95);
        }
        if let Some(h_cm) = parse_dim_cm(options, "height") {
            // Standard 16:9 Beamer slide height is 9.0cm
            h_norm = (h_cm / 9.0).clamp(0.20, 0.95);
        }

        let x_norm = ((1.0 - w_norm) / 2.0).clamp(0.0, 0.8);
        // On slides with top title and block, centered video macro boxes sit in the lower body
        let y_norm = if h_norm <= 0.55 { 0.48_f32 } else { 0.28_f32 };

        [x_norm, y_norm, w_norm, h_norm]
    }

    /// Extract the first balanced brace {...} from a string
    fn extract_first_balanced_brace(s: &str) -> Option<String> {
        let (start, end) = Self::find_balanced_brace_span(s)?;
        Some(s[start + 1..end - 1].to_string())
    }

    /// Find the span (start_idx, end_idx) of the first balanced { ... } in s
    fn find_balanced_brace_span(s: &str) -> Option<(usize, usize)> {
        let mut depth = 0;
        let mut start_idx = None;

        for (idx, ch) in s.char_indices() {
            if ch == '{' {
                if depth == 0 {
                    start_idx = Some(idx);
                }
                depth += 1;
            } else if ch == '}' {
                if depth == 1 {
                    if let Some(start) = start_idx {
                        return Some((start, idx + 1));
                    }
                }
                if depth > 0 {
                    depth -= 1;
                }
            }
        }

        None
    }

    /// Full resolution pipeline for a page:
    /// Combines companion .tex scanning + PDF page text extraction, resolves relative paths,
    /// and constructs verified MediaClip structs.
    pub fn resolve_page_clips(
        pdf_path: &Path,
        page_index: usize,
        page_text: &str,
        companion_map: &HashMap<usize, Vec<String>>,
    ) -> Vec<MediaClip> {
        let base_dir = pdf_path.parent().unwrap_or(Path::new("."));
        let mut raw_candidates: Vec<String> = Vec::new();

        // 1. Direct match by page index (1:1 slide mapping)
        if let Some(tex_paths) = companion_map.get(&page_index) {
            raw_candidates.extend(tex_paths.clone());
        }

        // 2. Fallback match for Beamer decks with \setbeameroption{show notes}
        // (Where odd pages are slides: page 1 -> frame 1, page 3 -> frame 2, page 5 -> frame 3, page 7 -> frame 4)
        if raw_candidates.is_empty() && page_index % 2 == 1 {
            let beamer_frame_index = (page_index + 1) / 2;
            if let Some(tex_paths) = companion_map.get(&beamer_frame_index) {
                raw_candidates.extend(tex_paths.clone());
            }
        }

        // 3. Match from companion frames by frame title if present in page text
        let companion_frames = Self::scan_companion_frames(pdf_path);
        for frame in &companion_frames {
            if !frame.title.is_empty() && page_text.contains(&frame.title) {
                for p in &frame.media_paths {
                    if !raw_candidates.contains(p) {
                        raw_candidates.push(p.clone());
                    }
                }
            }
        }

        // 4. Candidates from PDF page text (e.g. sample_video.mp4 mentioned on slide)
        let text_candidates = Self::extract_media_from_text(page_text);
        for tc in text_candidates {
            if !raw_candidates.contains(&tc) {
                raw_candidates.push(tc);
            }
        }

        let mut clips = Vec::new();
        let mut seen_canonical = Vec::new();

        for raw in raw_candidates {
            let resolved_path_opt = Self::resolve_path(base_dir, &raw);
            let is_resolved = resolved_path_opt.is_some();

            let canonical_str = if let Some(ref p) = resolved_path_opt {
                p.to_string_lossy().to_string()
            } else {
                base_dir.join(&raw).to_string_lossy().to_string()
            };

            // Deduplicate clips pointing to same file
            if seen_canonical.contains(&canonical_str) {
                continue;
            }
            seen_canonical.push(canonical_str.clone());

            let display_name = Path::new(&raw)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&raw)
                .to_string();

            let mime_type = if let Some(ref p) = resolved_path_opt {
                Self::detect_mime_type(p)
            } else {
                Self::detect_mime_type(Path::new(&raw))
            };

            let rect = companion_frames
                .iter()
                .find(|f| f.frame_index == page_index || (page_index % 2 == 1 && f.frame_index == (page_index + 1) / 2))
                .and_then(|f| f.media_rects.get(&raw).copied())
                .unwrap_or([0.15, 0.26, 0.70, 0.50]);

            clips.push(MediaClip {
                page_index,
                file_path: canonical_str,
                display_path: display_name,
                mime_type,
                rect,
                autostart: false,
                is_resolved,
            });
        }

        clips
    }

    /// Refines an approximate [x, y, w, h] media rectangle by analyzing the rendered slide image.
    /// In LaTeX Beamer presentations, multimedia targets (\movie, \includemedia) are typically
    /// backed by a visible poster box (commonly a solid dark background or \colorbox{black}).
    /// By scanning the rendered slide image around the approximate coordinates, we snap the
    /// video player overlay to the exact pixel-perfect poster bounds.
    pub fn refine_media_rect_from_image(image: &RgbaImage, initial_rect: [f32; 4]) -> [f32; 4] {
        let (img_w, img_h) = image.dimensions();
        if img_w == 0 || img_h == 0 {
            return initial_rect;
        }

        // Search window: around initial_rect expanded by 15%
        let margin_x = (img_w as f32 * 0.15) as u32;
        let margin_y = (img_h as f32 * 0.15) as u32;

        let init_x0 = (initial_rect[0] * img_w as f32) as u32;
        let init_y0 = (initial_rect[1] * img_h as f32) as u32;
        let init_w = (initial_rect[2] * img_w as f32) as u32;
        let init_h = (initial_rect[3] * img_h as f32) as u32;
        let init_x1 = (init_x0 + init_w).min(img_w);
        let init_y1 = (init_y0 + init_h).min(img_h);

        let search_x0 = init_x0.saturating_sub(margin_x);
        let search_y0 = init_y0.saturating_sub(margin_y);
        let search_x1 = (init_x1 + margin_x).min(img_w);
        let search_y1 = (init_y1 + margin_y).min(img_h);

        if search_x1 <= search_x0 || search_y1 <= search_y0 {
            return initial_rect;
        }

        let min_row_count = (init_w as f32 * 0.40) as usize;
        let mut row_counts = vec![0usize; (search_y1 - search_y0) as usize];

        for y in search_y0..search_y1 {
            let row_idx = (y - search_y0) as usize;
            let mut dark_count = 0;
            for x in search_x0..search_x1 {
                let p = image.get_pixel(x, y);
                if p[0] < 30 && p[1] < 30 && p[2] < 30 && p[3] > 200 {
                    dark_count += 1;
                }
            }
            row_counts[row_idx] = dark_count;
        }

        let valid_rows: Vec<usize> = row_counts
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c >= min_row_count)
            .map(|(i, _)| i)
            .collect();

        if valid_rows.is_empty() {
            return initial_rect;
        }

        let first_valid_row = *valid_rows.first().unwrap();
        let last_valid_row = *valid_rows.last().unwrap();
        let box_y0 = search_y0 + first_valid_row as u32;
        let box_y1 = search_y0 + last_valid_row as u32;

        let num_rows = (box_y1 - box_y0 + 1) as usize;
        let min_col_count = (num_rows as f32 * 0.40) as usize;
        let mut col_counts = vec![0usize; (search_x1 - search_x0) as usize];

        for y in box_y0..=box_y1 {
            for x in search_x0..search_x1 {
                let col_idx = (x - search_x0) as usize;
                let p = image.get_pixel(x, y);
                if p[0] < 30 && p[1] < 30 && p[2] < 30 && p[3] > 200 {
                    col_counts[col_idx] += 1;
                }
            }
        }

        let valid_cols: Vec<usize> = col_counts
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c >= min_col_count)
            .map(|(i, _)| i)
            .collect();

        if valid_cols.is_empty() {
            return initial_rect;
        }

        let first_valid_col = *valid_cols.first().unwrap();
        let last_valid_col = *valid_cols.last().unwrap();
        let box_x0 = search_x0 + first_valid_col as u32;
        let box_x1 = search_x0 + last_valid_col as u32;

        let detected_w = (box_x1 - box_x0 + 1) as f32;
        let detected_h = (box_y1 - box_y0 + 1) as f32;

        if detected_w > (img_w as f32 * 0.15) && detected_h > (img_h as f32 * 0.15) {
            [
                box_x0 as f32 / img_w as f32,
                box_y0 as f32 / img_h as f32,
                detected_w / img_w as f32,
                detected_h / img_h as f32,
            ]
        } else {
            initial_rect
        }
    }
}
