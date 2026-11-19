//! Comprehensive blackbox test suite for codec-to-codec conversion matrix.
//!
//! Validates end-to-end transcoding across every supported domain:
//! 1. Video -> Video (MP4 H.264 to MOV ProRes 422, WebM VP9)
//! 2. Audio -> Audio (WAV to MP3, FLAC, AIFF, Ogg Vorbis, CAF, Opus)
//! 3. PDF/Document -> Raster Image Sequences & Document Markdown (PDF to JPEG, PNG, TIFF, WebP, BMP, MD)
//! 4. Vector -> Raster & Vector (SVG to EPS, Vectorcraft to SVG)
//! 5. Still Image -> Still Image (PNG to JPEG, JPEG to WebP, WebP to TIFF, TIFF to BMP)
//! 6. 3D Spatial -> 3D Spatial (STL to GLB, STL to OBJ, STL to PLY)

use std::fs::File;
use std::path::{Path, PathBuf};

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{
    ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, Rational64, SourceDescriptor, SourceSelection, StartPolicy,
    VideoEncodeSettings, VideoFrame,
};
use encodercraft_geom::Dimensions;
use encodercraft_muxers::{Mp4Muxer, VideoSample, VideoStreamConfig};
use encodercraft_pipeline::TranscodePipeline;
use encodercraft_source::MediaReader;
use encodercraft_video_codecs::{VideoCodecKind, create_video_encoder};
use tempfile::tempdir;

const W: u32 = 128;
const H: u32 = 72;
const FRAMES: u64 = 8;

fn fps() -> Rational64 {
    Rational64 { num: 30_000, den: 1_001 }
}

fn build_job(source: PathBuf, targets: Vec<OutputTarget>) -> CraftJobSpec {
    CraftJobSpec {
        version: "1.0".into(),
        job_id: "blackbox-test-job".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: source, start_tick: None, duration_ticks: None, timebase: None },
        targets,
    }
}

fn write_synthetic_mp4(path: &Path) {
    let mut encoder = create_video_encoder(VideoCodecKind::H264, W, H, fps(), Some(2000)).unwrap();
    let mut mux = Mp4Muxer::new(File::create(path).unwrap(), false).unwrap();
    let stream_idx = mux
        .add_video_stream(&VideoStreamConfig {
            codec_fourcc: *b"avc1",
            dimensions: Dimensions::new(W, H).unwrap(),
            frame_rate: fps(),
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: encoder.config_record(),
            spatial: None,
        })
        .unwrap();

    let dims = Dimensions::new(W, H).unwrap();
    let mut packets = Vec::new();
    for i in 0..FRAMES {
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, fps()).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        for (n, px) in plane.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = (n as u32 % W) as u64;
            let y = (n as u32 / W) as u64;
            px.copy_from_slice(&[((x * 2 + i * 4) % 256) as u8, ((y * 3) % 256) as u8, 160, 255]);
        }
        frame.pts = i as i64 * 1_001;
        packets.extend(encoder.encode_frame(&frame).unwrap());
    }
    packets.extend(encoder.flush().unwrap());
    for p in packets {
        mux.write_video_sample(stream_idx, VideoSample { data: &p.data, pts: p.pts, dts: p.dts, duration_ticks: 1_001, is_keyframe: p.is_keyframe })
            .unwrap();
    }
    mux.finish(false).unwrap();
}

fn write_synthetic_wav(path: &Path) {
    let mut pcm_bytes = Vec::new();
    for i in 0..22050 {
        let sample = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 44100.0).sin() * 16384.0) as i16;
        pcm_bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let mut wav_data = Vec::new();
    wav_data.extend_from_slice(b"RIFF");
    wav_data.extend_from_slice(&((pcm_bytes.len() + 36) as u32).to_le_bytes());
    wav_data.extend_from_slice(b"WAVEfmt ");
    wav_data.extend_from_slice(&16u32.to_le_bytes());
    wav_data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav_data.extend_from_slice(&1u16.to_le_bytes()); // 1 channel
    wav_data.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
    wav_data.extend_from_slice(&(44100u32 * 2).to_le_bytes()); // byte rate
    wav_data.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav_data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav_data.extend_from_slice(b"data");
    wav_data.extend_from_slice(&(pcm_bytes.len() as u32).to_le_bytes());
    wav_data.extend_from_slice(&pcm_bytes);
    std::fs::write(path, wav_data).unwrap();
}

fn write_synthetic_pdf(path: &Path) {
    let mut doc = encodercraft_pipeline::document::DocumentCanvas::new("Blackbox PDF Test");
    doc.pages[0].add_text(54.0, 720.0, 500.0, 24.0, "Blackbox PDF First Page", 14.0);
    doc.add_page().add_text(54.0, 720.0, 500.0, 24.0, "Blackbox PDF Second Page", 14.0);
    std::fs::write(path, doc.to_pdf_x4_bytes().unwrap()).unwrap();
}

#[test]
fn test_blackbox_video_to_video_matrix() {
    let dir = tempdir().unwrap();
    let mp4_source = dir.path().join("source.mp4");
    write_synthetic_mp4(&mp4_source);

    // 1. MP4 -> ProRes MOV
    let mov_out = dir.path().join("output.mov");
    let mov_target = OutputTarget {
        target_id: "t_mov".into(),
        output_path: mov_out.clone(),
        preset_name: "Apple ProRes 422 HQ".into(),
        container: ContainerType::QuickTimeMov,
        export_video: true,
        export_audio: false,
        video: VideoEncodeSettings { codec: Some("prores".into()), pixel_format: Some(PixelFormat::Yuv422p10), ..Default::default() },
        ..Default::default()
    };
    TranscodePipeline::new(build_job(mp4_source.clone(), vec![mov_target])).execute().unwrap();
    assert!(mov_out.exists(), "MOV output must exist");
    assert!(std::fs::metadata(&mov_out).unwrap().len() > 100, "MOV output must have content");
    let mov_reader = MediaReader::open(&mov_out).unwrap();
    assert!(mov_reader.frame_count() > 0, "Decoded MOV must have video frames");

    // 2. MP4 -> WebM VP9
    let webm_out = dir.path().join("output.webm");
    let webm_target = OutputTarget {
        target_id: "t_webm".into(),
        output_path: webm_out.clone(),
        preset_name: "YouTube 1080p Full HD".into(),
        container: ContainerType::WebM,
        export_video: true,
        export_audio: false,
        target_bitrate_kbps: Some(1500),
        video: VideoEncodeSettings { codec: Some("vp9".into()), pixel_format: Some(PixelFormat::Yuv420p), ..Default::default() },
        ..Default::default()
    };
    TranscodePipeline::new(build_job(mp4_source, vec![webm_target])).execute().unwrap();
    assert!(webm_out.exists(), "WebM output must exist");
    assert!(std::fs::metadata(&webm_out).unwrap().len() > 100, "WebM output must have content");
}

#[test]
fn test_blackbox_audio_to_audio_matrix() {
    let dir = tempdir().unwrap();
    let wav_source = dir.path().join("source.wav");
    write_synthetic_wav(&wav_source);

    // 1. WAV -> MP3
    let mp3_out = dir.path().join("output.mp3");
    let mp3_target = OutputTarget {
        target_id: "t_mp3".into(),
        output_path: mp3_out.clone(),
        preset_name: "Podcast MP3".into(),
        container: ContainerType::Mp3,
        export_video: false,
        export_audio: true,
        audio: Some(encodercraft_core::AudioEncodeSettings {
            codec: Some("mp3".into()),
            sample_rate: Some(44100),
            layout: Some(encodercraft_core::ChannelLayout::Mono),
            bitrate_kbps: Some(192),
            bit_depth: None,
            delay_ms: 0,
        }),
        ..Default::default()
    };
    TranscodePipeline::new(build_job(wav_source.clone(), vec![mp3_target])).execute().unwrap();
    assert!(mp3_out.exists(), "MP3 output must exist");
    assert!(std::fs::metadata(&mp3_out).unwrap().len() > 50, "MP3 output must have content");

    // 2. WAV -> FLAC
    let flac_out = dir.path().join("output.flac");
    let flac_target = OutputTarget {
        target_id: "t_flac".into(),
        output_path: flac_out.clone(),
        preset_name: "Archival Audio FLAC".into(),
        container: ContainerType::Flac,
        export_video: false,
        export_audio: true,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(wav_source.clone(), vec![flac_target])).execute().unwrap();
    assert!(flac_out.exists(), "FLAC output must exist");
    assert!(std::fs::metadata(&flac_out).unwrap().len() > 50, "FLAC output must have content");

    // 3. WAV -> AIFF
    let aiff_out = dir.path().join("output.aif");
    let aiff_target = OutputTarget {
        target_id: "t_aiff".into(),
        output_path: aiff_out.clone(),
        preset_name: "Lossless Audio Master".into(),
        container: ContainerType::Aiff,
        export_video: false,
        export_audio: true,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(wav_source.clone(), vec![aiff_target])).execute().unwrap();
    assert!(aiff_out.exists(), "AIFF output must exist");
    assert!(std::fs::metadata(&aiff_out).unwrap().len() > 50, "AIFF output must have content");

    // 4. WAV -> Ogg Vorbis
    let ogg_out = dir.path().join("output.ogg");
    let ogg_target = OutputTarget {
        target_id: "t_ogg".into(),
        output_path: ogg_out.clone(),
        preset_name: "Ogg Vorbis".into(),
        container: ContainerType::OggAudio,
        export_video: false,
        export_audio: true,
        audio: Some(encodercraft_core::AudioEncodeSettings {
            codec: Some("vorbis".into()),
            sample_rate: Some(44100),
            layout: Some(encodercraft_core::ChannelLayout::Mono),
            bitrate_kbps: Some(160),
            bit_depth: None,
            delay_ms: 0,
        }),
        ..Default::default()
    };
    TranscodePipeline::new(build_job(wav_source.clone(), vec![ogg_target])).execute().unwrap();
    assert!(ogg_out.exists(), "Ogg Vorbis output must exist");
    assert!(std::fs::metadata(&ogg_out).unwrap().len() > 50, "Ogg output must have content");

    // 5. WAV -> CAF
    let caf_out = dir.path().join("output.caf");
    let caf_target = OutputTarget {
        target_id: "t_caf".into(),
        output_path: caf_out.clone(),
        preset_name: "Apple Core Audio CAF".into(),
        container: ContainerType::Caf,
        export_video: false,
        export_audio: true,
        audio: Some(encodercraft_core::AudioEncodeSettings {
            codec: Some("lpcm".into()),
            sample_rate: Some(44100),
            layout: Some(encodercraft_core::ChannelLayout::Mono),
            bitrate_kbps: None,
            bit_depth: None,
            delay_ms: 0,
        }),
        ..Default::default()
    };
    TranscodePipeline::new(build_job(wav_source.clone(), vec![caf_target])).execute().unwrap();
    assert!(caf_out.exists(), "CAF output must exist");
    let caf_bytes = std::fs::read(&caf_out).unwrap();
    assert!(caf_bytes.starts_with(b"caff"), "CAF output must start with caff header");

    // 6. WAV -> Opus
    let opus_out = dir.path().join("output.opus");
    let opus_target = OutputTarget {
        target_id: "t_opus".into(),
        output_path: opus_out.clone(),
        preset_name: "Voice & Streaming Opus".into(),
        container: ContainerType::Opus,
        export_video: false,
        export_audio: true,
        audio: Some(encodercraft_core::AudioEncodeSettings {
            codec: Some("opus".into()),
            sample_rate: Some(48000),
            layout: Some(encodercraft_core::ChannelLayout::Mono),
            bitrate_kbps: Some(128),
            bit_depth: None,
            delay_ms: 0,
        }),
        ..Default::default()
    };
    TranscodePipeline::new(build_job(wav_source, vec![opus_target])).execute().unwrap();
    assert!(opus_out.exists(), "Opus output must exist");
    assert!(std::fs::metadata(&opus_out).unwrap().len() > 50, "Opus output must have content");
}

#[test]
fn test_blackbox_pdf_document_to_images_and_markdown_matrix() {
    let dir = tempdir().unwrap();
    let pdf_source = dir.path().join("source.pdf");
    write_synthetic_pdf(&pdf_source);

    // 1. PDF -> JPEG (single page selection)
    let jpg_out = dir.path().join("page.jpg");
    let jpg_target = OutputTarget {
        target_id: "t_jpg".into(),
        output_path: jpg_out.clone(),
        preset_name: "Still Image JPEG".into(),
        container: ContainerType::Jpeg,
        selection: SourceSelection { pages: Some((1, 1)), ..Default::default() },
        ..Default::default()
    };
    TranscodePipeline::new(build_job(pdf_source.clone(), vec![jpg_target])).execute().unwrap();
    assert!(jpg_out.exists(), "JPEG output must exist");
    let jpg_bytes = std::fs::read(&jpg_out).unwrap();
    assert!(jpg_bytes.starts_with(&[0xFF, 0xD8, 0xFF]), "JPEG file must have valid SOI marker");

    // 2. PDF -> PNG (full sequence: 2 pages)
    let png_out = dir.path().join("rendered.png");
    let png_target = OutputTarget {
        target_id: "t_png".into(),
        output_path: png_out.clone(),
        preset_name: "Still Image PNG (Lossless)".into(),
        container: ContainerType::Png,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(pdf_source.clone(), vec![png_target])).execute().unwrap();
    let p1 = dir.path().join("rendered_0001.png");
    let p2 = dir.path().join("rendered_0002.png");
    assert!(p1.exists(), "First rendered page PNG must exist");
    assert!(p2.exists(), "Second rendered page PNG must exist");
    assert!(std::fs::read(&p1).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));

    // 3. PDF -> TIFF (single page)
    let tif_out = dir.path().join("page.tif");
    let tif_target = OutputTarget {
        target_id: "t_tif".into(),
        output_path: tif_out.clone(),
        preset_name: "Broadcast TIFF (16-bit RGB)".into(),
        container: ContainerType::Tiff,
        selection: SourceSelection { pages: Some((1, 1)), ..Default::default() },
        ..Default::default()
    };
    TranscodePipeline::new(build_job(pdf_source.clone(), vec![tif_target])).execute().unwrap();
    assert!(tif_out.exists(), "TIFF output must exist");
    let tif_bytes = std::fs::read(&tif_out).unwrap();
    assert!(tif_bytes.starts_with(b"II*\0") || tif_bytes.starts_with(b"MM\0*"));

    // 4. PDF -> WebP (single page)
    let webp_out = dir.path().join("page.webp");
    let webp_target = OutputTarget {
        target_id: "t_webp".into(),
        output_path: webp_out.clone(),
        preset_name: "Still Image WebP (Lossless)".into(),
        container: ContainerType::WebP,
        selection: SourceSelection { pages: Some((1, 1)), ..Default::default() },
        ..Default::default()
    };
    TranscodePipeline::new(build_job(pdf_source.clone(), vec![webp_target])).execute().unwrap();
    assert!(webp_out.exists(), "WebP output must exist");
    let webp_bytes = std::fs::read(&webp_out).unwrap();
    assert!(webp_bytes.starts_with(b"RIFF"));

    // 5. PDF -> BMP (single page)
    let bmp_out = dir.path().join("page.bmp");
    let bmp_target = OutputTarget {
        target_id: "t_bmp".into(),
        output_path: bmp_out.clone(),
        preset_name: "Windows Bitmap BMP (Uncompressed)".into(),
        container: ContainerType::Bmp,
        selection: SourceSelection { pages: Some((1, 1)), ..Default::default() },
        ..Default::default()
    };
    TranscodePipeline::new(build_job(pdf_source.clone(), vec![bmp_target])).execute().unwrap();
    assert!(bmp_out.exists(), "BMP output must exist");
    let bmp_bytes = std::fs::read(&bmp_out).unwrap();
    assert!(bmp_bytes.starts_with(b"BM"));

    // 6. PDF -> Markdown
    let md_out = dir.path().join("extracted.md");
    let md_target = OutputTarget {
        target_id: "t_md".into(),
        output_path: md_out.clone(),
        preset_name: "Markdown".into(),
        container: ContainerType::Markdown,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(pdf_source, vec![md_target])).execute().unwrap();
    assert!(md_out.exists(), "Markdown output must exist");
    let md_text = std::fs::read_to_string(&md_out).unwrap();
    assert!(md_text.contains("Blackbox PDF First Page"), "Markdown must contain extracted PDF text");
}

#[test]
fn test_blackbox_vector_matrix() {
    let dir = tempdir().unwrap();
    let svg_source = dir.path().join("logo.svg");
    let svg_content = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300">
        <rect x="10" y="10" width="380" height="280" fill="#204080"/>
        <circle cx="200" cy="150" r="80" fill="#ffaa00"/>
    </svg>"##;
    std::fs::write(&svg_source, svg_content).unwrap();

    // 1. SVG -> PostScript EPS
    let eps_out = dir.path().join("logo.eps");
    let eps_target = OutputTarget {
        target_id: "t_eps".into(),
        output_path: eps_out.clone(),
        preset_name: "PostScript EPS".into(),
        container: ContainerType::PostScriptEps,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(svg_source.clone(), vec![eps_target])).execute().unwrap();
    assert!(eps_out.exists(), "EPS output must exist");
    let eps_text = std::fs::read_to_string(&eps_out).unwrap();
    assert!(eps_text.starts_with("%!PS-Adobe-3.0 EPSF-3.0"), "EPS must start with standard header");

    // 2. SVG -> VectorCraft binary
    let vc_out = dir.path().join("logo.vectorcraft");
    let vc_target = OutputTarget {
        target_id: "t_vc".into(),
        output_path: vc_out.clone(),
        preset_name: "ArtCraft VectorCraft".into(),
        container: ContainerType::ArtcraftVectorcraft,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(svg_source, vec![vc_target])).execute().unwrap();
    assert!(vc_out.exists(), "Vectorcraft binary output must exist");
    let vc_bytes = std::fs::read(&vc_out).unwrap();
    assert!(vc_bytes.starts_with(b"\x89CRAFT\x00\x01V"), "Vectorcraft file must start with magic header");
}

#[test]
fn test_blackbox_still_image_matrix() {
    let dir = tempdir().unwrap();
    let png_source = dir.path().join("sample.png");

    // Write a genuine 64x64 PNG
    let dims = Dimensions::new(64, 64).unwrap();
    let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64 { num: 1, den: 1 }).unwrap();
    let png_bytes = encodercraft_image_codecs::encode_still(
        &frame,
        encodercraft_image_codecs::StillFormat::Png,
        &encodercraft_image_codecs::StillOptions::default(),
    )
    .unwrap();
    std::fs::write(&png_source, png_bytes).unwrap();

    // 1. PNG -> JPEG
    let jpg_out = dir.path().join("out.jpg");
    let jpg_target = OutputTarget {
        target_id: "t_jpg".into(),
        output_path: jpg_out.clone(),
        preset_name: "Still Image JPEG".into(),
        container: ContainerType::Jpeg,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(png_source.clone(), vec![jpg_target])).execute().unwrap();
    assert!(jpg_out.exists(), "JPEG output must exist");
    assert!(std::fs::read(&jpg_out).unwrap().starts_with(&[0xFF, 0xD8, 0xFF]));

    // 2. PNG -> WebP
    let webp_out = dir.path().join("out.webp");
    let webp_target = OutputTarget {
        target_id: "t_webp".into(),
        output_path: webp_out.clone(),
        preset_name: "Still Image WebP (Lossless)".into(),
        container: ContainerType::WebP,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(png_source.clone(), vec![webp_target])).execute().unwrap();
    assert!(webp_out.exists(), "WebP output must exist");
    assert!(std::fs::read(&webp_out).unwrap().starts_with(b"RIFF"));

    // 3. PNG -> TIFF
    let tif_out = dir.path().join("out.tif");
    let tif_target = OutputTarget {
        target_id: "t_tif".into(),
        output_path: tif_out.clone(),
        preset_name: "Broadcast TIFF (16-bit RGB)".into(),
        container: ContainerType::Tiff,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(png_source.clone(), vec![tif_target])).execute().unwrap();
    assert!(tif_out.exists(), "TIFF output must exist");
    let tif_bytes = std::fs::read(&tif_out).unwrap();
    assert!(tif_bytes.starts_with(b"II*\0") || tif_bytes.starts_with(b"MM\0*"));

    // 4. PNG -> BMP
    let bmp_out = dir.path().join("out.bmp");
    let bmp_target = OutputTarget {
        target_id: "t_bmp".into(),
        output_path: bmp_out.clone(),
        preset_name: "Windows Bitmap BMP (Uncompressed)".into(),
        container: ContainerType::Bmp,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(png_source, vec![bmp_target])).execute().unwrap();
    assert!(bmp_out.exists(), "BMP output must exist");
    assert!(std::fs::read(&bmp_out).unwrap().starts_with(b"BM"));
}

#[test]
fn test_blackbox_3d_spatial_matrix() {
    let dir = tempdir().unwrap();
    let stl_source = dir.path().join("cube.stl");

    // Write a valid ASCII STL tetrahedron
    let mut mesh = encodercraft_spatial_io::TriangleMesh::new("Tetrahedron");
    mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, 0.866, 0.0], [0.5, 0.288, 0.816]];
    mesh.indices = vec![[0, 1, 2], [0, 1, 3], [1, 2, 3], [2, 0, 3]];
    std::fs::write(&stl_source, encodercraft_spatial_io::write_stl_ascii(&mesh)).unwrap();

    // 1. STL -> GLTF Binary (GLB)
    let glb_out = dir.path().join("cube.glb");
    let glb_target = OutputTarget {
        target_id: "t_glb".into(),
        output_path: glb_out.clone(),
        preset_name: "glTF 2.0 Binary (GLB)".into(),
        container: ContainerType::Gltf,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(stl_source.clone(), vec![glb_target])).execute().unwrap();
    assert!(glb_out.exists(), "GLB output must exist");
    let glb_bytes = std::fs::read(&glb_out).unwrap();
    assert!(glb_bytes.starts_with(b"glTF"), "GLB output must have glTF magic header");

    // 2. STL -> Wavefront OBJ
    let obj_out = dir.path().join("cube.obj");
    let obj_target = OutputTarget {
        target_id: "t_obj".into(),
        output_path: obj_out.clone(),
        preset_name: "Wavefront OBJ 3D Mesh".into(),
        container: ContainerType::Obj,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(stl_source.clone(), vec![obj_target])).execute().unwrap();
    assert!(obj_out.exists(), "OBJ output must exist");
    let obj_text = std::fs::read_to_string(&obj_out).unwrap();
    assert!(obj_text.contains("v ") && obj_text.contains("f "), "OBJ output must contain vertices and faces");

    // 3. STL -> Stanford PLY
    let ply_out = dir.path().join("cube.ply");
    let ply_target = OutputTarget {
        target_id: "t_ply".into(),
        output_path: ply_out.clone(),
        preset_name: "Stanford Polygon PLY 3D".into(),
        container: ContainerType::Ply,
        ..Default::default()
    };
    TranscodePipeline::new(build_job(stl_source, vec![ply_target])).execute().unwrap();
    assert!(ply_out.exists(), "PLY output must exist");
    let ply_bytes = std::fs::read(&ply_out).unwrap();
    assert!(ply_bytes.starts_with(b"ply\n"), "PLY output must start with ply header");
}
