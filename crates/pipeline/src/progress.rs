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
    cancelled: AtomicBool,
    paused: AtomicBool,
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
            cancelled: AtomicBool::new(false),
            paused: AtomicBool::new(false),
        }
    }

    /// Sets the total expected frame count once known.
    pub fn set_total_frames(&self, total: u64) {
        self.total_frames.store(total, Ordering::Release);
    }

    /// Increments processed frame count by one and updates instantaneous FPS.
    pub fn advance_frame(&self, start_time: Instant) {
        let count = self.processed_frames.fetch_add(1, Ordering::AcqRel) + 1;
        let elapsed_sec = start_time.elapsed().as_secs_f64();
        if elapsed_sec > 0.05 {
            let fps = (count as f64) / elapsed_sec;
            let fixed = (fps * 65536.0) as u64;
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
        let total = self.total_frames();
        if total == 0 {
            return 0.0;
        }
        let done = self.processed_frames();
        ((done as f64) / (total as f64) * 100.0).clamp(0.0, 100.0)
    }

    /// Current instantaneous encoding speed in frames per second.
    pub fn current_fps(&self) -> f64 {
        let fixed = self.fps_fixed.load(Ordering::Acquire);
        (fixed as f64) / 65536.0
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
