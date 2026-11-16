//! Universal visual decoding for documents, CAD blueprints, and vector graphics into MediaReader frames.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use crate::document::{DocumentCanvas, pdf_page_count, render_pdf_page_to_rgba};
use encodercraft_core::EncoderError;
use encodercraft_source::MediaReader;
use std::path::Path;
use std::sync::Arc;

/// Opens a document, technical CAD drawing, or vector graphic as a [`MediaReader`].
pub fn open_universal_media_reader(path: &Path) -> Result<MediaReader, EncoderError> {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "document".to_string());
    let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("{}: {e}", path.display())))?;

    match ext.as_str() {
        "pdf" => {
            let total_pages = pdf_page_count(&bytes).unwrap_or(1);
            let mut frames = Vec::new();
            for page in 0..total_pages {
                if let Ok((w, h, rgba)) = render_pdf_page_to_rgba(&bytes, page, 1920.0) {
                    frames.push((w, h, rgba));
                }
            }
            if frames.is_empty() {
                return Err(EncoderError::InvalidContainer(format!("failed to rasterize any pages from PDF `{}`", path.display())));
            }
            MediaReader::from_frames(frames, name, "PDF".into())
        }
        "docx" | "epub" | "pptx" | "md" | "markdown" | "html" | "htm" | "txt" | "idml" => {
            let text_res = std::fs::read_to_string(path);
            let canvas = match ext.as_str() {
                "docx" => DocumentCanvas::from_docx_bytes(&bytes)?,
                "epub" => DocumentCanvas::from_epub_bytes(&bytes)?,
                "pptx" => DocumentCanvas::from_pptx_bytes(&bytes)?,
                "idml" => DocumentCanvas::from_idml_bytes(&bytes)?,
                "html" | "htm" => DocumentCanvas::from_html(&name, text_res.as_deref().unwrap_or(""))?,
                _ => DocumentCanvas::from_text(&name, text_res.as_deref().unwrap_or(""), ext.starts_with('m'))?,
            };
            let pdf_bytes = canvas.to_pdf_x4_bytes()?;
            let mut frames = Vec::new();
            for page in 0..canvas.pages.len() {
                if let Ok((w, h, rgba)) = render_pdf_page_to_rgba(&pdf_bytes, page, 1920.0) {
                    frames.push((w, h, rgba));
                }
            }
            if frames.is_empty() {
                return Err(EncoderError::InvalidContainer(format!("failed to render document `{}`", path.display())));
            }
            MediaReader::from_frames(frames, name, ext.to_ascii_uppercase())
        }
        "psd" | "psb" | "photocraft" => {
            let layered = encodercraft_image_codecs::LayeredImage::from_bytes_auto(&bytes)?;
            let w = layered.width;
            let h = layered.height;
            let rgba = layered.composite_rgba;
            MediaReader::from_frames(vec![(w, h, rgba)], name, ext.to_ascii_uppercase())
        }
        "dxf" | "dwg" | "cadcraft" => {
            let drawing = encodercraft_spatial_io::CadDrawing::from_bytes_auto(&bytes)?;
            let pdf_bytes = drawing.to_pdf_bytes()?;
            let (w, h, rgba) = render_pdf_page_to_rgba(&pdf_bytes, 0, 1920.0)?;
            MediaReader::from_frames(vec![(w, h, rgba)], name, ext.to_ascii_uppercase())
        }
        "svg" | "eps" | "wmf" | "ai" | "vectorcraft" => {
            let text_res = std::fs::read_to_string(path);
            let canvas = match ext.as_str() {
                "eps" => encodercraft_image_codecs::vector::VectorCanvas::from_eps(text_res.as_deref().unwrap_or("")),
                "wmf" => encodercraft_image_codecs::vector::VectorCanvas::from_wmf_bytes(&bytes),
                "ai" => encodercraft_image_codecs::vector::VectorCanvas::from_ai_or_pdf(&bytes),
                "vectorcraft" => encodercraft_image_codecs::vector::VectorCanvas::from_vectorcraft(&bytes),
                _ => encodercraft_image_codecs::vector::VectorCanvas::from_svg(text_res.as_deref().unwrap_or("")),
            }
            .map_err(|e| EncoderError::InvalidContainer(format!("failed to parse vector graphic `{}`: {e}", path.display())))?;

            let pdf_bytes = canvas
                .to_ai()
                .map_err(|e| EncoderError::Internal(format!("failed to convert vector canvas to PDF for `{}`: {e}", path.display())))?;
            let (w, h, rgba) = render_pdf_page_to_rgba(&pdf_bytes, 0, 1920.0)?;
            MediaReader::from_frames(vec![(w, h, rgba)], name, ext.to_ascii_uppercase())
        }
        _ => Err(EncoderError::UnsupportedCodec(format!("not a universal document or technical vector format: `{ext}`"))),
    }
}

/// Registers the universal document, CAD, and vector openers with `encodercraft-source`.
pub fn init_universal_media_openers() {
    encodercraft_source::register_media_opener(Arc::new(open_universal_media_reader));
}
