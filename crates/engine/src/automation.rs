//! Queue automation, file collision resolving, and completion alert utilities (AME Parity).

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Automatically increments a destination file path with `_1`, `_2`, etc. if the file
/// already exists on disk and `increment` is true.
pub fn resolve_unique_output_path(path: &Path, increment: bool) -> PathBuf {
    if !increment || !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let ext = path.extension().and_then(|s| s.to_str());

    // Check if stem already ends with _<number>
    let (base_stem, start_num) = if let Some(idx) = stem.rfind('_') {
        let suffix = &stem[idx + 1..];
        if let Ok(n) = suffix.parse::<u32>() { (&stem[..idx], n.saturating_add(1)) } else { (stem, 1) }
    } else {
        (stem, 1)
    };

    let mut counter = start_num;
    loop {
        let new_stem = format!("{base_stem}_{counter}");
        let candidate = match ext {
            Some(e) => parent.join(format!("{new_stem}.{e}")),
            None => parent.join(new_stem),
        };

        if !candidate.exists() {
            return candidate;
        }

        counter = counter.saturating_add(1);
        if counter == u32::MAX {
            return candidate;
        }
    }
}

/// Plays a non-blocking audio completion chime alert in a background thread.
/// Safely handles platform differences and never panics if audio output or system sound is missing.
pub fn play_completion_chime() {
    let _ = std::thread::Builder::new().name("encodercraft-chime".into()).spawn(|| {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("afplay").arg("/System/Library/Sounds/Glass.aiff").output();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("paplay")
                .arg("/usr/share/sounds/freedesktop/stereo/complete.oga")
                .output()
                .or_else(|_| std::process::Command::new("aplay").arg("/usr/share/sounds/alsa/Front_Center.wav").output());
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", "[System.Media.SystemSounds]::Asterisk.Play()"])
                .output();
        }
    });
}

/// Determines if the queue should auto-start due to user inactivity.
pub fn should_idle_auto_start(
    auto_start_enabled: bool,
    idle_elapsed: Duration,
    idle_timeout_minutes: u32,
    has_pending_jobs: bool,
    is_worker_running: bool,
) -> bool {
    if !auto_start_enabled || is_worker_running || !has_pending_jobs {
        return false;
    }

    let required_duration = Duration::from_secs(idle_timeout_minutes as u64 * 60);
    idle_elapsed >= required_duration
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_unique_output_path_nonexistent() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("video.mp4");
        assert_eq!(resolve_unique_output_path(&target, true), target);
        assert_eq!(resolve_unique_output_path(&target, false), target);
    }

    #[test]
    fn test_resolve_unique_output_path_existing_increments() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("render.mov");
        std::fs::write(&target, b"dummy").unwrap();

        let unique = resolve_unique_output_path(&target, true);
        assert_eq!(unique, dir.path().join("render_1.mov"));

        // If render_1 also exists, increments to render_2
        std::fs::write(&unique, b"dummy2").unwrap();
        let unique_2 = resolve_unique_output_path(&target, true);
        assert_eq!(unique_2, dir.path().join("render_2.mov"));
    }

    #[test]
    fn test_resolve_unique_output_path_disabled() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("output.mp4");
        std::fs::write(&target, b"existing").unwrap();

        let path = resolve_unique_output_path(&target, false);
        assert_eq!(path, target);
    }

    #[test]
    fn test_should_idle_auto_start() {
        assert!(!should_idle_auto_start(false, Duration::from_secs(300), 2, true, false));
        assert!(!should_idle_auto_start(true, Duration::from_secs(300), 2, false, false));
        assert!(!should_idle_auto_start(true, Duration::from_secs(300), 2, true, true));
        assert!(!should_idle_auto_start(true, Duration::from_secs(60), 2, true, false));
        assert!(should_idle_auto_start(true, Duration::from_secs(120), 2, true, false));
        assert!(should_idle_auto_start(true, Duration::from_secs(180), 2, true, false));
    }
}
