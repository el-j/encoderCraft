//! Queue footer: items-processed counter, Auto Computer Shutdown and the renderer choice.
//!
//! The shutdown decision is a pure state machine so it can be tested without powering anything off.

use encodercraft_core::JobStatus;
use encodercraft_engine::QueuedJob;
use std::time::{Duration, Instant};

/// Seconds the user has to cancel before the computer shuts down.
pub const SHUTDOWN_COUNTDOWN_SECS: u64 = 60;

/// Counts finished jobs against the total, e.g. `"2 of 5 items processed"`.
pub fn processed_counter(jobs: &[QueuedJob]) -> String {
    let done = jobs.iter().filter(|j| matches!(j.status, JobStatus::Completed | JobStatus::Failed(_))).count();
    format!("{done} of {} items processed", jobs.len())
}

/// True when the queue has work and every job has finished (completed or failed).
pub fn queue_finished(jobs: &[QueuedJob]) -> bool {
    !jobs.is_empty() && jobs.iter().all(|j| matches!(j.status, JobStatus::Completed | JobStatus::Failed(_)))
}

/// What the app must do after a shutdown watcher update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownAction {
    Idle,
    Countdown { remaining_secs: u64 },
    ShutDownNow,
}

/// Arms when the queue goes from working to finished, then counts down and fires once.
#[derive(Debug, Default)]
pub struct ShutdownWatch {
    was_working: bool,
    deadline: Option<Instant>,
}

impl ShutdownWatch {
    pub fn update(&mut self, enabled: bool, finished: bool, now: Instant) -> ShutdownAction {
        if !finished {
            self.was_working = true;
            self.deadline = None;
            return ShutdownAction::Idle;
        }
        if !enabled {
            self.deadline = None;
            return ShutdownAction::Idle;
        }
        if self.was_working && self.deadline.is_none() {
            self.was_working = false;
            self.deadline = Some(now + Duration::from_secs(SHUTDOWN_COUNTDOWN_SECS));
        }
        match self.deadline {
            Some(deadline) if now >= deadline => {
                self.deadline = None;
                ShutdownAction::ShutDownNow
            }
            Some(deadline) => ShutdownAction::Countdown { remaining_secs: deadline.saturating_duration_since(now).as_secs() + 1 },
            None => ShutdownAction::Idle,
        }
    }

    /// Abandons a running countdown; it re-arms only after the queue works again.
    pub fn cancel(&mut self) {
        self.deadline = None;
        self.was_working = false;
    }
}

/// Program and arguments that power the machine off on this platform.
pub fn shutdown_command() -> (&'static str, &'static [&'static str]) {
    if cfg!(windows) {
        ("shutdown", &["/s", "/t", "0"])
    } else if cfg!(target_os = "macos") {
        ("osascript", &["-e", "tell application \"System Events\" to shut down"])
    } else {
        ("systemctl", &["poweroff"])
    }
}

/// Requests the shutdown; returns the error text when the system refuses.
pub fn shut_down_computer() -> Result<(), String> {
    let (program, args) = shutdown_command();
    let status = std::process::Command::new(program).args(args).status().map_err(|e| format!("could not run {program}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{program} exited with {status}")) }
}

/// Render back ends that really work. Hardware encoders are listed only once the pipeline can use them.
pub const RENDERERS: &[&str] = &["Software (CPU)"];

/// Returns the detected hardware and software render backends for the host platform.
pub fn available_renderers() -> Vec<String> {
    encodercraft_platform::available_renderers()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_once_after_the_countdown() {
        let mut w = ShutdownWatch::default();
        let t0 = Instant::now();
        assert_eq!(w.update(true, false, t0), ShutdownAction::Idle);
        assert_eq!(w.update(true, true, t0), ShutdownAction::Countdown { remaining_secs: SHUTDOWN_COUNTDOWN_SECS + 1 });
        let late = t0 + Duration::from_secs(SHUTDOWN_COUNTDOWN_SECS);
        assert_eq!(w.update(true, true, late), ShutdownAction::ShutDownNow);
        assert_eq!(w.update(true, true, late), ShutdownAction::Idle);
    }

    #[test]
    fn never_arms_without_work_or_when_disabled() {
        let mut w = ShutdownWatch::default();
        let t0 = Instant::now();
        assert_eq!(w.update(true, true, t0), ShutdownAction::Idle);
        w.update(false, false, t0);
        assert_eq!(w.update(false, true, t0), ShutdownAction::Idle);
    }

    #[test]
    fn cancel_and_new_work_stop_the_countdown() {
        let mut w = ShutdownWatch::default();
        let t0 = Instant::now();
        w.update(true, false, t0);
        assert!(matches!(w.update(true, true, t0), ShutdownAction::Countdown { .. }));
        w.cancel();
        assert_eq!(w.update(true, true, t0 + Duration::from_secs(500)), ShutdownAction::Idle);
        w.update(true, false, t0);
        assert!(matches!(w.update(true, true, t0), ShutdownAction::Countdown { .. }));
        w.update(true, false, t0);
        assert_eq!(w.update(true, false, t0 + Duration::from_secs(500)), ShutdownAction::Idle);
    }

    #[test]
    fn command_is_defined() {
        let (program, _) = shutdown_command();
        assert!(!program.is_empty());
    }
}
