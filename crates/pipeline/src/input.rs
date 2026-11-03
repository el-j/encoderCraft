//! Source media access for a transcode job: real decoding through `encodercraft-source`,
//! trimming, frame-rate conversion and audio extraction. Nothing here fabricates media; a source
//! that cannot be decoded is a typed error.

use encodercraft_core::{AudioSampleBuffer, EncoderError, OutputTarget, Rational64, SourceDescriptor, VideoFrame};
use encodercraft_geom::Dimensions;
use encodercraft_source::{MediaReader, SourceKind};

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
        SourceDescriptor::Pipe { .. } => Err(EncoderError::UnsupportedCodec("pipe sources are not supported by file transcoding".into())),
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
    let start = start_tick.filter(|t| *t > 0).map(to_seconds).unwrap_or(0.0);
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

        let width = even(target.custom_width.unwrap_or(video.width));
        let height = even(target.custom_height.unwrap_or(video.height));
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

        Ok(Self { reader, src_fps, out_fps, dims, first_src, src_frames, count, still })
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

    /// Decodes output frame `i`, mapping it to the nearest source frame when the rates differ.
    pub(crate) fn frame(&self, i: u64) -> Result<VideoFrame, EncoderError> {
        if self.still {
            return self.reader.frame_at_index(0);
        }
        let ratio = self.src_fps.as_f64() / self.out_fps.as_f64();
        let offset = (i as f64 * ratio).round() as u64;
        let idx = self.first_src.saturating_add(offset).min(self.src_frames.saturating_sub(1));
        self.reader.frame_at_index(idx)
    }

    /// Audio matching the video window, or `None` when the source has no audio.
    pub(crate) fn audio(&self) -> Result<Option<AudioSampleBuffer>, EncoderError> {
        if self.still {
            return Ok(None);
        }
        read_audio(&self.reader, self.start_seconds(), Some(self.duration_seconds()))
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
