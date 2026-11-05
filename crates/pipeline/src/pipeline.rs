//! End-to-end transcoding pipeline coordinator.
//!
//! Orchestrates demuxing, video scaling, audio DSP normalization, encoding, and muxing.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use effectcraft_opusenc::OpusEncoder;
use encodercraft_audio_dsp::{AacEncoderWrapper, SincResampler, normalize_loudness, remix_channels};
use encodercraft_color::PixelFormat;
use encodercraft_core::{AudioSampleBuffer, ContainerType, CraftJobSpec, EncoderError, OutputTarget, Rational64, SourceDescriptor, VideoFrame};
use encodercraft_geom::{Dimensions, ScaleMode};
use encodercraft_image_codecs::{StillFormat, StillOptions, encode_still};
use encodercraft_muxers::{
    AudioSample, AudioStreamConfig, BextMetadata, BwfWavWriter, MkvMuxerBuilder, Mp4Muxer, MxfMuxer, VideoSample, VideoStreamConfig, WavFormatTag,
};
use encodercraft_video_codecs::capabilities::container_accepts;
use encodercraft_video_codecs::{EncodeParams, VideoCodecKind, create_video_encoder_with};

use crate::input::{VideoInput, load_audio_required};
use crate::progress::JobProgress;
use crate::scale::FrameScaler;
use crate::stream::{FrameProcessor, PipelineLimits, StreamSpec, StreamStats, run_stages};
use crate::transform::ColorTransform;

/// Execution coordinator for a single transcoding job.
pub struct TranscodePipeline {
    spec: CraftJobSpec,
    progress: Arc<JobProgress>,
    limits: PipelineLimits,
    stats: Mutex<Option<StreamStats>>,
}

/// Frees a received shared-memory stream when the job that consumed it ends, whatever the outcome.
struct PipeStreamGuard(Option<String>);

impl PipeStreamGuard {
    fn new(source: &SourceDescriptor) -> Self {
        Self(match source {
            SourceDescriptor::Pipe { endpoint, .. } => Some(endpoint.clone()),
            _ => None,
        })
    }
}

impl Drop for PipeStreamGuard {
    fn drop(&mut self) {
        if let Some(endpoint) = &self.0 {
            encodercraft_source::pipe::release(endpoint);
        }
    }
}

fn prepare_audio_for_target(mut audio: AudioSampleBuffer, target: &OutputTarget) -> Result<AudioSampleBuffer, EncoderError> {
    let settings = target.audio.as_ref();
    let layout = settings.and_then(|settings| settings.layout).unwrap_or(encodercraft_core::ChannelLayout::Stereo);
    if audio.layout != layout {
        audio = remix_channels(&audio, layout)?;
    }
    if let Some(rate) = settings.and_then(|settings| settings.sample_rate)
        && rate != audio.sample_rate
    {
        audio = SincResampler::default().resample(&audio, rate)?;
    }
    if let Some(target_lufs) = target.target_lufs {
        normalize_loudness(&mut audio, f64::from(target_lufs), -1.0)?;
    }
    Ok(audio)
}

enum Mp4AudioEncoder {
    Aac(Box<AacEncoderWrapper>),
    Pcm24Le,
}

enum MatroskaAudioEncoder {
    Aac(Box<AacEncoderWrapper>),
    Opus(Box<OpusEncoder>),
}

fn audio_codec(target: &OutputTarget) -> Option<&str> {
    target.audio.as_ref().and_then(|settings| settings.codec.as_deref())
}

fn aac_bitrate_bps(target: &OutputTarget) -> Result<u32, EncoderError> {
    let Some(bitrate_kbps) = target.audio.as_ref().and_then(|settings| settings.bitrate_kbps) else {
        return Ok(192_000);
    };
    let bitrate = bitrate_kbps
        .checked_mul(1_000)
        .filter(|bitrate| *bitrate > 0)
        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("invalid AAC audio bitrate: {bitrate_kbps} kbps")))?;
    Ok(bitrate)
}

fn encode_pcm_s24le(audio: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    let sample_count = audio
        .channel_count()
        .checked_mul(audio.samples_per_channel())
        .and_then(|count| count.checked_mul(3))
        .ok_or_else(|| EncoderError::Internal("overflow calculating PCM24 output size".into()))?;
    let mut output = Vec::with_capacity(sample_count);
    for sample_index in 0..audio.samples_per_channel() {
        for channel in 0..audio.channel_count() {
            let sample = audio
                .channel(channel)?
                .get(sample_index)
                .copied()
                .ok_or_else(|| EncoderError::InvalidJobSpec("audio channels have inconsistent sample counts".into()))?;
            let sample = if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 };
            let quantized = (sample * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
            let [least, middle, most, _] = quantized.to_le_bytes();
            output.extend_from_slice(&[least, middle, most]);
        }
    }
    Ok(output)
}

fn opus_bitrate_bps(target: &OutputTarget, channels: usize) -> Result<u32, EncoderError> {
    let bitrate_kbps = target.audio.as_ref().and_then(|settings| settings.bitrate_kbps).unwrap_or(128);
    let bitrate =
        bitrate_kbps.checked_mul(1_000).ok_or_else(|| EncoderError::InvalidJobSpec(format!("invalid Opus audio bitrate: {bitrate_kbps} kbps")))?;
    let channels = u32::try_from(channels).map_err(|_| EncoderError::InvalidJobSpec("Opus channel count exceeds the encoder limit".into()))?;
    if !matches!(channels, 1 | 2) || bitrate < 6_000u32.saturating_mul(channels) || bitrate > 510_000 {
        return Err(EncoderError::InvalidJobSpec(format!("Opus bitrate {bitrate} bps is outside the supported range for {channels} channels")));
    }
    Ok(bitrate)
}

fn encode_opus_buffer(encoder: &mut OpusEncoder, audio: &AudioSampleBuffer, pending: &mut Vec<f32>) -> Result<Vec<Vec<u8>>, EncoderError> {
    let channels = audio.channel_count();
    let frame_len =
        OpusEncoder::FRAME_SIZE.checked_mul(channels).ok_or_else(|| EncoderError::Internal("overflow calculating Opus frame size".into()))?;
    let mut packets = Vec::new();
    for sample_index in 0..audio.samples_per_channel() {
        for channel in 0..channels {
            let sample = audio
                .channel(channel)?
                .get(sample_index)
                .copied()
                .ok_or_else(|| EncoderError::InvalidJobSpec("audio channels have inconsistent sample counts".into()))?;
            pending.push(if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 });
        }
        if pending.len() == frame_len {
            packets.push(encoder.encode_float(pending));
            pending.clear();
        }
    }
    Ok(packets)
}

fn samples_to_micros(samples: u64) -> Result<i64, EncoderError> {
    let micros =
        u128::from(samples).checked_mul(1_000_000).ok_or_else(|| EncoderError::Internal("overflow calculating Opus timestamp".into()))? / 48_000;
    i64::try_from(micros).map_err(|_| EncoderError::InvalidJobSpec("Opus timestamp exceeds the container limit".into()))
}

impl TranscodePipeline {
    /// Creates a new pipeline for the specified job specification.
    pub fn new(spec: CraftJobSpec) -> Self {
        Self::with_limits(spec, PipelineLimits::default())
    }

    /// Creates a pipeline whose queues and whole-track audio stay within `limits`.
    pub fn with_limits(spec: CraftJobSpec, limits: PipelineLimits) -> Self {
        let progress = Arc::new(JobProgress::new(100));
        Self { spec, progress, limits, stats: Mutex::new(None) }
    }

    /// Queue depth and peak frames in flight of the last video stream this pipeline ran.
    pub fn last_stream_stats(&self) -> Option<StreamStats> {
        *self.stats.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn frame_processor(&self, target: &OutputTarget, fps: Rational64) -> Result<FrameProcessor, EncoderError> {
        let lut = match &target.lut_path {
            Some(path) => {
                let content = std::fs::read_to_string(path).map_err(|e| EncoderError::Io(format!("failed to read LUT file: {e}")))?;
                Some(crate::lut::Lut3D::parse_cube(&content)?)
            }
            None => None,
        };
        let timecode = target.burn_timecode.then(crate::overlay::TimecodeOverlay::default);
        let captions = match (target.burn_captions, &target.caption_path) {
            (true, Some(path)) => Some((crate::captions::CaptionBurner::default(), crate::captions::CaptionTrack::from_file(path)?)),
            _ => None,
        };
        Ok(FrameProcessor {
            scaler: FrameScaler::with_filter(target.transform.scale_mode, target.transform.scaling_filter),
            color: ColorTransform::new(target.video.color),
            lut,
            timecode,
            captions,
            fps,
        })
    }

    /// Streams the input through decode, convert and `sink` (encode and mux) with bounded memory.
    fn stream_video<K>(&self, input: &VideoInput, processor: &FrameProcessor, start_time: Instant, sink: K) -> Result<(), EncoderError>
    where
        K: FnMut(u64, &mut VideoFrame) -> Result<(), EncoderError>,
    {
        let dims = input.dims();
        let out_bytes = (dims.width as usize).saturating_mul(dims.height as usize).saturating_mul(4);
        let spec = StreamSpec {
            count: input.len(),
            frame_bytes: input.source_frame_bytes().max(out_bytes),
            limits: self.limits,
            progress: &self.progress,
            start_time,
        };
        let stats = run_stages(
            &spec,
            dims,
            |i| {
                let mut frame = input.frame(i)?;
                processor.color.apply(&mut frame)?;
                Ok(frame)
            },
            |i, src, out| processor.apply(i, src, out),
            sink,
        )?;
        *self.stats.lock().unwrap_or_else(PoisonError::into_inner) = Some(stats);
        Ok(())
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

        let _pipe_stream = PipeStreamGuard::new(&self.spec.source);
        let count = self.spec.targets.len();
        self.progress.set_target_count(count as u64);
        for (index, target) in self.spec.targets.iter().enumerate() {
            self.progress.begin_target_at(index as u64, start_time);
            self.execute_target(target, start_time).map_err(|e| match e {
                EncoderError::Cancelled => e,
                other => EncoderError::Internal(format!(
                    "target {} of {count} ('{}' -> {}) failed: {other}",
                    index + 1,
                    target.preset_name,
                    target.output_path.display()
                )),
            })?;
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
            ContainerType::AnimatedGif => {
                let is_still = match &self.spec.source {
                    SourceDescriptor::File { path, .. } => {
                        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
                        encodercraft_source::is_still_extension(&ext) && !encodercraft_source::is_sequence_pattern(path)
                    }
                    _ => false,
                };
                if is_still || FrameNaming::parse(&target.output_path).is_some() {
                    self.execute_image_transcode(target, start_time)?
                } else {
                    self.execute_animated_gif(target, start_time)?
                }
            }
            ContainerType::Svg | ContainerType::PostScriptEps | ContainerType::AdobeIllustratorAi | ContainerType::ArtcraftVectorcraft => {
                self.execute_vector_transcode(target, start_time)?
            }
            ContainerType::Pdf | ContainerType::Docx => self.execute_document_pdf(target, start_time)?,
            ContainerType::Idml | ContainerType::AdobeInDesignIndd | ContainerType::ArtcraftDesigncraft => {
                self.execute_document_publication(target, start_time)?
            }
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
            ContainerType::AdobePhotoshopPsd | ContainerType::AdobePhotoshopPsb | ContainerType::ArtcraftPhotocraft => {
                self.execute_layered_image(target, start_time)?
            }
            ContainerType::AutocadDxf | ContainerType::AutocadDwg | ContainerType::HpglPlotter | ContainerType::ArtcraftCadcraft => {
                self.execute_cad_transcode(target, start_time)?
            }
            ContainerType::AdobeAfterEffectsAep | ContainerType::ArtcraftEffectcraft => self.execute_motion_project(target, start_time)?,
            ContainerType::AdobePremierePrproj | ContainerType::ArtcraftFilmcraft => self.execute_timeline_project(target, start_time)?,
            ContainerType::AdobeAuditionSesx | ContainerType::ArtcraftSoundcraft => self.execute_daw_session(target, start_time)?,
            ContainerType::ArtcraftAnicraft => self.execute_image_transcode(target, start_time)?,
        }

        // Export multichannel audio stems if configured (for non-DAW sources; DAW sessions export stems directly in execute_daw_session)
        if let Some(mode) = target.audio_stems
            && mode != encodercraft_core::AudioStemMode::None
            && !self.is_daw_source()
        {
            self.export_audio_stems(target, mode)?;
        }

        Ok(())
    }

    fn is_daw_source(&self) -> bool {
        match &self.spec.source {
            SourceDescriptor::File { path, .. } => {
                if let Some(ext) = path.extension().and_then(|s| s.to_str())
                    && (ext.eq_ignore_ascii_case("sesx") || ext.eq_ignore_ascii_case("soundcraft"))
                {
                    return true;
                }
                let mut head = Vec::with_capacity(512);
                if let Ok(file) = File::open(path)
                    && std::io::Read::read_to_end(&mut std::io::Read::take(file, 512), &mut head).is_ok()
                {
                    if head.starts_with(b"\x89CRAFT\x00\x01S") {
                        return true;
                    }
                    let text = String::from_utf8_lossy(&head);
                    if text.contains("<sesx") || text.contains("<session") {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn load_source_audio(&self, target: &OutputTarget) -> Result<AudioSampleBuffer, EncoderError> {
        load_audio_required(&self.spec.source, target)
    }

    fn execute_audio_flac(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        if self.is_daw_source() {
            return self.execute_daw_session(target, start_time);
        }

        let sample_buf = prepare_audio_for_target(self.load_source_audio(target)?, target)?;

        let flac_bytes = encodercraft_audio_dsp::encode_audio_flac(&sample_buf)?;
        std::fs::write(&target.output_path, flac_bytes).map_err(|e| EncoderError::Io(format!("failed to write FLAC file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_audio_aiff(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        if self.is_daw_source() {
            return self.execute_daw_session(target, start_time);
        }

        let sample_buf = prepare_audio_for_target(self.load_source_audio(target)?, target)?;

        let aiff_bytes = encodercraft_audio_dsp::encode_audio_aiff(&sample_buf)?;
        std::fs::write(&target.output_path, aiff_bytes).map_err(|e| EncoderError::Io(format!("failed to write AIFF file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_audio_wav(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        if self.is_daw_source() {
            return self.execute_daw_session(target, start_time);
        }

        let out_file = File::create(&target.output_path).map_err(|e| EncoderError::Io(format!("failed to create output file: {e}")))?;
        let writer = BufWriter::new(out_file);

        let bext = BextMetadata {
            description: format!("Encoded by encoderCraft - {}", target.preset_name),
            originator: "encoderCraft".into(),
            ..Default::default()
        };

        let sample_buf = prepare_audio_for_target(self.load_source_audio(target)?, target)?;
        let sample_rate = sample_buf.sample_rate;
        let xmp_xml = target.metadata.as_ref().map(|m| m.to_xmp_packet());
        let format = match target.audio.as_ref().and_then(|audio| audio.codec.as_deref()) {
            Some("pcm_s24le" | "pcm_s24be") => WavFormatTag::Pcm24,
            Some("pcm_f32le" | "pcm_f32be") => WavFormatTag::IeeeFloat32,
            _ => WavFormatTag::Pcm16,
        };
        let channels = u16::try_from(sample_buf.channel_count())
            .map_err(|_| EncoderError::InvalidJobSpec("audio channel count exceeds the WAV format limit".into()))?;
        let mut wav_writer = BwfWavWriter::new_with_xmp(writer, sample_rate, channels, format, Some(&bext), xmp_xml.as_deref())?;
        wav_writer.write_buffer(&sample_buf)?;
        wav_writer.finish()?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_vector_transcode(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        // Direct SVG passthrough and CAD-to-SVG specialization
        if target.container == ContainerType::Svg
            && let SourceDescriptor::File { path, .. } = &self.spec.source
            && let Ok(bytes) = std::fs::read(path)
        {
            if let Ok(cad) = encodercraft_spatial_io::CadDrawing::from_bytes_auto(&bytes) {
                let svg = cad.to_svg();
                std::fs::write(&target.output_path, svg.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write SVG file: {e}")))?;
                self.progress.advance_frame(start_time);
                self.progress.set_total_frames(1);
                return Ok(());
            } else if let Ok(text) = std::str::from_utf8(&bytes)
                && text.contains("<svg")
            {
                std::fs::write(&target.output_path, text.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write SVG file: {e}")))?;
                self.progress.advance_frame(start_time);
                self.progress.set_total_frames(1);
                return Ok(());
            }
        }

        let mut canvas = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes =
                    std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source vector file `{}`: {e}", path.display())))?;
                if bytes.starts_with(b"\x89CRAFT\x00\x01V") {
                    encodercraft_image_codecs::VectorCanvas::from_vectorcraft(&bytes)
                        .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse .vectorcraft: {e}")))?
                } else if bytes.starts_with(b"%PDF-") || bytes.starts_with(b"%AI") || bytes.windows(26).any(|w| w == b"Creator: Adobe Illustrator") {
                    encodercraft_image_codecs::VectorCanvas::from_ai_or_pdf(&bytes)
                        .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse Illustrator AI / PDF: {e}")))?
                } else if bytes.starts_with(b"%!PS") {
                    let text = String::from_utf8_lossy(&bytes);
                    encodercraft_image_codecs::VectorCanvas::from_eps(&text)
                        .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse EPS: {e}")))?
                } else if let Ok(cad) = encodercraft_spatial_io::CadDrawing::from_bytes_auto(&bytes) {
                    let (min_x, min_y, max_x, max_y) = cad.bounding_box();
                    let w = if (max_x - min_x).is_finite() && (max_x - min_x) > 0.0 { max_x - min_x } else { 800.0 };
                    let h = if (max_y - min_y).is_finite() && (max_y - min_y) > 0.0 { max_y - min_y } else { 600.0 };
                    let mut c = encodercraft_image_codecs::VectorCanvas::new(w, h);
                    for ent in &cad.entities {
                        match ent {
                            encodercraft_spatial_io::CadEntity::Line { start, end, .. } => {
                                let mut path = encodercraft_image_codecs::VectorPath::new()
                                    .with_stroke(encodercraft_image_codecs::VectorStroke::new(encodercraft_image_codecs::VectorColor::BLACK, 1.0));
                                path.move_to(start.x, start.y);
                                path.line_to(end.x, end.y);
                                c.add_path(path);
                            }
                            encodercraft_spatial_io::CadEntity::Polyline { points, closed, .. } => {
                                let mut path = encodercraft_image_codecs::VectorPath::new()
                                    .with_stroke(encodercraft_image_codecs::VectorStroke::new(encodercraft_image_codecs::VectorColor::BLACK, 1.0));
                                for (i, pt) in points.iter().enumerate() {
                                    if i == 0 {
                                        path.move_to(pt.x, pt.y);
                                    } else {
                                        path.line_to(pt.x, pt.y);
                                    }
                                }
                                if *closed {
                                    path.close();
                                }
                                c.add_path(path);
                            }
                            _ => {}
                        }
                    }
                    c
                } else if let Ok(text) = std::str::from_utf8(&bytes) {
                    if text.contains("<svg") {
                        encodercraft_image_codecs::VectorCanvas::from_svg(text)
                            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse SVG: {e}")))?
                    } else {
                        let mut c = encodercraft_image_codecs::VectorCanvas::new(1920.0, 1080.0);
                        c.add_text(encodercraft_image_codecs::VectorText {
                            text: path.file_name().and_then(|n| n.to_str()).unwrap_or("vector").to_string(),
                            x: 50.0,
                            y: 100.0,
                            font_family: "sans-serif".to_string(),
                            font_size: 24.0,
                            fill: Some(encodercraft_image_codecs::VectorColor::BLACK),
                            stroke: None,
                        });
                        c
                    }
                } else {
                    encodercraft_image_codecs::VectorCanvas::new(1920.0, 1080.0)
                }
            }
            _ => encodercraft_image_codecs::VectorCanvas::new(1920.0, 1080.0),
        };

        // Text preservation and outlines
        canvas.text_to_outlines();

        match target.container {
            ContainerType::ArtcraftVectorcraft => {
                let bytes = canvas.to_vectorcraft().map_err(|e| EncoderError::Io(format!("failed to serialize .vectorcraft: {e}")))?;
                std::fs::write(&target.output_path, &bytes).map_err(|e| EncoderError::Io(format!("failed to write .vectorcraft file: {e}")))?;
            }
            ContainerType::AdobeIllustratorAi | ContainerType::Pdf => {
                let ai_pdf = canvas.to_ai().map_err(|e| EncoderError::Io(format!("failed to serialize Illustrator AI/PDF: {e}")))?;
                std::fs::write(&target.output_path, &ai_pdf).map_err(|e| EncoderError::Io(format!("failed to write Illustrator AI file: {e}")))?;
            }
            ContainerType::PostScriptEps => {
                let eps = canvas.to_eps();
                std::fs::write(&target.output_path, eps.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write EPS file: {e}")))?;
            }
            ContainerType::AutocadDxf => {
                let mut cad = encodercraft_spatial_io::CadDrawing::new("VectorCraft Export");
                for path in &canvas.paths {
                    let mut cur_x = 0.0;
                    let mut cur_y = 0.0;
                    for cmd in &path.commands {
                        match *cmd {
                            encodercraft_image_codecs::PathCommand::MoveTo(x, y) => {
                                cur_x = x;
                                cur_y = y;
                            }
                            encodercraft_image_codecs::PathCommand::LineTo(x, y) => {
                                cad.add_line(
                                    encodercraft_spatial_io::CadPoint3D::new(cur_x, cur_y, 0.0),
                                    encodercraft_spatial_io::CadPoint3D::new(x, y, 0.0),
                                    "0",
                                );
                                cur_x = x;
                                cur_y = y;
                            }
                            encodercraft_image_codecs::PathCommand::CubicTo(_, _, _, _, x, y)
                            | encodercraft_image_codecs::PathCommand::QuadTo(_, _, x, y) => {
                                cad.add_line(
                                    encodercraft_spatial_io::CadPoint3D::new(cur_x, cur_y, 0.0),
                                    encodercraft_spatial_io::CadPoint3D::new(x, y, 0.0),
                                    "0",
                                );
                                cur_x = x;
                                cur_y = y;
                            }
                            encodercraft_image_codecs::PathCommand::Close => {}
                        }
                    }
                }
                let dxf_str = cad.to_dxf_ascii();
                std::fs::write(&target.output_path, dxf_str.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write DXF file: {e}")))?;
            }
            _ => {
                // Default to standard SVG
                let svg = canvas.to_svg();
                std::fs::write(&target.output_path, svg.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write SVG file: {e}")))?;
            }
        }

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_cad_transcode(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mut drawing = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source CAD file `{}`: {e}", path.display())))?;
                if let Ok(cad) = encodercraft_spatial_io::CadDrawing::from_bytes_auto(&bytes) {
                    cad
                } else if let Ok(canvas) = encodercraft_image_codecs::VectorCanvas::from_svg(&String::from_utf8_lossy(&bytes))
                    .or_else(|_| encodercraft_image_codecs::VectorCanvas::from_vectorcraft(&bytes))
                    .or_else(|_| encodercraft_image_codecs::VectorCanvas::from_ai_or_pdf(&bytes))
                {
                    let mut cad = encodercraft_spatial_io::CadDrawing::new("VectorCraft CAD Export");
                    for path in &canvas.paths {
                        let mut cur_x = 0.0;
                        let mut cur_y = 0.0;
                        for cmd in &path.commands {
                            match *cmd {
                                encodercraft_image_codecs::PathCommand::MoveTo(x, y) => {
                                    cur_x = x;
                                    cur_y = y;
                                }
                                encodercraft_image_codecs::PathCommand::LineTo(x, y) => {
                                    cad.add_line(
                                        encodercraft_spatial_io::CadPoint3D::new(cur_x, cur_y, 0.0),
                                        encodercraft_spatial_io::CadPoint3D::new(x, y, 0.0),
                                        "0",
                                    );
                                    cur_x = x;
                                    cur_y = y;
                                }
                                encodercraft_image_codecs::PathCommand::CubicTo(_, _, _, _, x, y)
                                | encodercraft_image_codecs::PathCommand::QuadTo(_, _, x, y) => {
                                    cad.add_line(
                                        encodercraft_spatial_io::CadPoint3D::new(cur_x, cur_y, 0.0),
                                        encodercraft_spatial_io::CadPoint3D::new(x, y, 0.0),
                                        "0",
                                    );
                                    cur_x = x;
                                    cur_y = y;
                                }
                                encodercraft_image_codecs::PathCommand::Close => {}
                            }
                        }
                    }
                    cad
                } else {
                    encodercraft_spatial_io::CadDrawing::from_bytes_auto(&bytes)?
                }
            }
            _ => {
                let mut cad = encodercraft_spatial_io::CadDrawing::new("Default Drawing");
                cad.add_line(
                    encodercraft_spatial_io::CadPoint3D::new(0.0, 0.0, 0.0),
                    encodercraft_spatial_io::CadPoint3D::new(100.0, 100.0, 0.0),
                    "0",
                );
                cad
            }
        };

        if let Some(layers) = &target.selection.layers {
            drawing.retain_layers(layers)?;
        }

        match target.container {
            ContainerType::AutocadDxf | ContainerType::AutocadDwg => {
                let dxf = drawing.to_dxf_ascii();
                std::fs::write(&target.output_path, dxf.as_bytes())
                    .map_err(|e| EncoderError::Io(format!("failed to write CAD DXF output file: {e}")))?;
            }
            ContainerType::HpglPlotter => {
                let hpgl = drawing.to_hpgl();
                std::fs::write(&target.output_path, hpgl.as_bytes())
                    .map_err(|e| EncoderError::Io(format!("failed to write HPGL plotter file: {e}")))?;
            }
            ContainerType::ArtcraftCadcraft => {
                let bin = drawing.to_cadcraft_binary()?;
                std::fs::write(&target.output_path, &bin).map_err(|e| EncoderError::Io(format!("failed to write .cadcraft binary file: {e}")))?;
            }
            ContainerType::Svg => {
                let svg = drawing.to_svg();
                std::fs::write(&target.output_path, svg.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write SVG file: {e}")))?;
            }
            ContainerType::Pdf => {
                let pdf = drawing.to_pdf_bytes()?;
                std::fs::write(&target.output_path, &pdf).map_err(|e| EncoderError::Io(format!("failed to write PDF blueprint file: {e}")))?;
            }
            _ => {
                let dxf = drawing.to_dxf_ascii();
                std::fs::write(&target.output_path, dxf.as_bytes())
                    .map_err(|e| EncoderError::Io(format!("failed to write default CAD file: {e}")))?;
            }
        }

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
        if let Some(naming) = FrameNaming::parse(&target.output_path) {
            return self.execute_frame_export(target, &naming, start_time);
        }
        let reader = crate::input::open_reader(&self.spec.source)?;
        let frame = reader.frame_at_index(0)?;
        self.write_still(target, frame, &target.output_path)?;
        self.progress.set_total_frames(1);
        self.progress.advance_frame(start_time);
        Ok(())
    }

    /// Exports every frame of the (trimmed) source as `name_0001.png`, `name_0002.png`, ... for an
    /// output path such as `name_%04d.png`. Files written so far are removed if a frame fails.
    fn execute_frame_export(&self, target: &OutputTarget, naming: &FrameNaming, start_time: Instant) -> Result<(), EncoderError> {
        let input = crate::input::VideoInput::open(&self.spec.source, target)?;
        self.progress.set_total_frames(input.len());
        let mut written: Vec<PathBuf> = Vec::new();
        let result = (|| {
            for i in 0..input.len() {
                self.progress.checkpoint()?;
                let path = naming.path(i + 1);
                self.write_still(target, input.frame(i)?, &path)?;
                written.push(path);
                self.progress.advance_frame(start_time);
            }
            Ok(())
        })();
        if result.is_err() {
            for path in &written {
                let _ = std::fs::remove_file(path);
            }
        }
        result
    }

    fn write_still(&self, target: &OutputTarget, frame: VideoFrame, path: &Path) -> Result<(), EncoderError> {
        let final_frame = if let (Some(w), Some(h)) = (target.custom_width, target.custom_height) {
            if w != frame.dimensions.width || h != frame.dimensions.height {
                FrameScaler::new(ScaleMode::Fit).scale(&frame, Dimensions::new(w, h)?)?
            } else {
                frame
            }
        } else {
            frame
        };

        let (format, mut options) = match target.container {
            ContainerType::Png | ContainerType::StillSequence => (StillFormat::Png, StillOptions::default()),
            ContainerType::Tiff => (StillFormat::Tiff, StillOptions::default()),
            ContainerType::OpenExr => (StillFormat::OpenExr, StillOptions::default()),
            ContainerType::Bmp => (StillFormat::Bmp, StillOptions::default()),
            ContainerType::Jpeg => (StillFormat::Jpeg, StillOptions::default()),
            ContainerType::WebP => (StillFormat::WebP, StillOptions { webp_lossless: target.image_quality.is_none(), ..StillOptions::default() }),
            ContainerType::AnimatedGif => (StillFormat::Gif, StillOptions::default()),
            other => return Err(EncoderError::UnsupportedCodec(format!("{other:?} is not a still-image container"))),
        };
        if let Some(q) = target.image_quality {
            options.quality = q.clamp(1, 100);
        }
        let bytes = encode_still(&final_frame, format, &options)?;
        std::fs::write(path, bytes).map_err(|e| EncoderError::Io(format!("failed to write {}: {e}", path.display())))
    }

    fn execute_document_pdf(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let pages = target.selection.pages;
        let no_pages = |what: &str| -> Result<(), EncoderError> {
            if pages.is_some() {
                return Err(EncoderError::InvalidJobSpec(format!("a page range cannot be applied to {what}; it has no selectable pages")));
            }
            Ok(())
        };
        let pdf_bytes = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes =
                    std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source document file `{}`: {e}", path.display())))?;
                if bytes.starts_with(b"%PDF-") {
                    no_pages("a PDF source")?;
                    bytes
                } else if let Ok(cad) = encodercraft_spatial_io::CadDrawing::from_bytes_auto(&bytes) {
                    no_pages("a CAD drawing")?;
                    cad.to_pdf_bytes()?
                } else if let Ok(mut doc) = crate::document::DocumentCanvas::from_bytes_auto(&bytes) {
                    if let Some((first, last)) = pages {
                        doc.select_pages(first, last)?;
                    }
                    doc.to_pdf_x4_bytes()?
                } else {
                    no_pages("this source")?;
                    generate_minimal_pdf(&format!("Document export: {}", path.file_name().and_then(|n| n.to_str()).unwrap_or("document")))
                }
            }
            _ => {
                no_pages("a missing source")?;
                generate_minimal_pdf("Exported Document")
            }
        };

        std::fs::write(&target.output_path, &pdf_bytes).map_err(|e| EncoderError::Io(format!("failed to write PDF file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_document_publication(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mut doc = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes =
                    std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source document file `{}`: {e}", path.display())))?;
                if let Ok(canvas) = crate::document::DocumentCanvas::from_bytes_auto(&bytes) {
                    canvas
                } else {
                    let mut canvas = crate::document::DocumentCanvas::new(path.file_stem().and_then(|s| s.to_str()).unwrap_or("Document"));
                    if let Ok(text) = std::str::from_utf8(&bytes) {
                        canvas.pages[0].add_text(54.0, 720.0, 500.0, 20.0, text.lines().next().unwrap_or("Document Item"), 14.0);
                    }
                    canvas
                }
            }
            _ => crate::document::DocumentCanvas::new("Exported Document"),
        };
        if let Some((first, last)) = target.selection.pages {
            doc.select_pages(first, last)?;
        }

        match target.container {
            ContainerType::ArtcraftDesigncraft => {
                let bin = doc.to_designcraft_binary()?;
                std::fs::write(&target.output_path, &bin).map_err(|e| EncoderError::Io(format!("failed to write .designcraft file: {e}")))?;
            }
            ContainerType::Idml | ContainerType::AdobeInDesignIndd => {
                let idml = doc.to_idml_bytes()?;
                std::fs::write(&target.output_path, &idml).map_err(|e| EncoderError::Io(format!("failed to write IDML package: {e}")))?;
            }
            ContainerType::Pdf => {
                let pdf = doc.to_pdf_x4_bytes()?;
                std::fs::write(&target.output_path, &pdf).map_err(|e| EncoderError::Io(format!("failed to write PDF/X-4 document: {e}")))?;
            }
            _ => {
                let pdf = doc.to_pdf_x4_bytes()?;
                std::fs::write(&target.output_path, &pdf).map_err(|e| EncoderError::Io(format!("failed to write default publication file: {e}")))?;
            }
        }

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_layered_image(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mut image = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes =
                    std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source layered image `{}`: {e}", path.display())))?;
                encodercraft_image_codecs::LayeredImage::from_bytes_auto(&bytes)?
            }
            _ => {
                let mut img = encodercraft_image_codecs::LayeredImage::new("Default Composition", 100, 100);
                let layer = encodercraft_image_codecs::ImageLayer::new("Background", 100, 100, vec![200; 100 * 100 * 4]);
                img.add_layer(layer);
                img
            }
        };

        if let Some(layers) = &target.selection.layers {
            image.select_layers(layers)?;
        }

        match target.container {
            ContainerType::ArtcraftPhotocraft => {
                let bin = image.to_photocraft_binary()?;
                std::fs::write(&target.output_path, &bin).map_err(|e| EncoderError::Io(format!("failed to write .photocraft file: {e}")))?;
            }
            ContainerType::AdobePhotoshopPsb => {
                let psb = image.to_psd_bytes(true)?;
                std::fs::write(&target.output_path, &psb).map_err(|e| EncoderError::Io(format!("failed to write PSB file: {e}")))?;
            }
            ContainerType::AdobePhotoshopPsd => {
                let psd = image.to_psd_bytes(false)?;
                std::fs::write(&target.output_path, &psd).map_err(|e| EncoderError::Io(format!("failed to write PSD file: {e}")))?;
            }
            ContainerType::OpenExr => {
                let dims = encodercraft_geom::Dimensions::new(image.width, image.height)?;
                let frame = encodercraft_core::VideoFrame::from_raw_bytes(
                    dims,
                    encodercraft_color::PixelFormat::Rgba8,
                    encodercraft_color::ColorSpaceTag::SRGB,
                    0,
                    1,
                    encodercraft_core::Rational64::FPS_24,
                    image.composite_rgba,
                )?;
                let exr_bytes = encodercraft_image_codecs::encode_exr_half(&frame)?;
                std::fs::write(&target.output_path, &exr_bytes).map_err(|e| EncoderError::Io(format!("failed to write EXR file: {e}")))?;
            }
            _ => {
                let psd = image.to_psd_bytes(false)?;
                std::fs::write(&target.output_path, &psd).map_err(|e| EncoderError::Io(format!("failed to write default layered file: {e}")))?;
            }
        }

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_motion_project(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mut project = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes =
                    std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source motion file `{}`: {e}", path.display())))?;
                if bytes.starts_with(b"\x89CRAFT\x00\x01E") {
                    crate::project::MotionProject::from_effectcraft_binary(&bytes)?
                } else if bytes.starts_with(b"RIFX") {
                    crate::project::MotionProject::from_aep(&bytes)?
                } else {
                    let mut proj = crate::project::MotionProject::new("Default Motion Project");
                    proj.add_comp(crate::project::MotionComp::new("Comp 1", 1920, 1080, 24.0, 120));
                    proj
                }
            }
            _ => {
                let mut proj = crate::project::MotionProject::new("Default Motion Project");
                proj.add_comp(crate::project::MotionComp::new("Comp 1", 1920, 1080, 24.0, 120));
                proj
            }
        };

        if let Some(key) = &target.selection.sequence {
            project.select_comp(key)?;
        }

        match target.container {
            ContainerType::ArtcraftEffectcraft => {
                let bin = project.to_effectcraft_binary()?;
                std::fs::write(&target.output_path, &bin).map_err(|e| EncoderError::Io(format!("failed to write .effectcraft file: {e}")))?;
            }
            ContainerType::AdobeAfterEffectsAep => {
                let mut out = Vec::new();
                out.extend_from_slice(b"RIFX");
                let comp = project.comps.first();
                let w = comp.map(|c| c.width).unwrap_or(1920);
                let h = comp.map(|c| c.height).unwrap_or(1080);
                let mut chunk = Vec::new();
                chunk.extend_from_slice(b"Egg!");
                chunk.extend_from_slice(b"Comp");
                chunk.extend_from_slice(&8u32.to_be_bytes());
                chunk.extend_from_slice(&w.to_be_bytes());
                chunk.extend_from_slice(&h.to_be_bytes());
                out.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
                out.extend_from_slice(&chunk);
                std::fs::write(&target.output_path, &out).map_err(|e| EncoderError::Io(format!("failed to write AEP file: {e}")))?;
            }
            _ => {
                let lottie = project.to_lottie_json()?;
                std::fs::write(&target.output_path, lottie.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write motion file: {e}")))?;
            }
        }

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_timeline_project(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mut project = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source NLE file `{}`: {e}", path.display())))?;
                if bytes.starts_with(b"\x89CRAFT\x00\x01F") {
                    crate::project::TimelineProject::from_filmcraft_binary(&bytes)?
                } else if bytes.starts_with(&[0x1f, 0x8b]) || bytes.windows(13).any(|w| w == b"<PremiereData") {
                    crate::project::TimelineProject::from_prproj(&bytes)?
                } else {
                    let mut proj = crate::project::TimelineProject::new("Default Timeline");
                    proj.add_sequence(crate::project::TimelineSequence::new("Sequence 1", 1920, 1080, 24.0));
                    proj
                }
            }
            _ => {
                let mut proj = crate::project::TimelineProject::new("Default Timeline");
                proj.add_sequence(crate::project::TimelineSequence::new("Sequence 1", 1920, 1080, 24.0));
                proj
            }
        };

        if let Some(key) = &target.selection.sequence {
            project.select_sequence(key)?;
        }

        let ext = target.output_path.extension().and_then(|s| s.to_str()).unwrap_or("");
        match target.container {
            ContainerType::ArtcraftFilmcraft => {
                let bin = project.to_filmcraft_binary()?;
                std::fs::write(&target.output_path, &bin).map_err(|e| EncoderError::Io(format!("failed to write .filmcraft file: {e}")))?;
            }
            ContainerType::AdobePremierePrproj if ext == "fcpxml" => {
                let fcpxml = project.to_fcpxml();
                std::fs::write(&target.output_path, fcpxml.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write FCPXML file: {e}")))?;
            }
            ContainerType::AdobePremierePrproj if ext == "otio" => {
                let otio = project.to_otio_json()?;
                std::fs::write(&target.output_path, otio.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write OTIO file: {e}")))?;
            }
            ContainerType::AdobePremierePrproj => {
                let gzip = project.to_prproj();
                std::fs::write(&target.output_path, &gzip).map_err(|e| EncoderError::Io(format!("failed to write PRPROJ file: {e}")))?;
            }
            _ => {
                let otio = project.to_otio_json()?;
                std::fs::write(&target.output_path, otio.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write OTIO file: {e}")))?;
            }
        }

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_daw_session(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let session = match &self.spec.source {
            SourceDescriptor::File { path, .. } if path.exists() => {
                let bytes =
                    std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read source DAW session file `{}`: {e}", path.display())))?;
                if bytes.starts_with(b"\x89CRAFT\x00\x01S") {
                    encodercraft_audio_dsp::DawSession::from_soundcraft_binary(&bytes)?
                } else {
                    let text = std::str::from_utf8(&bytes)
                        .map_err(|_| EncoderError::InvalidJobSpec("DAW session file did not contain valid UTF-8 XML".into()))?;
                    encodercraft_audio_dsp::DawSession::from_sesx_xml(text)?
                }
            }
            _ => {
                return Err(EncoderError::InvalidJobSpec("DAW session source file is missing".into()));
            }
        };

        let session_dir = match &self.spec.source {
            SourceDescriptor::File { path, .. } => path.parent().map(std::path::Path::to_path_buf),
            _ => None,
        };

        if let Some(parent) = target.output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| EncoderError::Io(format!("failed to create output directory `{}`: {e}", parent.display())))?;
        }

        match target.container {
            ContainerType::ArtcraftSoundcraft => {
                let binary = session.to_soundcraft_binary()?;
                std::fs::write(&target.output_path, binary).map_err(|e| EncoderError::Io(format!("failed to write .soundcraft file: {e}")))?;
            }
            ContainerType::AdobeAuditionSesx => {
                let xml = session.to_sesx_xml();
                std::fs::write(&target.output_path, xml.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write .sesx XML file: {e}")))?;
            }
            ContainerType::Wave => {
                let bext = BextMetadata {
                    description: format!("Encoded by encoderCraft - {}", target.preset_name),
                    originator: "encoderCraft".into(),
                    ..Default::default()
                };

                let xmp_xml = target.metadata.as_ref().map(|m| m.to_xmp_packet());
                let ixml_xml = session.to_ixml(None);

                let wav_format = match session.bit_depth {
                    0..=16 => WavFormatTag::Pcm16,
                    17..=24 => WavFormatTag::Pcm24,
                    _ => WavFormatTag::IeeeFloat32,
                };
                let target_lufs = target.target_lufs.map(|l| l as f64);
                let mut renderer = session.renderer(session_dir.as_deref())?;
                let master_buf = renderer.render_master(target_lufs)?;

                let out_file = File::create(&target.output_path).map_err(|e| EncoderError::Io(format!("failed to create output WAV file: {e}")))?;
                let writer = BufWriter::new(out_file);

                let mut wav_writer =
                    BwfWavWriter::new_with_metadata(writer, session.sample_rate, 2, wav_format, Some(&bext), xmp_xml.as_deref(), Some(&ixml_xml))?;

                wav_writer.write_buffer(&master_buf)?;
                wav_writer.finish()?;

                // If stems requested, export track stems with iXML metadata
                if let Some(mode) = target.audio_stems
                    && mode != encodercraft_core::AudioStemMode::None
                {
                    let parent = target.output_path.parent().unwrap_or_else(|| std::path::Path::new("."));
                    let base_stem = target.output_path.file_stem().and_then(|s| s.to_str()).unwrap_or("stem");

                    for track_index in renderer.stem_track_indices() {
                        let (stem_name, stem_buf) = renderer.render_stem(track_index)?;
                        let track_name = session.tracks.get(track_index).map(|t| t.name.as_str());
                        let stem_ixml = session.to_ixml(track_name);

                        let mut outputs: Vec<(String, AudioSampleBuffer)> = Vec::new();
                        if mode == encodercraft_core::AudioStemMode::DiscreteMono && stem_buf.channel_count() == 2 {
                            for (suffix, ch) in [("L", 0usize), ("R", 1usize)] {
                                let plane = stem_buf.channel(ch)?.to_vec();
                                let mono = AudioSampleBuffer::from_planar_channels(
                                    stem_buf.sample_rate,
                                    encodercraft_core::ChannelLayout::Mono,
                                    0,
                                    vec![plane],
                                )?;
                                outputs.push((format!("{stem_name}_{suffix}"), mono));
                            }
                        } else {
                            outputs.push((stem_name, stem_buf));
                        }

                        for (name, buf) in outputs {
                            let stem_path = parent.join(format!("{base_stem}_Stem_{name}.wav"));
                            let stem_file =
                                File::create(&stem_path).map_err(|e| EncoderError::Io(format!("failed to create audio stem file: {e}")))?;
                            let mut s_writer = BwfWavWriter::new_with_metadata(
                                BufWriter::new(stem_file),
                                session.sample_rate,
                                buf.channel_count() as u16,
                                wav_format,
                                Some(&bext),
                                None,
                                Some(&stem_ixml),
                            )?;
                            s_writer.write_buffer(&buf)?;
                            s_writer.finish()?;
                        }
                    }
                }
            }
            ContainerType::Flac => {
                let master_buf = session.renderer(session_dir.as_deref())?.render_master(target.target_lufs.map(|l| l as f64))?;
                let flac_bytes = encodercraft_audio_dsp::encode_audio_flac(&master_buf)?;
                std::fs::write(&target.output_path, flac_bytes).map_err(|e| EncoderError::Io(format!("failed to write FLAC file: {e}")))?;
            }
            ContainerType::Aiff => {
                let master_buf = session.renderer(session_dir.as_deref())?.render_master(target.target_lufs.map(|l| l as f64))?;
                let aiff_bytes = encodercraft_audio_dsp::encode_audio_aiff(&master_buf)?;
                std::fs::write(&target.output_path, aiff_bytes).map_err(|e| EncoderError::Io(format!("failed to write AIFF file: {e}")))?;
            }
            _ => {
                return Err(EncoderError::InvalidJobSpec(format!("unsupported export container {:?} for DAW session", target.container)));
            }
        }

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);
        Ok(())
    }

    fn execute_video_mp4(&self, target: &OutputTarget, is_mov: bool, start_time: Instant) -> Result<(), EncoderError> {
        let input = VideoInput::open(&self.spec.source, target)?;
        let dims = input.dims();
        let fps = input.fps();
        let frame_ticks = u32::try_from(fps.den).map_err(|_| EncoderError::InvalidJobSpec("frame rate denominator is too large".into()))?;

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
        let codec_kind = resolve_codec(target, codec_kind)?;
        let pixel_format = target_pixel_format(target)?;

        let mut encoder = create_video_encoder_with(codec_kind, dims.width, dims.height, fps, &encode_params(target))?;
        // Faststart relocates `moov` and so must read the file back.
        let staged = StagedOutput::new(&target.output_path)?;
        let out_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(staged.path())
            .map_err(|e| EncoderError::Io(format!("failed to create output file: {e}")))?;
        let mut mp4_muxer = Mp4Muxer::new(out_file, is_mov)?;
        let v_cfg = VideoStreamConfig {
            codec_fourcc: encoder.fourcc().unwrap_or_else(|| codec_kind.default_fourcc()),
            dimensions: dims,
            frame_rate: fps,
            pixel_format,
            color_space: ColorTransform::target_tag(target.video.color),
            bit_depth: pixel_format.bits_per_component(),
            codec_private: encoder.config_record(),
        };
        let v_track_idx = mp4_muxer.add_video_stream(&v_cfg)?;

        let mut audio = match input.audio_feed(target, &self.limits)? {
            Some(feed) => {
                let codec = audio_codec(target).unwrap_or("aac");
                let (codec_fourcc, bits_per_sample, codec_private, audio_encoder) = if codec.eq_ignore_ascii_case("aac") {
                    let channels = u8::try_from(feed.layout().channel_count())
                        .map_err(|_| EncoderError::InvalidJobSpec("audio channel count exceeds the AAC encoder limit".into()))?;
                    let aac_enc = AacEncoderWrapper::new(feed.rate(), channels, aac_bitrate_bps(target)?)?;
                    (*b"mp4a", 16, aac_enc.audio_specific_config(), Mp4AudioEncoder::Aac(Box::new(aac_enc)))
                } else if is_mov && codec.eq_ignore_ascii_case("pcm_s24le") {
                    (*b"lpcm", 24, Vec::new(), Mp4AudioEncoder::Pcm24Le)
                } else {
                    return Err(EncoderError::UnsupportedCodec(format!("audio codec {codec} is not supported in this output container")));
                };
                let a_cfg = AudioStreamConfig { codec_fourcc, sample_rate: feed.rate(), layout: feed.layout(), bits_per_sample, codec_private };
                let track = mp4_muxer.add_audio_stream(&a_cfg)?;
                Some((feed, audio_encoder, track))
            }
            None => None,
        };

        self.progress.set_total_frames(input.len());

        let processor = self.frame_processor(target, fps)?;
        self.stream_video(&input, &processor, start_time, |index, frame| {
            // The track clock runs at `fps.num` Hz, so frame `index` starts at `index * fps.den`.
            frame.pts = i64::try_from(index).unwrap_or(i64::MAX).saturating_mul(fps.den);
            for pkt in encoder.encode_frame(frame)? {
                let sample = VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: frame_ticks, is_keyframe: pkt.is_keyframe };
                mp4_muxer.write_video_sample(v_track_idx, sample)?;
            }
            Ok(())
        })?;

        for pkt in encoder.flush()? {
            let sample = VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: frame_ticks, is_keyframe: pkt.is_keyframe };
            mp4_muxer.write_video_sample(v_track_idx, sample)?;
        }

        if let Some((feed, audio_encoder, a_track_idx)) = audio.as_mut() {
            let mut next_pts = 0i64;
            match audio_encoder {
                Mp4AudioEncoder::Aac(aac_encoder) => {
                    let mut write = |packets: Vec<Vec<u8>>, muxer: &mut Mp4Muxer<std::fs::File>| -> Result<(), EncoderError> {
                        for pkt in &packets {
                            muxer.write_audio_sample(*a_track_idx, AudioSample { data: pkt, pts: next_pts, duration_ticks: 1024 })?;
                            next_pts += 1024;
                        }
                        Ok(())
                    };
                    while let Some(window) = feed.next_window()? {
                        self.progress.checkpoint()?;
                        write(aac_encoder.encode_buffer(&window)?, &mut mp4_muxer)?;
                    }
                    write(aac_encoder.flush()?, &mut mp4_muxer)?;
                }
                Mp4AudioEncoder::Pcm24Le => {
                    while let Some(window) = feed.next_window()? {
                        self.progress.checkpoint()?;
                        let data = encode_pcm_s24le(&window)?;
                        let duration = u32::try_from(window.samples_per_channel())
                            .map_err(|_| EncoderError::InvalidJobSpec("PCM audio sample window exceeds the MP4 duration limit".into()))?;
                        mp4_muxer.write_audio_sample(*a_track_idx, AudioSample { data: &data, pts: next_pts, duration_ticks: duration })?;
                        next_pts = next_pts.saturating_add(i64::from(duration));
                    }
                }
            }
        }

        mp4_muxer.finish(!is_mov)?;

        // Append Adobe XMP UUID box if metadata is defined
        if let Some(ref meta) = target.metadata {
            let xmp_packet = meta.to_xmp_packet();
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(staged.path())
                .map_err(|e| EncoderError::Io(format!("failed to open MP4 for XMP embedding: {e}")))?;
            encodercraft_muxers::append_xmp_uuid_box(&mut file, &xmp_packet)?;
        }

        staged.commit()
    }

    fn execute_video_mxf(&self, target: &OutputTarget, is_op_atom: bool, start_time: Instant) -> Result<(), EncoderError> {
        let input = VideoInput::open(&self.spec.source, target)?;
        let dims = input.dims();
        let fps = input.fps();

        let lower_preset = target.preset_name.to_ascii_lowercase();
        let codec_kind = if lower_preset.contains("prores") {
            VideoCodecKind::ProRes
        } else if lower_preset.contains("h264") || lower_preset.contains("avc") {
            VideoCodecKind::H264
        } else {
            VideoCodecKind::DnxHd
        };
        let codec_kind = resolve_codec(target, codec_kind)?;

        let staged = StagedOutput::new(&target.output_path)?;
        let out_file = File::create(staged.path()).map_err(|e| EncoderError::Io(format!("failed to create MXF output file: {e}")))?;
        let writer = BufWriter::new(out_file);

        let pixel_format = target_pixel_format(target)?;
        let bit_depth = pixel_format.bits_per_component();

        let mut encoder =
            create_video_encoder_with(codec_kind, dims.width, dims.height, fps, &EncodeParams { annex_b: true, ..encode_params(target) })?;
        let v_cfg = VideoStreamConfig {
            codec_fourcc: encoder.fourcc().unwrap_or_else(|| codec_kind.default_fourcc()),
            dimensions: dims,
            frame_rate: fps,
            pixel_format,
            color_space: ColorTransform::target_tag(target.video.color),
            bit_depth,
            codec_private: encoder.config_record(),
        };

        let mut audio = input.audio_feed(target, &self.limits)?;
        if let Some(codec) = audio_codec(target)
            && !codec.eq_ignore_ascii_case("pcm_s24le")
        {
            return Err(EncoderError::UnsupportedCodec(format!("MXF audio codec {codec} is not supported")));
        }
        let a_cfg = audio.as_ref().map(|feed| AudioStreamConfig {
            codec_fourcc: *b"sowt",
            sample_rate: feed.rate(),
            layout: feed.layout(),
            bits_per_sample: 24,
            codec_private: Vec::new(),
        });

        let pkg_name = target.output_path.file_stem().and_then(|s| s.to_str()).unwrap_or("encoderCraft_MXF");
        let audio_cfgs: &[AudioStreamConfig] = match (&a_cfg, is_op_atom) {
            (Some(cfg), false) => std::slice::from_ref(cfg),
            _ => &[],
        };
        let mut mxf_muxer = MxfMuxer::new(writer, is_op_atom, pkg_name, Some(&v_cfg), audio_cfgs)?;

        self.progress.set_total_frames(input.len());

        let processor = self.frame_processor(target, fps)?;
        self.stream_video(&input, &processor, start_time, |index, frame| {
            frame.pts = i64::try_from(index).unwrap_or(i64::MAX);
            for pkt in encoder.encode_frame(frame)? {
                mxf_muxer.write_video_sample(VideoSample {
                    data: &pkt.data,
                    pts: pkt.pts,
                    dts: pkt.dts,
                    duration_ticks: 1,
                    is_keyframe: pkt.is_keyframe,
                })?;
            }
            Ok(())
        })?;

        for pkt in encoder.flush()? {
            mxf_muxer.write_video_sample(VideoSample {
                data: &pkt.data,
                pts: pkt.pts,
                dts: pkt.dts,
                duration_ticks: 1,
                is_keyframe: pkt.is_keyframe,
            })?;
        }

        if let (Some(feed), false) = (audio.as_mut(), is_op_atom) {
            while let Some(window) = feed.next_window()? {
                self.progress.checkpoint()?;
                mxf_muxer.write_audio_buffer(0, &window, 24)?;
            }
        }

        let mut finished = mxf_muxer.finish()?;
        std::io::Write::flush(&mut finished).map_err(|e| EncoderError::Io(format!("failed to flush MXF file: {e}")))?;
        drop(finished);
        staged.commit()
    }

    fn execute_video_mkv(&self, target: &OutputTarget, is_webm: bool, start_time: Instant) -> Result<(), EncoderError> {
        let input = VideoInput::open(&self.spec.source, target)?;
        let dims = input.dims();
        let fps = input.fps();

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
        let codec_kind = resolve_codec(target, codec_kind)?;
        let pixel_format = target_pixel_format(target)?;

        let staged = StagedOutput::new(&target.output_path)?;
        let out_file = File::create(staged.path()).map_err(|e| EncoderError::Io(format!("failed to create MKV/WebM file: {e}")))?;
        let writer = BufWriter::new(out_file);

        let mut encoder = create_video_encoder_with(codec_kind, dims.width, dims.height, fps, &encode_params(target))?;
        let v_cfg = VideoStreamConfig {
            codec_fourcc: encoder.fourcc().unwrap_or_else(|| codec_kind.default_fourcc()),
            dimensions: dims,
            frame_rate: fps,
            pixel_format,
            color_space: ColorTransform::target_tag(target.video.color),
            bit_depth: pixel_format.bits_per_component(),
            codec_private: encoder.config_record(),
        };

        let mut builder = MkvMuxerBuilder::new(writer, is_webm);
        let v_track = builder.add_video_stream(&v_cfg);

        let configured_codec = audio_codec(target);
        if is_webm
            && let Some(codec) = configured_codec
            && !codec.eq_ignore_ascii_case("opus")
        {
            return Err(EncoderError::UnsupportedCodec(format!("WebM does not support audio codec {codec}")));
        }
        let use_opus = is_webm || configured_codec.is_some_and(|codec| codec.eq_ignore_ascii_case("opus"));
        let mut opus_target;
        let audio_target = if use_opus {
            opus_target = target.clone();
            let settings = opus_target.audio.get_or_insert_with(Default::default);
            let layout = settings.layout.unwrap_or(encodercraft_core::ChannelLayout::Stereo);
            if !matches!(layout, encodercraft_core::ChannelLayout::Mono | encodercraft_core::ChannelLayout::Stereo) {
                return Err(EncoderError::UnsupportedCodec("Opus encoding supports mono or stereo channel layouts".into()));
            }
            settings.codec = Some("opus".into());
            settings.sample_rate = Some(48_000);
            settings.layout = Some(layout);
            &opus_target
        } else {
            target
        };
        let mut audio = match input.audio_feed(audio_target, &self.limits)? {
            Some(feed) if use_opus => {
                let channels = u8::try_from(feed.layout().channel_count())
                    .map_err(|_| EncoderError::InvalidJobSpec("audio channel count exceeds the Opus encoder limit".into()))?;
                let encoder = OpusEncoder::new(channels, opus_bitrate_bps(target, usize::from(channels))?);
                let a_cfg = AudioStreamConfig {
                    codec_fourcc: *b"Opus",
                    sample_rate: 48_000,
                    layout: feed.layout(),
                    bits_per_sample: 16,
                    codec_private: encoder.opus_head(),
                };
                let track = builder.add_audio_stream(&a_cfg)?;
                Some((feed, MatroskaAudioEncoder::Opus(Box::new(encoder)), track))
            }
            Some(feed) => {
                let codec = configured_codec.unwrap_or("aac");
                if !codec.eq_ignore_ascii_case("aac") {
                    return Err(EncoderError::UnsupportedCodec(format!("Matroska audio codec {codec} is not supported")));
                }
                let channels = u8::try_from(feed.layout().channel_count())
                    .map_err(|_| EncoderError::InvalidJobSpec("audio channel count exceeds the AAC encoder limit".into()))?;
                let aac_enc = AacEncoderWrapper::new(feed.rate(), channels, aac_bitrate_bps(target)?)?;
                let a_cfg = AudioStreamConfig {
                    codec_fourcc: *b"mp4a",
                    sample_rate: feed.rate(),
                    layout: feed.layout(),
                    bits_per_sample: 16,
                    codec_private: aac_enc.audio_specific_config(),
                };
                let track = builder.add_audio_stream(&a_cfg)?;
                Some((feed, MatroskaAudioEncoder::Aac(Box::new(aac_enc)), track))
            }
            None => None,
        };

        let mut mkv_muxer = builder.build()?;

        self.progress.set_total_frames(input.len());

        // The Matroska muxer takes microsecond ticks.
        let micros = |index: u64| -> i64 { (index as f64 * 1_000_000.0 * fps.den as f64 / fps.num as f64).round() as i64 };

        let processor = self.frame_processor(target, fps)?;
        self.stream_video(&input, &processor, start_time, |index, frame| {
            frame.pts = micros(index);
            let duration = u32::try_from(micros(index + 1) - micros(index)).unwrap_or(u32::MAX);
            for pkt in encoder.encode_frame(frame)? {
                mkv_muxer.write_video_sample(
                    v_track,
                    VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: duration, is_keyframe: pkt.is_keyframe },
                )?;
            }
            Ok(())
        })?;

        let last_duration = u32::try_from(micros(input.len()) - micros(input.len().saturating_sub(1))).unwrap_or(u32::MAX);
        for pkt in encoder.flush()? {
            mkv_muxer.write_video_sample(
                v_track,
                VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: last_duration, is_keyframe: pkt.is_keyframe },
            )?;
        }

        if let Some((feed, audio_encoder, audio_track)) = audio.as_mut() {
            match audio_encoder {
                MatroskaAudioEncoder::Aac(aac_encoder) => {
                    let rate = f64::from(feed.rate());
                    let duration = (1024.0 * 1_000_000.0 / rate).round() as u32;
                    let mut idx = 0u64;
                    let mut write = |packets: Vec<Vec<u8>>, muxer: &mut encodercraft_muxers::MkvMuxer<BufWriter<File>>| -> Result<(), EncoderError> {
                        for pkt in &packets {
                            let pts = (idx as f64 * 1024.0 * 1_000_000.0 / rate).round() as i64;
                            muxer.write_audio_sample(*audio_track, AudioSample { data: pkt, pts, duration_ticks: duration })?;
                            idx += 1;
                        }
                        Ok(())
                    };
                    while let Some(window) = feed.next_window()? {
                        self.progress.checkpoint()?;
                        write(aac_encoder.encode_buffer(&window)?, &mut mkv_muxer)?;
                    }
                    write(aac_encoder.flush()?, &mut mkv_muxer)?;
                }
                MatroskaAudioEncoder::Opus(opus_encoder) => {
                    let channels = feed.layout().channel_count();
                    let frame_len = OpusEncoder::FRAME_SIZE
                        .checked_mul(channels)
                        .ok_or_else(|| EncoderError::Internal("overflow calculating Opus frame size".into()))?;
                    let frame_samples = u64::try_from(OpusEncoder::FRAME_SIZE)
                        .map_err(|_| EncoderError::Internal("Opus frame size exceeds the timestamp limit".into()))?;
                    let frame_duration = u32::try_from(samples_to_micros(frame_samples)?)
                        .map_err(|_| EncoderError::Internal("Opus packet duration exceeds the container limit".into()))?;
                    let mut pending = Vec::with_capacity(frame_len);
                    let mut next_sample = 0u64;
                    while let Some(window) = feed.next_window()? {
                        self.progress.checkpoint()?;
                        for packet in encode_opus_buffer(opus_encoder, &window, &mut pending)? {
                            let pts = samples_to_micros(next_sample)?;
                            mkv_muxer.write_audio_sample(*audio_track, AudioSample { data: &packet, pts, duration_ticks: frame_duration })?;
                            next_sample = next_sample
                                .checked_add(frame_samples)
                                .ok_or_else(|| EncoderError::Internal("Opus sample timeline overflow".into()))?;
                        }
                    }
                    if pending.len() % channels != 0 {
                        return Err(EncoderError::Internal("incomplete interleaved Opus sample group".into()));
                    }
                    let remaining_samples = pending.len() / channels;
                    if remaining_samples > 0 {
                        pending.resize(frame_len, 0.0);
                        let packet = opus_encoder.encode_float(&pending);
                        let pts = samples_to_micros(next_sample)?;
                        let duration = u32::try_from(samples_to_micros(
                            u64::try_from(remaining_samples).map_err(|_| EncoderError::Internal("Opus tail exceeds the timestamp limit".into()))?,
                        )?)
                        .map_err(|_| EncoderError::Internal("Opus tail duration exceeds the container limit".into()))?;
                        mkv_muxer.write_audio_sample(*audio_track, AudioSample { data: &packet, pts, duration_ticks: duration })?;
                    }
                }
            }
        }

        let mut finished = mkv_muxer.finish()?;
        std::io::Write::flush(&mut finished).map_err(|e| EncoderError::Io(format!("failed to flush MKV file: {e}")))?;
        drop(finished);
        staged.commit()
    }

    fn execute_animated_gif(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let mut gif_target = target.clone();
        if gif_target.custom_frame_rate.is_none() {
            gif_target.custom_frame_rate = Some(Rational64::new(12, 1)?);
        }
        let input = VideoInput::open(&self.spec.source, &gif_target)?;
        let dims = input.dims();
        let fps = input.fps();
        let width = u16::try_from(dims.width).map_err(|_| EncoderError::InvalidJobSpec("GIF width exceeds 65535 pixels".into()))?;
        let height = u16::try_from(dims.height).map_err(|_| EncoderError::InvalidJobSpec("GIF height exceeds 65535 pixels".into()))?;

        self.progress.set_total_frames(input.len());
        let fps_u32 = (fps.as_f64().round() as u32).max(1);
        let mut gif_enc = encodercraft_video_codecs::GifEncoder::new(width, height, fps_u32)?;

        let processor = self.frame_processor(&gif_target, fps)?;
        self.stream_video(&input, &processor, start_time, |_, frame| gif_enc.add_frame(frame))?;

        let gif_bytes = gif_enc.finish();
        let staged = StagedOutput::new(&target.output_path)?;
        std::fs::write(staged.path(), gif_bytes).map_err(|e| EncoderError::Io(format!("failed to write GIF file: {e}")))?;
        staged.commit()
    }

    /// Exports multichannel audio stems (discrete mono or stereo pairs) conforming to AME multichannel workflow.
    fn export_audio_stems(&self, target: &OutputTarget, mode: encodercraft_core::AudioStemMode) -> Result<(), EncoderError> {
        let sample_buf = self.load_source_audio(target)?;
        let sample_rate = sample_buf.sample_rate;
        let channels = sample_buf.layout.channel_count();

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

/// An output written beside its destination and moved into place only when complete, so a failed
/// or cancelled job never leaves a truncated file (nor replaces an existing one).
struct StagedOutput {
    final_path: PathBuf,
    staged_path: PathBuf,
}

impl StagedOutput {
    fn new(final_path: &Path) -> Result<Self, EncoderError> {
        static NEXT_STAGE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let name = final_path
            .file_name()
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("output path {} has no file name", final_path.display())))?
            .to_string_lossy();
        for _ in 0..8 {
            let sequence = NEXT_STAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let staged_path = final_path.with_file_name(format!(".{name}.{}.{}.partial", std::process::id(), sequence));
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&staged_path) {
                Ok(file) => {
                    drop(file);
                    return Ok(Self { final_path: final_path.to_path_buf(), staged_path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(EncoderError::Io(format!("failed to create staged output {}: {error}", staged_path.display())));
                }
            }
        }
        Err(EncoderError::Io(format!("could not reserve a unique staged output beside {}", final_path.display())))
    }

    fn path(&self) -> &Path {
        &self.staged_path
    }

    /// Flushes the staged output and atomically replaces the destination through craft-atomic.
    fn commit(self) -> Result<(), EncoderError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.staged_path)
            .map_err(|e| EncoderError::Io(format!("failed to reopen {}: {e}", self.staged_path.display())))?;
        craft_atomic::sync_file(&file).map_err(|e| EncoderError::Io(format!("failed to flush {}: {e}", self.staged_path.display())))?;
        drop(file);
        let mut staged =
            File::open(&self.staged_path).map_err(|e| EncoderError::Io(format!("failed to read {}: {e}", self.staged_path.display())))?;
        craft_atomic::atomic_write_with(&self.final_path, |dest| std::io::copy(&mut staged, dest).map(|_| ()))
            .map_err(|e| EncoderError::Io(format!("failed to atomically replace {}: {e}", self.final_path.display())))?;
        Ok(())
    }
}

impl Drop for StagedOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.staged_path);
    }
}

#[cfg(test)]
mod staged_output_tests {
    use super::*;

    #[test]
    fn dropping_staged_output_preserves_destination_and_removes_partial() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("render.mp4");
        std::fs::write(&destination, b"previous render").unwrap();
        let staged = StagedOutput::new(&destination).unwrap();
        let partial = staged.path().to_path_buf();
        std::fs::write(&partial, b"incomplete render").unwrap();

        drop(staged);

        assert_eq!(std::fs::read(destination).unwrap(), b"previous render");
        assert!(!partial.exists());
    }

    #[test]
    fn commit_replaces_existing_destination_and_removes_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("render.mp4");
        std::fs::write(&destination, b"previous render").unwrap();
        let staged = StagedOutput::new(&destination).unwrap();
        let partial = staged.path().to_path_buf();
        std::fs::write(&partial, b"finished render").unwrap();

        staged.commit().unwrap();

        assert_eq!(std::fs::read(destination).unwrap(), b"finished render");
        assert!(!partial.exists());
    }
}

/// SMPTE-style timecode for output frame `f`, counting nominal (rounded) frames per second.
pub(crate) fn timecode_for_frame(f: u64, fps: Rational64) -> encodercraft_core::MediaTimecode {
    let nominal = (fps.as_f64().round() as u64).max(1);
    let total_seconds = f / nominal;
    let frames = (f % nominal) as u8;
    let seconds = (total_seconds % 60) as u8;
    let minutes = ((total_seconds / 60) % 60) as u8;
    let hours = ((total_seconds / 3600) % 100) as u8;
    encodercraft_core::MediaTimecode::new(hours, minutes, seconds, frames, false, fps)
}

/// Picks the video codec: an explicit target codec wins over the preset-name heuristic, and the
/// container must be able to store it.
fn resolve_codec(target: &OutputTarget, inferred: VideoCodecKind) -> Result<VideoCodecKind, EncoderError> {
    let kind = match target.video.codec.as_deref() {
        Some(id) => VideoCodecKind::parse(id).ok_or_else(|| EncoderError::UnsupportedCodec(format!("unknown video codec `{id}`")))?,
        None => inferred,
    };
    if !container_accepts(target.container, kind) {
        return Err(EncoderError::UnsupportedCodec(format!("{kind:?} video cannot be stored in {:?}", target.container)));
    }
    Ok(kind)
}

/// Pixel layout emitted by the selected software encoder. Explicit unsupported layouts fail
/// instead of being silently written with metadata that disagrees with the encoded bitstream.
pub fn target_pixel_format(target: &OutputTarget) -> Result<PixelFormat, EncoderError> {
    let name = target.preset_name.to_ascii_lowercase();
    let inferred = if target.container == ContainerType::AnimatedGif {
        VideoCodecKind::Gif
    } else if target.container == ContainerType::WebM {
        if name.contains("av1") { VideoCodecKind::Av1 } else { VideoCodecKind::Vp9 }
    } else if name.contains("prores") {
        VideoCodecKind::ProRes
    } else if name.contains("dnx") {
        VideoCodecKind::DnxHd
    } else if name.contains("hevc") || name.contains("h265") {
        VideoCodecKind::Hevc
    } else if name.contains("av1") {
        VideoCodecKind::Av1
    } else if name.contains("vp9") {
        VideoCodecKind::Vp9
    } else if name.contains("mjpeg") || name.contains("motion jpeg") {
        VideoCodecKind::Mjpeg
    } else if matches!(target.container, ContainerType::MxfOp1a | ContainerType::MxfOpAtom) {
        VideoCodecKind::DnxHd
    } else {
        VideoCodecKind::H264
    };
    let kind = resolve_codec(target, inferred)?;
    let output = match kind {
        VideoCodecKind::Hevc | VideoCodecKind::Av1 => PixelFormat::Yuv420p10,
        VideoCodecKind::ProRes => PixelFormat::Yuv422p10,
        VideoCodecKind::DnxHd if target.video.profile.as_deref().is_some_and(|p| p.eq_ignore_ascii_case("hqx")) || name.contains("hqx") => {
            PixelFormat::Yuv422p10
        }
        VideoCodecKind::DnxHd => PixelFormat::Yuv422p,
        VideoCodecKind::H264 | VideoCodecKind::Vp9 | VideoCodecKind::Mjpeg | VideoCodecKind::Gif => PixelFormat::Yuv420p,
    };
    if let Some(requested) = target.video.pixel_format
        && requested != output
    {
        return Err(EncoderError::UnsupportedCodec(format!("{kind:?} outputs {output:?}; requested pixel format {requested:?} is not supported")));
    }
    Ok(output)
}

fn encode_params(target: &OutputTarget) -> EncodeParams {
    EncodeParams::from_settings(&target.video, target.target_bitrate_kbps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_core::{ClientInfo, JobPriority, SourceDescriptor, StartPolicy, VideoFrame};
    use tempfile::tempdir;

    #[test]
    fn pcm24_audio_is_interleaved_little_endian_and_clamped() {
        let audio = AudioSampleBuffer::from_planar_channels(
            48_000,
            encodercraft_core::ChannelLayout::Stereo,
            0,
            vec![vec![-1.0, 0.5, f32::NAN], vec![1.0, -0.5, 0.0]],
        )
        .unwrap();

        assert_eq!(
            encode_pcm_s24le(&audio).unwrap(),
            vec![0x00, 0x00, 0x80, 0xff, 0xff, 0x7f, 0x00, 0x00, 0x40, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    /// A 64x48 RGBA gradient written as a real BMP file.
    pub(super) fn write_test_bmp(dir: &std::path::Path) -> std::path::PathBuf {
        let dims = Dimensions::new(64, 48).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        for (i, px) in plane.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px.copy_from_slice(&[(i % 64 * 4) as u8, (i / 64 * 5) as u8, 200, 255]);
        }
        let path = dir.join("source.bmp");
        std::fs::write(&path, encodercraft_image_codecs::encode_bmp(&frame).unwrap()).unwrap();
        path
    }

    /// Half a second of 440 Hz stereo audio written as a real WAV file.
    pub(super) fn write_test_wav(dir: &std::path::Path) -> std::path::PathBuf {
        let mut buf = AudioSampleBuffer::new_silent(48_000, encodercraft_core::ChannelLayout::Stereo, 24_000, 0).unwrap();
        for c in 0..2 {
            for (i, v) in buf.channel_mut(c).unwrap().iter_mut().enumerate() {
                *v = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
            }
        }
        let path = dir.join("source.wav");
        let file = File::create(&path).unwrap();
        let mut writer = BwfWavWriter::new(file, 48_000, 2, WavFormatTag::Pcm16, None).unwrap();
        writer.write_buffer(&buf).unwrap();
        writer.finish().unwrap();
        path
    }

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
            source: SourceDescriptor::File { path: write_test_wav(dir.path()), start_tick: None, duration_ticks: None, timebase: None },
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
            source: SourceDescriptor::File { path: write_test_bmp(dir.path()), start_tick: None, duration_ticks: Some(6), timebase: None },
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
            source: SourceDescriptor::File { path: write_test_wav(dir.path()), start_tick: None, duration_ticks: None, timebase: None },
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
            source: SourceDescriptor::File { path: write_test_bmp(dir.path()), start_tick: None, duration_ticks: Some(6), timebase: None },
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
            source: SourceDescriptor::File { path: write_test_bmp(dir.path()), start_tick: None, duration_ticks: Some(6), timebase: None },
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
            source: SourceDescriptor::File { path: write_test_bmp(dir.path()), start_tick: None, duration_ticks: Some(6), timebase: None },
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
            source: SourceDescriptor::File { path: write_test_wav(dir.path()), start_tick: None, duration_ticks: None, timebase: None },
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
            source: SourceDescriptor::File { path: write_test_bmp(dir.path()), start_tick: None, duration_ticks: Some(6), timebase: None },
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
    fn test_execute_cad_dxf_and_cadcraft_transcode() {
        let dir = tempdir().unwrap();
        let dxf_in = dir.path().join("blueprint.dxf");
        let cadcraft_out = dir.path().join("blueprint.cadcraft");
        let svg_out = dir.path().join("blueprint.svg");
        let hpgl_out = dir.path().join("blueprint.plt");

        let mut cad = encodercraft_spatial_io::CadDrawing::new("Foundation");
        cad.add_line(encodercraft_spatial_io::CadPoint3D::new(0.0, 0.0, 0.0), encodercraft_spatial_io::CadPoint3D::new(500.0, 200.0, 0.0), "Walls");
        cad.add_circle(encodercraft_spatial_io::CadPoint3D::new(250.0, 100.0, 0.0), 50.0, "Walls");
        std::fs::write(&dxf_in, cad.to_dxf_ascii()).unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-cad-job".into(),
            client_info: ClientInfo { app_name: "cadcraft".into(), app_version: "1.0".into(), pid: 42, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: dxf_in, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_cadcraft".into(),
                    output_path: cadcraft_out.clone(),
                    preset_name: "ArtCraft Native CadCraft".into(),
                    container: ContainerType::ArtcraftCadcraft,
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_svg".into(),
                    output_path: svg_out.clone(),
                    preset_name: "Web Vector SVG".into(),
                    container: ContainerType::Svg,
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_hpgl".into(),
                    output_path: hpgl_out.clone(),
                    preset_name: "HPGL Pen Plotter".into(),
                    container: ContainerType::HpglPlotter,
                    ..Default::default()
                },
            ],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(cadcraft_out.exists());
        let cad_bytes = std::fs::read(&cadcraft_out).unwrap();
        assert!(cad_bytes.starts_with(b"\x89CRAFT\x00\x01C"));

        assert!(svg_out.exists());
        let svg_text = std::fs::read_to_string(&svg_out).unwrap();
        assert!(svg_text.contains("<svg"));
        assert!(svg_text.contains("<line"));
        assert!(svg_text.contains("<circle"));

        assert!(hpgl_out.exists());
        let hpgl_text = std::fs::read_to_string(&hpgl_out).unwrap();
        assert!(hpgl_text.starts_with("IN;SP1;"));
    }

    #[test]
    fn test_execute_prepress_indd_and_designcraft_transcode() {
        let dir = tempdir().unwrap();
        let indd_in = dir.path().join("brochure.indd");
        let designcraft_out = dir.path().join("brochure.designcraft");
        let idml_out = dir.path().join("brochure.idml");
        let pdf_out = dir.path().join("brochure.pdf");

        let mut indd_bytes = vec![0x06, 0x06, 0xED, 0xF5, 0xD8, 0x1D, 0x46, 0xE5, 0xBD, 0x31, 0xEF, 0xE7, 0xFE, 0x74, 0xB7, 0x1D];
        indd_bytes.extend_from_slice(b"ObjectStreams<spread><story>Spring Collection</story></spread>");
        std::fs::write(&indd_in, &indd_bytes).unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-prepress-job".into(),
            client_info: ClientInfo { app_name: "designcraft".into(), app_version: "1.0".into(), pid: 101, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: indd_in, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_designcraft".into(),
                    output_path: designcraft_out.clone(),
                    preset_name: "ArtCraft Native DesignCraft".into(),
                    container: ContainerType::ArtcraftDesigncraft,
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_idml".into(),
                    output_path: idml_out.clone(),
                    preset_name: "InDesign IDML Package".into(),
                    container: ContainerType::Idml,
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_pdf".into(),
                    output_path: pdf_out.clone(),
                    preset_name: "Press Quality PDF/X-4".into(),
                    container: ContainerType::Pdf,
                    ..Default::default()
                },
            ],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(designcraft_out.exists());
        let dc_bytes = std::fs::read(&designcraft_out).unwrap();
        assert!(dc_bytes.starts_with(b"\x89CRAFT\x00\x01D"));

        assert!(idml_out.exists());
        let idml_bytes = std::fs::read(&idml_out).unwrap();
        assert!(idml_bytes.starts_with(b"PK\x03\x04"));

        assert!(pdf_out.exists());
        let pdf_data = std::fs::read(&pdf_out).unwrap();
        assert!(pdf_data.starts_with(b"%PDF-1.6"));
        let pdf_str = String::from_utf8_lossy(&pdf_data);
        assert!(pdf_str.contains("/GTS_PDFXVersion (PDF/X-4)"));
    }

    #[test]
    fn test_execute_layered_psd_and_photocraft_transcode() {
        let dir = tempdir().unwrap();
        let psd_in = dir.path().join("artwork.psd");
        let photocraft_out = dir.path().join("artwork.photocraft");
        let psb_out = dir.path().join("artwork.psb");
        let exr_out = dir.path().join("artwork.exr");

        let mut img = encodercraft_image_codecs::LayeredImage::new("Artwork", 64, 64);
        let layer = encodercraft_image_codecs::ImageLayer::new("Layer 1", 64, 64, vec![180; 64 * 64 * 4]);
        img.add_layer(layer);
        let psd_bytes = img.to_psd_bytes(false).unwrap();
        std::fs::write(&psd_in, &psd_bytes).unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-layered-job".into(),
            client_info: ClientInfo { app_name: "photocraft".into(), app_version: "1.0".into(), pid: 202, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: psd_in, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_photocraft".into(),
                    output_path: photocraft_out.clone(),
                    preset_name: "ArtCraft Native PhotoCraft".into(),
                    container: ContainerType::ArtcraftPhotocraft,
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_psb".into(),
                    output_path: psb_out.clone(),
                    preset_name: "Photoshop Large Document PSB".into(),
                    container: ContainerType::AdobePhotoshopPsb,
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_exr".into(),
                    output_path: exr_out.clone(),
                    preset_name: "VFX Composite OpenEXR".into(),
                    container: ContainerType::OpenExr,
                    ..Default::default()
                },
            ],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(photocraft_out.exists());
        let pc_bytes = std::fs::read(&photocraft_out).unwrap();
        assert!(pc_bytes.starts_with(b"\x89CRAFT\x00\x01P"));

        assert!(psb_out.exists());
        let psb_bytes = std::fs::read(&psb_out).unwrap();
        assert!(psb_bytes.starts_with(b"8BPS"));
        assert_eq!(psb_bytes[4..6], [0, 2]); // Version 2

        assert!(exr_out.exists());
        let exr_data = std::fs::read(&exr_out).unwrap();
        assert!(exr_data.starts_with(b"\x76\x2F\x31\x01")); // OpenEXR magic
    }

    fn selection_spec(
        source: std::path::PathBuf,
        out: std::path::PathBuf,
        container: ContainerType,
        selection: encodercraft_core::SourceSelection,
    ) -> CraftJobSpec {
        CraftJobSpec {
            version: "1.0".into(),
            job_id: "selection-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: source, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t".into(),
                output_path: out,
                preset_name: "p".into(),
                container,
                selection,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn selection_restricts_timeline_sequences_and_rejects_unknown() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("in.prproj");
        let mut t = crate::project::TimelineProject::new("P");
        t.add_sequence(crate::project::TimelineSequence::new("Main", 1920, 1080, 25.0));
        t.add_sequence(crate::project::TimelineSequence::new("Social", 1080, 1920, 30.0));
        std::fs::write(&src, t.to_prproj()).unwrap();

        let out = dir.path().join("all.prproj");
        TranscodePipeline::new(selection_spec(src.clone(), out.clone(), ContainerType::AdobePremierePrproj, Default::default())).execute().unwrap();
        assert_eq!(crate::inventory::inspect_source(&out).sequences.len(), 2);

        let out = dir.path().join("one.prproj");
        let sel = encodercraft_core::SourceSelection { sequence: Some("Social".into()), ..Default::default() };
        TranscodePipeline::new(selection_spec(src.clone(), out.clone(), ContainerType::AdobePremierePrproj, sel)).execute().unwrap();
        let inv = crate::inventory::inspect_source(&out);
        assert_eq!(inv.sequences.len(), 1);
        assert_eq!(inv.sequences[0].name, "Social");
        assert_eq!(inv.sequences[0].width, 1080);

        let out = dir.path().join("bad.prproj");
        let sel = encodercraft_core::SourceSelection { sequence: Some("Nope".into()), ..Default::default() };
        assert!(TranscodePipeline::new(selection_spec(src, out.clone(), ContainerType::AdobePremierePrproj, sel)).execute().is_err());
        assert!(!out.exists());
    }

    #[test]
    fn selection_restricts_document_pages_and_cad_layers() {
        let dir = tempdir().unwrap();
        let mut doc = crate::document::DocumentCanvas::new("Report");
        doc.pages[0].add_text(54.0, 720.0, 100.0, 20.0, "PAGE-ONE", 14.0);
        doc.add_page().add_text(54.0, 720.0, 100.0, 20.0, "PAGE-TWO", 14.0);
        let src = dir.path().join("in.designcraft");
        std::fs::write(&src, doc.to_designcraft_binary().unwrap()).unwrap();

        let out = dir.path().join("p2.pdf");
        let sel = encodercraft_core::SourceSelection { pages: Some((2, 2)), ..Default::default() };
        TranscodePipeline::new(selection_spec(src.clone(), out.clone(), ContainerType::Pdf, sel)).execute().unwrap();
        let pdf = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).to_string();
        assert!(pdf.contains("PAGE-TWO") && !pdf.contains("PAGE-ONE"));

        let out = dir.path().join("bad.pdf");
        let sel = encodercraft_core::SourceSelection { pages: Some((9, 9)), ..Default::default() };
        assert!(TranscodePipeline::new(selection_spec(src, out.clone(), ContainerType::Pdf, sel)).execute().is_err());
        assert!(!out.exists());

        let mut cad = encodercraft_spatial_io::CadDrawing::new("Plan");
        let p0 = encodercraft_spatial_io::CadPoint3D::new(0.0, 0.0, 0.0);
        cad.add_line(p0, encodercraft_spatial_io::CadPoint3D::new(10.0, 0.0, 0.0), "WALLS");
        cad.add_line(p0, encodercraft_spatial_io::CadPoint3D::new(0.0, 10.0, 0.0), "FURNITURE");
        let src = dir.path().join("plan.cadcraft");
        std::fs::write(&src, cad.to_cadcraft_binary().unwrap()).unwrap();
        let out = dir.path().join("walls.cadcraft");
        let sel = encodercraft_core::SourceSelection { layers: Some(vec!["WALLS".into()]), ..Default::default() };
        TranscodePipeline::new(selection_spec(src.clone(), out.clone(), ContainerType::ArtcraftCadcraft, sel)).execute().unwrap();
        let back = encodercraft_spatial_io::CadDrawing::from_cadcraft_binary(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(back.entities.len(), 1);
        assert!(!back.layer_names().contains(&"FURNITURE".to_string()));

        let out = dir.path().join("bad.cadcraft");
        let sel = encodercraft_core::SourceSelection { layers: Some(vec!["NOPE".into()]), ..Default::default() };
        assert!(TranscodePipeline::new(selection_spec(src, out.clone(), ContainerType::ArtcraftCadcraft, sel)).execute().is_err());
        assert!(!out.exists());
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
            source: SourceDescriptor::File { path: write_test_bmp(dir.path()), start_tick: Some(0), duration_ticks: Some(4), timebase: None },
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
        assert_eq!(mov_info.audio_streams.len(), 0, "a still source has no audio, so no audio track is written");

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
        assert_eq!(mxf_info.audio_streams.len(), 0, "a still source has no audio, so no audio track is written");
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
        assert_eq!(mp4_info.audio_streams.len(), 0, "a still source has no audio, so no audio track is written");
    }

    #[test]
    fn test_vector_transcoding_pipeline() {
        let tmp = tempfile::tempdir().unwrap();
        let src_svg = tmp.path().join("input.svg");
        let out_vc = tmp.path().join("output.vectorcraft");
        let out_ai = tmp.path().join("output.ai");
        let out_eps = tmp.path().join("output.eps");
        let out_dxf = tmp.path().join("output.dxf");

        std::fs::write(
            &src_svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="400">
  <path d="M 0.00 0.00 L 100.00 0.00 L 100.00 100.00 Z" fill="#ff0000" stroke="#000000" stroke-width="2" />
</svg>"##,
        )
        .unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "job-vector-test".into(),
            client_info: ClientInfo { app_name: "encoderCraft".into(), app_version: "1.0".into(), pid: 100, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: src_svg, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_vc".into(),
                    container: ContainerType::ArtcraftVectorcraft,
                    output_path: out_vc.clone(),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_ai".into(),
                    container: ContainerType::AdobeIllustratorAi,
                    output_path: out_ai.clone(),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_eps".into(),
                    container: ContainerType::PostScriptEps,
                    output_path: out_eps.clone(),
                    ..Default::default()
                },
                OutputTarget { target_id: "t_dxf".into(), container: ContainerType::AutocadDxf, output_path: out_dxf.clone(), ..Default::default() },
            ],
        };

        TranscodePipeline::new(spec).execute().unwrap();

        assert!(out_vc.exists());
        let vc_bytes = std::fs::read(&out_vc).unwrap();
        assert!(vc_bytes.starts_with(b"\x89CRAFT\x00\x01V"));

        assert!(out_ai.exists());
        let ai_bytes = std::fs::read(&out_ai).unwrap();
        assert!(ai_bytes.starts_with(b"%PDF-1.5"));
        assert!(String::from_utf8_lossy(&ai_bytes).contains("Adobe Illustrator"));

        assert!(out_eps.exists());
        let eps_str = std::fs::read_to_string(&out_eps).unwrap();
        assert!(eps_str.starts_with("%!PS-Adobe-3.0 EPSF-3.0"));

        assert!(out_dxf.exists());
        let dxf_str = std::fs::read_to_string(&out_dxf).unwrap();
        assert!(dxf_str.contains("ENTITIES"));
    }

    #[test]
    fn test_motion_and_nle_pipeline() {
        let tmp = tempfile::tempdir().unwrap();

        // 1. Motion Graphics: AEP -> EffectCraft
        let aep_in = tmp.path().join("motion.aep");
        let effectcraft_out = tmp.path().join("motion.effectcraft");
        let aep_out = tmp.path().join("exported.aep");

        let mut rifx = Vec::new();
        rifx.extend_from_slice(b"RIFX");
        rifx.extend_from_slice(&24u32.to_be_bytes());
        rifx.extend_from_slice(b"Egg!Comp");
        rifx.extend_from_slice(&8u32.to_be_bytes());
        rifx.extend_from_slice(&1920u32.to_be_bytes());
        rifx.extend_from_slice(&1080u32.to_be_bytes());
        std::fs::write(&aep_in, &rifx).unwrap();

        let motion_spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "job-motion".into(),
            client_info: ClientInfo { app_name: "effectcraft".into(), app_version: "1.0".into(), pid: 10, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: aep_in, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_eff".into(),
                    container: ContainerType::ArtcraftEffectcraft,
                    output_path: effectcraft_out.clone(),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_aep".into(),
                    container: ContainerType::AdobeAfterEffectsAep,
                    output_path: aep_out.clone(),
                    ..Default::default()
                },
            ],
        };
        TranscodePipeline::new(motion_spec).execute().unwrap();
        assert!(effectcraft_out.exists());
        let eff_bytes = std::fs::read(&effectcraft_out).unwrap();
        assert!(eff_bytes.starts_with(b"\x89CRAFT\x00\x01E"));
        assert!(aep_out.exists());
        let aep_bytes = std::fs::read(&aep_out).unwrap();
        assert!(aep_bytes.starts_with(b"RIFX"));

        // 2. NLE Timelines: PRPROJ -> FilmCraft, FCPXML, OTIO
        let prproj_in = tmp.path().join("edit.prproj");
        let filmcraft_out = tmp.path().join("edit.filmcraft");
        let fcpxml_out = tmp.path().join("edit.fcpxml");
        let otio_out = tmp.path().join("edit.otio");

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><PremiereData Version="3"><Sequence><Name>Cut_v01</Name></Sequence></PremiereData>"#;
        let compressed = miniz_oxide::deflate::compress_to_vec(xml.as_bytes(), 6);
        let mut gzip = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
        gzip.extend_from_slice(&compressed);
        std::fs::write(&prproj_in, &gzip).unwrap();

        let nle_spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "job-nle".into(),
            client_info: ClientInfo { app_name: "filmcraft".into(), app_version: "1.0".into(), pid: 11, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: prproj_in, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_film".into(),
                    container: ContainerType::ArtcraftFilmcraft,
                    output_path: filmcraft_out.clone(),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_fcp".into(),
                    container: ContainerType::AdobePremierePrproj,
                    output_path: fcpxml_out.clone(),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_otio".into(),
                    container: ContainerType::AdobePremierePrproj,
                    output_path: otio_out.clone(),
                    ..Default::default()
                },
            ],
        };
        TranscodePipeline::new(nle_spec).execute().unwrap();
        assert!(filmcraft_out.exists());
        let film_bytes = std::fs::read(&filmcraft_out).unwrap();
        assert!(film_bytes.starts_with(b"\x89CRAFT\x00\x01F"));
        assert!(fcpxml_out.exists());
        let fcpxml_str = std::fs::read_to_string(&fcpxml_out).unwrap();
        assert!(fcpxml_str.contains("<fcpxml version=\"1.9\">"));
        assert!(otio_out.exists());
        let otio_str = std::fs::read_to_string(&otio_out).unwrap();
        assert!(otio_str.contains("Timeline.1"));
    }

    #[test]
    fn test_daw_multitrack_pipeline() {
        let dir = tempfile::tempdir().unwrap();
        let sesx_in = dir.path().join("session.sesx");

        let tone = |freq: f32, amp: f32, rate: u32| -> Vec<f32> {
            (0..rate * 2).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()).collect()
        };
        let vocal_src = AudioSampleBuffer::from_planar_channels(
            44_100,
            encodercraft_core::ChannelLayout::Stereo,
            0,
            vec![tone(440.0, 0.3, 44_100), tone(440.0, 0.3, 44_100)],
        )
        .unwrap();
        std::fs::write(dir.path().join("vocal.aiff"), encodercraft_audio_dsp::encode_audio_aiff(&vocal_src).unwrap()).unwrap();
        let guitar_src =
            AudioSampleBuffer::from_planar_channels(48_000, encodercraft_core::ChannelLayout::Mono, 0, vec![tone(110.0, 0.4, 48_000)]).unwrap();
        std::fs::write(dir.path().join("guitar.flac"), encodercraft_audio_dsp::encode_audio_flac(&guitar_src).unwrap()).unwrap();

        let mut session = encodercraft_audio_dsp::DawSession::new("Album Mix", 48000, 24);
        let mut t1 = encodercraft_audio_dsp::DawTrack::new(1, "Vocal", encodercraft_audio_dsp::DawTrackType::Audio);
        t1.volume_db = -2.0;
        t1.pan = -0.2;
        t1.clips.push(encodercraft_audio_dsp::DawClip {
            id: 1,
            name: "Verse 1".to_string(),
            start_samples: 0,
            length_samples: 96000,
            source_in_samples: 0,
            source_path: Some("vocal.aiff".into()),
            gain_db: 0.0,
        });

        let mut t2 = encodercraft_audio_dsp::DawTrack::new(2, "Guitar", encodercraft_audio_dsp::DawTrackType::Audio);
        t2.volume_db = -4.0;
        t2.pan = 0.4;
        t2.clips.push(encodercraft_audio_dsp::DawClip {
            id: 2,
            name: "Rhythm".to_string(),
            start_samples: 0,
            length_samples: 96000,
            source_in_samples: 0,
            source_path: Some("guitar.flac".into()),
            gain_db: 0.0,
        });

        session.tracks.push(t1);
        session.tracks.push(t2);

        std::fs::write(&sesx_in, session.to_sesx_xml().as_bytes()).unwrap();

        let soundcraft_out = dir.path().join("session.soundcraft");
        let sesx_out = dir.path().join("session_copy.sesx");
        let wav_out = dir.path().join("master.wav");

        let job_spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "daw_job".into(),
            client_info: ClientInfo { app_name: "soundcraft".into(), app_version: "1.0".into(), pid: 12, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: sesx_in, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_sc".into(),
                    container: ContainerType::ArtcraftSoundcraft,
                    output_path: soundcraft_out.clone(),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_sesx".into(),
                    container: ContainerType::AdobeAuditionSesx,
                    output_path: sesx_out.clone(),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_wav".into(),
                    container: ContainerType::Wave,
                    output_path: wav_out.clone(),
                    target_lufs: Some(-23.0),
                    audio_stems: Some(encodercraft_core::AudioStemMode::DiscreteMono),
                    ..Default::default()
                },
            ],
        };

        TranscodePipeline::new(job_spec).execute().unwrap();

        // 1. Check .soundcraft binary
        assert!(soundcraft_out.exists());
        let sc_bytes = std::fs::read(&soundcraft_out).unwrap();
        assert!(sc_bytes.starts_with(b"\x89CRAFT\x00\x01S"));
        let parsed_sc = encodercraft_audio_dsp::DawSession::from_soundcraft_binary(&sc_bytes).unwrap();
        assert_eq!(parsed_sc.name, "Album Mix");
        assert_eq!(parsed_sc.tracks.len(), 2);

        // 2. Check .sesx XML
        assert!(sesx_out.exists());
        let sesx_str = std::fs::read_to_string(&sesx_out).unwrap();
        assert!(sesx_str.contains("<sesx"));
        assert!(sesx_str.contains("name=\"Vocal\""));
        assert!(sesx_str.contains("name=\"Guitar\""));

        // 3. Check WAV master & stems with iXML metadata
        assert!(wav_out.exists());
        let wav_bytes = std::fs::read(&wav_out).unwrap();
        assert!(wav_bytes.starts_with(b"RIFF"));
        assert!(wav_bytes.windows(4).any(|w| w == b"iXML"));
        assert!(wav_bytes.windows(4).any(|w| w == b"bext"));

        let master = encodercraft_audio_dsp::decode_audio(&wav_bytes).unwrap();
        assert_eq!(master.sample_rate, 48_000);
        assert_eq!(master.samples_per_channel(), 96_000);
        let stats = encodercraft_audio_dsp::measure_loudness(&master).unwrap();
        assert!((stats.integrated_lufs + 23.0).abs() < 0.5, "integrated {}", stats.integrated_lufs);
        assert!(stats.true_peak_dbtp <= -0.9);

        // DiscreteMono: stereo "Vocal" splits to L/R mono files, mono-source "Guitar" is panned into both
        for name in ["01_Vocal_L", "01_Vocal_R", "02_Guitar_L", "02_Guitar_R"] {
            let path = dir.path().join(format!("master_Stem_{name}.wav"));
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("missing stem {name}: {e}"));
            assert!(bytes.windows(4).any(|w| w == b"iXML"), "{name} lacks iXML");
            let stem = encodercraft_audio_dsp::decode_audio(&bytes).unwrap();
            assert_eq!(stem.channel_count(), 1);
            assert_eq!(stem.samples_per_channel(), 96_000);
            let plane = stem.channel(0).unwrap();
            assert!(plane.iter().any(|v| v.abs() > 0.05), "{name} is silent");
            assert!(plane.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        }
        let vocal_ixml = String::from_utf8_lossy(&std::fs::read(dir.path().join("master_Stem_01_Vocal_L.wav")).unwrap()).into_owned();
        assert!(vocal_ixml.contains("<NAME>Vocal</NAME>"));
        assert!(!vocal_ixml.contains("<NAME>Guitar</NAME>"));
    }

    #[test]
    fn test_daw_pipeline_missing_clip_audio_is_an_error_and_stereo_pairs_and_flac_work() {
        let dir = tempfile::tempdir().unwrap();
        let src =
            AudioSampleBuffer::from_planar_channels(48_000, encodercraft_core::ChannelLayout::Stereo, 0, vec![vec![0.2; 48_000], vec![0.2; 48_000]])
                .unwrap();
        std::fs::write(dir.path().join("bed.aiff"), encodercraft_audio_dsp::encode_audio_aiff(&src).unwrap()).unwrap();

        let make = |clip_file: &str| {
            let mut session = encodercraft_audio_dsp::DawSession::new("S", 48_000, 16);
            let mut t = encodercraft_audio_dsp::DawTrack::new(1, "Bed", encodercraft_audio_dsp::DawTrackType::Audio);
            t.clips.push(encodercraft_audio_dsp::DawClip {
                id: 1,
                name: "bed".into(),
                start_samples: 0,
                length_samples: 48_000,
                source_in_samples: 0,
                source_path: Some(clip_file.into()),
                gain_db: 0.0,
            });
            session.tracks.push(t);
            let sc_path = dir.path().join(format!("{clip_file}.soundcraft"));
            std::fs::write(&sc_path, session.to_soundcraft_binary().unwrap()).unwrap();
            sc_path
        };
        let job = |source: PathBuf, targets: Vec<OutputTarget>| CraftJobSpec {
            version: "1.0".into(),
            job_id: "daw_job2".into(),
            client_info: ClientInfo { app_name: "t".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: source, start_tick: None, duration_ticks: None, timebase: None },
            targets,
        };

        let flac_out = dir.path().join("out.flac");
        let wav_out = dir.path().join("pairs.wav");
        TranscodePipeline::new(job(
            make("bed.aiff"),
            vec![
                OutputTarget { target_id: "f".into(), container: ContainerType::Flac, output_path: flac_out.clone(), ..Default::default() },
                OutputTarget {
                    target_id: "w".into(),
                    container: ContainerType::Wave,
                    output_path: wav_out.clone(),
                    audio_stems: Some(encodercraft_core::AudioStemMode::StereoPairs),
                    ..Default::default()
                },
            ],
        ))
        .execute()
        .unwrap();

        let flac = encodercraft_audio_dsp::decode_audio(&std::fs::read(&flac_out).unwrap()).unwrap();
        assert!((flac.channel(0).unwrap()[24_000] - 0.2).abs() < 1e-3);
        let pair = encodercraft_audio_dsp::decode_audio(&std::fs::read(dir.path().join("pairs_Stem_01_Bed.wav")).unwrap()).unwrap();
        assert_eq!(pair.channel_count(), 2);
        // 16-bit session depth is honoured for the master
        let wav = std::fs::read(&wav_out).unwrap();
        let mut pos = 12;
        let mut bits = None;
        while let Some(header) = wav.get(pos..pos + 8) {
            let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
            if &header[..4] == b"fmt " {
                bits = wav.get(pos + 22..pos + 24).map(|b| u16::from_le_bytes([b[0], b[1]]));
                break;
            }
            pos += 8 + size + (size & 1);
        }
        assert_eq!(bits, Some(16));

        let err = TranscodePipeline::new(job(
            make("absent.aiff"),
            vec![OutputTarget { target_id: "w".into(), container: ContainerType::Wave, output_path: dir.path().join("x.wav"), ..Default::default() }],
        ))
        .execute();
        assert!(err.is_err());
        assert!(!dir.path().join("x.wav").exists(), "a failed render must not leave a partial output file");
    }
}

/// An output path with a printf-style frame number field, e.g. `out/shot_%04d.png`.
struct FrameNaming {
    prefix: PathBuf,
    suffix: String,
    width: usize,
}

impl FrameNaming {
    fn parse(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let at = name.find('%')?;
        let rest = name.get(at + 1..)?;
        let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
        let digits = rest.get(..digits_len)?;
        let suffix = rest.get(digits_len..)?.strip_prefix('d')?;
        let width = if digits.is_empty() { 0 } else { digits.parse::<usize>().ok()?.min(12) };
        let dir = path.parent().unwrap_or_else(|| Path::new(""));
        Some(Self { prefix: dir.join(name.get(..at)?), suffix: suffix.to_string(), width })
    }

    fn path(&self, number: u64) -> PathBuf {
        let mut name = self.prefix.clone().into_os_string();
        name.push(format!("{number:0width$}{}", self.suffix, width = self.width));
        PathBuf::from(name)
    }
}

#[cfg(test)]
mod frame_naming_tests {
    use super::FrameNaming;
    use std::path::Path;

    #[test]
    fn numbers_frames_with_padding() {
        let n = FrameNaming::parse(Path::new("out/shot_%04d.png")).unwrap();
        assert_eq!(n.path(7), Path::new("out/shot_0007.png"));
        assert_eq!(FrameNaming::parse(Path::new("a_%d.jpg")).unwrap().path(12), Path::new("a_12.jpg"));
        assert!(FrameNaming::parse(Path::new("plain.png")).is_none());
        assert!(FrameNaming::parse(Path::new("bad_%s.png")).is_none());
    }
}
