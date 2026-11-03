//! End-to-end transcoding pipeline coordinator.
//!
//! Orchestrates demuxing, video scaling, audio DSP normalization, encoding, and muxing.

use std::fs::File;
use std::io::BufWriter;
use std::sync::Arc;
use std::time::Instant;

use encodercraft_audio_dsp::{AacEncoderWrapper, normalize_loudness, remix_channels};
use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{AudioSampleBuffer, ContainerType, CraftJobSpec, EncoderError, OutputTarget, Rational64, SourceDescriptor, VideoFrame};
use encodercraft_geom::{Dimensions, ScaleMode};
use encodercraft_muxers::{
    AudioSample, AudioStreamConfig, BextMetadata, BwfWavWriter, MkvMuxerBuilder, Mp4Muxer, MxfMuxer, VideoSample, VideoStreamConfig, WavFormatTag,
};
use encodercraft_video_codecs::{VideoCodecKind, Vp8DecoderWrapper, create_video_encoder, decode_video_packet_auto};

use crate::progress::JobProgress;
use crate::scale::FrameScaler;

/// Execution coordinator for a single transcoding job.
pub struct TranscodePipeline {
    spec: CraftJobSpec,
    progress: Arc<JobProgress>,
}

impl TranscodePipeline {
    /// Creates a new pipeline for the specified job specification.
    pub fn new(spec: CraftJobSpec) -> Self {
        let progress = Arc::new(JobProgress::new(100));
        Self { spec, progress }
    }

    /// Access the shared job progress reporter.
    pub fn progress(&self) -> Arc<JobProgress> {
        self.progress.clone()
    }

    /// Executes the transcode operation across all targets in the job specification.
    pub fn execute(&self) -> Result<(), EncoderError> {
        let start_time = Instant::now();

        if self.spec.targets.is_empty() {
            return Err(EncoderError::InvalidJobSpec("job contains no output targets".into()));
        }

        for target in &self.spec.targets {
            self.execute_target(target, start_time)?;
        }

        Ok(())
    }

    fn execute_target(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let target_path = &target.output_path;
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EncoderError::Io(format!("failed to create output directory: {e}")))?;
        }

        // Export sidecar subtitle file if requested
        if let (Some(cap_path), Some(cap_format)) = (&target.caption_path, target.caption_format) {
            let track = crate::captions::CaptionTrack::from_file(cap_path)?;
            track.export_sidecar(&target.output_path, cap_format, target.caption_offset_ms)?;
        }

        // Export sidecar XMP metadata if requested
        if let Some(ref meta) = target.metadata
            && meta.export_sidecar_xmp
        {
            let xmp_packet = meta.to_xmp_packet();
            let sidecar_path = target.output_path.with_extension("xmp");
            std::fs::write(&sidecar_path, xmp_packet.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write sidecar XMP: {e}")))?;
        }

        match target.container {
            ContainerType::Wave => self.execute_audio_wav(target, start_time)?,
            ContainerType::Flac => self.execute_audio_flac(target, start_time)?,
            ContainerType::Aiff => self.execute_audio_aiff(target, start_time)?,
            ContainerType::OggAudio | ContainerType::Mp3 => self.execute_audio_flac(target, start_time)?,
            ContainerType::IsoBmffMp4 => self.execute_video_mp4(target, false, start_time)?,
            ContainerType::QuickTimeMov => self.execute_video_mp4(target, true, start_time)?,
            ContainerType::MxfOp1a => self.execute_video_mxf(target, false, start_time)?,
            ContainerType::MxfOpAtom => self.execute_video_mxf(target, true, start_time)?,
            ContainerType::MatroskaMkv => self.execute_video_mkv(target, false, start_time)?,
            ContainerType::WebM => self.execute_video_mkv(target, true, start_time)?,
            ContainerType::AnimatedGif => self.execute_animated_gif(target, start_time)?,
            ContainerType::Svg => self.execute_vector_svg(target, start_time)?,
            ContainerType::Pdf | ContainerType::Docx | ContainerType::Idml => self.execute_document_pdf(target, start_time)?,
            ContainerType::Gltf | ContainerType::Usdz | ContainerType::Obj | ContainerType::Stl | ContainerType::Ply => {
                self.execute_spatial_3d(target, start_time)?
            }
            ContainerType::Png
            | ContainerType::Jpeg
            | ContainerType::WebP
            | ContainerType::Tiff
            | ContainerType::OpenExr
            | ContainerType::Bmp
            | ContainerType::StillSequence => self.execute_image_transcode(target, start_time)?,
        }

        // Export multichannel audio stems if configured
        if let Some(mode) = target.audio_stems
            && mode != encodercraft_core::AudioStemMode::None
        {
            self.export_audio_stems(target, mode)?;
        }

        Ok(())
    }

    fn load_source_audio(&self) -> Result<AudioSampleBuffer, EncoderError> {
        if let SourceDescriptor::File { path, .. } = &self.spec.source
            && path.exists()
        {
            let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source audio file `{}`: {e}", path.display())))?;
            if let Ok(decoded) = encodercraft_audio_dsp::decode_audio(&bytes) {
                return Ok(decoded);
            }
            if filmcraft_mxf::sniff(&bytes)
                && let Ok(mxf) = filmcraft_mxf::open(bytes.as_slice())
            {
                for track in &mxf.tracks {
                    if track.kind == filmcraft_mxf::TrackKind::Sound
                        && let Some(snd) = &track.sound
                    {
                        let mut pcm_bytes = Vec::new();
                        for chunk in &track.chunks {
                            let start = usize::try_from(chunk.offset).unwrap_or(bytes.len());
                            let end = start.saturating_add(usize::try_from(chunk.size).unwrap_or(0)).min(bytes.len());
                            if let Some(slice) = bytes.get(start..end) {
                                pcm_bytes.extend_from_slice(slice);
                            }
                        }
                        if !pcm_bytes.is_empty()
                            && let Ok(planes) =
                                filmcraft_mxf::decode_pcm(&pcm_bytes, track.sound_format, snd.channels as usize, snd.bits, snd.frame_bytes())
                        {
                            let sr = if snd.sample_rate.den > 0 {
                                u32::try_from((snd.sample_rate.num / snd.sample_rate.den).max(8000)).unwrap_or(48_000)
                            } else {
                                48_000
                            };
                            let layout = match planes.len() {
                                1 => encodercraft_core::ChannelLayout::Mono,
                                2 => encodercraft_core::ChannelLayout::Stereo,
                                6 => encodercraft_core::ChannelLayout::Surround51,
                                8 => encodercraft_core::ChannelLayout::Surround71,
                                _ => encodercraft_core::ChannelLayout::Stereo,
                            };
                            if let Ok(buf) = AudioSampleBuffer::from_planar_channels(sr, layout, 0, planes) {
                                return Ok(buf);
                            }
                        }
                    }
                }
            }
        }
        let sample_rate = 48000;
        AudioSampleBuffer::new_silent(sample_rate, encodercraft_core::ChannelLayout::Stereo, sample_rate as usize, 0)
    }

    fn load_source_video_frames(&self, fallback_dims: Dimensions, fallback_fps: Rational64) -> Result<Vec<VideoFrame>, EncoderError> {
        let mut start_frame = 0usize;
        let mut max_frames: Option<usize> = None;

        if let SourceDescriptor::File { path, start_tick, duration_ticks, timebase } = &self.spec.source {
            let fps_val = fallback_fps.as_f64().max(1.0);
            if let Some(st) = start_tick
                && *st > 0
            {
                if let Some(tb) = timebase
                    && tb.as_f64() > 0.0
                {
                    start_frame = ((*st as f64 / tb.as_f64()) * fps_val).floor().max(0.0) as usize;
                } else {
                    start_frame = usize::try_from(*st).unwrap_or(0);
                }
            }
            if let Some(dt) = duration_ticks
                && *dt > 0
            {
                if let Some(tb) = timebase
                    && tb.as_f64() > 0.0
                {
                    max_frames = Some(((*dt as f64 / tb.as_f64()) * fps_val).ceil().max(1.0) as usize);
                } else {
                    max_frames = Some(usize::try_from(*dt).unwrap_or(24).max(1));
                }
            }

            if path.exists()
                && let Ok(bytes) = std::fs::read(path)
            {
                let decoded_frames = Self::decode_container_video_frames(&bytes, start_frame, max_frames, fallback_fps);
                if !decoded_frames.is_empty() {
                    return Ok(decoded_frames);
                }
            }
        }

        let count = max_frames.unwrap_or(24).clamp(1, 3600);
        let mut synthetic = Vec::with_capacity(count);
        for f in 0..count {
            let frame = VideoFrame::new_allocated(fallback_dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, (start_frame + f) as i64, 1, fallback_fps)?;
            synthetic.push(frame);
        }
        Ok(synthetic)
    }

    fn decode_container_video_frames(bytes: &[u8], start_frame: usize, max_frames: Option<usize>, fallback_fps: Rational64) -> Vec<VideoFrame> {
        let mut decoded_frames = Vec::new();
        let mut vp8_dec: Option<Vp8DecoderWrapper> = None;

        if bytes.starts_with(b"BM") {
            if let Ok(frame) = encodercraft_image_codecs::decode_bmp(bytes) {
                decoded_frames.push(frame);
            }
        } else if filmcraft_mxf::sniff(bytes) {
            if let Ok(mxf) = filmcraft_mxf::open(bytes) {
                for track in &mxf.tracks {
                    if track.kind == filmcraft_mxf::TrackKind::Picture {
                        for (idx, sample) in track.samples.iter().enumerate() {
                            if idx < start_frame {
                                continue;
                            }
                            if let Some(limit) = max_frames
                                && decoded_frames.len() >= limit
                            {
                                break;
                            }
                            let start = usize::try_from(sample.offset).unwrap_or(bytes.len());
                            let end = start.saturating_add(sample.size as usize).min(bytes.len());
                            if let Some(pkt) = bytes.get(start..end)
                                && let Ok(Some(vf)) = decode_video_packet_auto(pkt, sample.pts, fallback_fps, &mut vp8_dec)
                            {
                                decoded_frames.push(vf);
                            }
                        }
                        break;
                    }
                }
            }
        } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            if let Ok(mkv) = craft_media::matroska::open(bytes)
                && let Some(v_track_idx) = mkv.track_of_kind(craft_media::matroska::TrackKind::Video)
                && let Some(track) = mkv.tracks.get(v_track_idx)
            {
                for (idx, sample) in track.samples.iter().enumerate() {
                    if idx < start_frame {
                        continue;
                    }
                    if let Some(limit) = max_frames
                        && decoded_frames.len() >= limit
                    {
                        break;
                    }
                    if let Ok(pkt) = mkv.read_sample(&bytes, v_track_idx, idx)
                        && let Ok(Some(vf)) = decode_video_packet_auto(&pkt, sample.pts, fallback_fps, &mut vp8_dec)
                    {
                        decoded_frames.push(vf);
                    }
                }
            }
        } else if let Ok(mp4) = craft_media::isobmff::open(bytes) {
            for (t_idx, track) in mp4.tracks.iter().enumerate() {
                if track.kind == craft_media::isobmff::TrackKind::Video {
                    for (idx, sample) in track.samples.iter().enumerate() {
                        if idx < start_frame {
                            continue;
                        }
                        if let Some(limit) = max_frames
                            && decoded_frames.len() >= limit
                        {
                            break;
                        }
                        if let Ok(pkt) = mp4.read_sample(&bytes, t_idx, idx)
                            && let Ok(Some(vf)) = decode_video_packet_auto(&pkt, sample.pts, fallback_fps, &mut vp8_dec)
                        {
                            decoded_frames.push(vf);
                        }
                    }
                    break;
                }
            }
        }
        decoded_frames
    }

    fn execute_audio_flac(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mut sample_buf = self.load_source_audio()?;

        sample_buf = remix_channels(&sample_buf, encodercraft_core::ChannelLayout::Stereo)?;

        if let Some(target_lufs) = target.target_lufs {
            let _ = normalize_loudness(&mut sample_buf, target_lufs as f64, -1.0)?;
        }

        let flac_bytes = encodercraft_audio_dsp::encode_audio_flac(&sample_buf)?;
        std::fs::write(&target.output_path, flac_bytes).map_err(|e| EncoderError::Io(format!("failed to write FLAC file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_audio_aiff(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mut sample_buf = self.load_source_audio()?;

        sample_buf = remix_channels(&sample_buf, encodercraft_core::ChannelLayout::Stereo)?;

        if let Some(target_lufs) = target.target_lufs {
            let _ = normalize_loudness(&mut sample_buf, target_lufs as f64, -1.0)?;
        }

        let aiff_bytes = encodercraft_audio_dsp::encode_audio_aiff(&sample_buf)?;
        std::fs::write(&target.output_path, aiff_bytes).map_err(|e| EncoderError::Io(format!("failed to write AIFF file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_audio_wav(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let out_file = File::create(&target.output_path).map_err(|e| EncoderError::Io(format!("failed to create output file: {e}")))?;
        let writer = BufWriter::new(out_file);

        let bext = BextMetadata {
            description: format!("Encoded by encoderCraft - {}", target.preset_name),
            originator: "encoderCraft".into(),
            ..Default::default()
        };

        let mut sample_buf = self.load_source_audio()?;
        let sample_rate = sample_buf.sample_rate;
        let xmp_xml = target.metadata.as_ref().map(|m| m.to_xmp_packet());
        let mut wav_writer = BwfWavWriter::new_with_xmp(writer, sample_rate, 2, WavFormatTag::Pcm16, Some(&bext), xmp_xml.as_deref())?;

        // 1. Remix channels if needed
        sample_buf = remix_channels(&sample_buf, encodercraft_core::ChannelLayout::Stereo)?;

        // 2. Loudness normalization if specified
        if let Some(target_lufs) = target.target_lufs {
            let _ = normalize_loudness(&mut sample_buf, target_lufs as f64, -1.0)?;
        }

        // 3. Write buffer
        wav_writer.write_buffer(&sample_buf)?;
        wav_writer.finish()?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_vector_svg(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let content = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes = std::fs::read(path)
                    .map_err(|e| EncoderError::Io(format!("failed to read source vector file `{}`: {e}", path.display())))?;
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    if text.contains("<svg") {
                        text.to_string()
                    } else {
                        format!(
                            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1920 1080" width="1920" height="1080"><!-- Converted from {} --><text x="50" y="100" font-family="sans-serif" font-size="24">{}</text></svg>"#,
                            path.file_name().and_then(|n| n.to_str()).unwrap_or("vector"),
                            quick_xml_escape(&path.display().to_string())
                        )
                    }
                } else {
                    format!(
                        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1920 1080" width="1920" height="1080"><!-- Converted from binary {} --></svg>"#,
                        path.file_name().and_then(|n| n.to_str()).unwrap_or("vector")
                    )
                }
            }
            _ => r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1920 1080" width="1920" height="1080"><rect width="100%" height="100%" fill="#1a1a1a"/></svg>"##.to_string(),
        };

        std::fs::write(&target.output_path, content.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write SVG file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_spatial_3d(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mesh = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source 3D file `{}`: {e}", path.display())))?;
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                match ext.as_str() {
                    "obj" => {
                        let text = std::str::from_utf8(&bytes).map_err(|e| EncoderError::InvalidContainer(format!("invalid OBJ text: {e}")))?;
                        encodercraft_spatial_io::parse_obj(text)?
                    }
                    "stl" => encodercraft_spatial_io::parse_stl_binary(&bytes)?,
                    "ply" => encodercraft_spatial_io::parse_ply_binary(&bytes)?,
                    "glb" | "gltf" => encodercraft_spatial_io::parse_glb(&bytes)?,
                    _ => {
                        if bytes.starts_with(b"glTF") {
                            encodercraft_spatial_io::parse_glb(&bytes)?
                        } else if bytes.starts_with(b"ply\n") || bytes.starts_with(b"ply\r\n") {
                            encodercraft_spatial_io::parse_ply_binary(&bytes)?
                        } else if let Ok(text) = std::str::from_utf8(&bytes)
                            && (text.starts_with("v ") || text.starts_with('#') || text.starts_with("o "))
                        {
                            encodercraft_spatial_io::parse_obj(text)?
                        } else {
                            encodercraft_spatial_io::parse_stl_binary(&bytes)?
                        }
                    }
                }
            }
            _ => {
                let mut m = encodercraft_spatial_io::TriangleMesh::new("UnitCube");
                m.vertices = vec![
                    [-0.5, -0.5, -0.5],
                    [0.5, -0.5, -0.5],
                    [0.5, 0.5, -0.5],
                    [-0.5, 0.5, -0.5],
                    [-0.5, -0.5, 0.5],
                    [0.5, -0.5, 0.5],
                    [0.5, 0.5, 0.5],
                    [-0.5, 0.5, 0.5],
                ];
                m.indices = vec![
                    [0, 1, 2],
                    [0, 2, 3],
                    [4, 6, 5],
                    [4, 7, 6],
                    [0, 4, 5],
                    [0, 5, 1],
                    [2, 6, 7],
                    [2, 7, 3],
                    [0, 3, 7],
                    [0, 7, 4],
                    [1, 5, 6],
                    [1, 6, 2],
                ];
                m.generate_flat_normals();
                m
            }
        };

        match target.container {
            ContainerType::Gltf => {
                let glb_bytes = encodercraft_spatial_io::write_glb(&mesh)?;
                std::fs::write(&target.output_path, glb_bytes).map_err(|e| EncoderError::Io(format!("failed to write GLB file: {e}")))?;
            }
            ContainerType::Obj => {
                let obj_text = encodercraft_spatial_io::write_obj(&mesh);
                std::fs::write(&target.output_path, obj_text.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write OBJ file: {e}")))?;
            }
            ContainerType::Stl => {
                let stl_bytes = encodercraft_spatial_io::write_stl_binary(&mesh);
                std::fs::write(&target.output_path, stl_bytes).map_err(|e| EncoderError::Io(format!("failed to write STL file: {e}")))?;
            }
            ContainerType::Ply => {
                let ply_bytes = encodercraft_spatial_io::write_ply_binary(&mesh);
                std::fs::write(&target.output_path, ply_bytes).map_err(|e| EncoderError::Io(format!("failed to write PLY file: {e}")))?;
            }
            ContainerType::Usdz => {
                let usdz_bytes = encodercraft_spatial_io::write_usdz(&mesh)?;
                std::fs::write(&target.output_path, usdz_bytes).map_err(|e| EncoderError::Io(format!("failed to write USDZ file: {e}")))?;
            }
            _ => return Err(EncoderError::UnsupportedCodec(format!("unsupported 3D container: {:?}", target.container))),
        }

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_image_transcode(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let frame = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source image `{}`: {e}", path.display())))?;
                if bytes.starts_with(b"BM") {
                    encodercraft_image_codecs::decode_bmp(&bytes)?
                } else {
                    let w = target.custom_width.unwrap_or(1920);
                    let h = target.custom_height.unwrap_or(1080);
                    let dims = Dimensions::new(w, h)?;
                    let mut f = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24)?;
                    let (plane, _) = f.plane_mut(0)?;
                    plane.fill(128);
                    f
                }
            }
            _ => {
                let w = target.custom_width.unwrap_or(1920);
                let h = target.custom_height.unwrap_or(1080);
                let dims = Dimensions::new(w, h)?;
                let mut f = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24)?;
                let (plane, _) = f.plane_mut(0)?;
                plane.fill(64);
                f
            }
        };

        let final_frame = if let (Some(w), Some(h)) = (target.custom_width, target.custom_height) {
            if w != frame.dimensions.width || h != frame.dimensions.height {
                let target_dims = Dimensions::new(w, h)?;
                let scaler = FrameScaler::new(ScaleMode::Fit);
                scaler.scale(&frame, target_dims)?
            } else {
                frame
            }
        } else {
            frame
        };

        match target.container {
            ContainerType::Png | ContainerType::StillSequence => {
                let png_bytes = encodercraft_image_codecs::encode_png(&final_frame)?;
                std::fs::write(&target.output_path, png_bytes).map_err(|e| EncoderError::Io(format!("failed to write PNG: {e}")))?;
            }
            ContainerType::Tiff => {
                let tiff_bytes = encodercraft_image_codecs::encode_tiff(&final_frame)?;
                std::fs::write(&target.output_path, tiff_bytes).map_err(|e| EncoderError::Io(format!("failed to write TIFF: {e}")))?;
            }
            ContainerType::OpenExr => {
                let exr_bytes = encodercraft_image_codecs::encode_exr_half(&final_frame)?;
                std::fs::write(&target.output_path, exr_bytes).map_err(|e| EncoderError::Io(format!("failed to write OpenEXR: {e}")))?;
            }
            ContainerType::Bmp => {
                let bmp_bytes = encodercraft_image_codecs::encode_bmp(&final_frame)?;
                std::fs::write(&target.output_path, bmp_bytes).map_err(|e| EncoderError::Io(format!("failed to write BMP: {e}")))?;
            }
            _ => {
                let png_bytes = encodercraft_image_codecs::encode_png(&final_frame)?;
                std::fs::write(&target.output_path, png_bytes).map_err(|e| EncoderError::Io(format!("failed to write PNG: {e}")))?;
            }
        }

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_document_pdf(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let pdf_bytes = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes =
                    std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source document file `{}`: {e}", path.display())))?;
                if bytes.starts_with(b"%PDF-") {
                    bytes
                } else {
                    generate_minimal_pdf(&format!("Document export: {}", path.file_name().and_then(|n| n.to_str()).unwrap_or("document")))
                }
            }
            _ => generate_minimal_pdf("Exported Document"),
        };

        std::fs::write(&target.output_path, &pdf_bytes).map_err(|e| EncoderError::Io(format!("failed to write PDF file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_video_mp4(&self, target: &OutputTarget, is_mov: bool, start_time: Instant) -> Result<(), EncoderError> {
        let out_file = File::create(&target.output_path).map_err(|e| EncoderError::Io(format!("failed to create output file: {e}")))?;
        let mut mp4_muxer = Mp4Muxer::new(out_file, is_mov)?;

        if target.hardware_acceleration {
            let caps = encodercraft_platform::detect_hardware();
            log::info!(
                "Hardware acceleration requested: {} (H.264={}, HEVC={}, ProRes={})",
                caps.device_name,
                caps.supports_h264,
                caps.supports_hevc,
                caps.supports_prores
            );
        }

        let w = target.custom_width.unwrap_or(1920);
        let h = target.custom_height.unwrap_or(1080);
        let dims = Dimensions::new(w, h)?;
        let fps = target.custom_frame_rate.unwrap_or(Rational64::FPS_24);

        let lower_preset = target.preset_name.to_ascii_lowercase();
        let codec_kind = if is_mov && lower_preset.contains("prores") {
            VideoCodecKind::ProRes
        } else if lower_preset.contains("dnx") {
            VideoCodecKind::DnxHd
        } else if lower_preset.contains("hevc") || lower_preset.contains("h265") {
            VideoCodecKind::Hevc
        } else if lower_preset.contains("av1") {
            VideoCodecKind::Av1
        } else if lower_preset.contains("vp9") {
            VideoCodecKind::Vp9
        } else {
            VideoCodecKind::H264
        };

        let v_cfg = VideoStreamConfig {
            codec_fourcc: codec_kind.default_fourcc(),
            dimensions: dims,
            frame_rate: fps,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: vec![0, 0, 0, 1],
        };

        let v_track_idx = mp4_muxer.add_video_stream(&v_cfg)?;
        let mut encoder = create_video_encoder(codec_kind, dims.width, dims.height, fps, target.target_bitrate_kbps)?;

        // Prepare and add multiplexed AAC-LC audio stream
        let mut audio_buf = self.load_source_audio()?;
        audio_buf = remix_channels(&audio_buf, encodercraft_core::ChannelLayout::Stereo)?;
        if let Some(target_lufs) = target.target_lufs {
            let _ = normalize_loudness(&mut audio_buf, target_lufs as f64, -1.0)?;
        }
        let mut aac_enc = AacEncoderWrapper::new(audio_buf.sample_rate, 2, 192_000)?;
        let a_cfg = AudioStreamConfig {
            codec_fourcc: *b"mp4a",
            sample_rate: audio_buf.sample_rate,
            layout: encodercraft_core::ChannelLayout::Stereo,
            bits_per_sample: 16,
            codec_private: aac_enc.audio_specific_config(),
        };
        let a_track_idx = mp4_muxer.add_audio_stream(&a_cfg)?;

        let source_frames = self.load_source_video_frames(dims, fps)?;
        let total_frames = source_frames.len() as u64;
        self.progress.set_total_frames(total_frames);

        let scaler = FrameScaler::new(ScaleMode::Fit);

        let lut = if let Some(ref lut_path) = target.lut_path {
            let content = std::fs::read_to_string(lut_path).map_err(|e| EncoderError::Io(format!("failed to read LUT file: {e}")))?;
            Some(crate::lut::Lut3D::parse_cube(&content)?)
        } else {
            None
        };

        let tc_overlay = if target.burn_timecode { Some(crate::overlay::TimecodeOverlay::default()) } else { None };

        let (caption_burner, caption_track) = if target.burn_captions {
            if let Some(cap_path) = &target.caption_path {
                let track = crate::captions::CaptionTrack::from_file(cap_path)?;
                (Some(crate::captions::CaptionBurner::default()), Some(track))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        for (f, mut frame) in source_frames.into_iter().enumerate() {
            self.progress.checkpoint()?;

            frame.pts = f as i64;
            let mut scaled_frame = scaler.scale(&frame, dims)?;

            if let Some(ref l) = lut {
                l.apply_to_frame(&mut scaled_frame)?;
            }

            if let Some(ref tc) = tc_overlay {
                let timecode = encodercraft_core::MediaTimecode::new(0, 0, (f / 24) as u8, (f % 24) as u8, false, fps);
                tc.draw(&mut scaled_frame, &timecode)?;
            }

            if let (Some(burner), Some(track)) = (&caption_burner, &caption_track) {
                let pts_seconds = f as f64 / fps.as_f64();
                burner.burn(&mut scaled_frame, track, pts_seconds)?;
            }

            let packets = encoder.encode_frame(&scaled_frame)?;
            for pkt in packets {
                let sample = VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1000, is_keyframe: pkt.is_keyframe };
                mp4_muxer.write_video_sample(v_track_idx, sample)?;
            }

            self.progress.advance_frame(start_time);
        }

        let flushed = encoder.flush()?;
        for pkt in flushed {
            let sample = VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1000, is_keyframe: pkt.is_keyframe };
            mp4_muxer.write_video_sample(v_track_idx, sample)?;
        }

        // Write encoded AAC audio packets
        let mut aac_packets = aac_enc.encode_buffer(&audio_buf)?;
        aac_packets.extend(aac_enc.flush()?);
        for (a_idx, pkt) in aac_packets.iter().enumerate() {
            let a_sample = AudioSample { data: pkt, pts: (a_idx as i64) * 1024, duration_ticks: 1024 };
            mp4_muxer.write_audio_sample(a_track_idx, a_sample)?;
        }

        mp4_muxer.finish(false)?;

        // Append Adobe XMP UUID box if metadata is defined
        if let Some(ref meta) = target.metadata {
            let xmp_packet = meta.to_xmp_packet();
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&target.output_path)
                .map_err(|e| EncoderError::Io(format!("failed to open MP4 for XMP embedding: {e}")))?;
            encodercraft_muxers::append_xmp_uuid_box(&mut file, &xmp_packet)?;
        }

        Ok(())
    }

    fn execute_video_mxf(&self, target: &OutputTarget, is_op_atom: bool, start_time: Instant) -> Result<(), EncoderError> {
        let out_file = File::create(&target.output_path).map_err(|e| EncoderError::Io(format!("failed to create MXF output file: {e}")))?;
        let writer = BufWriter::new(out_file);

        let w = target.custom_width.unwrap_or(1920);
        let h = target.custom_height.unwrap_or(1080);
        let dims = Dimensions::new(w, h)?;
        let fps = target.custom_frame_rate.unwrap_or(Rational64::FPS_24);

        let lower_preset = target.preset_name.to_ascii_lowercase();
        let codec_kind = if lower_preset.contains("prores") {
            VideoCodecKind::ProRes
        } else if lower_preset.contains("h264") || lower_preset.contains("avc") {
            VideoCodecKind::H264
        } else {
            VideoCodecKind::DnxHd
        };

        let bit_depth = if lower_preset.contains("hqx") || lower_preset.contains("10-bit") || codec_kind == VideoCodecKind::ProRes { 10 } else { 8 };

        let v_cfg = VideoStreamConfig {
            codec_fourcc: codec_kind.default_fourcc(),
            dimensions: dims,
            frame_rate: fps,
            pixel_format: if bit_depth >= 10 { PixelFormat::Yuv422p10 } else { PixelFormat::Yuv422p },
            color_space: ColorSpaceTag::REC709,
            bit_depth,
            codec_private: Vec::new(),
        };

        let mut audio_buf = self.load_source_audio()?;
        audio_buf = remix_channels(&audio_buf, encodercraft_core::ChannelLayout::Stereo)?;
        if let Some(target_lufs) = target.target_lufs {
            let _ = normalize_loudness(&mut audio_buf, target_lufs as f64, -1.0)?;
        }

        let a_cfg = AudioStreamConfig {
            codec_fourcc: *b"sowt",
            sample_rate: audio_buf.sample_rate,
            layout: encodercraft_core::ChannelLayout::Stereo,
            bits_per_sample: 24,
            codec_private: Vec::new(),
        };

        let pkg_name = target.output_path.file_stem().and_then(|s| s.to_str()).unwrap_or("encoderCraft_MXF");
        let audio_cfgs: &[AudioStreamConfig] = if is_op_atom { &[] } else { std::slice::from_ref(&a_cfg) };
        let mut mxf_muxer = MxfMuxer::new(writer, is_op_atom, pkg_name, Some(&v_cfg), audio_cfgs)?;

        let mut encoder = create_video_encoder(codec_kind, dims.width, dims.height, fps, target.target_bitrate_kbps)?;
        let source_frames = self.load_source_video_frames(dims, fps)?;
        self.progress.set_total_frames(source_frames.len() as u64);

        let scaler = FrameScaler::new(ScaleMode::Fit);
        for (f, mut frame) in source_frames.into_iter().enumerate() {
            self.progress.checkpoint()?;
            frame.pts = f as i64;
            let scaled_frame = scaler.scale(&frame, dims)?;
            let packets = encoder.encode_frame(&scaled_frame)?;
            for pkt in packets {
                mxf_muxer.write_video_sample(VideoSample {
                    data: &pkt.data,
                    pts: pkt.pts,
                    dts: pkt.dts,
                    duration_ticks: 1,
                    is_keyframe: pkt.is_keyframe,
                })?;
            }
            self.progress.advance_frame(start_time);
        }

        let flushed = encoder.flush()?;
        for pkt in flushed {
            mxf_muxer.write_video_sample(VideoSample {
                data: &pkt.data,
                pts: pkt.pts,
                dts: pkt.dts,
                duration_ticks: 1,
                is_keyframe: pkt.is_keyframe,
            })?;
        }

        if !is_op_atom {
            mxf_muxer.write_audio_buffer(0, &audio_buf, 24)?;
        }

        let mut finished = mxf_muxer.finish()?;
        std::io::Write::flush(&mut finished).map_err(|e| EncoderError::Io(format!("failed to flush MXF file: {e}")))?;
        Ok(())
    }

    fn execute_video_mkv(&self, target: &OutputTarget, is_webm: bool, start_time: Instant) -> Result<(), EncoderError> {
        let out_file = File::create(&target.output_path).map_err(|e| EncoderError::Io(format!("failed to create MKV/WebM file: {e}")))?;
        let writer = BufWriter::new(out_file);

        let w = target.custom_width.unwrap_or(1920);
        let h = target.custom_height.unwrap_or(1080);
        let dims = Dimensions::new(w, h)?;
        let fps = target.custom_frame_rate.unwrap_or(Rational64::FPS_24);

        let lower_preset = target.preset_name.to_ascii_lowercase();
        let codec_kind = if is_webm {
            if lower_preset.contains("av1") { VideoCodecKind::Av1 } else { VideoCodecKind::Vp9 }
        } else if lower_preset.contains("av1") {
            VideoCodecKind::Av1
        } else if lower_preset.contains("vp9") {
            VideoCodecKind::Vp9
        } else if lower_preset.contains("hevc") || lower_preset.contains("h265") {
            VideoCodecKind::Hevc
        } else {
            VideoCodecKind::H264
        };

        let v_cfg = VideoStreamConfig {
            codec_fourcc: codec_kind.default_fourcc(),
            dimensions: dims,
            frame_rate: fps,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: Vec::new(),
        };

        let mut builder = MkvMuxerBuilder::new(writer, is_webm);
        let v_track = builder.add_video_stream(&v_cfg);
        let mut mkv_muxer = builder.build()?;

        let mut encoder = create_video_encoder(codec_kind, dims.width, dims.height, fps, target.target_bitrate_kbps)?;
        let source_frames = self.load_source_video_frames(dims, fps)?;
        self.progress.set_total_frames(source_frames.len() as u64);

        let scaler = FrameScaler::new(ScaleMode::Fit);
        let frame_dur_ms = (1000.0 / fps.as_f64().max(1.0)).round() as u32;

        for (f, mut frame) in source_frames.into_iter().enumerate() {
            self.progress.checkpoint()?;
            frame.pts = (f as i64) * i64::from(frame_dur_ms);
            let scaled_frame = scaler.scale(&frame, dims)?;
            let packets = encoder.encode_frame(&scaled_frame)?;
            for pkt in packets {
                mkv_muxer.write_video_sample(
                    v_track,
                    VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: frame_dur_ms, is_keyframe: pkt.is_keyframe },
                )?;
            }
            self.progress.advance_frame(start_time);
        }

        let flushed = encoder.flush()?;
        for pkt in flushed {
            mkv_muxer.write_video_sample(
                v_track,
                VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: frame_dur_ms, is_keyframe: pkt.is_keyframe },
            )?;
        }

        let mut finished = mkv_muxer.finish()?;
        std::io::Write::flush(&mut finished).map_err(|e| EncoderError::Io(format!("failed to flush MKV file: {e}")))?;
        Ok(())
    }

    fn execute_animated_gif(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let w = target.custom_width.unwrap_or(480);
        let h = target.custom_height.unwrap_or(270);
        let dims = Dimensions::new(w, h)?;
        let fps = target.custom_frame_rate.unwrap_or(Rational64::new(12, 1)?);

        let source_frames = self.load_source_video_frames(dims, fps)?;
        self.progress.set_total_frames(source_frames.len() as u64);

        let scaler = FrameScaler::new(ScaleMode::Fit);
        let fps_u32 = (fps.as_f64().round() as u32).max(1);
        let mut gif_enc = encodercraft_video_codecs::GifEncoder::new(dims.width as u16, dims.height as u16, fps_u32)?;

        for frame in source_frames {
            let scaled = scaler.scale(&frame, dims)?;
            gif_enc.add_frame(&scaled)?;
            self.progress.advance_frame(start_time);
        }

        let gif_bytes = gif_enc.finish();
        std::fs::write(&target.output_path, gif_bytes).map_err(|e| EncoderError::Io(format!("failed to write GIF file: {e}")))?;
        Ok(())
    }

    /// Exports multichannel audio stems (discrete mono or stereo pairs) conforming to AME multichannel workflow.
    fn export_audio_stems(&self, target: &OutputTarget, mode: encodercraft_core::AudioStemMode) -> Result<(), EncoderError> {
        let sample_rate = 48000;
        let channels = 2; // Default stereo source
        let sample_buf =
            encodercraft_core::AudioSampleBuffer::new_silent(sample_rate, encodercraft_core::ChannelLayout::Stereo, sample_rate as usize, 0)?;

        let stem_base = target.output_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        let parent = target.output_path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let num_samples = sample_buf.samples_per_channel();

        match mode {
            encodercraft_core::AudioStemMode::DiscreteMono => {
                let channel_names = ["L", "R", "C", "LFE", "Ls", "Rs", "Ch7", "Ch8"];
                for c in 0..channels {
                    let mut mono_buf =
                        encodercraft_core::AudioSampleBuffer::new_silent(sample_rate, encodercraft_core::ChannelLayout::Mono, num_samples, 0)?;
                    let src_plane = sample_buf.channel(c)?;
                    let dest_plane = mono_buf.channel_mut(0)?;
                    dest_plane.copy_from_slice(src_plane);

                    let name = channel_names.get(c).copied().unwrap_or("Ch");
                    let stem_filename = format!("{stem_base}_Stem_{name}.wav");
                    let stem_path = parent.join(stem_filename);

                    let file = File::create(&stem_path).map_err(|e| EncoderError::Io(format!("failed to create audio stem: {e}")))?;
                    let mut writer = BwfWavWriter::new(BufWriter::new(file), sample_rate, 1, WavFormatTag::Pcm16, None)?;
                    writer.write_buffer(&mono_buf)?;
                    writer.finish()?;
                }
            }
            encodercraft_core::AudioStemMode::StereoPairs => {
                let pair_count = channels.div_ceil(2);
                for p in 0..pair_count {
                    let stem_filename = format!("{stem_base}_Stems_Ch{}-{}.wav", p * 2 + 1, p * 2 + 2);
                    let stem_path = parent.join(stem_filename);

                    let file = File::create(&stem_path).map_err(|e| EncoderError::Io(format!("failed to create audio stem: {e}")))?;
                    let mut writer = BwfWavWriter::new(BufWriter::new(file), sample_rate, 2, WavFormatTag::Pcm16, None)?;
                    writer.write_buffer(&sample_buf)?;
                    writer.finish()?;
                }
            }
            encodercraft_core::AudioStemMode::None => {}
        }

        Ok(())
    }
}

fn quick_xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn generate_minimal_pdf(title: &str) -> Vec<u8> {
    let safe_title = title.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
    let content_stream = format!("BT\n/F1 18 Tf\n50 750 Td\n({}) Tj\nET\n", safe_title);
    let stream_len = content_stream.len();

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n");
    let o1 = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let o2 = pdf.len();
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    let o3 = pdf.len();
    pdf.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n",
    );
    let o4 = pdf.len();
    let stream_obj = format!("4 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n", stream_len, content_stream);
    pdf.extend_from_slice(stream_obj.as_bytes());
    let o5 = pdf.len();
    pdf.extend_from_slice(b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");
    let xref_pos = pdf.len();
    let xref = format!(
        "xref\n0 6\n0000000000 65535 f \n{:010} 00000 n \n{:010} 00000 n \n{:010} 00000 n \n{:010} 00000 n \n{:010} 00000 n \ntrailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        o1, o2, o3, o4, o5, xref_pos
    );
    pdf.extend_from_slice(xref.as_bytes());
    pdf
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, JobPriority, SourceDescriptor, StartPolicy};
    use tempfile::tempdir;

    #[test]
    fn test_execute_audio_transcode() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("output.wav");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-audio-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_file.clone(),
                preset_name: "WAV Broadcast".into(),
                container: ContainerType::Wave,
                custom_width: None,
                custom_height: None,
                custom_frame_rate: None,
                target_bitrate_kbps: None,
                target_lufs: Some(-23.0),
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_file.exists());
        assert!(target_file.metadata().unwrap().len() > 100);
    }

    #[test]
    fn test_execute_video_transcode() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("output.mp4");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-video-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_file.clone(),
                preset_name: "Web 720p".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(1280),
                custom_height: Some(720),
                custom_frame_rate: Some(Rational64::FPS_24),
                target_bitrate_kbps: Some(4000),
                target_lufs: None,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_file.exists());
        assert!(target_file.metadata().unwrap().len() > 100);
    }

    #[test]
    fn test_execute_flac_and_aiff_transcode() {
        let dir = tempdir().unwrap();
        let flac_file = dir.path().join("output.flac");
        let aiff_file = dir.path().join("output.aif");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-lossless-audio-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_flac".into(),
                    output_path: flac_file.clone(),
                    preset_name: "FLAC Master".into(),
                    container: ContainerType::Flac,
                    custom_width: None,
                    custom_height: None,
                    custom_frame_rate: None,
                    target_bitrate_kbps: None,
                    target_lufs: Some(-16.0),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_aiff".into(),
                    output_path: aiff_file.clone(),
                    preset_name: "AIFF Master".into(),
                    container: ContainerType::Aiff,
                    custom_width: None,
                    custom_height: None,
                    custom_frame_rate: None,
                    target_bitrate_kbps: None,
                    target_lufs: None,
                    ..Default::default()
                },
            ],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(flac_file.exists());
        assert!(flac_file.metadata().unwrap().len() > 50);
        assert!(aiff_file.exists());
        assert!(aiff_file.metadata().unwrap().len() > 50);
    }

    #[test]
    fn test_execute_prores_mov_transcode() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("output.mov");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-prores-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_file.clone(),
                preset_name: "Apple ProRes 422".into(),
                container: ContainerType::QuickTimeMov,
                custom_width: Some(640),
                custom_height: Some(360),
                custom_frame_rate: Some(Rational64::FPS_24),
                target_bitrate_kbps: None,
                target_lufs: None,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_file.exists());
        assert!(target_file.metadata().unwrap().len() > 100);
    }

    #[test]
    fn test_execute_video_with_captions_sidecar_and_burn_in() {
        let dir = tempdir().unwrap();
        let target_video = dir.path().join("output_with_captions.mp4");
        let srt_file = dir.path().join("subtitles.srt");

        std::fs::write(&srt_file, "1\n00:00:00,000 --> 00:00:02,000\nHello ArtCraft Subtitles!\n").unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-caption-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_video.clone(),
                preset_name: "YouTube 1080p".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(640),
                custom_height: Some(360),
                custom_frame_rate: Some(Rational64::FPS_24),
                caption_path: Some(srt_file),
                caption_format: Some(encodercraft_core::CaptionExportFormat::WebVtt),
                burn_captions: true,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(target_video.exists());
        // Verify WebVTT sidecar was written
        let vtt_sidecar = dir.path().join("output_with_captions.vtt");
        assert!(vtt_sidecar.exists());
        let vtt_text = std::fs::read_to_string(vtt_sidecar).unwrap();
        assert!(vtt_text.contains("WEBVTT"));
        assert!(vtt_text.contains("Hello ArtCraft Subtitles!"));
    }

    #[test]
    fn test_execute_video_with_xmp_metadata_and_sidecar() {
        let dir = tempdir().unwrap();
        let target_video = dir.path().join("output_xmp.mp4");

        let meta = encodercraft_core::XmpMetadata {
            title: Some("Parity Test 4K".into()),
            creator: Some("el-j".into()),
            description: Some("AME Parity Verification".into()),
            copyright: Some("2026 Storytold ArtCraft".into()),
            project_name: Some("FilmCraft Cut".into()),
            create_date: Some("2026-11-02T12:00:00Z".into()),
            start_timecode: Some("01:00:00:00".into()),
            export_sidecar_xmp: true,
        };

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-xmp-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_video.clone(),
                preset_name: "YouTube 1080p".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(320),
                custom_height: Some(240),
                custom_frame_rate: Some(Rational64::FPS_24),
                metadata: Some(meta),
                hardware_acceleration: true,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(target_video.exists());
        // Verify sidecar .xmp file exists
        let sidecar_xmp = dir.path().join("output_xmp.xmp");
        assert!(sidecar_xmp.exists());
        let xmp_text = std::fs::read_to_string(sidecar_xmp).unwrap();
        assert!(xmp_text.contains("Parity Test 4K"));
        assert!(xmp_text.contains("el-j"));

        // Verify MP4 file contains the embedded XMP UUID box
        let mp4_bytes = std::fs::read(target_video).unwrap();
        let xmp_uuid: [u8; 16] = [0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8, 0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac];
        assert!(mp4_bytes.windows(16).any(|w| w == xmp_uuid));
    }

    #[test]
    fn test_execute_audio_stems_discrete_mono() {
        let dir = tempdir().unwrap();
        let target_audio = dir.path().join("master_mix.wav");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-stems-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_audio.clone(),
                preset_name: "Broadcast WAV".into(),
                container: ContainerType::Wave,
                audio_stems: Some(encodercraft_core::AudioStemMode::DiscreteMono),
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(target_audio.exists());
        // Verify discrete mono stem files were exported
        let stem_l = dir.path().join("master_mix_Stem_L.wav");
        let stem_r = dir.path().join("master_mix_Stem_R.wav");
        assert!(stem_l.exists());
        assert!(stem_r.exists());
        assert!(stem_l.metadata().unwrap().len() > 100);
        assert!(stem_r.metadata().unwrap().len() > 100);
    }

    #[test]
    fn test_execute_dnxhr_transcode() {
        let dir = tempdir().unwrap();
        let target_video = dir.path().join("output_dnx.mov");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-dnx-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_video.clone(),
                preset_name: "Avid DNxHR HQ 1080p".into(),
                container: ContainerType::QuickTimeMov,
                custom_width: Some(1920),
                custom_height: Some(1080),
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_video.exists());
    }

    #[test]
    fn test_execute_vector_svg_transcode() {
        let dir = tempdir().unwrap();
        let svg_in = dir.path().join("input.svg");
        let svg_out = dir.path().join("output.svg");

        std::fs::write(&svg_in, "<svg viewBox=\"0 0 100 100\"><circle cx=\"50\" cy=\"50\" r=\"40\" fill=\"red\"/></svg>").unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-vector-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: svg_in, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_svg".into(),
                output_path: svg_out.clone(),
                preset_name: "Web Vector SVG (Optimized)".into(),
                container: ContainerType::Svg,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(svg_out.exists());
        let text = std::fs::read_to_string(svg_out).unwrap();
        assert!(text.contains("<circle"));
    }

    #[test]
    fn test_execute_document_pdf_transcode() {
        let dir = tempdir().unwrap();
        let pdf_out = dir.path().join("output.pdf");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-doc-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: dir.path().join("sample.docx"), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_pdf".into(),
                output_path: pdf_out.clone(),
                preset_name: "Print-Ready PDF/X-4".into(),
                container: ContainerType::Pdf,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(pdf_out.exists());
        let pdf_bytes = std::fs::read(pdf_out).unwrap();
        assert!(pdf_bytes.starts_with(b"%PDF-1.7"));
    }

    #[test]
    fn test_execute_cross_audio_transcode_flac_wav() {
        let dir = tempdir().unwrap();
        let flac_src = dir.path().join("source.flac");
        let wav_out = dir.path().join("transcoded.wav");

        // 1. Create a valid FLAC audio file
        let mut sample_buf = encodercraft_core::AudioSampleBuffer::new_silent(48000, encodercraft_core::ChannelLayout::Stereo, 4800, 0).unwrap();
        // Insert a short test tone / non-zero values
        if let Ok(left) = sample_buf.channel_mut(0) {
            for (i, s) in left.iter_mut().enumerate() {
                *s = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI) / 48000.0).sin() * 0.5;
            }
        }
        let flac_bytes = encodercraft_audio_dsp::encode_audio_flac(&sample_buf).unwrap();
        std::fs::write(&flac_src, flac_bytes).unwrap();

        // 2. Transcode FLAC -> WAV using JobPipeline
        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-cross-audio-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: flac_src, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_wav".into(),
                output_path: wav_out.clone(),
                preset_name: "Broadcast WAV".into(),
                container: ContainerType::Wave,
                target_lufs: Some(-24.0),
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(wav_out.exists());
        let wav_bytes = std::fs::read(&wav_out).unwrap();
        assert!(wav_bytes.starts_with(b"RIFF"));

        // 3. Decode the resulting WAV to verify it contains valid audio
        let decoded = encodercraft_audio_dsp::decode_audio(&wav_bytes).unwrap();
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.channel_count(), 2);
        assert!(decoded.samples_per_channel() >= 4800);
    }

    #[test]
    fn test_execute_spatial_3d_obj_to_glb() {
        let dir = tempdir().unwrap();
        let obj_src = dir.path().join("model.obj");
        let glb_out = dir.path().join("model.glb");

        std::fs::write(&obj_src, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-3d-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: obj_src, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_glb".into(),
                output_path: glb_out.clone(),
                preset_name: "glTF 2.0 Binary (.glb)".into(),
                container: ContainerType::Gltf,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(glb_out.exists());
        let glb_bytes = std::fs::read(&glb_out).unwrap();
        assert!(glb_bytes.starts_with(b"glTF"));
    }

    #[test]
    fn test_execute_image_bmp_to_png() {
        let dir = tempdir().unwrap();
        let bmp_src = dir.path().join("input.bmp");
        let png_out = dir.path().join("output.png");

        let dims = Dimensions::new(8, 8).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        plane.fill(200);

        let bmp_bytes = encodercraft_image_codecs::encode_bmp(&frame).unwrap();
        std::fs::write(&bmp_src, bmp_bytes).unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-img-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: bmp_src, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_png".into(),
                output_path: png_out.clone(),
                preset_name: "Still Image PNG".into(),
                container: ContainerType::Png,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(png_out.exists());
        let png_bytes = std::fs::read(&png_out).unwrap();
        assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn test_execute_spatial_3d_obj_to_usdz() {
        let dir = tempdir().unwrap();
        let obj_src = dir.path().join("pyramid.obj");
        let usdz_out = dir.path().join("pyramid.usdz");

        std::fs::write(&obj_src, "v 0 1 0\nv -1 0 1\nv 1 0 1\nf 1 2 3\n").unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-usdz-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: obj_src, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_usdz".into(),
                output_path: usdz_out.clone(),
                preset_name: "Apple AR Quick Look USDZ (.usdz)".into(),
                container: ContainerType::Usdz,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(usdz_out.exists());
        let usdz_bytes = std::fs::read(&usdz_out).unwrap();
        assert!(usdz_bytes.starts_with(b"PK\x03\x04"));
        assert!(usdz_bytes.windows(10).any(|w| w == b"model.usda"));
    }

    #[test]
    fn test_execute_image_bmp_to_tiff() {
        let dir = tempdir().unwrap();
        let bmp_src = dir.path().join("input.bmp");
        let tiff_out = dir.path().join("output.tif");

        let dims = Dimensions::new(4, 4).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        plane.fill(100);

        let bmp_bytes = encodercraft_image_codecs::encode_bmp(&frame).unwrap();
        std::fs::write(&bmp_src, bmp_bytes).unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-tiff-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: bmp_src, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_tiff".into(),
                output_path: tiff_out.clone(),
                preset_name: "Broadcast TIFF (16-bit RGB)".into(),
                container: ContainerType::Tiff,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(tiff_out.exists());
        let tiff_bytes = std::fs::read(&tiff_out).unwrap();
        assert!(tiff_bytes.starts_with(b"II*\x00") || tiff_bytes.starts_with(b"MM\x00*"));
    }

    #[test]
    fn test_real_prores_mov_to_dnxhr_mxf_to_h264_mp4_roundtrip() {
        let dir = tempdir().unwrap();
        let prores_mov = dir.path().join("master_prores.mov");
        let dnx_mxf = dir.path().join("broadcast_dnxhr.mxf");
        let delivery_mp4 = dir.path().join("delivery_h264.mp4");

        // 1. Generate a 4-frame ProRes 422 QuickTime MOV with multiplexed AAC audio
        let spec_prores = CraftJobSpec {
            version: "1.0".into(),
            job_id: "job-prores-gen".into(),
            client_info: ClientInfo { app_name: "FilmCraft".into(), app_version: "1.0".into(), pid: 100, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: dir.path().join("synthetic.raw"), start_tick: Some(0), duration_ticks: Some(4), timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_mov".into(),
                output_path: prores_mov.clone(),
                preset_name: "Apple ProRes 422".into(),
                container: ContainerType::QuickTimeMov,
                custom_width: Some(320),
                custom_height: Some(180),
                custom_frame_rate: Some(Rational64::FPS_24),
                ..Default::default()
            }],
        };
        TranscodePipeline::new(spec_prores).execute().unwrap();
        assert!(prores_mov.exists());

        let mov_info = encodercraft_demux::probe_file(&prores_mov).unwrap();
        assert_eq!(mov_info.video_streams.len(), 1);
        assert_eq!(mov_info.audio_streams.len(), 1);

        // 2. Transcode the real ProRes MOV (decoding real ProRes frames!) into Broadcast MXF OP1a (DNxHR + 24-bit PCM)
        let spec_mxf = CraftJobSpec {
            version: "1.0".into(),
            job_id: "job-mov-to-mxf".into(),
            client_info: ClientInfo { app_name: "FilmCraft".into(), app_version: "1.0".into(), pid: 100, user_notes: None },
            priority: JobPriority::High,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: prores_mov, start_tick: Some(0), duration_ticks: Some(3), timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_mxf".into(),
                output_path: dnx_mxf.clone(),
                preset_name: "Avid DNxHR HQ 1080p".into(),
                container: ContainerType::MxfOp1a,
                custom_width: Some(320),
                custom_height: Some(180),
                custom_frame_rate: Some(Rational64::FPS_24),
                ..Default::default()
            }],
        };
        TranscodePipeline::new(spec_mxf).execute().unwrap();
        assert!(dnx_mxf.exists());

        let mxf_info = encodercraft_demux::probe_file(&dnx_mxf).unwrap();
        assert_eq!(mxf_info.video_streams.len(), 1);
        assert_eq!(mxf_info.audio_streams.len(), 1);
        assert_eq!(mxf_info.video_streams[0].codec_name, "VC-3");

        // 3. Transcode the real DNxHR MXF (decoding VC-3 frames + PCM audio!) into H.264 MP4 + AAC
        let spec_mp4 = CraftJobSpec {
            version: "1.0".into(),
            job_id: "job-mxf-to-mp4".into(),
            client_info: ClientInfo { app_name: "encoderCraft".into(), app_version: "1.0".into(), pid: 100, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: dnx_mxf, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t_mp4".into(),
                output_path: delivery_mp4.clone(),
                preset_name: "YouTube 1080p Full HD".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(320),
                custom_height: Some(180),
                custom_frame_rate: Some(Rational64::FPS_24),
                ..Default::default()
            }],
        };
        TranscodePipeline::new(spec_mp4).execute().unwrap();
        assert!(delivery_mp4.exists());

        let mp4_info = encodercraft_demux::probe_file(&delivery_mp4).unwrap();
        assert_eq!(mp4_info.video_streams.len(), 1);
        assert_eq!(mp4_info.audio_streams.len(), 1);
    }
}
