//! Media streamed from a sibling application over the shared-memory transport.
//!
//! The producer publishes packets on a [`encodercraft_shared_stream`] service named by the job's
//! `Pipe` endpoint: video packets carry one straight-alpha RGBA8 frame of the declared size, audio
//! packets carry interleaved little-endian `f32` samples at the declared rate and channel count,
//! and an end-of-stream packet completes the stream. Everything received is held in memory under a
//! hard byte cap, so a runaway or hostile producer fails the job instead of exhausting memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use encodercraft_core::{EncoderError, Rational64};
use encodercraft_shared_stream::{SharedStreamKind, SharedStreamReceiver};
use filmcraft_frame::{AudioBuffer, VideoFrame};
use filmcraft_media::{FrameRequest, MediaError, MediaInfo, MediaKind, MediaSource, VideoStreamInfo};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};

/// Largest declared picture edge.
const MAX_EDGE: u32 = 16_384;
/// Default cap on received frame and sample bytes held for one stream.
pub const DEFAULT_MAX_PIPE_BYTES: usize = 1024 * 1024 * 1024;
/// Default time without any packet after which a stalled producer fails the job.
pub const DEFAULT_PIPE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Declared shape of a pipe stream, taken from the job's `SourceDescriptor::Pipe`.
#[derive(Debug, Clone, PartialEq)]
pub struct PipeFormat {
    pub endpoint: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rational64,
    pub sample_rate: u32,
    pub channels: u32,
}

/// Bounds applied while a stream is received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipeLimits {
    pub max_bytes: usize,
    pub idle_timeout: Duration,
}

impl Default for PipeLimits {
    fn default() -> Self {
        Self { max_bytes: DEFAULT_MAX_PIPE_BYTES, idle_timeout: DEFAULT_PIPE_IDLE_TIMEOUT }
    }
}

/// A fully received stream, served as a random-access media source.
pub struct PipeSource {
    info: MediaInfo,
    frames: Vec<Arc<VideoFrame>>,
    audio: Vec<Vec<f32>>,
    sample_rate: u32,
    frame_rate: Rational64,
}

impl PipeSource {
    /// Receives the whole stream described by `format`, blocking until its end-of-stream packet.
    pub fn receive(format: &PipeFormat, limits: PipeLimits) -> Result<Self, EncoderError> {
        validate(format)?;
        let receiver = SharedStreamReceiver::open(&format.endpoint)?;
        let channels = format.channels as usize;
        let frame_bytes = (format.width as usize) * (format.height as usize) * 4;
        let mut frames: Vec<Arc<VideoFrame>> = Vec::new();
        let mut audio: Vec<Vec<f32>> = vec![Vec::new(); channels];
        let mut held = 0usize;
        let mut last_packet = Instant::now();

        loop {
            let Some(packet) = receiver.receive()? else {
                if last_packet.elapsed() >= limits.idle_timeout {
                    return Err(EncoderError::Io(format!("pipe `{}`: no data from the producer for {:?}", format.endpoint, limits.idle_timeout)));
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
            };
            last_packet = Instant::now();
            let payload = packet.payload()?;
            match packet.header.kind {
                SharedStreamKind::EndOfStream => break,
                SharedStreamKind::VideoFrame => {
                    if payload.len() != frame_bytes {
                        return Err(EncoderError::InvalidJobSpec(format!(
                            "pipe `{}`: video packet has {} bytes, a {}x{} RGBA frame has {frame_bytes}",
                            format.endpoint,
                            payload.len(),
                            format.width,
                            format.height
                        )));
                    }
                    held = charge(held, payload.len(), limits.max_bytes, &format.endpoint)?;
                    frames.push(Arc::new(VideoFrame::rgba8(format.width, format.height, payload.to_vec())));
                }
                SharedStreamKind::AudioSamples => {
                    if channels == 0 {
                        return Err(EncoderError::InvalidJobSpec(format!(
                            "pipe `{}`: audio packet received but the stream declares no audio",
                            format.endpoint
                        )));
                    }
                    let stride = channels * 4;
                    if payload.len() % stride != 0 {
                        return Err(EncoderError::InvalidJobSpec(format!(
                            "pipe `{}`: audio packet of {} bytes is not a whole number of {channels}-channel f32 frames",
                            format.endpoint,
                            payload.len()
                        )));
                    }
                    held = charge(held, payload.len(), limits.max_bytes, &format.endpoint)?;
                    for frame in payload.chunks_exact(stride) {
                        for (plane, sample) in audio.iter_mut().zip(frame.as_chunks::<4>().0) {
                            let value = f32::from_le_bytes(*sample);
                            if !value.is_finite() {
                                return Err(EncoderError::InvalidContainer(format!(
                                    "pipe `{}`: audio contains a non-finite sample",
                                    format.endpoint
                                )));
                            }
                            plane.push(value);
                        }
                    }
                }
            }
        }

        Self::from_parts(format, frames, audio)
    }

    fn from_parts(format: &PipeFormat, frames: Vec<Arc<VideoFrame>>, audio: Vec<Vec<f32>>) -> Result<Self, EncoderError> {
        let audio_frames = audio.first().map_or(0, Vec::len);
        if frames.is_empty() && audio_frames == 0 {
            return Err(EncoderError::InvalidContainer(format!("pipe `{}` ended without any media", format.endpoint)));
        }
        let fps = format.frame_rate;
        let duration = if frames.is_empty() {
            Tick::from_units(i64::try_from(audio_frames).unwrap_or(i64::MAX), i64::from(format.sample_rate.max(1)))
        } else {
            Tick::from_rational(i64::try_from(frames.len()).unwrap_or(i64::MAX), fps.den, fps.num)
        };
        let info = MediaInfo {
            name: format.endpoint.clone(),
            kind: if frames.is_empty() { MediaKind::AudioOnly } else { MediaKind::Movie },
            duration,
            video: (!frames.is_empty()).then(|| VideoStreamInfo {
                width: format.width,
                height: format.height,
                frame_rate: FrameRate { num: fps.num, den: fps.den },
                par: (1, 1),
                codec: "Shared stream".into(),
                pixel_format: "RGBA 8-bit".into(),
                color: filmcraft_color::ColorInfo::SRGB_FULL,
                has_alpha: true,
                bitrate: None,
                hdr: None,
                color_assumed: None,
            }),
            audio_streams: if audio_frames > 0 {
                vec![filmcraft_media::AudioStreamInfo {
                    sample_rate: format.sample_rate,
                    channels: format.channels,
                    codec: "PCM f32".into(),
                    bits_per_sample: Some(32),
                }]
            } else {
                Vec::new()
            },
            container: "Shared stream".into(),
            start_timecode: None,
            file_size: None,
        };
        Ok(Self { info, frames, audio, sample_rate: format.sample_rate, frame_rate: fps })
    }
}

fn charge(held: usize, add: usize, max: usize, endpoint: &str) -> Result<usize, EncoderError> {
    held.checked_add(add)
        .filter(|total| *total <= max)
        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("pipe `{endpoint}` delivered more than the {} MiB limit", max >> 20)))
}

fn frame_bytes(format: &PipeFormat) -> Result<usize, EncoderError> {
    (format.width as usize)
        .checked_mul(format.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes > 0)
        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("pipe `{}` declares no video", format.endpoint)))
}

fn validate(format: &PipeFormat) -> Result<(), EncoderError> {
    let has_video = format.width > 0 || format.height > 0;
    if has_video && (format.width == 0 || format.height == 0 || format.width > MAX_EDGE || format.height > MAX_EDGE) {
        return Err(EncoderError::InvalidJobSpec(format!("pipe `{}`: unsupported picture size {}x{}", format.endpoint, format.width, format.height)));
    }
    if has_video && (format.frame_rate.num <= 0 || format.frame_rate.den <= 0) {
        return Err(EncoderError::InvalidJobSpec(format!(
            "pipe `{}`: invalid frame rate {}/{}",
            format.endpoint, format.frame_rate.num, format.frame_rate.den
        )));
    }
    if format.channels > 8 || (format.channels > 0 && !(8_000..=384_000).contains(&format.sample_rate)) {
        return Err(EncoderError::InvalidJobSpec(format!(
            "pipe `{}`: unsupported audio format {} channels at {} Hz",
            format.endpoint, format.channels, format.sample_rate
        )));
    }
    if !has_video && format.channels == 0 {
        return Err(EncoderError::InvalidJobSpec(format!("pipe `{}` declares neither video nor audio", format.endpoint)));
    }
    Ok(())
}

impl MediaSource for PipeSource {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn video_frame(&self, req: FrameRequest) -> Result<Arc<VideoFrame>, MediaError> {
        let last = self.frames.len().checked_sub(1).ok_or(MediaError::NoStream("video"))?;
        let scaled = i128::from(req.time.0.max(0)) * i128::from(self.frame_rate.num);
        let denom = i128::from(self.frame_rate.den) * i128::from(TICKS_PER_SECOND);
        let index = usize::try_from((scaled * 2 + denom) / (denom * 2)).unwrap_or(usize::MAX).min(last);
        self.frames.get(index).cloned().ok_or_else(|| MediaError::Decode("pipe frame index out of range".into()))
    }

    fn audio(&self, start: i64, frames: usize, sample_rate: u32) -> Result<AudioBuffer, MediaError> {
        if self.audio.is_empty() || self.audio.first().is_none_or(Vec::is_empty) {
            return Err(MediaError::NoStream("audio"));
        }
        if sample_rate != self.sample_rate {
            return Err(MediaError::Unsupported(format!("pipe audio is {} Hz and is not resampled to {sample_rate} Hz here", self.sample_rate)));
        }
        let channels = self
            .audio
            .iter()
            .map(|plane| {
                (0..frames)
                    .map(|offset| {
                        i64::try_from(offset)
                            .ok()
                            .and_then(|o| start.checked_add(o))
                            .and_then(|at| usize::try_from(at).ok())
                            .and_then(|at| plane.get(at).copied())
                            .unwrap_or(0.0)
                    })
                    .collect()
            })
            .collect();
        Ok(AudioBuffer { sample_rate, channels })
    }
}

type Slot = Arc<Mutex<Option<Arc<PipeSource>>>>;

fn registry() -> &'static Mutex<HashMap<String, Slot>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns the received stream for `format.endpoint`, receiving it on first use. A job opens its
/// source more than once (probe, then one open per target); a stream can be consumed only once, so
/// later opens share the first result until [`release`] is called.
pub fn open_shared(format: &PipeFormat, limits: PipeLimits) -> Result<Arc<PipeSource>, EncoderError> {
    let slot = {
        let mut map = registry().lock().unwrap_or_else(PoisonError::into_inner);
        map.entry(format.endpoint.clone()).or_default().clone()
    };
    let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(source) = guard.as_ref() {
        return Ok(source.clone());
    }
    let source = Arc::new(PipeSource::receive(format, limits)?);
    *guard = Some(source.clone());
    Ok(source)
}

/// Frees the stream held for `endpoint` once its job has finished.
pub fn release(endpoint: &str) {
    registry().lock().unwrap_or_else(PoisonError::into_inner).remove(endpoint);
}

/// Producer side of a pipe stream: the supported way for an application to hand frames and
/// audio to encoderCraft, following the protocol the receiver expects.
pub struct PipeProducer {
    format: PipeFormat,
    publisher: encodercraft_shared_stream::SharedStreamPublisher,
    frames_sent: i64,
}

impl PipeProducer {
    /// Declares the stream. The format must match the `Pipe` source of the job that consumes it.
    pub fn create(format: PipeFormat) -> Result<Self, EncoderError> {
        validate(&format)?;
        let publisher = encodercraft_shared_stream::SharedStreamPublisher::create(&format.endpoint)?;
        Ok(Self { format, publisher, frames_sent: 0 })
    }

    /// Waits until the receiving job has attached. Call this before sending anything.
    pub fn wait_for_receiver(&self, timeout: Duration) -> bool {
        self.publisher.wait_for_subscriber(timeout)
    }

    /// Sends one straight-alpha RGBA8 picture of exactly the declared size.
    pub fn send_video(&mut self, rgba: &[u8]) -> Result<(), EncoderError> {
        let expected = frame_bytes(&self.format)?;
        if rgba.len() != expected {
            return Err(EncoderError::InvalidJobSpec(format!(
                "pipe `{}`: video frame is {} bytes, expected {expected} for {}x{} RGBA8",
                self.format.endpoint,
                rgba.len(),
                self.format.width,
                self.format.height
            )));
        }
        self.publisher.publish_with(
            encodercraft_shared_stream::SharedStreamHeader {
                kind: SharedStreamKind::VideoFrame,
                pts: self.frames_sent,
                duration: 1,
                payload_bytes: expected as u64,
            },
            |payload| {
                payload.copy_from_slice(rgba);
                Ok(())
            },
        )?;
        self.frames_sent = self.frames_sent.saturating_add(1);
        Ok(())
    }

    /// Sends interleaved `f32` samples; the length must be a whole number of declared-channel frames.
    pub fn send_audio(&mut self, interleaved: &[f32]) -> Result<(), EncoderError> {
        let channels = self.format.channels as usize;
        if channels == 0 || interleaved.is_empty() || !interleaved.len().is_multiple_of(channels) {
            return Err(EncoderError::InvalidJobSpec(format!(
                "pipe `{}`: {} samples are not a whole number of {channels}-channel frames",
                self.format.endpoint,
                interleaved.len()
            )));
        }
        if interleaved.iter().any(|sample| !sample.is_finite()) {
            return Err(EncoderError::InvalidJobSpec(format!("pipe `{}`: audio contains a non-finite sample", self.format.endpoint)));
        }
        let bytes = interleaved.len().saturating_mul(4);
        self.publisher.publish_with(
            encodercraft_shared_stream::SharedStreamHeader {
                kind: SharedStreamKind::AudioSamples,
                pts: 0,
                duration: (interleaved.len() / channels) as i64,
                payload_bytes: bytes as u64,
            },
            |payload| {
                for (chunk, sample) in payload.as_chunks_mut::<4>().0.iter_mut().zip(interleaved) {
                    *chunk = sample.to_le_bytes();
                }
                Ok(())
            },
        )
    }

    /// Ends the stream and waits for the receiver to finish reading it.
    pub fn finish(self, timeout: Duration) -> Result<bool, EncoderError> {
        self.publisher.finish(timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_shared_stream::{SharedStreamHeader, SharedStreamPublisher};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn endpoint() -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        format!("encodercraft/pipe-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
    }

    fn format(endpoint: String) -> PipeFormat {
        PipeFormat { endpoint, width: 4, height: 2, frame_rate: Rational64 { num: 10, den: 1 }, sample_rate: 8_000, channels: 2 }
    }

    fn send(publisher: &SharedStreamPublisher, kind: SharedStreamKind, pts: i64, bytes: &[u8]) {
        publisher
            .publish_with(SharedStreamHeader { kind, pts, duration: 1, payload_bytes: bytes.len() as u64 }, |payload| {
                payload.copy_from_slice(bytes);
                Ok(())
            })
            .unwrap();
    }

    fn produce(name: String, video: &'static [u8], frames: u8, audio_samples: usize, extra_video: Option<Vec<u8>>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let publisher = SharedStreamPublisher::create(&name).unwrap();
            assert!(publisher.wait_for_subscriber(Duration::from_secs(10)));
            for i in 0..frames {
                let pixels: Vec<u8> = video.iter().map(|b| b.wrapping_add(i)).collect();
                send(&publisher, SharedStreamKind::VideoFrame, i64::from(i), &pixels);
            }
            if let Some(bad) = extra_video {
                send(&publisher, SharedStreamKind::VideoFrame, 99, &bad);
            }
            if audio_samples > 0 {
                let mut bytes = Vec::new();
                for n in 0..audio_samples {
                    bytes.extend_from_slice(&(n as f32 / 100.0).to_le_bytes());
                    bytes.extend_from_slice(&(-(n as f32) / 100.0).to_le_bytes());
                }
                send(&publisher, SharedStreamKind::AudioSamples, 0, &bytes);
            }
            assert!(publisher.finish(Duration::from_secs(10)).unwrap());
        })
    }

    static PIXELS: [u8; 32] = [10; 32];

    #[test]
    fn received_stream_serves_frames_and_audio() {
        let name = endpoint();
        let format = format(name.clone());
        let producer = produce(name, &PIXELS, 5, 400, None);
        let source = PipeSource::receive(&format, PipeLimits::default()).unwrap();
        producer.join().unwrap();

        assert_eq!(source.info().video.as_ref().unwrap().width, 4);
        assert!((source.info().duration.seconds() - 0.5).abs() < 1e-9);
        let frame = source.video_frame(FrameRequest::full(Tick::from_rational(3, 1, 10))).unwrap();
        assert_eq!(frame.width, 4);
        let filmcraft_frame::PixelData::Rgba8(data) = &frame.data else { panic!("expected RGBA8") };
        assert_eq!(data.first(), Some(&13));
        let audio = source.audio(100, 3, 8_000).unwrap();
        assert_eq!(audio.channels.len(), 2);
        assert_eq!(audio.channels.first().unwrap(), &vec![1.0, 1.01, 1.02]);
        assert_eq!(audio.channels.get(1).unwrap().first(), Some(&-1.0));
        assert_eq!(source.audio(399, 3, 8_000).unwrap().channels.first().unwrap().get(1), Some(&0.0));
    }

    #[test]
    fn wrong_sized_video_packet_is_rejected() {
        let name = endpoint();
        let format = format(name.clone());
        let producer = produce(name, &PIXELS, 1, 0, Some(vec![0; 7]));
        let error = PipeSource::receive(&format, PipeLimits::default()).err().unwrap();
        producer.join().unwrap();
        assert!(matches!(error, EncoderError::InvalidJobSpec(_)), "{error:?}");
    }

    #[test]
    fn byte_cap_stops_a_runaway_producer() {
        let name = endpoint();
        let format = format(name.clone());
        let producer = produce(name, &PIXELS, 5, 0, None);
        let limits = PipeLimits { max_bytes: 32 * 3, ..PipeLimits::default() };
        let error = PipeSource::receive(&format, limits).err().unwrap();
        producer.join().ok();
        assert!(error.to_string().contains("limit"), "{error}");
    }

    #[test]
    fn silent_producer_times_out() {
        let name = endpoint();
        let format = format(name.clone());
        let _publisher = SharedStreamPublisher::create(&name).unwrap();
        let limits = PipeLimits { idle_timeout: Duration::from_millis(100), ..PipeLimits::default() };
        let error = PipeSource::receive(&format, limits).err().unwrap();
        assert!(matches!(error, EncoderError::Io(_)), "{error:?}");
    }

    #[test]
    fn invalid_declarations_are_rejected() {
        let mut bad = format(endpoint());
        bad.width = 100_000;
        assert!(PipeSource::receive(&bad, PipeLimits::default()).is_err());
        let mut bad = format(endpoint());
        bad.frame_rate = Rational64 { num: 0, den: 1 };
        assert!(PipeSource::receive(&bad, PipeLimits::default()).is_err());
        let mut bad = format(endpoint());
        bad.width = 0;
        bad.height = 0;
        bad.channels = 0;
        assert!(PipeSource::receive(&bad, PipeLimits::default()).is_err());
    }

    #[test]
    fn later_opens_share_the_first_receive() {
        let name = endpoint();
        let format = format(name.clone());
        let producer = produce(name.clone(), &PIXELS, 2, 0, None);
        let first = open_shared(&format, PipeLimits::default()).unwrap();
        producer.join().unwrap();
        let second = open_shared(&format, PipeLimits::default()).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        release(&name);
    }

    #[test]
    fn producer_round_trips_through_the_receiver() {
        let format =
            PipeFormat { endpoint: endpoint(), width: 4, height: 2, frame_rate: Rational64 { num: 10, den: 1 }, sample_rate: 8_000, channels: 1 };
        let mut producer = PipeProducer::create(format.clone()).unwrap();
        let reader = std::thread::spawn({
            let format = format.clone();
            move || PipeSource::receive(&format, PipeLimits::default())
        });
        assert!(producer.wait_for_receiver(Duration::from_secs(10)));
        for i in 0..3u8 {
            producer.send_video(&[i; 4 * 2 * 4]).unwrap();
        }
        producer.send_audio(&[0.25; 800]).unwrap();
        assert!(producer.finish(Duration::from_secs(10)).unwrap());
        let source = reader.join().unwrap().unwrap();
        assert_eq!(source.info().video.as_ref().map(|v| (v.width, v.height)), Some((4, 2)));
    }

    #[test]
    fn producer_rejects_malformed_input_before_publishing() {
        let format =
            PipeFormat { endpoint: endpoint(), width: 4, height: 2, frame_rate: Rational64 { num: 10, den: 1 }, sample_rate: 8_000, channels: 2 };
        let mut producer = PipeProducer::create(format).unwrap();
        assert!(producer.send_video(&[0; 7]).is_err());
        assert!(producer.send_audio(&[0.0; 3]).is_err());
        assert!(producer.send_audio(&[]).is_err());
        assert!(producer.send_audio(&[f32::NAN, 0.0]).is_err());
        let no_video =
            PipeFormat { endpoint: endpoint(), width: 0, height: 0, frame_rate: Rational64 { num: 1, den: 1 }, sample_rate: 8_000, channels: 1 };
        let mut audio_only = PipeProducer::create(no_video).unwrap();
        assert!(audio_only.send_video(&[]).is_err());
    }
}
