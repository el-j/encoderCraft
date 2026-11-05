//! Staged video streaming: decode, convert/scale/filter and encode run on separate threads joined
//! by bounded channels, so memory stays constant however long the source is. Every stage is
//! panic-guarded, observes pause and cancel, and a failure anywhere shuts the other stages down.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use encodercraft_core::{EncoderError, Rational64, VideoFrame};
use encodercraft_geom::Dimensions;

use crate::captions::{CaptionBurner, CaptionTrack};
use crate::lut::Lut3D;
use crate::overlay::TimecodeOverlay;
use crate::progress::JobProgress;
use crate::scale::FrameScaler;
use crate::transform::ColorTransform;

/// Default budget for frames in flight between stages.
pub const DEFAULT_QUEUE_BYTES: usize = 256 << 20;
/// Default cap on audio held whole for loudness normalisation.
pub const DEFAULT_AUDIO_BYTES: usize = 1 << 30;
/// Deepest queue between two stages.
pub const MAX_QUEUE_DEPTH: usize = 8;
/// Frames beyond queued items: stage-local frames plus one recycled output buffer.
const STAGE_SLACK: usize = 6;

/// Memory limits of a transcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineLimits {
    /// Budget for decoded frames held between stages, in bytes.
    pub queue_bytes: usize,
    /// Most frames queued between two stages.
    pub max_depth: usize,
    /// Largest audio track loaded whole, which loudness normalisation needs; other audio streams in windows.
    pub audio_bytes: usize,
}

impl Default for PipelineLimits {
    fn default() -> Self {
        Self { queue_bytes: DEFAULT_QUEUE_BYTES, max_depth: MAX_QUEUE_DEPTH, audio_bytes: DEFAULT_AUDIO_BYTES }
    }
}

impl PipelineLimits {
    /// Queue depth whose worst case, `2 * depth + STAGE_SLACK` frames of `frame_bytes`, fits the budget.
    pub fn depth_for(&self, frame_bytes: usize) -> usize {
        let frames = self.queue_bytes / frame_bytes.max(1);
        (frames.saturating_sub(STAGE_SLACK) / 2).clamp(1, self.max_depth.max(1))
    }

    /// Most frames that can be alive at once with `depth`.
    pub fn max_in_flight(depth: usize) -> usize {
        depth.saturating_mul(2).saturating_add(STAGE_SLACK)
    }
}

/// What a finished stream did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamStats {
    pub frames: u64,
    pub depth: usize,
    /// Most frames alive at any moment.
    pub peak_in_flight: usize,
}

/// Geometry of one stream.
pub(crate) struct StreamSpec<'a> {
    pub count: u64,
    /// Bytes of the largest frame (source or output) a stage holds.
    pub frame_bytes: usize,
    pub limits: PipelineLimits,
    pub progress: &'a JobProgress,
    pub start_time: Instant,
}

/// Per-frame filters applied after decoding: scaling, LUT, timecode and caption burn-in.
pub(crate) struct FrameProcessor {
    pub scaler: FrameScaler,
    pub color: ColorTransform,
    pub lut: Option<Lut3D>,
    pub timecode: Option<TimecodeOverlay>,
    pub captions: Option<(CaptionBurner, CaptionTrack)>,
    pub fps: Rational64,
}

impl FrameProcessor {
    pub(crate) fn apply(&self, index: u64, src: &VideoFrame, out: &mut VideoFrame) -> Result<(), EncoderError> {
        self.scaler.scale_into(src, out)?;
        if let Some(lut) = &self.lut {
            lut.apply_to_frame(out)?;
        }
        if let Some(tc) = &self.timecode {
            tc.draw(out, &crate::pipeline::timecode_for_frame(index, self.fps))?;
        }
        if let Some((burner, track)) = &self.captions {
            burner.burn(out, track, index as f64 / self.fps.as_f64())?;
        }
        Ok(())
    }
}

/// Recycles output frame buffers so steady state allocates nothing per frame.
struct FramePool {
    free: Mutex<Vec<VideoFrame>>,
    cap: usize,
}

impl FramePool {
    fn new(cap: usize) -> Self {
        Self { free: Mutex::new(Vec::new()), cap }
    }

    /// A zeroed frame shaped like `template` at `dims`.
    fn take(&self, dims: Dimensions, template: &VideoFrame) -> Result<VideoFrame, EncoderError> {
        let reusable = {
            let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
            free.iter().position(|f| f.dimensions == dims && f.format == template.format).map(|i| free.swap_remove(i))
        };
        match reusable {
            Some(mut frame) => {
                frame.data_mut().fill(0);
                Ok(frame)
            }
            None => VideoFrame::new_allocated(dims, template.format, template.color_space, template.pts, template.duration, template.timebase),
        }
    }

    fn give(&self, frame: VideoFrame) {
        let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        if free.len() < self.cap {
            free.push(frame);
        }
    }
}

#[derive(Default)]
struct InFlight {
    now: AtomicUsize,
    peak: AtomicUsize,
}

impl InFlight {
    fn add(&self) {
        let now = self.now.fetch_add(1, Ordering::AcqRel) + 1;
        self.peak.fetch_max(now, Ordering::AcqRel);
    }

    fn sub(&self) {
        self.now.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Runs `f`, turning a panic in third-party or platform code into an error.
fn guarded<T>(stage: &str, f: impl FnOnce() -> Result<T, EncoderError>) -> Result<T, EncoderError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(EncoderError::Internal(format!("the {stage} stage panicked; the job was stopped"))))
}

type Item = Result<(u64, VideoFrame), EncoderError>;

/// Streams `spec.count` frames through `produce` (decode), `process` (convert/scale/filter) and
/// `sink` (encode and mux, called in order on the calling thread).
pub(crate) fn run_stages<P, X, K>(spec: &StreamSpec<'_>, dims: Dimensions, produce: P, process: X, mut sink: K) -> Result<StreamStats, EncoderError>
where
    P: Fn(u64) -> Result<VideoFrame, EncoderError> + Sync,
    X: Fn(u64, &VideoFrame, &mut VideoFrame) -> Result<(), EncoderError> + Sync,
    K: FnMut(u64, &mut VideoFrame) -> Result<(), EncoderError>,
{
    let minimum_working_set = spec.frame_bytes.saturating_mul(PipelineLimits::max_in_flight(1));
    if minimum_working_set > spec.limits.queue_bytes {
        return Err(EncoderError::InvalidJobSpec(format!(
            "minimum video pipeline working set is {} MiB, over the configured {} MiB frame budget",
            minimum_working_set >> 20,
            spec.limits.queue_bytes >> 20
        )));
    }
    let depth = spec.limits.depth_for(spec.frame_bytes);
    let pool = FramePool::new(1);
    let live = InFlight::default();
    let (decoded_tx, decoded_rx) = sync_channel::<Item>(depth);
    let (ready_tx, ready_rx) = sync_channel::<Item>(depth);
    let progress = spec.progress;
    let count = spec.count;
    let (pool, live, produce, process) = (&pool, &live, &produce, &process);

    let outcome = std::thread::scope(|scope| {
        let decoder = scope.spawn(move || {
            let result = guarded("decode", || {
                for index in 0..count {
                    progress.checkpoint()?;
                    let frame = produce(index)?;
                    live.add();
                    if decoded_tx.send(Ok((index, frame))).is_err() {
                        live.sub();
                        return Ok(());
                    }
                }
                Ok(())
            });
            if let Err(e) = result {
                let _ = decoded_tx.send(Err(e));
            }
        });

        let converter = scope.spawn(move || {
            let result = guarded("convert", || {
                for item in decoded_rx {
                    let (index, src) = item?;
                    let mut out = pool.take(dims, &src)?;
                    live.add();
                    let applied = process(index, &src, &mut out);
                    drop(src);
                    live.sub();
                    applied?;
                    progress.checkpoint()?;
                    if ready_tx.send(Ok((index, out))).is_err() {
                        live.sub();
                        return Ok(());
                    }
                }
                Ok(())
            });
            if let Err(e) = result {
                let _ = ready_tx.send(Err(e));
            }
        });

        let consumed = consume(ready_rx, spec, pool, live, &mut sink);
        // `consume` owns the receiver, so a failed or cancelled sink has already released both stages.
        let joined = [decoder.join(), converter.join()];
        if joined.iter().any(Result::is_err) && consumed.is_ok() {
            return Err(EncoderError::Internal("a pipeline stage thread terminated abnormally".into()));
        }
        consumed
    });

    let frames = outcome?;
    if frames != count {
        return Err(EncoderError::Internal(format!("the pipeline ended after {frames} of {count} frames")));
    }
    Ok(StreamStats { frames, depth, peak_in_flight: live.peak.load(Ordering::Acquire) })
}

fn consume<K>(
    rx: std::sync::mpsc::Receiver<Item>,
    spec: &StreamSpec<'_>,
    pool: &FramePool,
    live: &InFlight,
    sink: &mut K,
) -> Result<u64, EncoderError>
where
    K: FnMut(u64, &mut VideoFrame) -> Result<(), EncoderError>,
{
    let mut frames = 0u64;
    for item in rx {
        let (index, mut frame) = item?;
        spec.progress.checkpoint()?;
        guarded("encode", || sink(index, &mut frame))?;
        pool.give(frame);
        live.sub();
        frames += 1;
        spec.progress.advance_frame(spec.start_time);
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use std::time::Duration;

    fn dims() -> Dimensions {
        Dimensions::new(16, 16).unwrap()
    }

    fn frame(index: u64) -> Result<VideoFrame, EncoderError> {
        let mut f = VideoFrame::new_allocated(dims(), PixelFormat::Rgba8, ColorSpaceTag::SRGB, index as i64, 1, Rational64::FPS_24)?;
        f.data_mut().fill(index as u8);
        Ok(f)
    }

    fn copy(_: u64, src: &VideoFrame, out: &mut VideoFrame) -> Result<(), EncoderError> {
        out.data_mut().copy_from_slice(src.data());
        out.pts = src.pts;
        Ok(())
    }

    fn spec<'a>(count: u64, progress: &'a JobProgress, limits: PipelineLimits) -> StreamSpec<'a> {
        StreamSpec { count, frame_bytes: 16 * 16 * 4, limits, progress, start_time: Instant::now() }
    }

    #[test]
    fn frames_arrive_in_order_and_are_counted() {
        let progress = JobProgress::new(50);
        let mut seen = Vec::new();
        let stats = run_stages(&spec(50, &progress, PipelineLimits::default()), dims(), frame, copy, |i, f| {
            assert_eq!(f.pts, i as i64);
            assert_eq!(f.data()[0], i as u8);
            seen.push(i);
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, (0..50).collect::<Vec<_>>());
        assert_eq!(stats.frames, 50);
        assert_eq!(progress.processed_frames(), 50);
    }

    #[test]
    fn a_slow_consumer_never_lets_frames_pile_up() {
        let progress = JobProgress::new(200);
        let limits = PipelineLimits { queue_bytes: 16 * 16 * 4 * 12, max_depth: 8, audio_bytes: 0 };
        let depth = limits.depth_for(16 * 16 * 4);
        let stats = run_stages(&spec(200, &progress, limits), dims(), frame, copy, |_, _| {
            std::thread::sleep(Duration::from_micros(300));
            Ok(())
        })
        .unwrap();
        assert_eq!(stats.depth, depth);
        assert!(stats.peak_in_flight <= PipelineLimits::max_in_flight(depth), "peak {} over bound for depth {depth}", stats.peak_in_flight);
        assert!(stats.peak_in_flight >= depth, "queues were never filled, so back-pressure was not exercised");
    }

    #[test]
    fn multi_gibibyte_stream_keeps_in_flight_frames_bounded() {
        const FRAMES: u64 = 3_000_000;
        let progress = JobProgress::new(FRAMES);
        let limits = PipelineLimits::default();
        let stats = run_stages(&spec(FRAMES, &progress, limits), dims(), frame, copy, |_, _| Ok(())).unwrap();

        assert_eq!(stats.frames, FRAMES);
        assert!(stats.peak_in_flight <= PipelineLimits::max_in_flight(stats.depth));
        assert_eq!(progress.processed_frames(), FRAMES);
    }

    #[test]
    fn the_budget_chooses_the_depth() {
        let limits = PipelineLimits { queue_bytes: 100 << 20, max_depth: 8, audio_bytes: 0 };
        assert_eq!(limits.depth_for(4 << 20), 8);
        assert_eq!(limits.depth_for(8 << 20), 3);
        assert_eq!(limits.depth_for(33 << 20), 1);
        assert_eq!(limits.depth_for(1 << 30), 1);
        assert_eq!(PipelineLimits { queue_bytes: 0, max_depth: 0, audio_bytes: 0 }.depth_for(10), 1);
    }

    #[test]
    fn a_frame_larger_than_the_budget_is_rejected_before_streaming() {
        let progress = JobProgress::new(1);
        let limits = PipelineLimits { queue_bytes: 1, max_depth: 8, audio_bytes: 0 };
        let result = run_stages(&spec(1, &progress, limits), dims(), frame, copy, |_, _| Ok(()));
        assert!(matches!(result, Err(EncoderError::InvalidJobSpec(_))));
    }

    #[test]
    fn buffers_are_recycled_after_warm_up() {
        let progress = JobProgress::new(100);
        let seen_ptrs = Mutex::new(std::collections::HashSet::new());
        run_stages(&spec(100, &progress, PipelineLimits::default()), dims(), frame, copy, |_, f| {
            seen_ptrs.lock().unwrap().insert(f.data().as_ptr() as usize);
            Ok(())
        })
        .unwrap();
        let distinct = seen_ptrs.lock().unwrap().len();
        assert!(distinct < 100 / 2, "{distinct} distinct buffers for 100 frames: the pool is not reusing them");
    }

    #[test]
    fn a_decode_error_stops_the_job_after_the_frames_before_it() {
        let progress = JobProgress::new(20);
        let mut seen = 0;
        let err = run_stages(
            &spec(20, &progress, PipelineLimits::default()),
            dims(),
            |i| if i == 7 { Err(EncoderError::InvalidContainer("truncated".into())) } else { frame(i) },
            copy,
            |_, _| {
                seen += 1;
                Ok(())
            },
        )
        .unwrap_err();
        assert!(matches!(err, EncoderError::InvalidContainer(_)), "{err:?}");
        assert_eq!(seen, 7);
    }

    #[test]
    fn a_panic_in_any_stage_becomes_an_error() {
        let progress = JobProgress::new(10);
        let in_decode = run_stages(
            &spec(10, &progress, PipelineLimits::default()),
            dims(),
            |i| if i == 3 { panic!("decoder bug") } else { frame(i) },
            copy,
            |_, _| Ok(()),
        );
        assert!(matches!(in_decode, Err(EncoderError::Internal(m)) if m.contains("decode")));

        let in_convert = run_stages(&spec(10, &progress, PipelineLimits::default()), dims(), frame, |_, _, _| panic!("filter bug"), |_, _| Ok(()));
        assert!(matches!(in_convert, Err(EncoderError::Internal(m)) if m.contains("convert")));

        let in_encode =
            run_stages(
                &spec(10, &progress, PipelineLimits::default()),
                dims(),
                frame,
                copy,
                |i, _| if i == 2 { panic!("encoder bug") } else { Ok(()) },
            );
        assert!(matches!(in_encode, Err(EncoderError::Internal(m)) if m.contains("encode")));
    }

    #[test]
    fn a_sink_error_releases_blocked_stages() {
        let progress = JobProgress::new(10_000);
        let started = Instant::now();
        let err = run_stages(&spec(10_000, &progress, PipelineLimits::default()), dims(), frame, copy, |i, _| {
            if i == 5 { Err(EncoderError::Io("disk full".into())) } else { Ok(()) }
        })
        .unwrap_err();
        assert!(matches!(err, EncoderError::Io(_)));
        assert!(started.elapsed() < Duration::from_secs(5), "stages did not shut down");
    }

    #[test]
    fn cancel_is_prompt_and_reports_cancelled() {
        let progress = JobProgress::new(1_000_000);
        let started = Instant::now();
        let err = run_stages(&spec(1_000_000, &progress, PipelineLimits::default()), dims(), frame, copy, |i, _| {
            if i == 10 {
                progress.cancel();
            }
            std::thread::sleep(Duration::from_micros(100));
            Ok(())
        })
        .unwrap_err();
        assert!(matches!(err, EncoderError::Cancelled), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(progress.processed_frames() < 100);
    }

    #[test]
    fn pause_holds_the_stream_until_resumed() {
        let progress = JobProgress::new(30);
        progress.set_paused(true);
        let stats = std::thread::scope(|s| {
            let resume = s.spawn(|| {
                std::thread::sleep(Duration::from_millis(150));
                assert_eq!(progress.processed_frames(), 0, "frames moved while paused");
                progress.set_paused(false);
            });
            let stats = run_stages(&spec(30, &progress, PipelineLimits::default()), dims(), frame, copy, |_, _| Ok(()));
            resume.join().unwrap();
            stats
        });
        assert_eq!(stats.unwrap().frames, 30);
    }
}
