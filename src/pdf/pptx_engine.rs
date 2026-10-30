use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

use super::beamer::MediaClip;
use super::deck::{RenderQuality, Slide, TocEntry};
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
    /// Slide width and height in EMUs (1 cm = 360,000 EMU)
    pub slide_size_emu: Option<(u64, u64)>,
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

        // 4. Augment slides with PPTX notes, titles, and media
        for slide in &mut slides {
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

        // 5. Enhance TOC with sections or titles if PDF outline was empty
        if toc.is_empty() {
            if !metadata.sections.is_empty() {
                toc = metadata.sections;
            } else if !metadata.slide_titles.is_empty() {
                for slide in &slides {
                    let title = if slide.title.is_empty() || slide.title.starts_with("Slide ") {
                        format!("Slide {}", slide.page_index)
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

        Ok((slides, toc))
    }

    /// Converts the presentation to PDF if necessary, or locates a pre-existing companion or cached PDF.
    pub fn ensure_pdf_available(pptx_path: &Path) -> Result<PathBuf, String> {
        let cache_dir = Self::get_pptx_cache_dir(pptx_path)?;
        let cached_pdf = cache_dir.join("presentation.pdf");
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

        // Attempt 1: Native Microsoft PowerPoint (macOS AppleScript / Windows COM)
        #[cfg(target_os = "macos")]
        {
            if Self::has_powerpoint_macos() {
                println!("[PPTX Engine] Converting with Microsoft PowerPoint on macOS...");
                match Self::convert_with_powerpoint_macos(pptx_path, &cached_pdf) {
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
                if let Ok(path) = Self::convert_with_powerpoint_windows(pptx_path, &cached_pdf) {
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
            if let Ok(path) =
                Self::convert_with_libreoffice(&soffice_bin, pptx_path, &cache_dir, &cached_pdf)
            {
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

        // 1. Read presentation.xml for slide dimensions & sections
        if let Ok(mut pres_file) = archive.by_name("ppt/presentation.xml") {
            let mut pres_xml = String::new();
            if pres_file.read_to_string(&mut pres_xml).is_ok() {
                meta.slide_size_emu = Self::extract_slide_size(&pres_xml);
                meta.sections = Self::extract_sections(&pres_xml);
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
        let mut slide_idx = 1;
        loop {
            let slide_xml_opt = {
                if let Ok(mut slide_file) =
                    archive.by_name(&format!("ppt/slides/slide{}.xml", slide_idx))
                {
                    let mut s = String::new();
                    let _ = slide_file.read_to_string(&mut s);
                    Some(s)
                } else {
                    None
                }
            };

            let Some(slide_xml) = slide_xml_opt else {
                break;
            };

            if let Some(title) = Self::extract_slide_title(&slide_xml) {
                meta.slide_titles.insert(slide_idx, title);
            }

            // Check slide relationships for media links
            let rels_xml_opt = {
                if let Ok(mut rels_file) =
                    archive.by_name(&format!("ppt/slides/_rels/slide{}.xml.rels", slide_idx))
                {
                    let mut s = String::new();
                    let _ = rels_file.read_to_string(&mut s);
                    Some(s)
                } else {
                    None
                }
            };

            if let Some(rels_xml) = rels_xml_opt {
                let r_map = Self::extract_relationship_map(&rels_xml);
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

            slide_idx += 1;
        }

        // 4. Parse notes slides (`ppt/notesSlides/notesSlide{N}.xml`)
        let mut note_idx = 1;
        while let Ok(mut note_file) =
            archive.by_name(&format!("ppt/notesSlides/notesSlide{}.xml", note_idx))
        {
            let mut note_xml = String::new();
            if note_file.read_to_string(&mut note_xml).is_ok() {
                let note_text = Self::extract_notes_text(&note_xml);
                if !note_text.is_empty() {
                    // Notes slides typically map 1:1 to slide indices
                    meta.slide_notes.insert(note_idx, note_text);
                }
            }
            note_idx += 1;
        }

        Ok(meta)
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

    /// Extract sections if present in `ppt/presentation.xml`
    fn extract_sections(xml: &str) -> Vec<TocEntry> {
        let mut entries = Vec::new();
        // e.g. <p:section name="Introduction" id="...">
        for part in xml.split("<p:section ") {
            if let Some(name) = Self::extract_xml_attribute(part, "name") {
                entries.push(TocEntry {
                    title: name,
                    page_index: 1, // Will align with first slide of section if mapped
                    level: 0,
                    frame_index: entries.len() + 1,
                });
            }
        }
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

    /// Extract relationship ID map from `ppt/slides/_rels/slide{N}.xml.rels`
    fn extract_relationship_map(rels_xml: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for part in rels_xml.split("<Relationship ") {
            if let (Some(id), Some(target)) = (
                Self::extract_xml_attribute(part, "Id"),
                Self::extract_xml_attribute(part, "Target"),
            ) {
                map.insert(id, target);
            }
        }
        map
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
