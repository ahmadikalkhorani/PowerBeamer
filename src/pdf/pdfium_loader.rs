use super::beamer::{BeamerMetadata, NotesLayoutMode};
use super::deck::{PdfLinkInfo, PdfLinkTarget, RenderQuality, Slide, TocEntry};
use crate::ink::SlideInking;
use pdfium_render::prelude::*;
use std::path::Path;
use std::sync::OnceLock;

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

pub struct PdfiumLoader;

impl PdfiumLoader {
    /// Get thread-safe singleton Pdfium instance
    pub fn get_pdfium() -> Result<&'static Pdfium, String> {
        let res = PDFIUM.get_or_init(|| {
            let mut pdfium_binding = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"));
            if pdfium_binding.is_err() {
                pdfium_binding = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("/opt/homebrew/lib/"));
            }
            if pdfium_binding.is_err() {
                if let Ok(exe_path) = std::env::current_exe() {
                    if let Some(exe_dir) = exe_path.parent() {
                        pdfium_binding = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(exe_dir));
                    }
                }
            }
            if pdfium_binding.is_err() {
                pdfium_binding = Pdfium::bind_to_system_library();
            }

            match pdfium_binding {
                Ok(bindings) => Ok(Pdfium::new(bindings)),
                Err(e) => Err(format!(
                    "Pdfium library not found ({:?}). Please ensure libpdfium is installed or in LD_LIBRARY_PATH.",
                    e
                )),
            }
        });
        match res {
            Ok(p) => Ok(p),
            Err(e) => Err(e.clone()),
        }
    }

    /// Backward-compatible loader returning only slides
    #[allow(dead_code)]
    pub fn load_file(path: &Path, quality: RenderQuality) -> Result<Vec<Slide>, String> {
        Self::load_deck_from_file(path, quality).map(|(slides, _)| slides)
    }

    /// Complete deck loader returning slides with Beamer frame overlays and extracted TOC outline
    pub fn load_deck_from_file(
        path: &Path,
        quality: RenderQuality,
    ) -> Result<(Vec<Slide>, Vec<TocEntry>), String> {
        Self::load_deck_cancellable(path, quality, &|| false)
    }

    pub fn load_deck_cancellable(
        path: &Path,
        quality: RenderQuality,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(Vec<Slide>, Vec<TocEntry>), String> {
        if !path.exists() {
            return Err(format!("File does not exist: {}", path.display()));
        }

        let pdfium = Self::get_pdfium()?;
        let doc = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| format!("Failed to open PDF document: {:?}", e))?;

        let num_pages = doc.pages().len() as usize;
        if num_pages == 0 {
            return Ok((Vec::new(), Vec::new()));
        }

        // 1. Gather page dimensions, labels, and text across all pages
        let mut page_sizes = Vec::with_capacity(num_pages);
        let mut page_labels = Vec::with_capacity(num_pages);
        let mut page_texts = Vec::with_capacity(num_pages);

        for page in doc.pages().iter() {
            if cancelled() {
                return Err("load cancelled".into());
            }
            page_sizes.push((page.width().value, page.height().value));
            page_labels.push(page.label().map(|s| s.to_string()));
            let text = page.text().map(|t| t.all()).unwrap_or_default();
            page_texts.push(text);
        }

        let nav_info = BeamerMetadata::parse_nav_file(path);
        let (mappings, notes_mode) = BeamerMetadata::detect_slide_mappings(
            num_pages,
            &page_sizes,
            &page_labels,
            &page_texts,
            nav_info.as_ref(),
        );

        let is_dual_mode = match notes_mode {
            NotesLayoutMode::DualScreen { is_vertical } => Some(is_vertical),
            _ => None,
        };

        // Mapping from 0-indexed physical PDF page to 0-indexed slide index in `slides`
        let mut page_to_slide_map = std::collections::HashMap::new();
        for (slide_idx, m) in mappings.iter().enumerate() {
            page_to_slide_map.insert(m.slide_page_idx, slide_idx);
            if let Some(note_p) = m.note_page_idx {
                page_to_slide_map.insert(note_p, slide_idx);
            }
        }

        let pdf_has_media_annotations = doc.pages().iter().any(|page| {
            page.annotations()
                .iter()
                .any(|a| is_media_annotation(a.annotation_type()))
        });

        let mut slides = Vec::with_capacity(mappings.len());
        let dpi = quality.dpi();
        let mut media_context = super::media_resolver::MediaContext::new(path);

        for m in &mappings {
            if cancelled() {
                return Err("load cancelled".into());
            }
            let slide_page = doc
                .pages()
                .get(m.slide_page_idx as i32)
                .map_err(|e| format!("Failed to get page {}: {:?}", m.slide_page_idx + 1, e))?;
            let page_width = slide_page.width().value;
            let page_height = slide_page.height().value;

            // Render audience slide at high resolution based on selected DPI
            let render_config = PdfRenderConfig::new()
                .set_target_width((page_width * (dpi / 72.0)) as i32)
                .set_target_height((page_height * (dpi / 72.0)) as i32);

            let rendered_bitmap = slide_page.render_with_config(&render_config).map_err(|e| {
                format!("Rendering error on page {}: {:?}", m.slide_page_idx + 1, e)
            })?;

            let dynamic_img = rendered_bitmap
                .as_image()
                .map_err(|e| format!("Image conversion error: {:?}", e))?;
            // `into_rgba8` reuses the buffer when pdfium already produced RGBA8 (no extra copy).
            let rgba_img = dynamic_img.into_rgba8();

            // Auto-split dual-screen Beamer pgfpages slides OR load separate note page
            let (audience_img, notes_img) = if let Some(is_vertical) = is_dual_mode {
                let w = rgba_img.width();
                let h = rgba_img.height();
                if is_vertical {
                    let half_w = w / 2;
                    let left = image::imageops::crop_imm(&rgba_img, 0, 0, half_w, h).to_image();
                    let right =
                        image::imageops::crop_imm(&rgba_img, half_w, 0, half_w, h).to_image();
                    (left, Some(right))
                } else {
                    let half_h = h / 2;
                    let top = image::imageops::crop_imm(&rgba_img, 0, 0, w, half_h).to_image();
                    let bottom =
                        image::imageops::crop_imm(&rgba_img, 0, half_h, w, half_h).to_image();
                    (top, Some(bottom))
                }
            } else if let Some(note_p_idx) = m.note_page_idx {
                let note_page = doc
                    .pages()
                    .get(note_p_idx as i32)
                    .map_err(|e| format!("Failed to get note page {}: {:?}", note_p_idx + 1, e))?;
                let nw = note_page.width().value;
                let nh = note_page.height().value;
                let note_render_config = PdfRenderConfig::new()
                    .set_target_width((nw * (dpi / 72.0)) as i32)
                    .set_target_height((nh * (dpi / 72.0)) as i32);
                let note_bitmap =
                    note_page
                        .render_with_config(&note_render_config)
                        .map_err(|e| {
                            format!("Rendering error on note page {}: {:?}", note_p_idx + 1, e)
                        })?;
                let note_dynamic = note_bitmap
                    .as_image()
                    .map_err(|e| format!("Image conversion error: {:?}", e))?;
                (rgba_img, Some(note_dynamic.into_rgba8()))
            } else {
                (rgba_img, None)
            };

            // Extract text and annotations for notes
            let mut extracted_notes =
                if let Some(tex_note) = media_context.notes.get(&m.slide_index) {
                    tex_note.clone()
                } else if let Some(tex_note) = media_context.notes.get(&(m.slide_page_idx + 1)) {
                    tex_note.clone()
                } else {
                    String::new()
                };

            if let Some(note_p_idx) = m.note_page_idx {
                let note_text = &page_texts[note_p_idx];
                let cleaned = BeamerMetadata::clean_beamer_note_text(note_text);
                // The companion .tex note is the same text without PDF extraction artifacts
                // (math comes out mangled), so the note page only fills in when it is missing.
                if extracted_notes.is_empty() {
                    extracted_notes = cleaned;
                }
            }

            let page_text = &page_texts[m.slide_page_idx];
            if extracted_notes.is_empty()
                && (page_text.contains("NOTES") || page_text.contains("Note"))
            {
                extracted_notes = page_text.clone();
            }

            for annot in slide_page.annotations().iter() {
                if let Some(contents) = annot.contents() {
                    if !contents.trim().is_empty() {
                        if !extracted_notes.is_empty() {
                            extracted_notes.push_str("\n\n");
                        }
                        extracted_notes.push_str(&contents);
                    }
                }
            }

            if extracted_notes.is_empty() {
                if notes_img.is_some() {
                    extracted_notes = "BEAMER SPEAKER NOTE:\n• High-resolution visual note slide is rendered above.".to_string();
                } else {
                    extracted_notes = format!(
                        "Beamer Slide {} - Speaker Notes\n• Tap to write comments, derivations, and practice reminders here.",
                        m.slide_index
                    );
                }
            }

            // Detect and resolve media clips
            let mut media_clips = crate::pdf::MediaResolver::resolve_page_clips_cached(
                path,
                m.slide_index,
                page_text,
                &mut media_context,
            );
            // Physical-page lookup only makes sense without a .tex frame mapping to go by.
            if media_clips.is_empty()
                && media_context.media.is_empty()
                && m.slide_page_idx + 1 != m.slide_index
            {
                media_clips = crate::pdf::MediaResolver::resolve_page_clips_cached(
                    path,
                    m.slide_page_idx + 1,
                    page_text,
                    &mut media_context,
                );
            }

            // Exact placement: `\movie` / media9 write Movie/Screen/RichMedia annotations whose
            // rectangle is precisely the poster graphic. Match them to clips in document order and
            // only fall back to the image heuristic when the PDF has no such annotation.
            let media_rects: Vec<[f32; 4]> = slide_page
                .annotations()
                .iter()
                .filter(|a| is_media_annotation(a.annotation_type()))
                .filter_map(|a| a.bounds().ok())
                .filter_map(|r| normalize_page_rect(&r, page_width, page_height, is_dual_mode))
                .filter(|r| r[2] > 0.0 && r[3] > 0.0)
                .collect();
            // When the PDF carries media annotations they are the ground truth: a page without one
            // has no media, whatever the .tex/page-number heuristics matched. Extra matches beyond
            // the annotation count are heuristic noise too.
            if pdf_has_media_annotations {
                media_clips.truncate(media_rects.len());
            }
            for (i, clip) in media_clips.iter_mut().enumerate() {
                clip.rect = match media_rects.get(i) {
                    Some(exact) => *exact,
                    None => crate::pdf::MediaResolver::refine_media_rect_from_image(
                        &audience_img,
                        clip.rect,
                    ),
                };
            }

            let slide_aspect = audience_img.width() as f32 / audience_img.height() as f32;
            let audience_img = std::sync::Arc::new(super::SlideImage::new(audience_img));

            // Extract interactive links from PDF slide page
            let mut extracted_links = Vec::new();
            for link in slide_page.links().iter() {
                let Ok(rect) = link.rect() else {
                    continue;
                };
                if rect.width().value <= 0.0 || rect.height().value <= 0.0 {
                    continue;
                }

                let target = if let Some(action) = link.action() {
                    match action {
                        PdfAction::Uri(u) => {
                            if let Ok(uri) = u.uri() {
                                if !uri.trim().is_empty() {
                                    Some(PdfLinkTarget::Url(uri))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        PdfAction::LocalDestination(d) => {
                            if let Ok(dest) = d.destination() {
                                if let Ok(p_idx) = dest.page_index() {
                                    let raw_p = p_idx as usize;
                                    let mapped_p = page_to_slide_map
                                        .get(&raw_p)
                                        .copied()
                                        .unwrap_or(raw_p.min(mappings.len().saturating_sub(1)));
                                    Some(PdfLinkTarget::Page(mapped_p))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                let target = target.or_else(|| {
                    if let Some(dest) = link.destination() {
                        if let Ok(p_idx) = dest.page_index() {
                            let raw_p = p_idx as usize;
                            let mapped_p = page_to_slide_map
                                .get(&raw_p)
                                .copied()
                                .unwrap_or(raw_p.min(mappings.len().saturating_sub(1)));
                            return Some(PdfLinkTarget::Page(mapped_p));
                        }
                    }
                    None
                });

                let Some(target) = target else {
                    continue;
                };

                let Some([norm_x, norm_y, norm_w, norm_h]) =
                    normalize_page_rect(&rect, page_width, page_height, is_dual_mode)
                else {
                    continue;
                };

                let tooltip = match &target {
                    PdfLinkTarget::Url(u) => format!("Open: {}", u),
                    PdfLinkTarget::Page(p) => format!("Jump to Slide {}", p + 1),
                };

                extracted_links.push(PdfLinkInfo {
                    rect: [norm_x, norm_y, norm_w, norm_h],
                    target,
                    tooltip,
                });
            }

            slides.push(Slide {
                page_index: m.slide_index,
                physical_page_index: m.slide_page_idx + 1,
                frame_index: m.slide_index,
                total_frames: mappings.len(),
                overlay_step: 1,
                overlay_total: 1,
                title: format!("Slide {}", m.slide_index),
                notes: extracted_notes,
                is_whiteboard: false,
                is_dual_screen: is_dual_mode.is_some(),
                aspect_ratio: slide_aspect,
                media_clips,
                inking: SlideInking::new(),
                links: extracted_links,
                audio_progress_box: None,
                content_hash: super::deck::PresentationDeck::text_fingerprint(page_text),
                presenter_image: audience_img.clone(),
                audience_image: audience_img,
                notes_image: notes_img
                    .map(|img| std::sync::Arc::new(super::SlideImage::new_notes(img))),
            });
        }

        // 1. Assign Beamer frame overlays from .nav file or heuristic run-length
        BeamerMetadata::assign_frame_metadata(&mut slides, nav_info);

        // 2. Extract Document Outline / TOC
        let toc = Self::extract_toc(&doc, &slides, path, &page_to_slide_map);

        Ok((slides, toc))
    }

    /// Extract hierarchical TOC entries from PDF bookmarks, companion .nav, or fallback frames
    pub fn extract_toc(
        doc: &PdfDocument,
        slides: &[Slide],
        path: &Path,
        page_to_slide_map: &std::collections::HashMap<usize, usize>,
    ) -> Vec<TocEntry> {
        let mut toc = Vec::new();

        // 1. Try PDFium native document bookmarks
        for bookmark in doc.bookmarks().iter() {
            let title = bookmark.title().unwrap_or_else(|| "Section".to_string());
            let page_0 = if let Some(dest) = bookmark.destination() {
                dest.page_index().ok().map(|idx| idx as usize)
            } else if let Some(action) = bookmark.action() {
                action
                    .as_local_destination_action()
                    .and_then(|a| a.destination().ok())
                    .and_then(|d| d.page_index().ok().map(|idx| idx as usize))
            } else {
                None
            };

            let slide_idx = page_0
                .and_then(|p| page_to_slide_map.get(&p).copied())
                .unwrap_or_else(|| {
                    let p_raw = page_0.map(|p| p + 1).unwrap_or(1);
                    slides
                        .iter()
                        .position(|s| s.physical_page_index == p_raw || s.page_index == p_raw)
                        .unwrap_or(0)
                });
            let page_index = slide_idx + 1;
            let frame_index = slides
                .get(slide_idx)
                .map(|s| s.frame_index)
                .unwrap_or(page_index);

            let mut level = 0;
            let mut p = bookmark.parent();
            while let Some(parent) = p {
                level += 1;
                p = parent.parent();
            }

            toc.push(TocEntry {
                title,
                page_index,
                level,
                frame_index,
            });
        }

        // 2. Check companion .nav file for sections/subsections if PDF bookmarks are absent
        if toc.is_empty() {
            if let Some(nav_toc) = Self::parse_nav_toc(path, slides) {
                if !nav_toc.is_empty() {
                    toc = nav_toc;
                }
            }
        }

        // 3. Fallback: Generate TOC from logical frames
        if toc.is_empty() {
            let mut seen_frames = std::collections::HashSet::new();
            for slide in slides {
                if seen_frames.insert(slide.frame_index) {
                    let title = if slide.title.is_empty() || slide.title.starts_with("Slide ") {
                        if slide.overlay_total > 1 {
                            format!(
                                "Frame {} ({} overlays)",
                                slide.frame_index, slide.overlay_total
                            )
                        } else {
                            format!("Frame {}", slide.frame_index)
                        }
                    } else {
                        slide.title.clone()
                    };
                    toc.push(TocEntry {
                        title,
                        page_index: slide.page_index,
                        level: 0,
                        frame_index: slide.frame_index,
                    });
                }
            }
        }

        toc
    }

    /// Parse sections and subsections from Beamer companion .nav file
    pub fn parse_nav_toc(path: &Path, slides: &[Slide]) -> Option<Vec<TocEntry>> {
        let nav_path = path.with_extension("nav");
        if !nav_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&nav_path).ok()?;
        let mut toc = Vec::new();

        for line in content.lines() {
            let is_sec = line.contains("sectionentry") || line.contains("beamer@sectionintoc");
            let is_subsec = line.contains("beamer@subsectionintoc");
            if !is_sec && !is_subsec {
                continue;
            }

            let args = Self::extract_braces_args(line);
            if is_sec && args.len() >= 3 {
                let title = args[1].clone();
                if let Ok(page) = args[2].trim().parse::<usize>() {
                    let slide_idx = slides
                        .iter()
                        .position(|s| s.physical_page_index == page || s.page_index == page)
                        .or_else(|| slides.iter().rposition(|s| s.physical_page_index <= page))
                        .unwrap_or(0);
                    let target_page = slide_idx + 1;
                    let frame_index = slides
                        .get(slide_idx)
                        .map(|s| s.frame_index)
                        .unwrap_or(target_page);
                    toc.push(TocEntry {
                        title,
                        page_index: target_page,
                        level: 0,
                        frame_index,
                    });
                }
            } else if is_subsec && args.len() >= 4 {
                let title = args[2].clone();
                if let Ok(page) = args[3].trim().parse::<usize>() {
                    let slide_idx = slides
                        .iter()
                        .position(|s| s.physical_page_index == page || s.page_index == page)
                        .or_else(|| slides.iter().rposition(|s| s.physical_page_index <= page))
                        .unwrap_or(0);
                    let target_page = slide_idx + 1;
                    let frame_index = slides
                        .get(slide_idx)
                        .map(|s| s.frame_index)
                        .unwrap_or(target_page);
                    toc.push(TocEntry {
                        title,
                        page_index: target_page,
                        level: 1,
                        frame_index,
                    });
                }
            }
        }

        if toc.is_empty() { None } else { Some(toc) }
    }

    /// Extract arguments inside curly braces {arg1}{arg2}... from a LaTeX command line
    fn extract_braces_args(s: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                let mut arg = String::new();
                let mut depth = 1;
                while let Some(inner) = chars.next() {
                    if inner == '{' {
                        depth += 1;
                        arg.push(inner);
                    } else if inner == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        } else {
                            arg.push(inner);
                        }
                    } else {
                        arg.push(inner);
                    }
                }
                args.push(arg.trim().to_string());
            }
        }
        args
    }
}

/// Converts a PDF-space rectangle (origin bottom-left, points) into normalized `[x, y, w, h]`
/// over the audience half of the page. Returns `None` when it lies on the notes half.
fn normalize_page_rect(
    rect: &PdfRect,
    page_width: f32,
    page_height: f32,
    is_dual_mode: Option<bool>,
) -> Option<[f32; 4]> {
    let rx = rect.left().value;
    let ry_bottom = rect.bottom().value;
    let rw = rect.width().value;
    let rh = rect.height().value;
    let ry_top = rect.top().value;

    if let Some(is_vertical) = is_dual_mode {
        if is_vertical {
            let aud_w = page_width / 2.0;
            if rx >= aud_w {
                return None;
            }
            let nx = (rx / aud_w).clamp(0.0, 1.0);
            let nw = (rw / aud_w).clamp(0.0, 1.0 - nx);
            let ny = ((page_height - ry_top) / page_height).clamp(0.0, 1.0);
            let nh = (rh / page_height).clamp(0.0, 1.0 - ny);
            Some([nx, ny, nw, nh])
        } else {
            let aud_h = page_height / 2.0;
            if ry_bottom < aud_h {
                return None;
            }
            let nx = (rx / page_width).clamp(0.0, 1.0);
            let nw = (rw / page_width).clamp(0.0, 1.0 - nx);
            let ny = ((page_height - ry_top) / aud_h).clamp(0.0, 1.0);
            let nh = (rh / aud_h).clamp(0.0, 1.0 - ny);
            Some([nx, ny, nw, nh])
        }
    } else {
        let nx = (rx / page_width).clamp(0.0, 1.0);
        let nw = (rw / page_width).clamp(0.0, 1.0 - nx);
        let ny = ((page_height - ry_top) / page_height).clamp(0.0, 1.0);
        let nh = (rh / page_height).clamp(0.0, 1.0 - ny);
        Some([nx, ny, nw, nh])
    }
}

fn is_media_annotation(kind: PdfPageAnnotationType) -> bool {
    matches!(
        kind,
        PdfPageAnnotationType::Movie
            | PdfPageAnnotationType::Screen
            | PdfPageAnnotationType::RichMedia
    )
}
