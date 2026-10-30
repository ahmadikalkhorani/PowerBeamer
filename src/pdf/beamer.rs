#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeamerAnnotation {
    pub page_index: usize,
    pub title: String,
    pub contents: String,
    pub rect: [f32; 4],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaClip {
    pub page_index: usize,
    pub file_path: String,
    pub display_path: String,
    pub mime_type: String,
    pub rect: [f32; 4], // [x, y, w, h] in normalized 0.0..1.0 coordinates
    pub autostart: bool,
    pub is_resolved: bool,
}

impl MediaClip {
    pub fn new(page_index: usize, file_path: String, mime_type: String) -> Self {
        let display = std::path::Path::new(&file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file_path)
            .to_string();
        Self {
            page_index,
            file_path,
            display_path: display,
            mime_type,
            rect: [0.1, 0.4, 0.8, 0.5],
            autostart: false,
            is_resolved: true,
        }
    }

    pub fn is_audio(&self) -> bool {
        self.mime_type.starts_with("audio/")
            || self.file_path.ends_with(".mp3")
            || self.file_path.ends_with(".wav")
            || self.file_path.ends_with(".ogg")
            || self.file_path.ends_with(".flac")
            || self.file_path.ends_with(".m4a")
            || self.file_path.ends_with(".aac")
    }

    /// Check if a normalized point (x, y) falls inside or near the bounding box of this clip.
    pub fn contains_point(&self, x: f32, y: f32, tolerance: f32) -> bool {
        let rx = self.rect[0] - tolerance;
        let ry = self.rect[1] - tolerance;
        let rw = self.rect[2] + tolerance * 2.0;
        let rh = self.rect[3] + tolerance * 2.0;
        x >= rx && x <= (rx + rw) && y >= ry && y <= (ry + rh)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeamerMetadata {
    pub is_dual_screen: bool,
    pub split_vertical: bool, // true if side-by-side, false if top-bottom
    pub notes: String,
    pub media_clips: Vec<MediaClip>,
    pub annotations: Vec<BeamerAnnotation>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum NotesLayoutMode {
    /// Each page is dual-screen (split left/right or top/bottom)
    DualScreen { is_vertical: bool },
    /// Beamer companion .nav file mapping (identifies frame pages vs note pages)
    BeamerNavMapping,
    /// Labeled pages matching `notes:<label>` (Pympress page mapping mode)
    PageLabelMapping,
    /// LibreOffice mode: first N/2 pages are slides, second N/2 pages are notes (Pympress AFTER mode)
    TwoHalvesAfter,
    /// Alternating page parity: odd physical pages are slides, even physical pages are notes (Pympress page parity mode)
    PageParity,
    /// Interleaved note pages detected from page text markers
    InterleavedNotes,
    /// Standard presentation with no separate note pages
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SlidePageMapping {
    /// 1-indexed logical slide number in presentation (1..=total_slides)
    pub slide_index: usize,
    /// 0-indexed physical page index in the PDF for the presentation slide
    pub slide_page_idx: usize,
    /// 0-indexed physical page index in the PDF for the speaker note (if any)
    pub note_page_idx: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BeamerFrameMapping {
    pub frame_index: usize,
    pub start_page: usize,
    pub end_page: usize,
}

impl BeamerMetadata {
    pub fn new() -> Self {
        Self {
            is_dual_screen: false,
            split_vertical: true,
            notes: String::new(),
            media_clips: Vec::new(),
            annotations: Vec::new(),
        }
    }

    /// Automatically inspect page aspect ratio to detect pgfpages dual-screen Beamer slides
    pub fn detect_dual_screen(width: f32, height: f32) -> (bool, bool) {
        if height <= 0.0 {
            return (false, true);
        }
        let aspect = width / height;
        // 8:3 is ~2.666, 32:9 is ~3.555 (horizontal dual-screen)
        if aspect >= 1.95 {
            return (true, true);
        }
        // Top-bottom dual-screen (aspect ~0.75 or ~0.56)
        if aspect <= 0.65 {
            return (true, false);
        }
        (false, true)
    }

    /// Parse companion .nav file to extract exact Beamer frame-to-page mappings and total frames
    pub fn parse_nav_file(path: &std::path::Path) -> Option<(Vec<BeamerFrameMapping>, usize)> {
        let nav_path = path.with_extension("nav");
        if !nav_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&nav_path).ok()?;
        Self::parse_nav_content(&content)
    }

    pub fn parse_nav_content(content: &str) -> Option<(Vec<BeamerFrameMapping>, usize)> {
        let mut mappings = Vec::new();
        let mut total_frames = 0;

        for line in content.lines() {
            // Check for \beamer@framepages {start}{end}
            if let Some(pos) = line.find("beamer@framepages") {
                let rest = &line[pos + "beamer@framepages".len()..];
                if let (Some(s_start), Some(s_end)) = (rest.find('{'), rest.find('}')) {
                    if let Ok(start) = rest[s_start + 1..s_end].trim().parse::<usize>() {
                        let after_first = &rest[s_end + 1..];
                        if let (Some(e_start), Some(e_end)) = (after_first.find('{'), after_first.find('}')) {
                            if let Ok(end) = after_first[e_start + 1..e_end].trim().parse::<usize>() {
                                mappings.push(BeamerFrameMapping {
                                    frame_index: mappings.len() + 1,
                                    start_page: start,
                                    end_page: end,
                                });
                            }
                        }
                    }
                }
            } else if let Some(pos) = line.find("inserttotalframenumber") {
                let rest = &line[pos + "inserttotalframenumber".len()..];
                if let (Some(s_start), Some(s_end)) = (rest.find('{'), rest.find('}')) {
                    if let Ok(tot) = rest[s_start + 1..s_end].trim().parse::<usize>() {
                        total_frames = tot;
                    }
                }
            }
        }

        if mappings.is_empty() {
            None
        } else {
            if total_frames == 0 {
                total_frames = mappings.len();
            }
            Some((mappings, total_frames))
        }
    }

    /// Cleans raw extracted text from a Beamer note page, removing headers/footers
    pub fn clean_beamer_note_text(raw: &str) -> String {
        let mut lines = Vec::new();
        let mut started = false;
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Skip page numbers like "1 / 5" or "2/10"
            if let Some((left, right)) = trimmed.split_once('/') {
                if !left.trim().is_empty()
                    && !right.trim().is_empty()
                    && left.trim().chars().all(|c| c.is_ascii_digit())
                    && right.trim().chars().all(|c| c.is_ascii_digit())
                {
                    continue;
                }
            }

            let upper = trimmed.to_uppercase();
            // Start capturing at note marker or bullet
            if upper.contains("NOTES")
                || upper.contains("NOTE:")
                || trimmed.starts_with("•")
                || trimmed.starts_with("-")
                || started
            {
                started = true;
                lines.push(trimmed.to_string());
            }
        }
        if lines.is_empty() {
            raw.trim().to_string()
        } else {
            lines.join("\n")
        }
    }

    /// Automatically detects slide-to-note page mappings for both dual-screen and non-dual-screen presentations.
    /// Supports:
    /// 1. Dual-screen slides (pgfpages 32:9 or 8:3)
    /// 2. Beamer .nav companion file frame-to-page mappings
    /// 3. Pympress `notes:` page label mapping (PdfPage.MAP)
    /// 4. Pympress LibreOffice two-halves mode (PdfPage.AFTER)
    /// 5. Pympress page parity alternating mode (PdfPage.ODD/EVEN)
    /// 6. Interleaved note page heuristic from page text / headers
    pub fn detect_slide_mappings(
        num_pages: usize,
        page_sizes: &[(f32, f32)],
        page_labels: &[Option<String>],
        page_texts: &[String],
        nav_info: Option<&(Vec<BeamerFrameMapping>, usize)>,
    ) -> (Vec<SlidePageMapping>, NotesLayoutMode) {
        if num_pages == 0 {
            return (Vec::new(), NotesLayoutMode::None);
        }

        // Mode 1: Dual-screen detection (side-by-side or stacked on single pages)
        if let Some(&(w, h)) = page_sizes.first() {
            let (is_dual, is_vert) = Self::detect_dual_screen(w, h);
            if is_dual {
                let mappings = (0..num_pages)
                    .map(|i| SlidePageMapping {
                        slide_index: i + 1,
                        slide_page_idx: i,
                        note_page_idx: None,
                    })
                    .collect();
                return (mappings, NotesLayoutMode::DualScreen { is_vertical: is_vert });
            }
        }

        if num_pages < 2 {
            let mappings = vec![SlidePageMapping {
                slide_index: 1,
                slide_page_idx: 0,
                note_page_idx: None,
            }];
            return (mappings, NotesLayoutMode::None);
        }

        // Mode 2: Beamer .nav companion file frame-to-page mappings
        if let Some((mappings, _tot)) = nav_info {
            if !mappings.is_empty() {
                let mut is_frame_page = vec![false; num_pages + 1];
                for m in mappings {
                    for p in m.start_page..=m.end_page {
                        if p <= num_pages {
                            is_frame_page[p] = true;
                        }
                    }
                }

                let has_skipped_pages = (1..=num_pages).any(|p| !is_frame_page[p]);
                if has_skipped_pages {
                    let mut slide_mappings = Vec::new();
                    for (f_idx, m) in mappings.iter().enumerate() {
                        let next_start = mappings
                            .get(f_idx + 1)
                            .map(|next| next.start_page)
                            .unwrap_or(num_pages + 1);
                        let note_page_1 = if m.end_page < next_start
                            && m.end_page + 1 <= num_pages
                            && !is_frame_page[m.end_page + 1]
                        {
                            Some(m.end_page + 1)
                        } else {
                            None
                        };
                        let note_page_0 = note_page_1.map(|p| p - 1);

                        for p in m.start_page..=m.end_page {
                            if p <= num_pages {
                                slide_mappings.push(SlidePageMapping {
                                    slide_index: slide_mappings.len() + 1,
                                    slide_page_idx: p - 1,
                                    note_page_idx: note_page_0,
                                });
                            }
                        }
                    }
                    if !slide_mappings.is_empty() {
                        return (slide_mappings, NotesLayoutMode::BeamerNavMapping);
                    }
                }
            }
        }

        // Mode 3: Pympress `notes:` page label mapping (PdfPage.MAP)
        let has_notes_label = page_labels.iter().any(|lbl| {
            lbl.as_deref().map_or(false, |s| s.starts_with("notes:"))
        });
        if has_notes_label {
            let mut is_note = vec![false; num_pages];
            for n in 1..num_pages {
                if let (Some(lbl), Some(prev_lbl)) = (&page_labels[n], &page_labels[n - 1]) {
                    if lbl == &format!("notes:{}", prev_lbl) && !is_note[n - 1] {
                        is_note[n] = true;
                    }
                }
            }
            let mut slide_mappings = Vec::new();
            for n in 0..num_pages {
                if is_note[n] {
                    continue;
                }
                let note_page_0 = if n + 1 < num_pages && is_note[n + 1] {
                    Some(n + 1)
                } else {
                    None
                };
                slide_mappings.push(SlidePageMapping {
                    slide_index: slide_mappings.len() + 1,
                    slide_page_idx: n,
                    note_page_idx: note_page_0,
                });
            }
            if !slide_mappings.is_empty() && slide_mappings.len() < num_pages {
                return (slide_mappings, NotesLayoutMode::PageLabelMapping);
            }
        }

        // Helper to check if text contains note headers
        let is_note_text = |text: &str| -> bool {
            let upper = text.to_uppercase();
            upper.contains("PRESENTER NOTES")
                || upper.contains("SPEAKER NOTES")
                || upper.contains("NOTES (SLIDE")
                || upper.contains("NOTE (SLIDE")
                || upper.contains("NOTES FOR SLIDE")
                || upper.contains("NOTES FOR")
                || upper.contains("SPEAKER COMMENTS")
                || (upper.contains("NOTES:") && !upper.contains("KEYNOTE"))
                || (upper.contains("NOTE:") && !upper.contains("KEYNOTE"))
                || text.contains("\\note")
        };

        // Mode 4: LibreOffice Two-Halves Notes Mode (PdfPage.AFTER)
        if num_pages % 2 == 0 {
            let half = num_pages / 2;
            let ar_slides = page_sizes[0].0 / page_sizes[0].1.max(1.0);
            let ar_notes = page_sizes[half].0 / page_sizes[half].1.max(1.0);
            let ar_diff = (ar_slides - ar_notes).abs() > 0.05;
            let slides_consistent = (1..half).all(|i| {
                let ar = page_sizes[i].0 / page_sizes[i].1.max(1.0);
                (ar - ar_slides).abs() < 0.05
            });
            let notes_consistent = (half + 1..num_pages).all(|i| {
                let ar = page_sizes[i].0 / page_sizes[i].1.max(1.0);
                (ar - ar_notes).abs() < 0.05
            });
            let second_half_notes_text = (half..num_pages).any(|i| is_note_text(&page_texts[i]))
                && !(0..half).any(|i| is_note_text(&page_texts[i]));

            if (ar_diff && slides_consistent && notes_consistent) || second_half_notes_text {
                let slide_mappings = (0..half)
                    .map(|n| SlidePageMapping {
                        slide_index: n + 1,
                        slide_page_idx: n,
                        note_page_idx: Some(n + half),
                    })
                    .collect();
                return (slide_mappings, NotesLayoutMode::TwoHalvesAfter);
            }
        }

        // Mode 5: Page Parity / Alternating Note Pages (PdfPage.ODD/EVEN)
        if num_pages % 2 == 0 {
            let odd_note_count = (0..num_pages)
                .filter(|&i| i % 2 == 1 && is_note_text(&page_texts[i]))
                .count();
            let even_note_count = (0..num_pages)
                .filter(|&i| i % 2 == 0 && is_note_text(&page_texts[i]))
                .count();

            let half = num_pages / 2;
            if odd_note_count >= (half + 1) / 2 && even_note_count == 0 {
                let slide_mappings = (0..half)
                    .map(|i| SlidePageMapping {
                        slide_index: i + 1,
                        slide_page_idx: 2 * i,
                        note_page_idx: Some(2 * i + 1),
                    })
                    .collect();
                return (slide_mappings, NotesLayoutMode::PageParity);
            }
        }

        // Mode 6: Interleaved Note Page Auto-Detection
        let is_note_page: Vec<bool> = page_texts.iter().map(|t| is_note_text(t)).collect();
        let note_count = is_note_page.iter().filter(|&&b| b).count();
        if note_count > 0 && note_count < num_pages && !is_note_page[0] {
            let has_consecutive_notes = (1..num_pages).any(|i| is_note_page[i] && is_note_page[i - 1]);
            if !has_consecutive_notes {
                let mut slide_mappings: Vec<SlidePageMapping> = Vec::new();
                for i in 0..num_pages {
                    if is_note_page[i] {
                        if let Some(last_slide) = slide_mappings.last_mut() {
                            if last_slide.note_page_idx.is_none() {
                                last_slide.note_page_idx = Some(i);
                                continue;
                            }
                        }
                    }
                    slide_mappings.push(SlidePageMapping {
                        slide_index: slide_mappings.len() + 1,
                        slide_page_idx: i,
                        note_page_idx: None,
                    });
                }
                if !slide_mappings.is_empty() {
                    return (slide_mappings, NotesLayoutMode::InterleavedNotes);
                }
            }
        }

        // Mode 7: Fallback to standard 1:1 presentation
        let slide_mappings = (0..num_pages)
            .map(|i| SlidePageMapping {
                slide_index: i + 1,
                slide_page_idx: i,
                note_page_idx: None,
            })
            .collect();
        (slide_mappings, NotesLayoutMode::None)
    }

    /// Assigns frame_index, total_frames, overlay_step, and overlay_total to slides
    pub fn assign_frame_metadata(
        slides: &mut [super::deck::Slide],
        nav_info: Option<(Vec<BeamerFrameMapping>, usize)>,
    ) {
        if let Some((mappings, total_frames)) = nav_info {
            let num_slides = slides.len();
            for slide in slides.iter_mut() {
                let page = if slide.physical_page_index > 0 {
                    slide.physical_page_index
                } else {
                    slide.page_index
                };
                if let Some(mapping) = mappings.iter().find(|m| page >= m.start_page && page <= m.end_page) {
                    slide.frame_index = mapping.frame_index;
                    slide.total_frames = total_frames;
                    slide.overlay_step = page - mapping.start_page + 1;
                    slide.overlay_total = mapping.end_page - mapping.start_page + 1;
                } else {
                    slide.frame_index = slide.page_index;
                    slide.total_frames = total_frames.max(num_slides);
                    slide.overlay_step = 1;
                    slide.overlay_total = 1;
                }
            }
        } else {
            // Heuristic grouping: consecutive slides with identical non-empty titles belong to the same frame
            let mut current_frame = 1usize;
            let mut i = 0;
            while i < slides.len() {
                let title = slides[i].title.clone();
                let mut run_len = 1;
                if !title.is_empty() && !title.starts_with("Slide ") {
                    while i + run_len < slides.len() && slides[i + run_len].title == title {
                        run_len += 1;
                    }
                }
                for step in 0..run_len {
                    slides[i + step].frame_index = current_frame;
                    slides[i + step].overlay_step = step + 1;
                    slides[i + step].overlay_total = run_len;
                }
                i += run_len;
                current_frame += 1;
            }
            let total = current_frame - 1;
            for slide in slides.iter_mut() {
                slide.total_frames = total;
            }
        }
    }
}
