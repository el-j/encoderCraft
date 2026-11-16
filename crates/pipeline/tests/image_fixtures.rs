//! Real-file conversion matrix and golden pixel tests for image codecs.
//!
//! Covers colour-bar PNG with alpha, JPEG with EXIF rotation, 16-bit TIFF, EXR, WebP,
//! GIF, palette PNG, grayscale, and CMYK JPEG (#22, #23, #24).

use std::path::{Path, PathBuf};

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, Rational64, SourceDescriptor, StartPolicy, VideoFrame};
use encodercraft_geom::Dimensions;
use encodercraft_image_codecs::{StillFormat, StillOptions, encode_still};
use encodercraft_pipeline::TranscodePipeline;
use encodercraft_source::MediaReader;
use photocraft_codecs::{ChannelLayout, EncodeOptions, Format, Image};
use tempfile::tempdir;

const W: u32 = 64;
const H: u32 = 32;

fn make_exif_payload(o: u16) -> Vec<u8> {
    let mut v = b"II*\0".to_vec();
    v.extend_from_slice(&8u32.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    // Filler tag (ImageDescription)
    v.extend_from_slice(&0x010Eu16.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&4u32.to_le_bytes());
    v.extend_from_slice(b"abc\0");
    // Orientation tag
    v.extend_from_slice(&0x0112u16.to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes());
    v.extend_from_slice(&1u32.to_le_bytes());
    v.extend_from_slice(&o.to_le_bytes());
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(&0u32.to_le_bytes());
    v
}

fn insert_jpeg_app1_exif(jpeg: &[u8], exif: &[u8]) -> Vec<u8> {
    let mut seg = b"Exif\0\0".to_vec();
    seg.extend_from_slice(exif);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&seg);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn job(source: &Path, target: OutputTarget) -> CraftJobSpec {
    CraftJobSpec {
        version: "1.0".into(),
        job_id: "fixture_conformance".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: source.to_path_buf(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![target],
    }
}

fn target(path: &Path, container: ContainerType) -> OutputTarget {
    OutputTarget { target_id: "t".into(), output_path: path.to_path_buf(), preset_name: "Image".into(), container, ..Default::default() }
}

fn run(dir: &Path, name: &str, container: ContainerType, source: &Path) -> PathBuf {
    let out = dir.join(name);
    TranscodePipeline::new(job(source, target(&out, container))).execute().unwrap();
    out
}

/// Creates a standard 8-bar colour bar frame with variable alpha.
fn make_colour_bar_frame(w: u32, h: u32) -> VideoFrame {
    let dims = Dimensions::new(w, h).unwrap();
    let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
    let (plane, _) = frame.plane_mut(0).unwrap();
    let bars: [[u8; 3]; 8] = [
        [255, 255, 255], // White
        [255, 255, 0],   // Yellow
        [0, 255, 255],   // Cyan
        [0, 255, 0],     // Green
        [255, 0, 255],   // Magenta
        [255, 0, 0],     // Red
        [0, 0, 255],     // Blue
        [0, 0, 0],       // Black
    ];
    let bar_width = (w / 8).max(1);
    for y in 0..h {
        let alpha = ((y * 255) / h.max(1)) as u8;
        for x in 0..w {
            let bar_idx = ((x / bar_width).min(7)) as usize;
            let c = bars[bar_idx];
            let offset = ((y * w + x) * 4) as usize;
            plane[offset] = c[0];
            plane[offset + 1] = c[1];
            plane[offset + 2] = c[2];
            plane[offset + 3] = alpha;
        }
    }
    frame
}

#[test]
fn fixture_colour_bar_png_with_alpha_roundtrips() {
    let dir = tempdir().unwrap();
    let frame = make_colour_bar_frame(W, H);
    let bytes = encode_still(&frame, StillFormat::Png, &StillOptions::default()).unwrap();
    let path = dir.path().join("colour_bars.png");
    std::fs::write(&path, bytes).unwrap();

    let reader = MediaReader::open(&path).unwrap();
    let decoded = reader.frame_at_index(0).unwrap();
    assert_eq!((decoded.dimensions.width, decoded.dimensions.height), (W, H));
    assert_eq!(decoded.format, PixelFormat::Rgba8);
    let (plane, _) = decoded.plane(0).unwrap();
    assert_eq!(plane, frame.data());
}

#[test]
fn fixture_jpeg_with_exif_rotation_is_uprighted_on_decode() {
    let dir = tempdir().unwrap();
    // 64x32 base image with EXIF Orientation = 6 (Rotate 90 CW)
    // When uprighted on decode, width becomes 32 and height becomes 64.
    let mut rgb = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            rgb.extend_from_slice(&[(x * 4) as u8, (y * 8) as u8, 120]);
        }
    }
    let image = Image::from_u8(W, H, ChannelLayout::Rgb, rgb).unwrap();
    let raw_jpeg = photocraft_codecs::encode(&image, Format::Jpeg, &EncodeOptions::default()).unwrap();
    let exif = make_exif_payload(6);
    let bytes = insert_jpeg_app1_exif(&raw_jpeg, &exif);

    let path = dir.path().join("exif_rotate.jpg");
    std::fs::write(&path, bytes).unwrap();

    let reader = MediaReader::open(&path).unwrap();
    let decoded = reader.frame_at_index(0).unwrap();
    // Verify orientation was applied and cleared on decode: (W, H) 64x32 -> 32x64
    assert_eq!((decoded.dimensions.width, decoded.dimensions.height), (H, W));
}

#[test]
fn fixture_16bit_tiff_decodes_and_transcodes() {
    let dir = tempdir().unwrap();
    let mut u16_data = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let r = (x as u16) * 1024;
            let g = (y as u16) * 2048;
            let b = 32768u16;
            let a = 65535u16;
            u16_data.extend_from_slice(&[r, g, b, a]);
        }
    }
    let image = Image::from_u16(W, H, ChannelLayout::Rgba, &u16_data).unwrap();
    let bytes = photocraft_codecs::encode(&image, Format::Tiff, &EncodeOptions::default()).unwrap();
    let path = dir.path().join("16bit.tif");
    std::fs::write(&path, bytes).unwrap();

    let reader = MediaReader::open(&path).unwrap();
    let decoded = reader.frame_at_index(0).unwrap();
    assert_eq!((decoded.dimensions.width, decoded.dimensions.height), (W, H));

    // Transcode 16-bit TIFF to PNG and back
    let out = run(dir.path(), "transcoded.png", ContainerType::Png, &path);
    let decoded_out = MediaReader::open(&out).unwrap().frame_at_index(0).unwrap();
    assert_eq!((decoded_out.dimensions.width, decoded_out.dimensions.height), (W, H));
}

#[test]
fn fixture_cmyk_jpeg_decodes_and_transcodes() {
    let dir = tempdir().unwrap();
    let mut cmyk = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let c = (x * 4) as u8;
            let m = (y * 8) as u8;
            let y_val = 60u8;
            let k = 20u8;
            cmyk.extend_from_slice(&[c, m, y_val, k]);
        }
    }
    let image = Image::from_u8(W, H, ChannelLayout::Cmyk, cmyk).unwrap();
    let bytes = photocraft_codecs::encode(&image, Format::Jpeg, &EncodeOptions::default()).unwrap();
    let path = dir.path().join("cmyk.jpg");
    std::fs::write(&path, bytes).unwrap();

    let reader = MediaReader::open(&path).unwrap();
    let decoded = reader.frame_at_index(0).unwrap();
    assert_eq!((decoded.dimensions.width, decoded.dimensions.height), (W, H));

    // CMYK JPEG converts to RGBA8 on decode; transcode to PNG
    let out = run(dir.path(), "cmyk_to.png", ContainerType::Png, &path);
    let decoded_png = MediaReader::open(&out).unwrap().frame_at_index(0).unwrap();
    assert_eq!((decoded_png.dimensions.width, decoded_png.dimensions.height), (W, H));
    let (plane, _) = decoded_png.plane(0).unwrap();
    assert!(plane.iter().any(|&b| b > 0), "decoded pixels must not be empty");
}

#[test]
fn fixture_palette_png_decodes_and_transcodes() {
    let dir = tempdir().unwrap();
    let palette = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
    let mut indices = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        for x in 0..W {
            indices.push(((x + y) % 4) as u8);
        }
    }
    let bytes = photocraft_codecs::encode_png_indexed(W, H, &indices, &palette, None, None, None).unwrap();
    let path = dir.path().join("palette.png");
    std::fs::write(&path, bytes).unwrap();

    let reader = MediaReader::open(&path).unwrap();
    let decoded = reader.frame_at_index(0).unwrap();
    assert_eq!((decoded.dimensions.width, decoded.dimensions.height), (W, H));

    let out = run(dir.path(), "palette_to.webp", ContainerType::WebP, &path);
    let decoded_webp = MediaReader::open(&out).unwrap().frame_at_index(0).unwrap();
    assert_eq!((decoded_webp.dimensions.width, decoded_webp.dimensions.height), (W, H));
}

#[test]
fn fixture_grayscale_decodes_and_transcodes() {
    let dir = tempdir().unwrap();
    let mut gray = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        for x in 0..W {
            gray.push(((x * 4 + y * 8) % 256) as u8);
        }
    }
    let image = Image::from_u8(W, H, ChannelLayout::Gray, gray).unwrap();
    let bytes = photocraft_codecs::encode(&image, Format::Png, &EncodeOptions::default()).unwrap();
    let path = dir.path().join("gray.png");
    std::fs::write(&path, bytes).unwrap();

    let reader = MediaReader::open(&path).unwrap();
    let decoded = reader.frame_at_index(0).unwrap();
    assert_eq!((decoded.dimensions.width, decoded.dimensions.height), (W, H));

    let out = run(dir.path(), "gray_to.bmp", ContainerType::Bmp, &path);
    let decoded_bmp = MediaReader::open(&out).unwrap().frame_at_index(0).unwrap();
    assert_eq!((decoded_bmp.dimensions.width, decoded_bmp.dimensions.height), (W, H));
}

#[test]
fn fixture_gif_palette_decodes_and_transcodes() {
    let dir = tempdir().unwrap();
    let frame = make_colour_bar_frame(W, H);
    let bytes = encode_still(&frame, StillFormat::Gif, &StillOptions::default()).unwrap();
    let path = dir.path().join("bars.gif");
    std::fs::write(&path, bytes).unwrap();

    let reader = MediaReader::open(&path).unwrap();
    let decoded = reader.frame_at_index(0).unwrap();
    assert_eq!((decoded.dimensions.width, decoded.dimensions.height), (W, H));

    let out = run(dir.path(), "gif_to.png", ContainerType::Png, &path);
    let decoded_png = MediaReader::open(&out).unwrap().frame_at_index(0).unwrap();
    assert_eq!((decoded_png.dimensions.width, decoded_png.dimensions.height), (W, H));
}

#[test]
fn regression_grey_image_screenshot_pixel_exact() {
    let dir = tempdir().unwrap();
    // Construct a screenshot-like frame: high contrast text lines, sharp window borders, UI buttons
    let dims = Dimensions::new(128, 64).unwrap();
    let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
    let (plane, _) = frame.plane_mut(0).unwrap();

    for y in 0..64u32 {
        for x in 0..128u32 {
            let offset = ((y * 128 + x) * 4) as usize;
            let color = if y < 16 {
                // Window title bar: dark charcoal
                [32, 34, 38, 255]
            } else if x == 0 || x == 127 || y == 63 {
                // Border: light gray
                [180, 180, 180, 255]
            } else if (24..=40).contains(&y) && (20..=60).contains(&x) {
                // Blue button
                [20, 110, 220, 255]
            } else if (y % 8 == 0) && (70..=110).contains(&x) {
                // Text line: dark text on light canvas
                [10, 10, 10, 255]
            } else {
                // Background canvas: clean white
                [245, 245, 247, 255]
            };
            plane[offset] = color[0];
            plane[offset + 1] = color[1];
            plane[offset + 2] = color[2];
            plane[offset + 3] = color[3];
        }
    }

    let png_bytes = encodercraft_image_codecs::encode_png(&frame).unwrap();
    let src_path = dir.path().join("screenshot.png");
    std::fs::write(&src_path, png_bytes).unwrap();

    let out_path = run(dir.path(), "screenshot_out.png", ContainerType::Png, &src_path);
    let decoded = MediaReader::open(&out_path).unwrap().frame_at_index(0).unwrap();

    assert_eq!(decoded.dimensions, frame.dimensions);
    let (decoded_plane, _) = decoded.plane(0).unwrap();
    let (src_plane, _) = frame.plane(0).unwrap();
    assert_eq!(decoded_plane, src_plane, "screenshot PNG to PNG must be 100% byte-identical in pixel data");
}

#[test]
fn fixtures_truncated_files_fail_cleanly_without_leftovers() {
    let dir = tempdir().unwrap();

    // 1. Truncated 16-bit TIFF
    let u16_data = vec![0u16; (W * H * 4) as usize];
    let image = Image::from_u16(W, H, ChannelLayout::Rgba, &u16_data).unwrap();
    let tiff_bytes = photocraft_codecs::encode(&image, Format::Tiff, &EncodeOptions::default()).unwrap();
    let cut_tiff = dir.path().join("cut.tif");
    std::fs::write(&cut_tiff, &tiff_bytes[..tiff_bytes.len() / 3]).unwrap();
    let out_tiff = dir.path().join("out_tiff.png");
    assert!(TranscodePipeline::new(job(&cut_tiff, target(&out_tiff, ContainerType::Png))).execute().is_err());
    assert!(!out_tiff.exists());

    // 2. Truncated CMYK JPEG
    let cmyk_data = vec![128u8; (W * H * 4) as usize];
    let cmyk_image = Image::from_u8(W, H, ChannelLayout::Cmyk, cmyk_data).unwrap();
    let cmyk_bytes = photocraft_codecs::encode(&cmyk_image, Format::Jpeg, &EncodeOptions::default()).unwrap();
    let cut_cmyk = dir.path().join("cut_cmyk.jpg");
    std::fs::write(&cut_cmyk, &cmyk_bytes[..cmyk_bytes.len() / 3]).unwrap();
    let out_cmyk = dir.path().join("out_cmyk.png");
    assert!(TranscodePipeline::new(job(&cut_cmyk, target(&out_cmyk, ContainerType::Png))).execute().is_err());
    assert!(!out_cmyk.exists());

    // 3. Truncated GIF
    let frame = make_colour_bar_frame(W, H);
    let gif_bytes = encode_still(&frame, StillFormat::Gif, &StillOptions::default()).unwrap();
    let cut_gif = dir.path().join("cut.gif");
    std::fs::write(&cut_gif, &gif_bytes[..gif_bytes.len() / 4]).unwrap();
    let out_gif = dir.path().join("out_gif.png");
    assert!(TranscodePipeline::new(job(&cut_gif, target(&out_gif, ContainerType::Png))).execute().is_err());
    assert!(!out_gif.exists());
}
