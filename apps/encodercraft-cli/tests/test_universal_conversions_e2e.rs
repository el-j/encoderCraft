//! End-to-end blackbox test matrix for encoderCraft CLI universal conversions.
//!
//! Exercises the compiled CLI binary across:
//! - Document publishing (PDF -> JPEG, PNG sequence, DOCX, Markdown, HTML5)
//! - CAD & Technical drafting (DXF -> PDF, SVG)
//! - Universal audio transcoding (WAV -> MP3, FLAC, OGG, Opus)
//! - Layered image transcoding (PSD -> PhotoCraft, PSB)

use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_encodercraft-cli")
}

fn write_synthetic_pdf(path: &Path) {
    let mut doc = encodercraft_pipeline::document::DocumentCanvas::new("E2E Document Canvas");
    doc.pages[0].add_text(54.0, 720.0, 500.0, 24.0, "E2E Universal Conversion Page 1", 14.0);
    doc.add_page().add_text(54.0, 720.0, 500.0, 24.0, "E2E Universal Conversion Page 2", 14.0);
    std::fs::write(path, doc.to_pdf_x4_bytes().expect("PDF/X-4 generation should succeed")).expect("write PDF");
}

fn write_synthetic_dxf(path: &Path) {
    let mut drawing = encodercraft_spatial_io::CadDrawing::new("E2E Architectural Blueprint");
    drawing.add_line(encodercraft_spatial_io::CadPoint3D::new(0.0, 0.0, 0.0), encodercraft_spatial_io::CadPoint3D::new(100.0, 100.0, 0.0), "WALLS");
    drawing.add_circle(encodercraft_spatial_io::CadPoint3D::new(50.0, 50.0, 0.0), 25.0, "COLUMNS");
    std::fs::write(path, drawing.to_dxf_ascii()).expect("write DXF");
}

fn write_synthetic_wav(path: &Path) {
    let samples: Vec<u8> = (0..1600i32).flat_map(|i| ((((i as f32) * 0.2).sin() * 6000.0) as i16).to_le_bytes()).collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36u32 + samples.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&16_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    b.extend_from_slice(&samples);
    std::fs::write(path, b).expect("write wav file");
}

fn write_synthetic_psd(path: &Path) {
    let mut img = encodercraft_image_codecs::LayeredImage::new("Artwork", 64, 64);
    let layer = encodercraft_image_codecs::ImageLayer::new("Layer 1", 64, 64, vec![180; 64 * 64 * 4]);
    img.add_layer(layer);
    let psd_bytes = img.to_psd_bytes(false).expect("PSD export");
    std::fs::write(path, psd_bytes).expect("write PSD");
}

#[test]
fn test_cli_document_conversions_e2e() {
    let dir = tempdir().expect("tempdir");
    let pdf_source = dir.path().join("document.pdf");
    write_synthetic_pdf(&pdf_source);

    // 1. PDF -> JPEG still / sequence
    let jpg_out = dir.path().join("doc.jpg");
    let status = Command::new(cli_bin()).args(["encode", pdf_source.to_str().unwrap(), "-o", jpg_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "PDF -> JPEG transcode failed");
    let page1_jpg = dir.path().join("doc_0001.jpg");
    assert!(jpg_out.exists() || page1_jpg.exists(), "neither doc.jpg nor doc_0001.jpg was created");

    // 2. PDF -> DOCX
    let docx_out = dir.path().join("doc.docx");
    let status = Command::new(cli_bin()).args(["encode", pdf_source.to_str().unwrap(), "-o", docx_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "PDF -> DOCX transcode failed");
    assert!(docx_out.exists(), "doc.docx was not created");

    // 3. PDF -> Markdown
    let md_out = dir.path().join("doc.md");
    let status = Command::new(cli_bin()).args(["encode", pdf_source.to_str().unwrap(), "-o", md_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "PDF -> Markdown transcode failed");
    assert!(md_out.exists(), "doc.md was not created");

    // 4. PDF -> HTML5
    let html_out = dir.path().join("doc.html");
    let status = Command::new(cli_bin()).args(["encode", pdf_source.to_str().unwrap(), "-o", html_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "PDF -> HTML transcode failed");
    assert!(html_out.exists(), "doc.html was not created");
}

#[test]
fn test_cli_cad_conversions_e2e() {
    let dir = tempdir().expect("tempdir");
    let dxf_source = dir.path().join("blueprint.dxf");
    write_synthetic_dxf(&dxf_source);

    // 1. DXF -> PDF
    let pdf_out = dir.path().join("blueprint.pdf");
    let status = Command::new(cli_bin()).args(["encode", dxf_source.to_str().unwrap(), "-o", pdf_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "DXF -> PDF transcode failed");
    assert!(pdf_out.exists());

    // 2. DXF -> SVG
    let svg_out = dir.path().join("blueprint.svg");
    let status = Command::new(cli_bin()).args(["encode", dxf_source.to_str().unwrap(), "-o", svg_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "DXF -> SVG transcode failed");
    assert!(svg_out.exists());
}

#[test]
fn test_cli_audio_conversions_e2e() {
    let dir = tempdir().expect("tempdir");
    let wav_source = dir.path().join("audio.wav");
    write_synthetic_wav(&wav_source);

    // 1. WAV -> MP3
    let mp3_out = dir.path().join("audio.mp3");
    let status = Command::new(cli_bin()).args(["encode", wav_source.to_str().unwrap(), "-o", mp3_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "WAV -> MP3 transcode failed");
    assert!(mp3_out.exists());

    // 2. WAV -> FLAC
    let flac_out = dir.path().join("audio.flac");
    let status = Command::new(cli_bin()).args(["encode", wav_source.to_str().unwrap(), "-o", flac_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "WAV -> FLAC transcode failed");
    assert!(flac_out.exists());

    // 3. WAV -> OGG Vorbis
    let ogg_out = dir.path().join("audio.ogg");
    let status = Command::new(cli_bin()).args(["encode", wav_source.to_str().unwrap(), "-o", ogg_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "WAV -> OGG transcode failed");
    assert!(ogg_out.exists());
}

#[test]
fn test_cli_layered_graphics_e2e() {
    let dir = tempdir().expect("tempdir");
    let psd_source = dir.path().join("graphic.psd");
    write_synthetic_psd(&psd_source);

    // 1. PSD -> PhotoCraft
    let pc_out = dir.path().join("graphic.photocraft");
    let status = Command::new(cli_bin()).args(["encode", psd_source.to_str().unwrap(), "-o", pc_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "PSD -> PhotoCraft transcode failed");
    assert!(pc_out.exists());

    // 2. PSD -> PSB
    let psb_out = dir.path().join("graphic.psb");
    let status = Command::new(cli_bin()).args(["encode", psd_source.to_str().unwrap(), "-o", psb_out.to_str().unwrap()]).status().expect("run cli");
    assert!(status.success(), "PSD -> PSB transcode failed");
    assert!(psb_out.exists());
}
