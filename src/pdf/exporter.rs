use super::deck::PresentationDeck;
use crate::ink::InkingRasterizer;
use std::path::Path;

pub struct PdfExporter;

impl PdfExporter {
    /// Save annotated slides to a destination file or sidecar session
    pub fn export_annotated_presentation(
        deck: &PresentationDeck,
        destination_path: &Path,
    ) -> Result<String, String> {
        // Also save sidecar annotations JSON so state is completely non-destructive
        let session_file = destination_path.with_extension("presnotes.json");
        use serde::ser::{SerializeSeq, Serializer};
        let file = std::fs::File::create(&session_file)
            .map_err(|e| format!("Failed to write session file: {e}"))?;
        let mut writer = std::io::BufWriter::new(file);
        let mut serializer = serde_json::Serializer::pretty(&mut writer);
        let mut sequence = serializer
            .serialize_seq(Some(deck.slides.len()))
            .map_err(|e| e.to_string())?;
        for (idx, slide) in deck.slides.iter().enumerate() {
            #[derive(serde::Serialize)]
            struct SessionSlide<'a> {
                slide_index: usize,
                title: &'a str,
                is_whiteboard: bool,
                notes: &'a str,
                audio_progress_box: Option<[f32; 4]>,
                strokes_count: usize,
                strokes: &'a [crate::ink::Stroke],
            }
            sequence
                .serialize_element(&SessionSlide {
                    slide_index: idx + 1,
                    title: &slide.title,
                    is_whiteboard: slide.is_whiteboard,
                    notes: &slide.notes,
                    audio_progress_box: slide.audio_progress_box,
                    strokes_count: slide.inking.strokes.len(),
                    strokes: &slide.inking.strokes,
                })
                .map_err(|e| format!("Serialization error: {e}"))?;
        }
        sequence.end().map_err(|e| e.to_string())?;
        std::io::Write::flush(&mut writer)
            .map_err(|e| format!("Failed to write session file: {e}"))?;

        // Render composite images (Slide + Inking overlay)
        let export_dir = destination_path.with_extension("annotated_pages");
        std::fs::create_dir_all(&export_dir)
            .map_err(|e| format!("Failed to create export directory: {e}"))?;

        for (idx, slide) in deck.slides.iter().enumerate() {
            let page_path = export_dir.join(format!("page_{:03}.png", idx + 1));
            if slide.inking.strokes.is_empty() {
                slide
                    .audience_image
                    .save(&page_path)
                    .map_err(|e| format!("Failed to save page {}: {e}", idx + 1))?;
                continue;
            }
            let mut composite = (*slide.audience_image.pixels()).clone();
            let ink_img = InkingRasterizer::render_strokes(
                composite.width(),
                composite.height(),
                &slide.inking.strokes,
                None,
                None,
            );

            // Overlay ink onto slide
            image::imageops::overlay(&mut composite, &ink_img, 0, 0);

            composite
                .save(&page_path)
                .map_err(|e| format!("Failed to save page {}: {e}", idx + 1))?;
        }

        Ok(format!(
            "Saved session annotations to {} and {} high-res annotated pages in {}",
            session_file
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            deck.slides.len(),
            export_dir.file_name().unwrap_or_default().to_string_lossy()
        ))
    }
}
