#![allow(dead_code)]
use super::beamer::MediaClip;
use crate::ink::SlideInking;
use image::{Rgba, RgbaImage};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderQuality {
    Eco,        // 150 DPI
    Balanced,   // 300 DPI
    UltraVector,// 600 DPI
}

impl RenderQuality {
    pub fn dpi(&self) -> f32 {
        match self {
            RenderQuality::Eco => 150.0,
            RenderQuality::Balanced => 300.0,
            RenderQuality::UltraVector => 600.0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            RenderQuality::Eco => "Eco Battery (150 DPI)",
            RenderQuality::Balanced => "Balanced (300 DPI)",
            RenderQuality::UltraVector => "Ultra Vector (600 DPI)",
        }
    }
    /// Inverse of `label` (also accepts any string naming the DPI).
    pub fn from_label(label: &str) -> Self {
        if label.contains("600") {
            RenderQuality::UltraVector
        } else if label.contains("300") {
            RenderQuality::Balanced
        } else {
            RenderQuality::Eco
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct TocEntry {
    pub title: String,
    pub page_index: usize,   // 1-indexed slide page
    pub level: usize,        // 0 = section/top, 1 = subsection/slide
    pub frame_index: usize,  // logical Beamer frame
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PdfLinkTarget {
    Url(String),
    Page(usize), // 0-indexed slide index in deck
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PdfLinkInfo {
    pub rect: [f32; 4], // [x, y, width, height] normalized (0.0..1.0)
    pub target: PdfLinkTarget,
    pub tooltip: String,
}

impl PdfLinkInfo {
    pub fn new_url(rect: [f32; 4], url: String) -> Self {
        let tooltip = format!("Open: {}", url);
        Self {
            rect,
            target: PdfLinkTarget::Url(url),
            tooltip,
        }
    }

    pub fn new_page(rect: [f32; 4], page_idx: usize) -> Self {
        let tooltip = format!("Jump to Slide {}", page_idx + 1);
        Self {
            rect,
            target: PdfLinkTarget::Page(page_idx),
            tooltip,
        }
    }

    pub fn contains(&self, x: f32, y: f32, padding: f32) -> bool {
        let left = self.rect[0] - padding;
        let top = self.rect[1] - padding;
        let right = self.rect[0] + self.rect[2] + padding;
        let bottom = self.rect[1] + self.rect[3] + padding;
        x >= left && x <= right && y >= top && y <= bottom
    }
}

#[derive(Clone)]
pub struct Slide {
    pub page_index: usize,
    pub physical_page_index: usize,
    pub frame_index: usize,     // 1-indexed logical Beamer frame
    pub total_frames: usize,    // total logical Beamer frames
    pub overlay_step: usize,    // e.g. 1
    pub overlay_total: usize,   // e.g. 3
    pub title: String,
    pub notes: String,
    pub is_whiteboard: bool,
    pub is_dual_screen: bool,
    pub aspect_ratio: f32,
    pub media_clips: Vec<MediaClip>,
    pub inking: SlideInking,
    pub links: Vec<PdfLinkInfo>,
    /// Fingerprint of the page's text (0 = none), used to re-identify a slide when the
    /// source PDF is rebuilt and pages shift.
    pub content_hash: u64,
    // Rendered slide cache. Shared via `Arc`: the presenter image is usually the same
    // pixels as the audience image, and cloning a slide must not duplicate megabytes.
    pub audience_image: Arc<RgbaImage>,
    pub presenter_image: Arc<RgbaImage>,
    pub notes_image: Option<Arc<RgbaImage>>,
}

impl Slide {
    pub fn find_link_at(&self, x: f32, y: f32, padding: f32) -> Option<&PdfLinkInfo> {
        self.links.iter().find(|link| link.contains(x, y, padding))
    }

    /// Find the index of a media clip under the given normalized coordinate (x, y).
    pub fn find_media_clip_at(&self, x: f32, y: f32, tolerance: f32) -> Option<usize> {
        self.media_clips.iter().position(|clip| clip.contains_point(x, y, tolerance))
    }
}

pub struct PresentationDeck {
    pub file_path: Option<PathBuf>,
    pub slides: Vec<Slide>,
    pub toc: Vec<TocEntry>,
    pub current_index: usize,
    pub quality: RenderQuality,
}

impl PresentationDeck {
    pub fn new_demo_deck(quality: RenderQuality) -> Self {
        let mut deck = Self {
            file_path: None,
            slides: Vec::new(),
            toc: Vec::new(),
            current_index: 0,
            quality,
        };
        deck.populate_demo_slides();
        deck
    }

    pub fn current_slide(&self) -> &Slide {
        &self.slides[self.current_index]
    }

    pub fn current_slide_mut(&mut self) -> &mut Slide {
        &mut self.slides[self.current_index]
    }

    pub fn total_slides(&self) -> usize {
        self.slides.len()
    }

    pub fn next(&mut self) -> bool {
        if self.current_index + 1 < self.slides.len() {
            self.current_index += 1;
            true
        } else {
            false
        }
    }

    pub fn prev(&mut self) -> bool {
        if self.current_index > 0 {
            self.current_index -= 1;
            true
        } else {
            false
        }
    }

    pub fn jump_to(&mut self, index: usize) -> bool {
        if index < self.slides.len() {
            self.current_index = index;
            true
        } else {
            false
        }
    }

    pub fn next_slide(&mut self) -> bool {
        self.next()
    }

    pub fn prev_slide(&mut self) -> bool {
        self.prev()
    }

    pub fn first_slide(&mut self) -> bool {
        self.jump_to(0)
    }

    pub fn last_slide(&mut self) -> bool {
        if !self.slides.is_empty() {
            self.jump_to(self.slides.len() - 1)
        } else {
            false
        }
    }

    pub fn delete_slide(&mut self, idx: usize) -> bool {
        if self.slides.len() <= 1 || idx >= self.slides.len() {
            return false;
        }
        self.slides.remove(idx);
        for (i, s) in self.slides.iter_mut().enumerate() {
            s.page_index = i + 1;
        }
        if self.current_index >= self.slides.len() {
            self.current_index = self.slides.len() - 1;
        }
        true
    }

    /// Jump to next logical Beamer frame (skips remaining overlays in current frame)
    pub fn next_frame(&mut self) -> bool {
        let current_frame = self.slides.get(self.current_index).map(|s| s.frame_index).unwrap_or(0);
        if let Some(target_idx) = self.slides.iter().position(|s| s.frame_index > current_frame) {
            self.current_index = target_idx;
            true
        } else {
            false
        }
    }

    /// Jump to previous logical Beamer frame
    pub fn prev_frame(&mut self) -> bool {
        let current_frame = self.slides.get(self.current_index).map(|s| s.frame_index).unwrap_or(1);
        if current_frame <= 1 {
            return false;
        }
        let target_frame = current_frame - 1;
        if let Some(target_idx) = self.slides.iter().position(|s| s.frame_index == target_frame) {
            self.current_index = target_idx;
            true
        } else {
            false
        }
    }

    /// Move a slide from `from_idx` to `to_idx` (0-based) and re-index
    pub fn move_slide(&mut self, from_idx: usize, to_idx: usize) -> bool {
        let len = self.slides.len();
        if from_idx >= len || to_idx >= len || from_idx == to_idx {
            return false;
        }
        let slide = self.slides.remove(from_idx);
        self.slides.insert(to_idx, slide);
        for (i, s) in self.slides.iter_mut().enumerate() {
            s.page_index = i + 1;
        }
        self.current_index = to_idx;
        true
    }

    /// Insert a page with a selected template ("current", "blank", "ruled", "grid", "dotted")
    /// and background color [R, G, B, A].
    /// `position_mode`:
    ///   0 => After current page
    ///   1 => Before current page
    ///   2 => At the end
    pub fn insert_page_with_template(
        &mut self,
        position_mode: usize,
        template_kind: &str,
        bg_rgba: [u8; 4],
    ) -> usize {
        let insert_idx = match position_mode {
            0 => (self.current_index + 1).min(self.slides.len()),
            1 => self.current_index.min(self.slides.len()),
            _ => self.slides.len(),
        };

        let (w, h, aspect_ratio) = if let Some(curr) = self.slides.get(self.current_index) {
            (
                curr.audience_image.width().max(800),
                curr.audience_image.height().max(600),
                curr.aspect_ratio,
            )
        } else {
            (1920, 1080, 16.0 / 9.0)
        };

        let (aud_img, pres_img, title) = if template_kind == "current" && !self.slides.is_empty() {
            let curr = &self.slides[self.current_index];
            (
                curr.audience_image.clone(),
                curr.presenter_image.clone(),
                format!("{} (Copy)", curr.title),
            )
        } else {
            let mut img = RgbaImage::new(w, h);
            let bg_color = Rgba(bg_rgba);
            for px in img.as_mut().chunks_exact_mut(4) {
                px.copy_from_slice(&bg_color.0);
            }

            // Contrast line color based on luminance
            let luminance =
                0.299 * (bg_rgba[0] as f32) + 0.587 * (bg_rgba[1] as f32) + 0.114 * (bg_rgba[2] as f32);
            let is_dark_bg = luminance < 128.0;

            let line_color = if is_dark_bg {
                Rgba([
                    (bg_rgba[0] as u16 + 50).min(255) as u8,
                    (bg_rgba[1] as u16 + 50).min(255) as u8,
                    (bg_rgba[2] as u16 + 50).min(255) as u8,
                    255,
                ])
            } else {
                Rgba([
                    (bg_rgba[0] as i16 - 45).max(0) as u8,
                    (bg_rgba[1] as i16 - 45).max(0) as u8,
                    (bg_rgba[2] as i16 - 45).max(0) as u8,
                    255,
                ])
            };

            match template_kind {
                "ruled" => {
                    // Horizontal rule lines
                    let line_spacing: usize = 48;
                    let top_margin = 96;
                    for y in (top_margin..h).step_by(line_spacing) {
                        for x in 0..w {
                            img.put_pixel(x, y, line_color);
                        }
                    }
                    // Notebook vertical margin line
                    let margin_x = (w as f32 * 0.10) as u32;
                    let red_margin = if is_dark_bg {
                        Rgba([190, 80, 80, 200])
                    } else {
                        Rgba([239, 68, 68, 160])
                    };
                    for y in 0..h {
                        if margin_x < w {
                            img.put_pixel(margin_x, y, red_margin);
                        }
                    }
                }
                "grid" => {
                    let grid_spacing: usize = 40;
                    for y in (0..h).step_by(grid_spacing) {
                        for x in 0..w {
                            img.put_pixel(x, y, line_color);
                        }
                    }
                    for x in (0..w).step_by(grid_spacing) {
                        for y in 0..h {
                            img.put_pixel(x, y, line_color);
                        }
                    }
                }
                "dotted" => {
                    let dot_spacing: usize = 40;
                    for y in (dot_spacing as u32..h).step_by(dot_spacing) {
                        for x in (dot_spacing as u32..w).step_by(dot_spacing) {
                            if x + 1 < w && y + 1 < h {
                                img.put_pixel(x, y, line_color);
                                img.put_pixel(x + 1, y, line_color);
                                img.put_pixel(x, y + 1, line_color);
                                img.put_pixel(x + 1, y + 1, line_color);
                            }
                        }
                    }
                }
                _ => {
                    // "blank" - clean page
                }
            }

            let img = Arc::new(img);
            let pres = img.clone();
            let title = match template_kind {
                "ruled" => format!("Ruled Note #{}", insert_idx + 1),
                "grid" => format!("Grid Page #{}", insert_idx + 1),
                "dotted" => format!("Dotted Sheet #{}", insert_idx + 1),
                _ => format!("Page #{}", insert_idx + 1),
            };
            (img, pres, title)
        };

        let (frame_idx, total_frames, step, steps) = if let Some(curr) = self.slides.get(self.current_index) {
            (curr.frame_index, curr.total_frames, curr.overlay_step, curr.overlay_total)
        } else {
            (insert_idx + 1, insert_idx + 1, 1, 1)
        };

        let slide = Slide {
            page_index: insert_idx + 1,
            physical_page_index: insert_idx + 1,
            frame_index: frame_idx,
            total_frames,
            overlay_step: step,
            overlay_total: steps,
            title,
            notes: "Inserted template note page.".to_string(),
            is_whiteboard: true,
            is_dual_screen: false,
            aspect_ratio,
            media_clips: Vec::new(),
            inking: SlideInking::new(),
            content_hash: 0,
            links: Vec::new(),
            audience_image: aud_img,
            presenter_image: pres_img,
            notes_image: None,
        };

        self.slides.insert(insert_idx, slide);
        for (i, s) in self.slides.iter_mut().enumerate() {
            s.page_index = i + 1;
        }
        self.current_index = insert_idx;
        insert_idx
    }

    /// Replaces the PDF-derived slides with a freshly loaded copy of the source PDF while
    /// keeping user work. Each old PDF slide is matched to a new one by page text first (so ink
    /// follows its slide when pages are added or removed earlier in the deck), then by physical
    /// page number. Ink moves to the matched slide, inserted pages stay after the slide they
    /// followed, and the current slide is preserved when it still exists.
    pub fn merge_reloaded(&mut self, fresh: Vec<Slide>, toc: Vec<TocEntry>) {
        use std::collections::{HashMap, VecDeque};

        let old = std::mem::take(&mut self.slides);
        let old_current = self.current_index;

        // Old PDF slides, plus inserted slides keyed by the old PDF slide they followed.
        let mut old_pdf: Vec<(usize, Slide)> = Vec::new(); // (old deck index, slide)
        let mut inserted_after: HashMap<Option<usize>, Vec<(usize, Slide)>> = HashMap::new();
        for (i, slide) in old.into_iter().enumerate() {
            if slide.is_whiteboard {
                let anchor = old_pdf.len().checked_sub(1);
                inserted_after.entry(anchor).or_default().push((i, slide));
            } else {
                old_pdf.push((i, slide));
            }
        }

        // Match new slides to old PDF slides: content first, page number second.
        let mut by_hash: HashMap<u64, VecDeque<usize>> = HashMap::new();
        let mut by_page: HashMap<usize, usize> = HashMap::new();
        for (k, (_, s)) in old_pdf.iter().enumerate() {
            if s.content_hash != 0 {
                by_hash.entry(s.content_hash).or_default().push_back(k);
            }
            by_page.entry(s.physical_page_index).or_insert(k);
        }
        let mut claimed = vec![false; old_pdf.len()];
        let mut matched: Vec<Option<usize>> = vec![None; fresh.len()];
        for (n, s) in fresh.iter().enumerate() {
            if s.content_hash == 0 {
                continue;
            }
            if let Some(queue) = by_hash.get_mut(&s.content_hash) {
                while let Some(k) = queue.pop_front() {
                    if !claimed[k] {
                        claimed[k] = true;
                        matched[n] = Some(k);
                        break;
                    }
                }
            }
        }
        for (n, s) in fresh.iter().enumerate() {
            if matched[n].is_none() {
                if let Some(&k) = by_page.get(&s.physical_page_index) {
                    if !claimed[k] {
                        claimed[k] = true;
                        matched[n] = Some(k);
                    }
                }
            }
        }

        let mut new_current = None;
        let mut out = Vec::with_capacity(fresh.len());
        let mut push_inserted = |key: Option<usize>, out: &mut Vec<Slide>, new_current: &mut Option<usize>| {
            for (old_idx, slide) in inserted_after.remove(&key).unwrap_or_default() {
                if old_idx == old_current {
                    *new_current = Some(out.len());
                }
                out.push(slide);
            }
        };

        push_inserted(None, &mut out, &mut new_current);
        for (mut slide, m) in fresh.into_iter().zip(matched) {
            let Some(k) = m else {
                out.push(slide);
                continue;
            };
            let (old_idx, old_slide) = &mut old_pdf[k];
            slide.inking = std::mem::replace(&mut old_slide.inking, crate::ink::SlideInking::new());
            if *old_idx == old_current {
                new_current = Some(out.len());
            }
            out.push(slide);
            push_inserted(Some(k), &mut out, &mut new_current);
        }
        // Inserted slides whose anchor slide no longer exists go to the end, in original order.
        let mut orphans: Vec<(usize, Slide)> = inserted_after.into_values().flatten().collect();
        orphans.sort_by_key(|(i, _)| *i);
        for (old_idx, slide) in orphans {
            if old_idx == old_current {
                new_current = Some(out.len());
            }
            out.push(slide);
        }

        for (i, s) in out.iter_mut().enumerate() {
            s.page_index = i + 1;
        }
        self.current_index = new_current.unwrap_or(old_current).min(out.len().saturating_sub(1));
        self.slides = out;
        self.toc = toc;
    }

    /// Fingerprint of a page's text for `content_hash`. Digits are ignored so page-number
    /// footers ("3 / 12") don't change when slides shift; empty text yields 0 (no identity).
    pub fn text_fingerprint(text: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut any = false;
        for c in text.chars().filter(|c| !c.is_ascii_digit() && !c.is_whitespace()) {
            c.hash(&mut h);
            any = true;
        }
        if any { h.finish().max(1) } else { 0 }
    }

    /// Insert a new whiteboard page directly after the current slide
    pub fn insert_whiteboard_slide(&mut self, is_dark: bool) -> usize {
        let bg_color = if is_dark {
            [24, 24, 27, 255]
        } else {
            [255, 255, 255, 255]
        };
        self.insert_page_with_template(0, "dotted", bg_color)
    }

    /// Populate standard Beamer LaTeX demo presentation with math, TikZ diagram, notes, and dual-screen simulation
    fn populate_demo_slides(&mut self) {
        let (w, h) = (1600, 900);

        // Slide 1: Title Slide
        let s1_img = Arc::new(render_demo_slide(
            w,
            h,
            "Advanced Quantum Field Theory & Computation",
            "Prof. LaTeX Beamer, Department of Mathematical Physics",
            &[
                "• Unified Gauge Theories and Differential Geometry",
                "• Modern computational methods in non-Abelian fields",
                "• Interactive lecture notes & live mathematical proofs",
            ],
            "#0f172a",
            "#38bdf8",
        ));
        self.slides.push(Slide {
            page_index: 1,
            physical_page_index: 1,
            frame_index: 1,
            total_frames: 4,
            overlay_step: 1,
            overlay_total: 1,
            title: "Title: Quantum Field Theory".to_string(),
            notes: "BEAMER NOTES (Slide 1):\n• Welcome attendees and students.\n• Mention that this presentation is rendered in 600 DPI vector quality.\n• Explain that pen and highlighter tools are available on the left toolbar.\n• Press 'L' at any time to point with the laser.".to_string(),
            is_whiteboard: false,
            is_dual_screen: false,
            aspect_ratio: 16.0 / 9.0,
            media_clips: Vec::new(),
            inking: SlideInking::new(),
            content_hash: 0,
            links: vec![
                PdfLinkInfo::new_url([0.05, 0.90, 0.35, 0.08], "https://github.com".to_string()),
                PdfLinkInfo::new_page([0.80, 0.90, 0.16, 0.08], 1),
            ],
            presenter_image: s1_img.clone(),
            audience_image: s1_img,
            notes_image: None,
        });

        // Slide 2: Equations & Formulas Slide
        let s2_img = Arc::new(render_demo_slide(
            w,
            h,
            "Yang-Mills Action & Path Integral Derivation",
            "Action S[A] = -1/4 ∫ Tr(F_μν F^μν) d^4x",
            &[
                "1. Gauge Field Tensor: F_μν = ∂_μ A_ν - ∂_ν A_μ - ig [A_μ, A_ν]",
                "2. Faddeev-Popov quantization and ghost Lagrangian L_ghost",
                "3. BRST invariance: s A_μ = D_μ c, s c = -g/2 [c, c]",
                "4. Note for derivation: Use pen tool (P) or Add Whiteboard (N) for step-by-step tensor expansion.",
            ],
            "#1e1b4b",
            "#a855f7",
        ));
        self.slides.push(Slide {
            page_index: 2,
            physical_page_index: 2,
            frame_index: 2,
            total_frames: 4,
            overlay_step: 1,
            overlay_total: 1,
            title: "Yang-Mills Field Theory".to_string(),
            notes: "BEAMER NOTES (Slide 2):\n• Caution students regarding index contraction conventions (Minkowski metric +---).\n• Highlight BRST symmetry transformation.\n• Insert a whiteboard page with '+' if students ask for the explicit ghost matrix algebra.".to_string(),
            is_whiteboard: false,
            is_dual_screen: false,
            aspect_ratio: 16.0 / 9.0,
            media_clips: Vec::new(),
            inking: SlideInking::new(),
            content_hash: 0,
            links: vec![
                PdfLinkInfo::new_page([0.80, 0.90, 0.16, 0.08], 3),
            ],
            presenter_image: s2_img.clone(),
            audience_image: s2_img,
            notes_image: None,
        });

        // Slide 3: Beamer pgfpages Dual-Screen Simulated Slide
        // Dual screen has double width (3200 x 900)
        let _dual_w = 3200;
        let _dual_h = 900;
        let s3_audience = Arc::new(render_demo_slide(
            w,
            h,
            "Beamer pgfpages: Automatic Dual-Screen Detection",
            "Left half is Audience Output — Right half is Presenter Notes",
            &[
                "• \\setbeameroption{show notes on second screen=right}",
                "• Presenter automatically recognizes aspect ratio >= 2.0 (8:3 or 32:9)",
                "• Audience only sees clean slides with real-time vector inking",
                "• Speaker notes, timers, and next-slide previews stay private on your laptop",
            ],
            "#064e3b",
            "#34d399",
        ));
        let s3_notes = render_demo_slide(
            w,
            h,
            "SPEAKER'S PRIVATE DUAL-SCREEN NOTES",
            "Rendered from the second-screen LaTeX output",
            &[
                "• High-priority discussion points:",
                "  - Remind audience that conference paper submission is next Friday.",
                "  - Check lecture hall microphone level.",
                "  - Play embedded simulation video on the next slide.",
            ],
            "#111827",
            "#f59e0b",
        );
        self.slides.push(Slide {
            page_index: 3,
            physical_page_index: 3,
            frame_index: 3,
            total_frames: 4,
            overlay_step: 1,
            overlay_total: 1,
            title: "Beamer pgfpages Dual-Screen".to_string(),
            notes: "LaTeX BEAMER SECOND SCREEN (Auto-Split from pgfpages):\n• This slide was detected as an ultra-wide double-screen layout!\n• The audience display receives ONLY the left half.\n• Your presenter screen sees this full note breakdown seamlessly.".to_string(),
            is_whiteboard: false,
            is_dual_screen: true,
            aspect_ratio: 16.0 / 9.0,
            media_clips: Vec::new(),
            inking: SlideInking::new(),
            content_hash: 0,
            links: Vec::new(),
            presenter_image: s3_audience.clone(),
            audience_image: s3_audience,
            notes_image: Some(Arc::new(s3_notes)),
        });

        // Slide 4: Embedded Multimedia Demo
        let s4_img = Arc::new(render_demo_slide(
            w,
            h,
            "Embedded Beamer Multimedia & Dynamic Video",
            "Package: \\usepackage{multimedia} and \\movie[autostart]{placeholder}{sim.mp4}",
            &[
                "• Native hardware video acceleration via integrated engine",
                "• Supports MP4, WebM, MKV, OGG, and MP3 audio clips",
                "• Spacebar toggles playback; presenter has scrub bar",
                "• Video is synced across both Presenter and Audience windows",
            ],
            "#311042",
            "#f43f5e",
        ));
        self.slides.push(Slide {
            page_index: 4,
            physical_page_index: 4,
            frame_index: 4,
            total_frames: 4,
            overlay_step: 1,
            overlay_total: 1,
            title: "Multimedia Simulation".to_string(),
            notes: "BEAMER NOTES (Slide 4):\n• Video clip: simulation of non-linear wave turbulence.\n• Pause at 0:15 to highlight soliton collision.\n• Use yellow highlighter to trace wave envelope.".to_string(),
            is_whiteboard: false,
            is_dual_screen: false,
            aspect_ratio: 16.0 / 9.0,
            media_clips: vec![MediaClip {
                page_index: 4,
                file_path: "simulation.mp4".to_string(),
                display_path: "simulation.mp4".to_string(),
                mime_type: "video/mp4".to_string(),
                rect: [0.1, 0.4, 0.8, 0.5],
                autostart: false,
                is_resolved: false,
            }],
            inking: SlideInking::new(),
            content_hash: 0,
            links: vec![
                PdfLinkInfo::new_page([0.05, 0.90, 0.16, 0.08], 0),
            ],
            presenter_image: s4_img.clone(),
            audience_image: s4_img,
            notes_image: None,
        });

        self.toc = vec![
            TocEntry {
                title: "Quantum Field Theory".to_string(),
                page_index: 1,
                level: 0,
                frame_index: 1,
            },
            TocEntry {
                title: "Yang-Mills Field Theory".to_string(),
                page_index: 2,
                level: 0,
                frame_index: 2,
            },
            TocEntry {
                title: "pgfpages Dual-Screen Layout".to_string(),
                page_index: 3,
                level: 0,
                frame_index: 3,
            },
            TocEntry {
                title: "Multimedia Simulation".to_string(),
                page_index: 4,
                level: 0,
                frame_index: 4,
            },
        ];
    }
}

/// Helper function to render a crisp synthetic LaTeX Beamer slide
fn render_demo_slide(
    w: u32,
    h: u32,
    _title: &str,
    _subtitle: &str,
    _bullets: &[&str],
    bg_hex: &str,
    accent_hex: &str,
) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    let bg_color = parse_hex_color(bg_hex);
    let accent_color = parse_hex_color(accent_hex);

    // Fill background with soft vertical gradient
    for y in 0..h {
        let grad = (y as f32 / h as f32) * 20.0;
        let r = (bg_color[0] as f32 + grad).min(255.0) as u8;
        let g = (bg_color[1] as f32 + grad).min(255.0) as u8;
        let b = (bg_color[2] as f32 + grad).min(255.0) as u8;
        let row = (y * w * 4) as usize..((y + 1) * w * 4) as usize;
        for px in img.as_mut()[row].chunks_exact_mut(4) {
            px.copy_from_slice(&[r, g, b, 255]);
        }
    }

    // Top accent bar (Beamer theme header)
    for y in 0..12 {
        for x in 0..w {
            img.put_pixel(x, y, accent_color);
        }
    }

    // Draw decorative cards / LaTeX blocks
    draw_card(&mut img, 80, 80, w - 160, 160, Rgba([255, 255, 255, 15]));
    draw_card(&mut img, 80, 270, w - 160, h - 350, Rgba([255, 255, 255, 10]));

    // Draw Beamer slide footer (Institute, Author, Date, Page)
    for y in (h - 36)..h {
        for x in 0..w {
            img.put_pixel(x, y, Rgba([15, 23, 42, 230]));
        }
    }

    img
}

fn draw_card(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    let max_x = (x + w).min(img.width());
    let max_y = (y + h).min(img.height());
    for py in y..max_y {
        for px in x..max_x {
            // Border highlight
            if py == y || py == max_y - 1 || px == x || px == max_x - 1 {
                img.put_pixel(px, py, Rgba([255, 255, 255, 60]));
            } else {
                img.put_pixel(px, py, color);
            }
        }
    }
}

fn parse_hex_color(hex: &str) -> Rgba<u8> {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(24);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(24);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(27);
        Rgba([r, g, b, 255])
    } else {
        Rgba([24, 24, 27, 255])
    }
}
