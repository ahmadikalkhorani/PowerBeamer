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
        let mut session_data = Vec::new();

        for (idx, slide) in deck.slides.iter().enumerate() {
            session_data.push(serde_json::json!({
                "slide_index": idx + 1,
                "title": slide.title,
                "is_whiteboard": slide.is_whiteboard,
                "notes": slide.notes,
                "strokes_count": slide.inking.strokes.len(),
                "strokes": slide.inking.strokes,
            }));
        }

        let json_str = serde_json::to_string_pretty(&session_data)
            .map_err(|e| format!("Serialization error: {}", e))?;

        std::fs::write(&session_file, json_str)
            .map_err(|e| format!("Failed to write session file: {}", e))?;

        // Render composite images (Slide + Inking overlay)
        let export_dir = destination_path.with_extension("annotated_pages");
        let _ = std::fs::create_dir_all(&export_dir);

        for (idx, slide) in deck.slides.iter().enumerate() {
            let mut composite = (*slide.audience_image).clone();
            let ink_img = InkingRasterizer::render_strokes(
                composite.width(),
                composite.height(),
                &slide.inking.strokes,
                None,
                None,
            );

            // Overlay ink onto slide
            image::imageops::overlay(&mut composite, &ink_img, 0, 0);

            let page_path = export_dir.join(format!("page_{:03}.png", idx + 1));
            let _ = composite.save(&page_path);
        }

        Ok(format!(
            "Saved session annotations to {} and {} high-res annotated pages in {}",
            session_file.file_name().unwrap_or_default().to_string_lossy(),
            deck.slides.len(),
            export_dir.file_name().unwrap_or_default().to_string_lossy()
        ))
    }
}
