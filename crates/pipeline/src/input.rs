//! Source media access for a transcode job: real decoding through `encodercraft-source`,
//! trimming, frame-rate conversion and audio extraction. Nothing here fabricates media; a source
//! that cannot be decoded is a typed error.

use encodercraft_audio_dsp::{SincResampler, normalize_loudness, remix_channels};
use encodercraft_core::{
    AudioSampleBuffer, ChannelLayout, DeinterlaceMode, EncoderError, FlipMode, FrameRateMode, OutputTarget, Rational64, Rotation, SourceDescriptor,
    VideoFrame,
};
use encodercraft_geom::Dimensions;
use encodercraft_source::{MediaReader, SourceKind};

use crate::stream::PipelineLimits;

/// Largest accepted output edge in pixels.
const MAX_EDGE: u32 = 16_384;

/// A decoded-on-demand video input resolved against one output target.
pub(crate) struct VideoInput {
    reader: MediaReader,
    src_fps: Rational64,
    out_fps: Rational64,
    dims: Dimensions,
    first_src: u64,
    src_frames: u64,
    count: u64,
    still: bool,
    rotation: Rotation,
    flip: FlipMode,
    deinterlace: DeinterlaceMode,
    frame_rate_mode: FrameRateMode,
}

/// Opens the media file of a job source.
pub(crate) fn open_reader(source: &SourceDescriptor) -> Result<MediaReader, EncoderError> {
    open_reader_at(source, None)
}

/// Like [`open_reader`]; `sequence_rate` is the frame rate numbered image sequences play at.
pub(crate) fn open_reader_at(source: &SourceDescriptor, sequence_rate: Option<Rational64>) -> Result<MediaReader, EncoderError> {
    match source {
        SourceDescriptor::File { path, .. } => MediaReader::open_with_rate(path, sequence_rate),
        SourceDescriptor::StitchedClips { .. } => Err(EncoderError::UnsupportedCodec("stitched sources are processed by the clip stitcher".into())),
        SourceDescriptor::Pipe { endpoint, width, height, frame_rate, sample_rate, channels } => {
            MediaReader::open_pipe(&encodercraft_source::pipe::PipeFormat {
                endpoint: endpoint.clone(),
                width: *width,
                height: *height,
                frame_rate: *frame_rate,
                sample_rate: *sample_rate,
                channels: *channels,
            })
        }
        SourceDescriptor::ProjectReference { .. } => {
            Err(EncoderError::UnsupportedCodec("project references must be rendered by their host application first".into()))
        }
    }
}

/// Seconds covered by the requested trim window: `(start, duration)`.
/// Ticks are seconds-per-tick scaled by `timebase`; without a timebase they are output frames.
fn trim_window(source: &SourceDescriptor, out_fps: Rational64) -> (f64, Option<f64>) {
    let SourceDescriptor::File { start_tick, duration_ticks, timebase, .. } = source else {
        return (0.0, None);
    };
    let fps = out_fps.as_f64();
    let to_seconds = |ticks: i64| -> f64 {
        match timebase {
            Some(tb) if tb.as_f64() > 0.0 => ticks as f64 * tb.as_f64(),
            _ if fps > 0.0 => ticks as f64 / fps,
            _ => 0.0,
        }
    };
    let start = start_tick.filter(|t| *t >= 0).map(to_seconds).unwrap_or(0.0);
    let duration = duration_ticks.filter(|t| *t > 0).map(to_seconds);
    (start, duration)
}

fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

impl VideoInput {
    /// Opens the source for `target`. Output size and frame rate default to the source's.
    pub(crate) fn open(source: &SourceDescriptor, target: &OutputTarget) -> Result<Self, EncoderError> {
        let reader = open_reader_at(source, target.custom_frame_rate)?;
        let probe = reader.probe();
        let video = probe
            .video
            .clone()
            .ok_or_else(|| EncoderError::InvalidContainer(format!("{} has no video stream and cannot be encoded as video", probe.name)))?;
        let still = matches!(probe.kind, SourceKind::Still);

        let src_fps = video.frame_rate;
        let out_fps = target.custom_frame_rate.unwrap_or(if still { Rational64::FPS_24 } else { src_fps });
        if out_fps.num <= 0 || out_fps.den <= 0 {
            return Err(EncoderError::InvalidJobSpec(format!("invalid frame rate {}/{}", out_fps.num, out_fps.den)));
        }

        let rotation = target.transform.rotation;
        let flip = target.transform.flip;
        let deinterlace = target.transform.deinterlace;
        let (picture_w, picture_h) = if rotation.swaps_axes() { (video.height, video.width) } else { (video.width, video.height) };
        let width = even(target.custom_width.unwrap_or(picture_w));
        let height = even(target.custom_height.unwrap_or(picture_h));
        if width > MAX_EDGE || height > MAX_EDGE {
            return Err(EncoderError::InvalidJobSpec(format!("output size {width}x{height} exceeds the {MAX_EDGE} pixel limit")));
        }
        let dims = Dimensions::new(width, height)?;

        let src_frames = reader.frame_count();
        let src_fps_f = src_fps.as_f64();
        let out_fps_f = out_fps.as_f64();
        let (start_s, duration_s) = trim_window(source, out_fps);
        let first_src = if still { 0 } else { ((start_s * src_fps_f).round().max(0.0) as u64).min(src_frames.saturating_sub(1)) };

        let available_s = if still { 0.0 } else { (src_frames.saturating_sub(first_src)) as f64 / src_fps_f };
        let count = if still {
            duration_s.map(|d| (d * out_fps_f).round().max(1.0) as u64).unwrap_or(1)
        } else {
            let wanted = duration_s.map(|d| d.min(available_s)).unwrap_or(available_s);
            ((wanted * out_fps_f).round().max(1.0)) as u64
        };

        Ok(Self {
            reader,
            src_fps,
            out_fps,
            dims,
            first_src,
            src_frames,
            count,
            still,
            rotation,
            flip,
            deinterlace,
            frame_rate_mode: target.transform.frame_rate_mode,
        })
    }

    pub(crate) fn dims(&self) -> Dimensions {
        self.dims
    }

    pub(crate) fn fps(&self) -> Rational64 {
        self.out_fps
    }

    /// Number of output frames.
    pub(crate) fn len(&self) -> u64 {
        self.count
    }

    /// First trimmed audio second offset is derived from the same window as the video.
    pub(crate) fn start_seconds(&self) -> f64 {
        self.first_src as f64 / self.src_fps.as_f64()
    }

    pub(crate) fn duration_seconds(&self) -> f64 {
        self.count as f64 / self.out_fps.as_f64()
    }

    /// Decodes output frame `i`: the nearest source frame, or a blend of the two around the instant
    /// when the rates differ and blending is requested; then deinterlaced, rotated, and flipped.
    pub(crate) fn frame(&self, i: u64) -> Result<VideoFrame, EncoderError> {
        let frame = if self.still {
            self.reader.frame_at_index(0)?
        } else {
            let ratio = self.src_fps.as_f64() / self.out_fps.as_f64();
            let position = i as f64 * ratio;
            let last = self.src_frames.saturating_sub(1);
            let clamp = |offset: u64| self.first_src.saturating_add(offset).min(last);
            let floor = position.floor();
            let fraction = position - floor;
            let weight = (fraction * 256.0).round() as u32;
            if self.frame_rate_mode == FrameRateMode::Blend && weight > 0 && weight < 256 {
                let a = self.reader.frame_at_index(clamp(floor as u64))?;
                let b = self.reader.frame_at_index(clamp(floor as u64 + 1))?;
                crate::transform::blend(&a, &b, weight)?
            } else {
                self.reader.frame_at_index(clamp(position.round() as u64))?
            }
        };
        let frame = crate::transform::deinterlace(&frame, self.deinterlace)?;
        let frame = crate::transform::rotate(&frame, self.rotation)?;
        crate::transform::flip(&frame, self.flip)
    }

    /// Bytes of one decoded source frame.
    pub(crate) fn source_frame_bytes(&self) -> usize {
        self.reader.probe().video.as_ref().map_or(0, |video| {
            let bytes_per_pixel = if video.bit_depth > 8 { 16 } else { 4 };
            (video.width as usize).saturating_mul(video.height as usize).saturating_mul(bytes_per_pixel)
        })
    }

    /// Audio matching the target format and video window, or `None` when the source has no audio.
    /// Loudness normalisation needs the whole track, which must fit `limits`; otherwise the audio is
    /// delivered in bounded windows.
    pub(crate) fn audio_feed(&self, target: &OutputTarget, limits: &PipelineLimits) -> Result<Option<AudioFeed<'_>>, EncoderError> {
        if self.still {
            return Ok(None);
        }
        let Some(stream) = self.reader.probe().audio.first() else {
            return Ok(None);
        };
        let source_rate = stream.sample_rate.max(8_000);
        let settings = target.audio.as_ref();
        let rate = settings.and_then(|settings| settings.sample_rate).unwrap_or(source_rate);
        if !(8_000..=384_000).contains(&rate) {
            return Err(EncoderError::InvalidJobSpec(format!("unsupported output audio sample rate {rate} Hz")));
        }
        let layout = settings.and_then(|settings| settings.layout).unwrap_or(ChannelLayout::Stereo);
        let start = self.start_seconds();
        let remaining = (self.reader.probe().duration_seconds - start).max(0.0);
        let seconds = self.duration_seconds().min(remaining);
        let first = (start.max(0.0) * f64::from(source_rate)).round() as i64;
        let total = (seconds * f64::from(source_rate)).ceil().max(0.0) as i64;
        if total == 0 {
            return Ok(None);
        }
        let whole = match target.target_lufs {
            Some(lufs) => {
                let source_samples =
                    usize::try_from(total).map_err(|_| EncoderError::InvalidJobSpec("audio sample count exceeds the platform limit".into()))?;
                let output_samples = (source_samples as u128)
                    .checked_mul(u128::from(rate))
                    .ok_or_else(|| EncoderError::InvalidJobSpec("audio sample count overflow".into()))?
                    / u128::from(source_rate);
                let output_samples =
                    usize::try_from(output_samples).map_err(|_| EncoderError::InvalidJobSpec("resampled audio exceeds the platform limit".into()))?;
                let source_bytes = source_samples.saturating_mul(stream.channels.clamp(1, 8) as usize).saturating_mul(std::mem::size_of::<f32>());
                let output_bytes = output_samples.saturating_mul(layout.channel_count()).saturating_mul(std::mem::size_of::<f32>());
                let transient_bytes = output_bytes.saturating_mul(if rate == source_rate { 1 } else { 2 });
                let bytes = source_bytes.saturating_add(transient_bytes);
                if bytes > limits.audio_bytes {
                    return Err(EncoderError::InvalidJobSpec(format!(
                        "loudness normalisation holds source and target-format audio buffers: {} MiB is over the {} MiB limit",
                        bytes >> 20,
                        limits.audio_bytes >> 20
                    )));
                }
                let raw = self.reader.read_audio_range(first, source_samples, source_rate)?;
                let mut output = remix_channels(&raw, layout)?;
                if rate != source_rate {
                    output = SincResampler::default().resample(&output, rate)?;
                }
                let _ = normalize_loudness(&mut output, f64::from(lufs), -1.0)?;
                Some(output)
            }
            None => None,
        };
        let window = usize::try_from(source_rate).unwrap_or(48_000).saturating_mul(AUDIO_FEED_SECONDS).max(1);
        Ok(Some(AudioFeed { reader: &self.reader, source_rate, rate, layout, next: first, end: first.saturating_add(total), window, whole }))
    }
}

/// Seconds of audio decoded per window.
const AUDIO_FEED_SECONDS: usize = 10;

/// Stereo audio delivered one bounded window at a time.
pub(crate) struct AudioFeed<'a> {
    reader: &'a MediaReader,
    source_rate: u32,
    rate: u32,
    layout: ChannelLayout,
    next: i64,
    end: i64,
    window: usize,
    whole: Option<AudioSampleBuffer>,
}

impl AudioFeed<'_> {
    pub(crate) fn rate(&self) -> u32 {
        self.rate
    }

    pub(crate) fn layout(&self) -> ChannelLayout {
        self.layout
    }

    /// The next target-format window, or `None` at the end of the track.
    pub(crate) fn next_window(&mut self) -> Result<Option<AudioSampleBuffer>, EncoderError> {
        if let Some(whole) = self.whole.take() {
            self.next = self.end;
            return Ok(Some(whole));
        }
        if self.next >= self.end {
            return Ok(None);
        }
        let take = usize::try_from(self.end - self.next).unwrap_or(usize::MAX).min(self.window);
        let chunk = self.reader.audio(self.next, take, self.source_rate)?;
        self.next += i64::try_from(take).unwrap_or(i64::MAX);
        let mut output = remix_channels(&chunk, self.layout)?;
        if self.rate != self.source_rate {
            output = SincResampler::default().resample(&output, self.rate)?;
        }
        Ok(Some(output))
    }
}

/// Reads the audio of `reader` between `start_s` and `start_s + duration_s` (or to the end).
/// Returns `None` when the source has no audio stream.
pub(crate) fn read_audio(reader: &MediaReader, start_s: f64, duration_s: Option<f64>) -> Result<Option<AudioSampleBuffer>, EncoderError> {
    let Some(stream) = reader.probe().audio.first() else {
        return Ok(None);
    };
    let rate = stream.sample_rate.max(8_000);
    let first = (start_s.max(0.0) * f64::from(rate)).round() as i64;
    let remaining = (reader.probe().duration_seconds - start_s).max(0.0);
    let seconds = duration_s.map(|d| d.min(remaining)).unwrap_or(remaining);
    let total = (seconds * f64::from(rate)).ceil().max(0.0) as usize;
    if total == 0 {
        return Ok(None);
    }
    reader.read_audio_range(first, total, rate).map(Some)
}

/// Audio-only access for audio targets: the whole trimmed audio track, required to exist.
pub(crate) fn load_audio_required(source: &SourceDescriptor, target: &OutputTarget) -> Result<AudioSampleBuffer, EncoderError> {
    let reader = open_reader(source)?;
    let fps = target.custom_frame_rate.unwrap_or(Rational64::FPS_24);
    let (start, duration) = trim_window(source, fps);
    read_audio(&reader, start, duration)?.ok_or_else(|| EncoderError::InvalidContainer(format!("{} has no audio to export", reader.probe().name)))
}
