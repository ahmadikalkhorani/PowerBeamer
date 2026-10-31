use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

use super::beamer::MediaClip;
use super::deck::{PdfLinkInfo, PdfLinkTarget, RenderQuality, Slide, TocEntry};
use super::pdfium_loader::PdfiumLoader;

/// Represents metadata extracted directly from the .pptx OpenXML structure.
#[derive(Debug, Default, Clone)]
pub struct PptxMetadata {
    /// 1-indexed slide index -> title
    pub slide_titles: HashMap<usize, String>,
    /// 1-indexed slide index -> speaker notes text
    pub slide_notes: HashMap<usize, String>,
    /// 1-indexed slide index -> media clips
    pub slide_media: HashMap<usize, Vec<MediaClip>>,
    /// Hierarchical sections or top-level TOC outline
    pub sections: Vec<TocEntry>,
    pub hidden_slides: std::collections::HashSet<usize>,
    pub slide_links: HashMap<usize, Vec<PdfLinkInfo>>,
    pub slide_count: usize,
    /// Slide width and height in EMUs (1 cm = 360,000 EMU)
    pub slide_size_emu: Option<(u64, u64)>,
}

#[derive(Debug)]
struct PptxRelationship {
    target: String,
    kind: String,
    external: bool,
}

pub struct PptxEngine;

impl PptxEngine {
    /// Detect if a given file path is a PowerPoint presentation.
    pub fn is_pptx(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                let lower = ext.to_lowercase();
                lower == "pptx" || lower == "ppt" || lower == "odp"
            })
            .unwrap_or(false)
    }

    /// Load a presentation from a PowerPoint or OpenDocument file.
    ///
    /// 1. Extracts notes, titles, and media from the `.pptx` archive into a cache.
    /// 2. Converts the presentation to PDF using PowerPoint (COM / AppleScript) or LibreOffice.
    ///    If neither is installed, checks for a companion PDF with the same basename.
    /// 3. Loads the slides with PDFium and enhances each slide with the extracted PPTX notes, titles, and media clips.
    #[allow(dead_code)] // Public library convenience entry point.
    pub fn load_deck_from_pptx(
        pptx_path: &Path,
        quality: RenderQuality,
    ) -> Result<(Vec<Slide>, Vec<TocEntry>), String> {
        Self::load_deck_cancellable(pptx_path, quality, &|| false)
    }

    pub fn load_deck_cancellable(
        pptx_path: &Path,
        quality: RenderQuality,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(Vec<Slide>, Vec<TocEntry>), String> {
        if cancelled() {
            return Err("load cancelled".into());
        }
        if !pptx_path.exists() {
            return Err(format!("File does not exist: {}", pptx_path.display()));
        }

        // 1 & 2. Run metadata extraction (XML parsing & media unzipping) and PDF conversion in parallel
        let pptx_path_clone = pptx_path.to_path_buf();
        let meta_handle = std::thread::spawn(move || {
            Self::parse_pptx_metadata(&pptx_path_clone).unwrap_or_default()
        });

        let pdf_path = Self::ensure_pdf_available(pptx_path)?;
        let metadata = meta_handle.join().unwrap_or_default();

        // 3. Render slides using PDFium
        let (mut slides, mut toc) =
            PdfiumLoader::load_deck_cancellable(&pdf_path, quality, cancelled)?;

        // Companion PDFs must contain every slide to preserve PPTX destinations.
        if metadata.slide_count > 0 && slides.len() != metadata.slide_count {
            return Err(format!(
                "The PDF has {} pages but the PowerPoint has {} slides. Export the companion PDF with hidden slides included, or remove the companion PDF and cached conversion so PowerBeamer can regenerate it.",
                slides.len(),
                metadata.slide_count
            ));
        }

        // 4. Augment slides with PPTX notes, titles, and media
        let slide_count = slides.len();
        for slide in &mut slides {
            slide.links.retain(|link| match &link.target {
                PdfLinkTarget::Url(url) => Self::is_web_or_email_link(url),
                PdfLinkTarget::Page(index) => *index < slide_count,
                PdfLinkTarget::EndShow => true,
            });
            slide.is_hidden = metadata.hidden_slides.contains(&slide.page_index);
            if let Some(links) = metadata.slide_links.get(&slide.page_index) {
                Self::merge_slide_links(&mut slide.links, links);
            }
            // Apply extracted speaker notes if PDF notes were empty or generic
            if let Some(note) = metadata.slide_notes.get(&slide.page_index) {
                if !note.trim().is_empty() {
                    slide.notes = note.clone();
                }
            }

            // Apply slide titles
            if let Some(title) = metadata.slide_titles.get(&slide.page_index) {
                if !title.trim().is_empty() {
                    slide.title = title.clone();
                }
            }

            // Apply media clips: for PPTX presentations, the extracted PPTX shapes and
            // coordinates are the exact ground truth (unlike PDFium's heuristic scan).
            if let Some(clips) = metadata.slide_media.get(&slide.page_index) {
                slide.media_clips = clips.clone();
            } else {
                slide.media_clips.clear();
            }
        }

        // The PDF loader always supplies a fallback outline, even without bookmarks.
        // Native PowerPoint sections must take precedence over that generated outline.
        if !metadata.sections.is_empty() {
            toc = metadata.sections;
        } else if !metadata.slide_titles.is_empty() {
            toc = slides
                .iter()
                .map(|slide| TocEntry {
                    title: if slide.title.is_empty() || slide.title.starts_with("Slide ") {
                        format!("Slide {}", slide.page_index)
                    } else {
                        slide.title.clone()
                    },
                    page_index: slide.page_index,
                    level: 0,
                    frame_index: slide.frame_index,
                })
                .collect();
        }

        Ok((slides, toc))
    }

    /// Converts the presentation to PDF if necessary, or locates a pre-existing companion or cached PDF.
    pub fn ensure_pdf_available(pptx_path: &Path) -> Result<PathBuf, String> {
        let cache_dir = Self::get_pptx_cache_dir(pptx_path)?;
        let cached_pdf = cache_dir.join("presentation-all-slides.pdf");
        let companion_pdf = pptx_path.with_extension("pdf");

        // Priority 1: Check for an up-to-date cached PDF in .cache/powerbeamer/<stem>/
        if cached_pdf.exists() {
            if let (Ok(m_src), Ok(m_cached)) = (
                std::fs::metadata(pptx_path).and_then(|m| m.modified()),
                std::fs::metadata(&cached_pdf).and_then(|m| m.modified()),
            ) {
                if m_cached >= m_src {
                    return Ok(cached_pdf);
                }
            }
        }

        // Priority 2: If user already manually exported a companion PDF alongside the presentation, use it
        if companion_pdf.exists() {
            if let (Ok(m_src), Ok(m_comp)) = (
                std::fs::metadata(pptx_path).and_then(|m| m.modified()),
                std::fs::metadata(&companion_pdf).and_then(|m| m.modified()),
            ) {
                if m_comp >= m_src {
                    println!(
                        "[PPTX Engine] Found companion PDF alongside presentation: {}",
                        companion_pdf.display()
                    );
                    return Ok(companion_pdf);
                }
            }
        }

        // Export a temporary visible copy so all renderers include hidden slides.
        // The original archive and its hidden flags remain unchanged.
        let export_copy = Self::visible_export_copy(pptx_path)?;
        let conversion_input = export_copy
            .as_ref()
            .map(|(_, path)| path.as_path())
            .unwrap_or(pptx_path);

        // Attempt 1: Native Microsoft PowerPoint (macOS AppleScript / Windows COM)
        #[cfg(target_os = "macos")]
        {
            if Self::has_powerpoint_macos() {
                println!("[PPTX Engine] Converting with Microsoft PowerPoint on macOS...");
                match Self::convert_with_powerpoint_macos(conversion_input, &cached_pdf) {
                    Ok(path) => return Ok(path),
                    Err(e) => eprintln!(
                        "[PPTX Engine] PowerPoint conversion failed: {}. Falling back to LibreOffice...",
                        e
                    ),
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            if Self::has_powerpoint_windows() {
                println!(
                    "[PPTX Engine] Converting with Microsoft PowerPoint on Windows (PowerShell/COM)..."
                );
                if let Ok(path) =
                    Self::convert_with_powerpoint_windows(conversion_input, &cached_pdf)
                {
                    return Ok(path);
                }
            }
        }

        // Attempt 2: LibreOffice / OpenOffice headless conversion (macOS, Windows, Linux)
        if let Some(soffice_bin) = Self::find_libreoffice_binary() {
            println!(
                "[PPTX Engine] Converting with LibreOffice ({})...",
                soffice_bin
            );
            if let Ok(path) = Self::convert_with_libreoffice(
                &soffice_bin,
                conversion_input,
                &cache_dir,
                &cached_pdf,
            ) {
                return Ok(path);
            }
        }

        // Attempt 3: Fallback to companion PDF (same directory & filename with .pdf extension)
        if companion_pdf.exists() {
            println!(
                "[PPTX Engine] Neither PowerPoint nor LibreOffice converted successfully; using companion PDF: {}",
                companion_pdf.display()
            );
            return Ok(companion_pdf);
        }

        Err(format!(
            "Could not convert '{}' to PDF. Neither Microsoft PowerPoint nor LibreOffice ('soffice') is available, \
            and no companion PDF was found at '{}'. Please install LibreOffice or export to PDF first.",
            pptx_path.display(),
            companion_pdf.display()
        ))
    }

    /// Finds LibreOffice binary path across Windows, macOS, and Linux
    pub fn find_libreoffice_binary() -> Option<String> {
        let candidates = [
            // Standard CLI commands
            "soffice",
            "libreoffice",
            // macOS standard application bundles
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
            // Windows standard installation paths
            r"C:\Program Files\LibreOffice\program\soffice.exe",
            r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
            // Linux common paths
            "/usr/bin/soffice",
            "/usr/bin/libreoffice",
            "/usr/local/bin/soffice",
            "/opt/homebrew/bin/soffice",
        ];

        for &cmd in &candidates {
            if Path::new(cmd).exists() {
                return Some(cmd.to_string());
            }
            if Command::new(cmd).arg("--version").output().is_ok() {
                return Some(cmd.to_string());
            }
        }
        None
    }

    /// Converts PPTX using LibreOffice headless
    fn convert_with_libreoffice(
        bin: &str,
        input: &Path,
        out_dir: &Path,
        cached_target: &Path,
    ) -> Result<PathBuf, String> {
        let status = Command::new(bin)
            .arg("--headless")
            .arg("--convert-to")
            .arg("pdf")
            .arg(input)
            .arg("--outdir")
            .arg(out_dir)
            .status()
            .map_err(|e| format!("Failed to spawn LibreOffice ({}): {}", bin, e))?;

        if !status.success() {
            return Err("LibreOffice exited with error status".to_string());
        }

        let input_stem = input.file_stem().unwrap_or_default().to_string_lossy();
        let expected_out = out_dir.join(format!("{}.pdf", input_stem));

        if expected_out.exists() {
            if expected_out != *cached_target {
                let _ = std::fs::rename(&expected_out, cached_target);
            }
            if cached_target.exists() {
                return Ok(cached_target.to_path_buf());
            }
            return Ok(expected_out);
        }

        Err("LibreOffice finished but converted PDF was not found".to_string())
    }

    #[cfg(target_os = "macos")]
    fn has_powerpoint_macos() -> bool {
        Command::new("osascript")
            .arg("-e")
            .arg("id of app \"Microsoft PowerPoint\"")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(target_os = "macos")]
    fn convert_with_powerpoint_macos(input: &Path, output: &Path) -> Result<PathBuf, String> {
        let abs_in = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
        let abs_out = if output.is_absolute() {
            output.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(output)
        };

        // In macOS, `std::env::temp_dir()` returns `/var/folders/.../T/` which triggers sandbox prompts,
        // while PowerPoint's container is protected by macOS TCC and cannot be read by external apps.
        // Plain `/tmp` (`/private/tmp`) is universally accessible to both PowerPoint and PowerBeamer without prompts.
        let temp_pdf = PathBuf::from(format!("/tmp/powerbeamer_{}.pdf", std::process::id()));

        let script = format!(
            r#"
            set inPath to POSIX file "{in_path}"
            set outPath to POSIX file "{out_path}"
            tell application "Microsoft PowerPoint"
                open inPath
                set pres to active presentation
                save pres in outPath as save as PDF
                close pres saving no
            end tell
            "#,
            in_path = abs_in.display(),
            out_path = temp_pdf.display(),
        );

        let output_cmd = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| format!("Failed to run AppleScript for PowerPoint: {}", e))?;

        if !output_cmd.status.success() {
            let stderr = String::from_utf8_lossy(&output_cmd.stderr);
            let _ = std::fs::remove_file(&temp_pdf);
            eprintln!(
                "[PPTX Engine] PowerPoint export error: {}. Cascading...",
                stderr.trim()
            );
            return Err(format!("PowerPoint AppleScript error: {}", stderr));
        }

        if temp_pdf.exists() {
            if let Some(parent) = abs_out.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::rename(&temp_pdf, &abs_out) {
                // Rename across filesystem boundaries may fail; fallback to copy + remove
                if let Err(copy_err) = std::fs::copy(&temp_pdf, &abs_out) {
                    let _ = std::fs::remove_file(&temp_pdf);
                    return Err(format!(
                        "Failed to move converted PDF to cache: {} / {}",
                        e, copy_err
                    ));
                }
                let _ = std::fs::remove_file(&temp_pdf);
            }
            Ok(abs_out)
        } else {
            Err("PowerPoint AppleScript reported success but output PDF was not found".to_string())
        }
    }

    #[cfg(target_os = "windows")]
    fn has_powerpoint_windows() -> bool {
        let test_cmd = "$app = New-Object -ComObject PowerPoint.Application; if ($app) { [System.Runtime.InteropServices.Marshal]::ReleaseComObject($app) | Out-Null; exit 0 } else { exit 1 }";
        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(test_cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(target_os = "windows")]
    fn convert_with_powerpoint_windows(input: &Path, output: &Path) -> Result<PathBuf, String> {
        let abs_in = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
        let abs_out = if output.is_absolute() {
            output.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(output)
        };

        let ps_script = format!(
            "$pp = New-Object -ComObject PowerPoint.Application; \
             $pres = $pp.Presentations.Open('{in_path}', $true, $false, $false); \
             $pres.SaveAs('{out_path}', 32); \
             $pres.Close(); \
             $pp.Quit();",
            in_path = abs_in.display(),
            out_path = abs_out.display()
        );

        let status = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(&ps_script)
            .status()
            .map_err(|e| format!("PowerShell COM execution failed: {}", e))?;

        if status.success() && abs_out.exists() {
            Ok(abs_out)
        } else {
            Err("PowerPoint COM conversion failed on Windows".to_string())
        }
    }

    /// Unpacks media and parses XML files inside the PPTX archive
    pub fn parse_pptx_metadata(pptx_path: &Path) -> Result<PptxMetadata, String> {
        let file = File::open(pptx_path).map_err(|e| format!("Failed to open PPTX zip: {}", e))?;
        let mut archive =
            ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {}", e))?;

        let mut meta = PptxMetadata::default();
        let cache_media_dir = Self::get_pptx_cache_dir(pptx_path)?.join("media");
        let _ = std::fs::create_dir_all(&cache_media_dir);

        // Follow presentation relationships: slide filenames do not define display order.
        let pres_xml = Self::read_part(&mut archive, "ppt/presentation.xml").unwrap_or_default();
        meta.slide_size_emu = Self::extract_slide_size(&pres_xml);
        meta.sections = Self::extract_sections(&pres_xml);
        let pres_rels = Self::read_part(&mut archive, "ppt/_rels/presentation.xml.rels")
            .map(|xml| Self::relationships(&xml))
            .unwrap_or_default();
        let slide_parts: Vec<String> = roxmltree::Document::parse(&pres_xml)
            .ok()
            .map(|doc| {
                doc.root_element()
                    .children()
                    .find(|node| node.has_tag_name((Self::P, "sldIdLst")))
                    .into_iter()
                    .flat_map(|list| list.children())
                    .filter(|node| node.has_tag_name((Self::P, "sldId")))
                    .filter_map(|node| pres_rels.get(node.attribute((Self::R, "id"))?))
                    .filter(|rel| !rel.external)
                    .map(|rel| Self::resolve_part("ppt/presentation.xml", &rel.target))
                    .collect()
            })
            .unwrap_or_default();
        meta.slide_count = slide_parts.len();
        let part_positions: HashMap<_, _> = slide_parts
            .iter()
            .enumerate()
            .map(|(index, part)| (part.clone(), index))
            .collect();
        // Scan flags before resolving next/previous action buttons.
        for (index, part) in slide_parts.iter().enumerate() {
            if let Some(xml) = Self::read_part(&mut archive, part) {
                if roxmltree::Document::parse(&xml).ok().is_some_and(|doc| {
                    matches!(doc.root_element().attribute("show"), Some("0" | "false"))
                }) {
                    meta.hidden_slides.insert(index + 1);
                }
            }
        }

        // 2. Extract media files (audio/video only) from `ppt/media/*` to cache directory
        let mut media_filename_to_local_path = HashMap::new();
        for i in 0..archive.len() {
            if let Ok(mut zf) = archive.by_index(i) {
                if let Ok(name) = zf.name() {
                    let name = name.to_string();
                    if name.starts_with("ppt/media/") && !name.ends_with('/') {
                        if let Some(file_name) =
                            Path::new(&name).file_name().and_then(|n| n.to_str())
                        {
                            // Only extract audio and video files — static slide images (.png, .jpg, etc.)
                            // are part of the visual slide rendering and must NOT be treated as playable media.
                            if Self::is_playable_media_extension(file_name) {
                                let out_file = cache_media_dir.join(file_name);
                                if out_file.exists()
                                    && std::fs::metadata(&out_file)
                                        .map(|m| m.len() > 0)
                                        .unwrap_or(false)
                                {
                                    media_filename_to_local_path.insert(
                                        file_name.to_string(),
                                        out_file.to_string_lossy().to_string(),
                                    );
                                } else if let Ok(mut dest) = File::create(&out_file) {
                                    let _ = std::io::copy(&mut zf, &mut dest);
                                    media_filename_to_local_path.insert(
                                        file_name.to_string(),
                                        out_file.to_string_lossy().to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Parse individual slides (titles, bounding boxes for media, rels)
        let (slide_width_emu, slide_height_emu) =
            meta.slide_size_emu.unwrap_or((12192000, 6858000));
        for (index, slide_part) in slide_parts.iter().enumerate() {
            let slide_idx = index + 1;
            let Some(slide_xml) = Self::read_part(&mut archive, slide_part) else {
                continue;
            };
            if let Some(title) = Self::extract_slide_title(&slide_xml) {
                meta.slide_titles.insert(slide_idx, title);
            }

            // Check slide relationships for media links
            let rels_xml_opt = Self::read_part(&mut archive, &Self::rels_part(slide_part));
            {
                // Relative navigation actions need no relationship file.
                let relationships = rels_xml_opt
                    .as_deref()
                    .map(Self::relationships)
                    .unwrap_or_default();
                let r_map = relationships
                    .iter()
                    .map(|(id, rel)| (id.clone(), rel.target.clone()))
                    .collect();
                let links = Self::extract_slide_links(
                    &slide_xml,
                    slide_part,
                    &relationships,
                    &part_positions,
                    index,
                    &meta.hidden_slides,
                    slide_width_emu,
                    slide_height_emu,
                );
                meta.slide_links.insert(slide_idx, links);
                for rel in relationships
                    .values()
                    .filter(|rel| !rel.external && rel.kind.ends_with("/notesSlide"))
                {
                    if let Some(notes) =
                        Self::read_part(&mut archive, &Self::resolve_part(slide_part, &rel.target))
                    {
                        let text = Self::extract_notes_text(&notes);
                        if !text.is_empty() {
                            meta.slide_notes.insert(slide_idx, text);
                        }
                    }
                }
                let mut clips = Self::extract_slide_media_clips(
                    slide_idx,
                    &slide_xml,
                    &r_map,
                    &media_filename_to_local_path,
                    slide_width_emu,
                    slide_height_emu,
                );
                // Sort geometrically (top-to-bottom, left-to-right) rather than arbitrary XML traversal order
                clips.sort_by(|a, b| {
                    a.rect[1]
                        .partial_cmp(&b.rect[1])
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| {
                            a.rect[0]
                                .partial_cmp(&b.rect[0])
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                });
                if !clips.is_empty() {
                    meta.slide_media.insert(slide_idx, clips);
                }
            }
        }

        Ok(meta)
    }

    fn visible_export_copy(input: &Path) -> Result<Option<(tempfile::TempDir, PathBuf)>, String> {
        if !input
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pptx"))
        {
            return Ok(None);
        }
        let mut archive = ZipArchive::new(File::open(input).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let mut replacements = HashMap::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.to_string();
            if !name.starts_with("ppt/slides/") || !name.ends_with(".xml") {
                continue;
            }
            let mut xml = String::new();
            entry.read_to_string(&mut xml).map_err(|e| e.to_string())?;
            let range = roxmltree::Document::parse(&xml).ok().and_then(|doc| {
                let root = doc.root_element();
                root.attribute_node("show")
                    .filter(|attr| matches!(attr.value(), "0" | "false"))
                    .map(|attr| attr.range_value())
            });
            if let Some(range) = range {
                xml.replace_range(range, "1");
                replacements.insert(name, xml);
            }
        }
        if replacements.is_empty() {
            return Ok(None);
        }
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let path = dir
            .path()
            .join(input.file_name().ok_or("Missing PPTX filename")?);
        let mut writer = zip::ZipWriter::new(File::create(&path).map_err(|e| e.to_string())?);
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.to_string();
            if let Some(xml) = replacements.get(&name) {
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .map_err(|e| e.to_string())?;
                writer
                    .write_all(xml.as_bytes())
                    .map_err(|e| e.to_string())?;
            } else {
                writer.raw_copy_file(entry).map_err(|e| e.to_string())?;
            }
        }
        writer.finish().map_err(|e| e.to_string())?;
        Ok(Some((dir, path)))
    }

    const P: &'static str = "http://schemas.openxmlformats.org/presentationml/2006/main";
    const A: &'static str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const R: &'static str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    fn read_part(archive: &mut ZipArchive<File>, path: &str) -> Option<String> {
        let mut xml = String::new();
        archive.by_name(path).ok()?.read_to_string(&mut xml).ok()?;
        Some(xml)
    }

    fn relationships(xml: &str) -> HashMap<String, PptxRelationship> {
        let Ok(doc) = roxmltree::Document::parse(xml) else {
            return HashMap::new();
        };
        doc.root_element()
            .children()
            .filter(|n| n.is_element())
            .filter_map(|node| {
                Some((
                    node.attribute("Id")?.to_string(),
                    PptxRelationship {
                        target: node.attribute("Target")?.to_string(),
                        kind: node.attribute("Type").unwrap_or_default().to_string(),
                        external: node.attribute("TargetMode") == Some("External"),
                    },
                ))
            })
            .collect()
    }

    fn resolve_part(source: &str, target: &str) -> String {
        let mut parts: Vec<&str> = if target.starts_with('/') {
            Vec::new()
        } else {
            source
                .rsplit_once('/')
                .map(|(dir, _)| dir.split('/').collect())
                .unwrap_or_default()
        };
        for part in target.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(part),
            }
        }
        parts.join("/")
    }

    fn rels_part(part: &str) -> String {
        let (dir, name) = part.rsplit_once('/').unwrap_or(("", part));
        format!("{dir}/_rels/{name}.rels")
    }

    #[allow(clippy::too_many_arguments)]
    fn extract_slide_links(
        xml: &str,
        slide_part: &str,
        rels: &HashMap<String, PptxRelationship>,
        positions: &HashMap<String, usize>,
        index: usize,
        hidden: &std::collections::HashSet<usize>,
        width: u64,
        height: u64,
    ) -> Vec<PdfLinkInfo> {
        let Ok(doc) = roxmltree::Document::parse(xml) else {
            return Vec::new();
        };
        let visible = |i: &usize| !hidden.contains(&(i + 1));
        doc.descendants()
            .filter(|node| node.has_tag_name((Self::A, "hlinkClick")))
            .filter_map(|link| {
                let action = link.attribute("action").unwrap_or_default();
                let target = if let Some(jump) =
                    action.strip_prefix("ppaction://hlinkshowjump?jump=")
                {
                    match jump {
                        "nextslide" => (index + 1..positions.len())
                            .find(visible)
                            .map(PdfLinkTarget::Page)
                            .unwrap_or(PdfLinkTarget::EndShow),
                        "previousslide" => PdfLinkTarget::Page((0..index).rev().find(visible)?),
                        "firstslide" => PdfLinkTarget::Page((0..positions.len()).find(visible)?),
                        "lastslide" => {
                            PdfLinkTarget::Page((0..positions.len()).rev().find(visible)?)
                        }
                        "endshow" => PdfLinkTarget::EndShow,
                        _ => return None,
                    }
                } else {
                    let rel = rels.get(link.attribute((Self::R, "id"))?)?;
                    if action == "ppaction://hlinksldjump"
                        || (action.is_empty() && rel.kind.ends_with("/slide"))
                    {
                        if rel.external {
                            return None;
                        }
                        PdfLinkTarget::Page(
                            *positions.get(&Self::resolve_part(slide_part, &rel.target))?,
                        )
                    } else if action.is_empty() && rel.external && rel.kind.ends_with("/hyperlink")
                    {
                        // Only normal browser/email links are opened by the presenter.
                        if !Self::is_web_or_email_link(&rel.target) {
                            return None;
                        }
                        PdfLinkTarget::Url(rel.target.clone())
                    } else {
                        return None;
                    }
                };
                let shape = link.ancestors().find(|node| {
                    node.has_tag_name((Self::P, "sp")) || node.has_tag_name((Self::P, "pic"))
                })?;
                let rect = Self::shape_link_rect(shape, width, height)?;
                let tooltip = link
                    .attribute("tooltip")
                    .filter(|text| !text.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| match &target {
                        PdfLinkTarget::Url(url) => format!("Open: {url}"),
                        PdfLinkTarget::Page(page) => format!("Jump to Slide {}", page + 1),
                        PdfLinkTarget::EndShow => "End presentation".to_string(),
                    });
                Some(PdfLinkInfo {
                    rect,
                    target,
                    tooltip,
                })
            })
            .collect()
    }

    fn is_web_or_email_link(url: &str) -> bool {
        url.split_once(':').is_some_and(|(scheme, _)| {
            ["https", "http", "mailto"]
                .iter()
                .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
        })
    }

    fn merge_slide_links(pdf_links: &mut Vec<PdfLinkInfo>, native: &[PdfLinkInfo]) {
        let additions: Vec<_> = native
            .iter()
            .filter(|link| {
                // Retain precise PDF text-run regions when conversion preserved them.
                let precise = pdf_links.iter().any(|pdf| {
                    pdf.target == link.target
                        && link.contains(
                            pdf.rect[0] + pdf.rect[2] / 2.0,
                            pdf.rect[1] + pdf.rect[3] / 2.0,
                            0.0,
                        )
                });
                // Several distinct text links can share a text box. Without individual
                // PDF regions, making that entire box open the first URL would be wrong.
                let ambiguous = native
                    .iter()
                    .any(|other| other.rect == link.rect && other.target != link.target);
                !precise && !ambiguous
            })
            .cloned()
            .collect();
        pdf_links.splice(0..0, additions);
    }

    fn shape_link_rect(
        shape: roxmltree::Node<'_, '_>,
        width: u64,
        height: u64,
    ) -> Option<[f32; 4]> {
        let number =
            |node: roxmltree::Node<'_, '_>, attr| node.attribute(attr)?.parse::<f32>().ok();
        let transform = shape
            .descendants()
            .find(|node| node.has_tag_name((Self::A, "xfrm")))?;
        let vector = |transform: roxmltree::Node<'_, '_>, name, x, y| {
            let node = transform
                .children()
                .find(|node| node.has_tag_name((Self::A, name)))?;
            Some([number(node, x)?, number(node, y)?])
        };
        let offset = vector(transform, "off", "x", "y")?;
        let extent = vector(transform, "ext", "cx", "cy")?;
        let mut corners = [
            offset,
            [offset[0] + extent[0], offset[1]],
            [offset[0], offset[1] + extent[1]],
            [offset[0] + extent[0], offset[1] + extent[1]],
        ];
        let orient = |corners: &mut [[f32; 2]; 4],
                      transform: roxmltree::Node<'_, '_>,
                      offset: [f32; 2],
                      extent: [f32; 2]| {
            let angle = transform
                .attribute("rot")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0)
                / 60000.0;
            let (sin, cos) = angle.to_radians().sin_cos();
            for point in corners {
                let mut x = point[0] - offset[0] - extent[0] / 2.0;
                let mut y = point[1] - offset[1] - extent[1] / 2.0;
                if matches!(transform.attribute("flipH"), Some("1" | "true")) {
                    x = -x;
                }
                if matches!(transform.attribute("flipV"), Some("1" | "true")) {
                    y = -y;
                }
                *point = [
                    offset[0] + extent[0] / 2.0 + x * cos - y * sin,
                    offset[1] + extent[1] / 2.0 + x * sin + y * cos,
                ];
            }
        };
        orient(&mut corners, transform, offset, extent);
        // Map nested group child coordinates into each parent's coordinate system.
        for group in shape
            .ancestors()
            .filter(|node| node.has_tag_name((Self::P, "grpSp")))
        {
            let transform = group
                .children()
                .find(|node| node.has_tag_name((Self::P, "grpSpPr")))?
                .children()
                .find(|node| node.has_tag_name((Self::A, "xfrm")))?;
            let offset = vector(transform, "off", "x", "y")?;
            let extent = vector(transform, "ext", "cx", "cy")?;
            let child_offset = vector(transform, "chOff", "x", "y")?;
            let child_extent = vector(transform, "chExt", "cx", "cy")?;
            if child_extent.contains(&0.0) {
                return None;
            }
            for point in &mut corners {
                for axis in 0..2 {
                    point[axis] = offset[axis]
                        + (point[axis] - child_offset[axis]) * extent[axis] / child_extent[axis];
                }
            }
            orient(&mut corners, transform, offset, extent);
        }
        if width == 0 || height == 0 {
            return None;
        }
        let min = [
            corners.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
            corners.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
        ];
        let max = [
            corners
                .iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max),
            corners
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max),
        ];
        let rect = [
            min[0] / width as f32,
            min[1] / height as f32,
            (max[0] - min[0]) / width as f32,
            (max[1] - min[1]) / height as f32,
        ];
        (rect.iter().all(|v| v.is_finite()) && rect[2] > 0.0 && rect[3] > 0.0).then_some(rect)
    }

    /// Extract slide dimensions from `ppt/presentation.xml`
    fn extract_slide_size(xml: &str) -> Option<(u64, u64)> {
        // e.g. <p:sldSz cx="12192000" cy="6858000"/>
        let sld_sz_start = xml.find("<p:sldSz")?;
        let sld_sz_tag = &xml[sld_sz_start..xml[sld_sz_start..].find('>')? + sld_sz_start];
        let cx = Self::extract_xml_attribute(sld_sz_tag, "cx")?
            .parse::<u64>()
            .ok()?;
        let cy = Self::extract_xml_attribute(sld_sz_tag, "cy")?
            .parse::<u64>()
            .ok()?;
        Some((cx, cy))
    }

    /// Map native PowerPoint sections to presentation positions, not slide file numbers.
    fn extract_sections(xml: &str) -> Vec<TocEntry> {
        const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
        const P14: &str = "http://schemas.microsoft.com/office/powerpoint/2010/main";
        let Ok(doc) = roxmltree::Document::parse(xml) else {
            return Vec::new();
        };
        let presentation = doc.root_element();
        let slide_positions: HashMap<_, _> = presentation
            .children()
            .find(|node| node.has_tag_name((P, "sldIdLst")))
            .into_iter()
            .flat_map(|list| {
                list.children()
                    .filter(|node| node.has_tag_name((P, "sldId")))
            })
            .enumerate()
            .filter_map(|(index, node)| node.attribute("id").map(|id| (id, index + 1)))
            .collect();

        let mut entries: Vec<_> = presentation
            .descendants()
            .filter(|node| node.has_tag_name((P14, "section")))
            .filter_map(|section| {
                let title = section.attribute("name")?;
                if title.trim().is_empty() {
                    return None;
                }
                let page_index = section
                    .children()
                    .filter(|node| node.has_tag_name((P14, "sldIdLst")))
                    .flat_map(|list| list.children())
                    .filter(|node| node.has_tag_name((P14, "sldId")))
                    .filter_map(|node| slide_positions.get(node.attribute("id")?).copied())
                    .min()?;
                Some(TocEntry {
                    title: title.to_string(),
                    page_index,
                    level: 0,
                    frame_index: page_index,
                })
            })
            .collect();
        entries.sort_by_key(|entry| entry.page_index);
        entries
    }

    /// Extract title text from `<p:sp>` blocks marked as title placeholder
    fn extract_slide_title(xml: &str) -> Option<String> {
        for sp in xml.split("<p:sp>") {
            if sp.contains("type=\"title\"") || sp.contains("type=\"ctrTitle\"") {
                let text = Self::extract_all_text_runs(sp);
                if !text.trim().is_empty() {
                    return Some(text.trim().to_string());
                }
            }
        }
        None
    }

    /// Extract speaker notes from `ppt/notesSlides/notesSlide{N}.xml`
    fn extract_notes_text(xml: &str) -> String {
        let mut paragraphs = Vec::new();
        for sp in xml.split("<p:sp>") {
            // Skip slide number and header/footer placeholders
            if sp.contains("type=\"sldNum\"")
                || sp.contains("type=\"hdr\"")
                || sp.contains("type=\"ftr\"")
            {
                continue;
            }
            // Notes body usually has type="body"
            if sp.contains("type=\"body\"") || sp.contains("<p:txBody>") {
                let text = Self::extract_all_text_runs(sp);
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    paragraphs.push(trimmed.to_string());
                }
            }
        }
        paragraphs.join("\n\n")
    }

    /// Collect all `<a:t>...</a:t>` text fragments in an XML snippet
    fn extract_all_text_runs(snippet: &str) -> String {
        let mut result = String::new();
        let mut cur = snippet;
        while let Some(start) = cur.find("<a:t>") {
            let after = &cur[start + 5..];
            if let Some(end) = after.find("</a:t>") {
                result.push_str(&after[..end]);
                result.push(' ');
                cur = &after[end + 6..];
            } else {
                break;
            }
        }
        result
    }

    /// Extract MediaClips from a slide's XML and its relationships
    fn extract_slide_media_clips(
        page_index: usize,
        slide_xml: &str,
        rels: &HashMap<String, String>,
        local_media_map: &HashMap<String, String>,
        slide_w: u64,
        slide_h: u64,
    ) -> Vec<MediaClip> {
        let mut clips = Vec::new();

        // Search through shapes and pictures: <p:pic> or <p:sp>
        for shape in slide_xml.split("<p:pic>") {
            // Find all relationship IDs embedded or linked in this shape
            let mut matched_media: Option<(String, String)> = None;

            for attr in &["r:embed=\"", "r:link=\""] {
                let mut search_from = 0;
                while let Some(pos) = shape[search_from..].find(attr) {
                    let start = search_from + pos + attr.len();
                    if let Some(end) = shape[start..].find('"') {
                        let id = &shape[start..start + end];
                        if let Some(target) = rels.get(id) {
                            if let Some(filename) =
                                Path::new(target).file_name().and_then(|n| n.to_str())
                            {
                                if let Some(local_path) = local_media_map.get(filename) {
                                    matched_media =
                                        Some((filename.to_string(), local_path.clone()));
                                    break;
                                }
                            }
                        }
                    }
                    search_from = start;
                }
                if matched_media.is_some() {
                    break;
                }
            }

            if let Some((filename, local_path)) = matched_media {
                let rect = Self::extract_normalized_rect(shape, slide_w, slide_h)
                    .unwrap_or([0.1, 0.2, 0.8, 0.6]);

                let mime = Self::guess_mime(&filename);
                let mut clip = MediaClip::new(page_index, local_path, mime);
                clip.rect = rect;
                clip.autostart = shape.contains("<p:cond delay=\"0\"/>");
                clips.push(clip);
            }
        }
        clips
    }

    /// Parse `<a:xfrm>` coordinates and convert EMUs to normalized 0.0..1.0 coordinates
    fn extract_normalized_rect(snippet: &str, slide_w: u64, slide_h: u64) -> Option<[f32; 4]> {
        // Find <a:xfrm>...</a:xfrm> block to avoid matching unrelated <a:ext uri="..."> extension tags
        let xfrm_start = snippet.find("<a:xfrm")?;
        let xfrm_end = snippet[xfrm_start..].find("</a:xfrm>")? + xfrm_start;
        let xfrm_block = &snippet[xfrm_start..xfrm_end];

        let off_idx = xfrm_block.find("<a:off")?;
        let off_tag = &xfrm_block[off_idx..xfrm_block[off_idx..].find("/>")? + off_idx];

        let ext_idx = xfrm_block.find("<a:ext")?;
        let ext_tag = &xfrm_block[ext_idx..xfrm_block[ext_idx..].find("/>")? + ext_idx];

        let x = Self::extract_xml_attribute(off_tag, "x")?
            .parse::<f32>()
            .ok()?;
        let y = Self::extract_xml_attribute(off_tag, "y")?
            .parse::<f32>()
            .ok()?;
        let w = Self::extract_xml_attribute(ext_tag, "cx")?
            .parse::<f32>()
            .ok()?;
        let h = Self::extract_xml_attribute(ext_tag, "cy")?
            .parse::<f32>()
            .ok()?;

        let sw = slide_w as f32;
        let sh = slide_h as f32;

        if sw > 0.0 && sh > 0.0 {
            Some([
                (x / sw).clamp(0.0, 1.0),
                (y / sh).clamp(0.0, 1.0),
                (w / sw).clamp(0.0, 1.0),
                (h / sh).clamp(0.0, 1.0),
            ])
        } else {
            None
        }
    }

    fn extract_xml_attribute(tag: &str, attr: &str) -> Option<String> {
        let pattern = format!("{}=\"", attr);
        let start = tag.find(&pattern)? + pattern.len();
        let end = tag[start..].find('"')? + start;
        Some(tag[start..end].to_string())
    }

    /// Check if filename has an actual audio or video extension (excludes images like .png, .jpg)
    pub fn is_playable_media_extension(filename: &str) -> bool {
        let lower = filename.to_lowercase();
        lower.ends_with(".mp4")
            || lower.ends_with(".mov")
            || lower.ends_with(".mkv")
            || lower.ends_with(".avi")
            || lower.ends_with(".webm")
            || lower.ends_with(".m4v")
            || lower.ends_with(".ogv")
            || lower.ends_with(".mp3")
            || lower.ends_with(".wav")
            || lower.ends_with(".ogg")
            || lower.ends_with(".flac")
            || lower.ends_with(".m4a")
            || lower.ends_with(".aac")
    }

    fn guess_mime(filename: &str) -> String {
        let lower = filename.to_lowercase();
        if lower.ends_with(".mp4") || lower.ends_with(".m4v") {
            "video/mp4".to_string()
        } else if lower.ends_with(".mov") {
            "video/quicktime".to_string()
        } else if lower.ends_with(".webm") {
            "video/webm".to_string()
        } else if lower.ends_with(".mkv") {
            "video/x-matroska".to_string()
        } else if lower.ends_with(".avi") {
            "video/x-msvideo".to_string()
        } else if lower.ends_with(".mp3") {
            "audio/mpeg".to_string()
        } else if lower.ends_with(".wav") {
            "audio/wav".to_string()
        } else if lower.ends_with(".m4a") || lower.ends_with(".aac") {
            "audio/mp4".to_string()
        } else if lower.ends_with(".ogg") || lower.ends_with(".ogv") {
            "audio/ogg".to_string()
        } else if lower.ends_with(".flac") {
            "audio/flac".to_string()
        } else {
            "application/octet-stream".to_string()
        }
    }

    /// Provides a dedicated cache directory for PPTX intermediate artifacts and extracted media.
    /// Prefers `.cache/powerbeamer/<stem>` inside the same directory as the presentation file,
    /// keeping all associated presentation files co-located and self-contained.
    pub fn get_pptx_cache_dir(pptx_path: &Path) -> Result<PathBuf, String> {
        let file_stem = pptx_path.file_stem().unwrap_or_default().to_string_lossy();

        // 1. Try local cache in the same directory: <presentation_folder>/.cache/powerbeamer/<file_stem>
        if let Some(parent_dir) = pptx_path.parent() {
            let local_cache = parent_dir
                .join(".cache")
                .join("powerbeamer")
                .join(file_stem.as_ref());
            if std::fs::create_dir_all(&local_cache).is_ok() {
                return Ok(local_cache);
            }
        }

        // 2. Fallback to global user cache if the presentation directory is read-only
        let base_cache = if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home)
                .join(".cache")
                .join("powerbeamer")
                .join("pptx")
        } else if let Some(appdata) = std::env::var_os("LOCALAPPDATA") {
            PathBuf::from(appdata).join("PowerBeamer").join("pptx")
        } else {
            std::env::temp_dir().join("powerbeamer").join("pptx")
        };

        let target_dir = base_cache.join(file_stem.as_ref());
        std::fs::create_dir_all(&target_dir).map_err(|e| {
            format!(
                "Failed to create cache directory '{}': {}",
                target_dir.display(),
                e
            )
        })?;

        Ok(target_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_pptx_detection() {
        assert!(PptxEngine::is_pptx(Path::new("slides.pptx")));
        assert!(PptxEngine::is_pptx(Path::new("presentation.PPTX")));
        assert!(PptxEngine::is_pptx(Path::new("talk.ppt")));
        assert!(PptxEngine::is_pptx(Path::new("open.odp")));
        assert!(!PptxEngine::is_pptx(Path::new("presentation.pdf")));
        assert!(!PptxEngine::is_pptx(Path::new("notes.txt")));
    }

    #[test]
    fn test_pptx_metadata_extraction() {
        let sample = Path::new("test_deck/sample_deck.pptx");
        if sample.exists() {
            let meta =
                PptxEngine::parse_pptx_metadata(sample).expect("Failed to parse sample pptx");
            assert_eq!(meta.slide_size_emu, Some((12192000, 6858000)));
            assert_eq!(
                meta.slide_titles.get(&1).map(|s| s.as_str()),
                Some("Quantum Computing Overview")
            );
            assert_eq!(
                meta.slide_notes.get(&1).map(|s| s.as_str()),
                Some("Welcome everyone. Highlight the physical qubit coherence times today.")
            );
            assert_eq!(meta.sections.len(), 1);
            assert_eq!(meta.sections[0].title, "Introduction");
        }
    }

    #[test]
    fn native_sections_replace_pdf_frame_fallback_on_load() {
        use pdfium_render::prelude::*;

        let temp = tempfile::tempdir().unwrap();
        let pptx = temp.path().join("sections.pptx");
        std::fs::copy("test_deck/sample_deck.pptx", &pptx).unwrap();
        // A companion PDF exercises loading without invoking PowerPoint/LibreOffice.
        let pdfium = PdfiumLoader::get_pdfium().unwrap();
        let mut pdf = pdfium.create_new_pdf().unwrap();
        pdf.pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(
                PdfPoints::new(960.0),
                PdfPoints::new(540.0),
            ))
            .unwrap();
        let pdf_path = pptx.with_extension("pdf");
        pdf.save_to_file(&pdf_path).unwrap();
        drop(pdf);

        let (slides, toc) = PptxEngine::load_deck_from_pptx(&pptx, RenderQuality::Eco).unwrap();
        assert_eq!(slides.len(), 1);
        assert_eq!(
            toc,
            vec![TocEntry {
                title: "Introduction".into(),
                page_index: 1,
                frame_index: 1,
                level: 0,
            }]
        );
    }

    #[test]
    fn sections_follow_slide_order_and_decode_names() {
        // Prefixes are arbitrary; slide IDs are neither positions nor file numbers.
        let xml = r#"<deck:presentation
            xmlns:deck="http://schemas.openxmlformats.org/presentationml/2006/main"
            xmlns:sec="http://schemas.microsoft.com/office/powerpoint/2010/main">
          <deck:sldIdLst>
            <deck:sldId id="300"/><deck:sldId id="256"/>
            <deck:sldId id="900"/><deck:sldId id="257"/>
          </deck:sldIdLst>
          <deck:extLst><deck:ext><sec:sectionLst>
            <sec:section name="Later"><sec:sldIdLst>
              <sec:sldId id="257"/><sec:sldId id="900"/>
            </sec:sldIdLst></sec:section>
            <sec:section name="Intro &amp; &quot;Goals&quot;"><sec:sldIdLst>
              <sec:sldId id="300"/><sec:sldId id="256"/>
            </sec:sldIdLst></sec:section>
            <sec:section name="Empty"/>
            <sec:section name="Stale"><sec:sldIdLst>
              <sec:sldId id="999"/>
            </sec:sldIdLst></sec:section>
          </sec:sectionLst></deck:ext></deck:extLst>
        </deck:presentation>"#;
        let sections = PptxEngine::extract_sections(xml);
        assert_eq!(
            sections,
            vec![
                TocEntry {
                    title: "Intro & \"Goals\"".into(),
                    page_index: 1,
                    frame_index: 1,
                    level: 0
                },
                TocEntry {
                    title: "Later".into(),
                    page_index: 3,
                    frame_index: 3,
                    level: 0
                },
            ]
        );
    }

    #[test]
    fn sections_absent_or_invalid_allow_title_fallback() {
        assert!(PptxEngine::extract_sections("<broken").is_empty());
        assert!(PptxEngine::extract_sections(
            r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#
        ).is_empty());
    }

    fn write_interactive_deck(path: &Path) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let rel_ns = "http://schemas.openxmlformats.org/package/2006/relationships";
        let mut part = |name: &str, xml: &str| {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        };
        part(
            "ppt/presentation.xml",
            &format!(
                r#"<p:presentation xmlns:p="{}" xmlns:r="{}">
            <p:sldIdLst><p:sldId id="900" r:id="one"/><p:sldId id="256" r:id="two"/><p:sldId id="500" r:id="three"/></p:sldIdLst>
            <p:sldSz cx="1000" cy="500"/>
        </p:presentation>"#,
                PptxEngine::P,
                PptxEngine::R
            ),
        );
        part(
            "ppt/_rels/presentation.xml.rels",
            &format!(
                r#"<Relationships xmlns="{rel_ns}">
            <Relationship Id="one" Target="slides/slide7.xml"/>
            <Relationship Id="two" Target="slides/slide2.xml"/>
            <Relationship Id="three" Target="slides/slide19.xml"/>
        </Relationships>"#
            ),
        );
        let mut shapes = String::new();
        for (button_index, (id, action)) in [
            ("web", ""),
            ("hidden", "ppaction://hlinksldjump"),
            ("", "ppaction://hlinkshowjump?jump=nextslide"),
            ("", "ppaction://hlinkshowjump?jump=previousslide"),
            ("", "ppaction://hlinkshowjump?jump=firstslide"),
            ("", "ppaction://hlinkshowjump?jump=lastslide"),
            ("", "ppaction://hlinkshowjump?jump=endshow"),
            ("web", "ppaction://macro?name=DoSomething"),
            ("file", ""),
            ("missing", "ppaction://hlinksldjump"),
        ]
        .into_iter()
        .enumerate()
        {
            shapes.push_str(&format!(r#"<p:sp><p:nvSpPr><p:cNvPr><a:hlinkClick r:id="{id}" action="{action}"/></p:cNvPr></p:nvSpPr>
                <p:spPr><a:xfrm><a:off x="{x}" y="50"/><a:ext cx="200" cy="100"/></a:xfrm></p:spPr></p:sp>"#, x = 100 + button_index * 10));
        }
        part(
            "ppt/slides/slide7.xml",
            &format!(
                r#"<p:sld xmlns:p="{}" xmlns:a="{}" xmlns:r="{}">
            <p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
            <p:txBody><a:p><a:r><a:t>First in display order</a:t></a:r></a:p></p:txBody></p:sp>{shapes}</p:spTree></p:cSld>
        </p:sld>"#,
                PptxEngine::P,
                PptxEngine::A,
                PptxEngine::R
            ),
        );
        part(
            "ppt/slides/slide2.xml",
            &format!(r#"<p:sld xmlns:p="{}" show="false"/>"#, PptxEngine::P),
        );
        part(
            "ppt/slides/slide19.xml",
            &format!(
                r#"<p:sld xmlns:p="{}" xmlns:a="{}" show="1"><p:sp>
                <p:nvSpPr><p:cNvPr><a:hlinkClick action="ppaction://hlinkshowjump?jump=nextslide"/></p:cNvPr></p:nvSpPr>
                <p:spPr><a:xfrm><a:off x="10" y="10"/><a:ext cx="20" cy="20"/></a:xfrm></p:spPr>
            </p:sp></p:sld>"#,
                PptxEngine::P,
                PptxEngine::A
            ),
        );
        part(
            "ppt/slides/_rels/slide7.xml.rels",
            &format!(
                r#"<Relationships xmlns="{rel_ns}">
            <Relationship Id="web" Type="{}/hyperlink" Target="https://example.org/?a=1&amp;b=2" TargetMode="External"/>
            <Relationship Id="hidden" Type="{}/slide" Target="../slides/slide2.xml"/>
            <Relationship Id="file" Type="{}/hyperlink" Target="file:///tmp/program" TargetMode="External"/>
            <Relationship Id="note" Type="{}/notesSlide" Target="../notesSlides/notesSlide42.xml"/>
        </Relationships>"#,
                PptxEngine::R,
                PptxEngine::R,
                PptxEngine::R,
                PptxEngine::R
            ),
        );
        part(
            "ppt/notesSlides/notesSlide42.xml",
            &format!(
                r#"<p:notes xmlns:p="{}" xmlns:a="{}"><p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Correct notes</a:t></a:r></a:p></p:txBody></p:sp></p:notes>"#,
                PptxEngine::P,
                PptxEngine::A
            ),
        );
        zip.finish().unwrap();
    }

    fn write_companion_pdf(path: &Path, count: usize) {
        use pdfium_render::prelude::*;
        let pdfium = PdfiumLoader::get_pdfium().unwrap();
        let mut pdf = pdfium.create_new_pdf().unwrap();
        for _ in 0..count {
            pdf.pages_mut()
                .create_page_at_end(PdfPagePaperSize::Custom(
                    PdfPoints::new(160.0),
                    PdfPoints::new(90.0),
                ))
                .unwrap();
        }
        pdf.save_to_file(&path.with_extension("pdf")).unwrap();
    }

    #[test]
    fn reordered_slides_hidden_flags_and_click_actions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("interactive.pptx");
        write_interactive_deck(&path);
        let meta = PptxEngine::parse_pptx_metadata(&path).unwrap();
        assert_eq!(meta.slide_count, 3);
        assert_eq!(meta.hidden_slides, [2].into_iter().collect());
        assert_eq!(meta.slide_titles[&1], "First in display order");
        assert_eq!(meta.slide_notes[&1], "Correct notes");
        assert_eq!(meta.slide_links[&3][0].target, PdfLinkTarget::EndShow);
        let links = &meta.slide_links[&1];
        assert_eq!(
            links
                .iter()
                .map(|link| link.target.clone())
                .collect::<Vec<_>>(),
            vec![
                PdfLinkTarget::Url("https://example.org/?a=1&b=2".into()),
                PdfLinkTarget::Page(1), // explicit destination can be hidden
                PdfLinkTarget::Page(2), // next action skips the hidden slide
                PdfLinkTarget::Page(0),
                PdfLinkTarget::Page(2),
                PdfLinkTarget::EndShow,
            ]
        );
        assert_eq!(links[0].rect, [0.1, 0.1, 0.2, 0.2]);
        assert!(links[0].contains(0.2, 0.2, 0.0));
        assert!(!links[0].contains(0.8, 0.8, 0.0));

        write_companion_pdf(&path, 3);
        let (slides, _) = PptxEngine::load_deck_from_pptx(&path, RenderQuality::Eco).unwrap();
        assert_eq!(
            slides.iter().map(|s| s.is_hidden).collect::<Vec<_>>(),
            [false, true, false]
        );
        assert_eq!(slides[0].links, *links);
        assert!(slides[1].export_snapshot().is_hidden);
        let mut deck = super::super::PresentationDeck {
            file_path: Some(path),
            slides,
            toc: Vec::new(),
            current_index: 0,
            quality: RenderQuality::Eco,
        };
        assert_eq!(deck.next_visible_index(), Some(2));
        assert!(deck.next());
        assert_eq!(deck.current_index, 2);
        assert!(!deck.next());
        assert!(deck.prev());
        assert_eq!(deck.current_index, 0);
        assert!(!deck.prev());
        assert!(deck.jump_to(1)); // hidden slides remain directly accessible
        assert!(deck.next());
        assert_eq!(deck.current_index, 2);
        assert!(deck.first_slide());
        assert!(deck.next_frame());
        assert_eq!(deck.current_index, 2);
        assert!(deck.prev_frame());
        assert_eq!(deck.current_index, 0);
        deck.slides[0].is_hidden = true;
        assert!(deck.first_slide());
        assert_eq!(deck.current_index, 2);
        deck.slides[2].is_hidden = true;
        assert!(!deck.first_slide());
        assert!(!deck.last_slide());
        assert!(!deck.next());
        assert!(!deck.prev());
    }

    #[test]
    fn hidden_export_copy_preserves_original_and_relationships() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hidden.pptx");
        write_interactive_deck(&path);
        let original = std::fs::read(&path).unwrap();
        let (_copy_dir, copy) = PptxEngine::visible_export_copy(&path).unwrap().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let meta = PptxEngine::parse_pptx_metadata(&copy).unwrap();
        assert!(meta.hidden_slides.is_empty());
        assert_eq!(meta.slide_count, 3);
        assert_eq!(meta.slide_notes[&1], "Correct notes");
        assert!(PptxEngine::visible_export_copy(&copy).unwrap().is_none());
    }

    #[test]
    fn grouped_buttons_and_precise_text_link_regions() {
        let xml = format!(
            r#"<p:grpSp xmlns:p="{}" xmlns:a="{}">
            <p:grpSpPr><a:xfrm><a:off x="100" y="50"/><a:ext cx="400" cy="200"/>
            <a:chOff x="0" y="0"/><a:chExt cx="200" cy="100"/></a:xfrm></p:grpSpPr>
            <p:sp><p:spPr><a:xfrm><a:off x="10" y="5"/><a:ext cx="20" cy="10"/></a:xfrm></p:spPr></p:sp>
        </p:grpSp>"#,
            PptxEngine::P,
            PptxEngine::A
        );
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let shape = doc
            .descendants()
            .find(|n| n.has_tag_name((PptxEngine::P, "sp")))
            .unwrap();
        let rect = PptxEngine::shape_link_rect(shape, 1000, 500).unwrap();
        assert_eq!(rect, [0.12, 0.12, 0.04, 0.04]);
        let native = vec![
            PdfLinkInfo::new_url([0.0, 0.0, 1.0, 1.0], "https://one.example".into()),
            PdfLinkInfo::new_url([0.0, 0.0, 1.0, 1.0], "https://two.example".into()),
        ];
        let mut precise = vec![
            PdfLinkInfo::new_url([0.1, 0.1, 0.2, 0.1], "https://one.example".into()),
            PdfLinkInfo::new_url([0.1, 0.3, 0.2, 0.1], "https://two.example".into()),
        ];
        let expected = precise.clone();
        PptxEngine::merge_slide_links(&mut precise, &native);
        assert_eq!(precise, expected);
        let mut missing = Vec::new();
        PptxEngine::merge_slide_links(&mut missing, &native);
        assert!(missing.is_empty());
    }

    #[test]
    fn omitted_hidden_pages_are_rejected_instead_of_misaligning_links() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hidden.pptx");
        write_interactive_deck(&path);
        write_companion_pdf(&path, 2);
        let error = PptxEngine::load_deck_from_pptx(&path, RenderQuality::Eco)
            .err()
            .unwrap();
        assert!(error.contains("hidden slides included"), "{error}");
    }

    #[test]
    fn test_static_images_not_media() {
        assert!(!PptxEngine::is_playable_media_extension("image1.png"));
        assert!(!PptxEngine::is_playable_media_extension("photo.jpg"));
        assert!(!PptxEngine::is_playable_media_extension("diagram.jpeg"));
        assert!(!PptxEngine::is_playable_media_extension("vector.svg"));
        assert!(PptxEngine::is_playable_media_extension("video.mp4"));
        assert!(PptxEngine::is_playable_media_extension("audio.mp3"));
        assert!(PptxEngine::is_playable_media_extension("clip.mov"));
    }
}
