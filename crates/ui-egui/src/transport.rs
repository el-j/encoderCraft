//! Playback transport for the preview monitor: a monotonic clock at the source rate, JKL
//! shuttle, frame stepping, In/Out range and timecode formatting. Pure logic, no UI.

use std::time::Instant;

/// Shuttle speeds reached by repeated J / L presses.
const SPEEDS: [f64; 4] = [1.0, 2.0, 4.0, 8.0];

#[derive(Debug, Clone)]
pub struct Transport {
    fps: f64,
    frame_count: u64,
    position: u64,
    pub in_point: Option<u64>,
    pub out_point: Option<u64>,
    /// Signed playback rate; 0 when stopped, negative when playing backwards.
    rate: f64,
    clock: Option<Clock>,
    pub loop_range: bool,
}

#[derive(Debug, Clone, Copy)]
struct Clock {
    started: Instant,
    origin: f64,
}

impl Default for Transport {
    fn default() -> Self {
        Self { fps: 30.0, frame_count: 0, position: 0, in_point: None, out_point: None, rate: 0.0, clock: None, loop_range: false }
    }
}

impl Transport {
    pub fn set_media(&mut self, fps: f64, frame_count: u64) {
        self.fps = if fps.is_finite() && fps > 0.0 { fps } else { 30.0 };
        self.frame_count = frame_count;
        self.position = self.position.min(self.last_frame());
    }

    pub fn fps(&self) -> f64 {
        self.fps
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn last_frame(&self) -> u64 {
        self.frame_count.saturating_sub(1)
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    pub fn is_playing(&self) -> bool {
        self.rate != 0.0
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Inclusive playback range: the In/Out points, or the whole clip.
    pub fn range(&self) -> (u64, u64) {
        let last = self.last_frame();
        let lo = self.in_point.unwrap_or(0).min(last);
        let hi = self.out_point.unwrap_or(last).min(last);
        (lo.min(hi), lo.max(hi))
    }

    pub fn seek(&mut self, frame: u64) {
        self.position = frame.min(self.last_frame());
        if self.is_playing() {
            self.clock = Some(Clock { started: Instant::now(), origin: self.position as f64 });
        }
    }

    pub fn step(&mut self, delta: i64) {
        self.stop();
        let target = (self.position as i64).saturating_add(delta).clamp(0, self.last_frame() as i64);
        self.position = target as u64;
    }

    pub fn home(&mut self) {
        let (lo, _) = self.range();
        self.seek(lo);
    }

    pub fn end(&mut self) {
        let (_, hi) = self.range();
        self.seek(hi);
    }

    pub fn stop(&mut self) {
        self.rate = 0.0;
        self.clock = None;
    }

    /// Space bar: play forward at 1x, or stop.
    pub fn toggle_play(&mut self, now: Instant) {
        if self.is_playing() {
            self.stop();
        } else {
            let (lo, hi) = self.range();
            if self.position >= hi {
                self.position = lo;
            }
            self.set_rate(1.0, now);
        }
    }

    /// L: play forward; each further press doubles the speed up to 8x.
    pub fn shuttle_forward(&mut self, now: Instant) {
        self.shuttle(1.0, now);
    }

    /// J: play backwards; each further press doubles the speed up to 8x.
    pub fn shuttle_reverse(&mut self, now: Instant) {
        self.shuttle(-1.0, now);
    }

    fn shuttle(&mut self, direction: f64, now: Instant) {
        let next = if self.rate * direction > 0.0 {
            let magnitude = self.rate.abs();
            SPEEDS.iter().copied().find(|s| *s > magnitude).unwrap_or(SPEEDS[SPEEDS.len() - 1])
        } else {
            SPEEDS[0]
        };
        self.set_rate(next * direction, now);
    }

    fn set_rate(&mut self, rate: f64, now: Instant) {
        self.rate = rate;
        self.clock = Some(Clock { started: now, origin: self.position as f64 });
    }

    pub fn set_in(&mut self) {
        self.in_point = Some(self.position);
        if self.out_point.is_some_and(|o| o < self.position) {
            self.out_point = None;
        }
    }

    pub fn set_out(&mut self) {
        self.out_point = Some(self.position);
        if self.in_point.is_some_and(|i| i > self.position) {
            self.in_point = None;
        }
    }

    pub fn clear_range(&mut self) {
        self.in_point = None;
        self.out_point = None;
    }

    /// Moves the playhead to where the clock says it should be. Frames that were due while the
    /// UI was busy are skipped, never queued. Returns true when the position changed.
    pub fn advance(&mut self, now: Instant) -> bool {
        let Some(clock) = self.clock else { return false };
        if self.frame_count == 0 {
            self.stop();
            return false;
        }
        let elapsed = now.saturating_duration_since(clock.started).as_secs_f64();
        let ideal = clock.origin + elapsed * self.rate * self.fps;
        let (lo, hi) = self.range();
        let (lo_f, hi_f) = (lo as f64, hi as f64);
        let before = self.position;
        if ideal > hi_f || ideal < lo_f {
            if self.loop_range && hi > lo {
                let span = (hi - lo + 1) as f64;
                let wrapped = lo_f + (ideal - lo_f).rem_euclid(span);
                self.position = wrapped.floor() as u64;
            } else {
                self.position = if ideal > hi_f { hi } else { lo };
                self.stop();
            }
        } else {
            self.position = ideal.floor() as u64;
        }
        self.position != before
    }
}

/// SMPTE timecode. Drop-frame numbering is used for 29.97 and 59.94 fps material.
pub fn timecode(frame: u64, fps: f64) -> String {
    let nominal = fps.round().max(1.0) as u64;
    let drop = if (fps - 30_000.0 / 1_001.0).abs() < 0.01 || (fps - 60_000.0 / 1_001.0).abs() < 0.01 { nominal / 15 } else { 0 };
    let mut n = frame;
    if drop > 0 {
        let per_10min = nominal * 600 - drop * 9;
        let per_min = nominal * 60 - drop;
        let tens = n / per_10min;
        let rem = n % per_10min;
        n += drop * 9 * tens;
        if rem >= drop {
            n += drop * ((rem - drop) / per_min);
        }
    }
    let frames = n % nominal;
    let secs = n / nominal;
    let sep = if drop > 0 { ';' } else { ':' };
    format!("{:02}:{:02}:{:02}{sep}{frames:02}", secs / 3600, (secs / 60) % 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ready(frames: u64) -> (Transport, Instant) {
        let mut t = Transport::default();
        t.set_media(25.0, frames);
        (t, Instant::now())
    }

    #[test]
    fn plays_at_the_source_rate_and_skips_late_frames() {
        let (mut t, t0) = ready(1000);
        t.toggle_play(t0);
        assert!(t.advance(t0 + Duration::from_millis(1000)));
        assert_eq!(t.position(), 25);
        // A long UI stall skips frames instead of replaying them one by one.
        t.advance(t0 + Duration::from_millis(3000));
        assert_eq!(t.position(), 75);
    }

    #[test]
    fn stops_at_the_out_point_and_can_loop_the_range() {
        let (mut t, t0) = ready(1000);
        t.in_point = Some(100);
        t.out_point = Some(124);
        t.seek(100);
        t.toggle_play(t0);
        t.advance(t0 + Duration::from_secs(5));
        assert_eq!((t.position(), t.is_playing()), (124, false));

        t.loop_range = true;
        t.seek(100);
        t.toggle_play(t0);
        t.advance(t0 + Duration::from_millis(1040));
        assert!(t.is_playing());
        assert_eq!(t.position(), 101, "26 frames into a 25 frame loop");
    }

    #[test]
    fn jkl_shuttle_doubles_speed_and_reverses() {
        let (mut t, t0) = ready(1000);
        t.seek(500);
        t.shuttle_forward(t0);
        assert_eq!(t.rate(), 1.0);
        t.shuttle_forward(t0);
        t.shuttle_forward(t0);
        assert_eq!(t.rate(), 4.0);
        t.shuttle_reverse(t0);
        assert_eq!(t.rate(), -1.0);
        t.advance(t0 + Duration::from_secs(1));
        assert_eq!(t.position(), 475);
        t.stop();
        assert!(!t.is_playing());
    }

    #[test]
    fn stepping_clamps_and_stops_playback() {
        let (mut t, t0) = ready(10);
        t.toggle_play(t0);
        t.step(-5);
        assert_eq!((t.position(), t.is_playing()), (0, false));
        t.step(100);
        assert_eq!(t.position(), 9);
    }

    #[test]
    fn range_marking_keeps_in_before_out() {
        let (mut t, _) = ready(100);
        t.seek(50);
        t.set_in();
        t.seek(20);
        t.set_out();
        assert_eq!((t.in_point, t.out_point), (None, Some(20)));
        t.seek(10);
        t.set_in();
        assert_eq!(t.range(), (10, 20));
        t.home();
        assert_eq!(t.position(), 10);
        t.end();
        assert_eq!(t.position(), 20);
        t.clear_range();
        assert_eq!(t.range(), (0, 99));
    }

    #[test]
    fn empty_media_never_moves() {
        let mut t = Transport::default();
        let t0 = Instant::now();
        t.toggle_play(t0);
        assert!(!t.advance(t0 + Duration::from_secs(1)));
        assert!(!t.is_playing());
    }

    #[test]
    fn timecode_non_drop_and_drop_frame() {
        assert_eq!(timecode(0, 25.0), "00:00:00:00");
        assert_eq!(timecode(25 * 3661 + 7, 25.0), "01:01:01:07");
        let ntsc = 30_000.0 / 1_001.0;
        assert_eq!(timecode(1799, ntsc), "00:00:59;29");
        assert_eq!(timecode(1800, ntsc), "00:01:00;02");
        assert_eq!(timecode(17_982, ntsc), "00:10:00;00");
    }
}
