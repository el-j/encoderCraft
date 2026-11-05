//! Lockless atomic progress counters and cancellation tokens.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use encodercraft_core::EncoderError;

/// Lock-free progress reporter and cancellation token for background encoding jobs.
#[derive(Debug)]
pub struct JobProgress {
    processed_frames: AtomicU64,
    total_frames: AtomicU64,
    fps_fixed: AtomicU64, // 16.16 fixed point FPS
    target_start_nanos: AtomicU64,
    cancelled: AtomicBool,
    paused: AtomicBool,
    target_index: AtomicU64,
    target_count: AtomicU64,
}

impl Default for JobProgress {
    fn default() -> Self {
        Self::new(0)
    }
}

impl JobProgress {
    /// Creates a new progress tracker with the estimated total frame count.
    pub fn new(total_frames: u64) -> Self {
        Self {
            processed_frames: AtomicU64::new(0),
            total_frames: AtomicU64::new(total_frames),
            fps_fixed: AtomicU64::new(0),
            target_start_nanos: AtomicU64::new(0),
            cancelled: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            target_index: AtomicU64::new(0),
            target_count: AtomicU64::new(1),
        }
    }

    /// Declares how many output targets the job has; overall progress is spread evenly over them.
    pub fn set_target_count(&self, count: u64) {
        self.target_count.store(count.max(1), Ordering::Release);
    }

    /// Starts target `index`: its own frame counters restart, overall progress stays monotonic.
    pub fn begin_target(&self, index: u64) {
        self.reset_target(index, 0);
    }

    pub(crate) fn begin_target_at(&self, index: u64, start_time: Instant) {
        self.reset_target(index, start_time.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
    }

    fn reset_target(&self, index: u64, start_nanos: u64) {
        self.processed_frames.store(0, Ordering::Release);
        self.total_frames.store(0, Ordering::Release);
        self.fps_fixed.store(0, Ordering::Release);
        self.target_start_nanos.store(start_nanos, Ordering::Release);
        self.target_index.store(index, Ordering::Release);
    }

    /// Zero-based index of the target currently encoding.
    pub fn target_index(&self) -> u64 {
        self.target_index.load(Ordering::Acquire)
    }

    /// Number of targets in the job.
    pub fn target_count(&self) -> u64 {
        self.target_count.load(Ordering::Acquire)
    }

    /// Sets the total expected frame count once known.
    pub fn set_total_frames(&self, total: u64) {
        self.total_frames.store(total, Ordering::Release);
    }

    /// Increments processed frame count by one and updates instantaneous FPS.
    pub fn advance_frame(&self, start_time: Instant) {
        let count = self.processed_frames.fetch_add(1, Ordering::AcqRel) + 1;
        let elapsed_nanos =
            ((start_time.elapsed().as_nanos().min(u128::from(u64::MAX))) as u64).saturating_sub(self.target_start_nanos.load(Ordering::Acquire));
        let elapsed_sec = elapsed_nanos as f64 / 1_000_000_000.0;
        if elapsed_sec > 0.05 {
            let fps = (count as f64) / elapsed_sec;
            let fixed = (fps * 65536.0).min(u64::MAX as f64) as u64;
            self.fps_fixed.store(fixed, Ordering::Release);
        }
    }

    /// Returns the number of frames completed.
    pub fn processed_frames(&self) -> u64 {
        self.processed_frames.load(Ordering::Acquire)
    }

    /// Returns the total number of frames in the job.
    pub fn total_frames(&self) -> u64 {
        self.total_frames.load(Ordering::Acquire)
    }

    /// Returns completion progress percentage in the range [0.0, 100.0].
    pub fn percent_complete(&self) -> f64 {
        let count = self.target_count().max(1) as f64;
        let index = (self.target_index() as f64).min(count);
        let total = self.total_frames();
        let within = if total == 0 { 0.0 } else { (self.processed_frames() as f64 / total as f64).clamp(0.0, 1.0) };
        ((index + within) / count * 100.0).clamp(0.0, 100.0)
    }

    /// Current instantaneous encoding speed in frames per second.
    pub fn current_fps(&self) -> f64 {
        let fixed = self.fps_fixed.load(Ordering::Acquire);
        (fixed as f64) / 65536.0
    }

    /// Estimated seconds remaining in the current target, or `None` before a usable rate exists.
    pub fn eta_seconds(&self) -> Option<u64> {
        let remaining = self.total_frames().saturating_sub(self.processed_frames());
        let fps = self.current_fps();
        if fps <= 0.0 || !fps.is_finite() {
            return None;
        }
        let seconds = (remaining as f64 / fps).ceil();
        Some(seconds.min(u64::MAX as f64) as u64)
    }

    /// Signals cancellation of the running encoding job.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns true if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Pauses or unpauses execution.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    /// Returns true if the job is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    /// Cooperative control point for encode loops: blocks while the job is paused and
    /// returns `EncoderError::Cancelled` as soon as cancellation is requested.
    pub fn checkpoint(&self) -> Result<(), EncoderError> {
        while self.is_paused() && !self.is_cancelled() {
            std::thread::sleep(Duration::from_millis(20));
        }
        if self.is_cancelled() {
            return Err(EncoderError::Cancelled);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_accounting() {
        let progress = JobProgress::new(100);
        let start = Instant::now();

        assert_eq!(progress.percent_complete(), 0.0);
        progress.advance_frame(start);
        assert_eq!(progress.processed_frames(), 1);
        assert_eq!(progress.percent_complete(), 1.0);

        assert!(!progress.is_cancelled());
        progress.cancel();
        assert!(progress.is_cancelled());
    }

    #[test]
    fn overall_progress_is_weighted_across_targets_and_monotonic() {
        let progress = JobProgress::new(0);
        progress.set_target_count(2);
        let start = Instant::now();
        let mut last = 0.0;
        for target in 0..2 {
            progress.begin_target(target);
            progress.set_total_frames(4);
            for _ in 0..4 {
                progress.advance_frame(start);
                let pct = progress.percent_complete();
                assert!(pct >= last, "progress went backwards: {last} -> {pct}");
                last = pct;
            }
        }
        assert_eq!(progress.target_index(), 1);
        assert_eq!(last, 100.0);
    }

    #[test]
    fn eta_uses_the_current_target_rate_and_resets_between_targets() {
        let progress = JobProgress::new(100);
        let start = Instant::now();
        progress.set_target_count(2);
        progress.begin_target_at(0, start);
        progress.set_total_frames(100);
        assert_eq!(progress.eta_seconds(), None);
        std::thread::sleep(Duration::from_millis(60));
        for _ in 0..10 {
            progress.advance_frame(start);
        }
        let eta = progress.eta_seconds().unwrap();
        assert!((1..=2).contains(&eta), "unexpected ETA: {eta}");
        assert!(progress.current_fps() > 0.0);

        progress.begin_target_at(1, start);
        assert_eq!(progress.current_fps(), 0.0);
        assert_eq!(progress.eta_seconds(), None);
    }

    #[test]
    fn test_checkpoint_reports_cancellation() {
        let progress = JobProgress::new(10);
        assert!(progress.checkpoint().is_ok());
        progress.cancel();
        assert!(matches!(progress.checkpoint(), Err(EncoderError::Cancelled)));
    }

    #[test]
    fn test_checkpoint_blocks_while_paused_and_resumes() {
        let progress = std::sync::Arc::new(JobProgress::new(10));
        progress.set_paused(true);
        let worker = std::sync::Arc::clone(&progress);
        let handle = std::thread::spawn(move || worker.checkpoint());
        std::thread::sleep(Duration::from_millis(80));
        assert!(!handle.is_finished());
        progress.set_paused(false);
        assert!(matches!(handle.join(), Ok(Ok(()))));
    }

    #[test]
    fn test_checkpoint_paused_job_can_still_be_cancelled() {
        let progress = std::sync::Arc::new(JobProgress::new(10));
        progress.set_paused(true);
        let worker = std::sync::Arc::clone(&progress);
        let handle = std::thread::spawn(move || worker.checkpoint());
        progress.cancel();
        assert!(matches!(handle.join(), Ok(Err(EncoderError::Cancelled))));
    }
}
