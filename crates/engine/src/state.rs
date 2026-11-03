//! Queue state models and atomic filesystem journaling.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use encodercraft_core::{CraftJobSpec, EncoderError, JobStatus};
use serde::{Deserialize, Serialize};

/// Snapshot of an individual job in the encoding queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueuedJob {
    pub spec: CraftJobSpec,
    pub status: JobStatus,
    pub progress_percent: f64,
    pub current_fps: f64,
    pub added_at_unix_ms: u64,
    pub completed_at_unix_ms: Option<u64>,
    pub error_message: Option<String>,
}

impl QueuedJob {
    /// Creates a new queued job instance with default initial state.
    pub fn new(spec: CraftJobSpec) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);

        Self {
            spec,
            status: JobStatus::Queued,
            progress_percent: 0.0,
            current_fps: 0.0,
            added_at_unix_ms: now,
            completed_at_unix_ms: None,
            error_message: None,
        }
    }
}

/// Historical log entry recording an executed transcode job (AME Parity).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp_unix_ms: u64,
    pub job_id: String,
    pub source_name: String,
    pub preset_name: String,
    pub output_path: String,
    pub duration_ms: u64,
    pub status: String,
    pub error: Option<String>,
}

/// User application preferences and defaults (AME Parity).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppPreferences {
    /// Schema of the stored file; files written before versioning load as 0 and are migrated.
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default = "default_true")]
    pub auto_start_queue: bool,
    #[serde(default = "default_true")]
    pub play_completion_sound: bool,
    #[serde(default)]
    pub remove_finished_on_exit: bool,
    #[serde(default)]
    pub default_output_dir: Option<std::path::PathBuf>,
    #[serde(default = "default_parallelism")]
    pub max_parallel_jobs: usize,
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_true() -> bool {
    true
}

fn default_parallelism() -> usize {
    2
}

fn default_theme() -> String {
    "Dark".into()
}

/// Current preferences file schema.
pub const PREFERENCES_SCHEMA_VERSION: u32 = 1;

impl AppPreferences {
    /// Upgrades older files and clamps values a hand-edited file could set out of range.
    pub fn migrate(&mut self) {
        if self.schema_version < PREFERENCES_SCHEMA_VERSION {
            self.schema_version = PREFERENCES_SCHEMA_VERSION;
        }
        self.max_parallel_jobs = self.max_parallel_jobs.clamp(1, 8);
        if self.theme != "Dark" && self.theme != "High Contrast" {
            self.theme = default_theme();
        }
    }
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            auto_start_queue: true,
            play_completion_sound: true,
            remove_finished_on_exit: false,
            default_output_dir: None,
            max_parallel_jobs: 2,
            theme: "Dark".into(),
        }
    }
}

/// Persistent snapshot of the queue state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueJournal {
    pub version: u32,
    pub active_parallelism: usize,
    pub jobs: Vec<QueuedJob>,
    #[serde(default)]
    pub log_entries: Vec<LogEntry>,
}

impl Default for QueueJournal {
    fn default() -> Self {
        Self { version: 1, active_parallelism: 2, jobs: Vec::new(), log_entries: Vec::new() }
    }
}

/// Atomically saves the queue journal to disk using a temporary file and rename.
pub fn save_journal_atomic(journal_path: &Path, journal: &QueueJournal) -> Result<(), EncoderError> {
    if let Some(parent) = journal_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| EncoderError::Io(format!("failed to create journal directory: {e}")))?;
    }

    let tmp_path = journal_path.with_extension("tmp");
    let json_bytes = serde_json::to_vec_pretty(journal).map_err(|e| EncoderError::Internal(format!("failed to serialize queue journal: {e}")))?;

    let mut tmp_file = File::create(&tmp_path).map_err(|e| EncoderError::Io(format!("failed to create temp journal: {e}")))?;
    tmp_file.write_all(&json_bytes).map_err(|e| EncoderError::Io(format!("failed to write temp journal: {e}")))?;
    tmp_file.sync_all().map_err(|e| EncoderError::Io(format!("failed to sync temp journal: {e}")))?;

    std::fs::rename(&tmp_path, journal_path).map_err(|e| EncoderError::Io(format!("failed to atomically commit journal: {e}")))?;

    Ok(())
}

/// Loads the queue journal from disk, recovering from interrupted jobs if necessary.
pub fn load_journal(journal_path: &Path) -> Result<QueueJournal, EncoderError> {
    if !journal_path.exists() {
        return Ok(QueueJournal::default());
    }

    let file = File::open(journal_path).map_err(|e| EncoderError::Io(format!("failed to open queue journal: {e}")))?;
    let mut journal: QueueJournal =
        serde_json::from_reader(file).map_err(|e| EncoderError::Internal(format!("failed to deserialize queue journal: {e}")))?;

    // Crash recovery: Any job that was `Processing` when the process exited becomes `Queued` (or marked interrupted)
    for job in &mut journal.jobs {
        if matches!(job.status, JobStatus::Processing) {
            job.status = JobStatus::Queued;
            job.error_message = Some("Job was interrupted by unexpected process exit; reset to Queued".into());
        }
    }

    Ok(journal)
}

/// Loads application preferences from disk, returning defaults if not found.
pub fn load_preferences(prefs_path: &Path) -> Result<AppPreferences, EncoderError> {
    if !prefs_path.exists() {
        return Ok(AppPreferences::default());
    }

    let file = File::open(prefs_path).map_err(|e| EncoderError::Io(format!("failed to open preferences: {e}")))?;
    let mut prefs: AppPreferences =
        serde_json::from_reader(file).map_err(|e| EncoderError::Internal(format!("failed to deserialize preferences: {e}")))?;
    prefs.migrate();
    Ok(prefs)
}

/// Atomically saves application preferences to disk.
pub fn save_preferences_atomic(prefs_path: &Path, prefs: &AppPreferences) -> Result<(), EncoderError> {
    if let Some(parent) = prefs_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| EncoderError::Io(format!("failed to create preferences directory: {e}")))?;
    }

    let tmp_path = prefs_path.with_extension("tmp");
    let json_bytes = serde_json::to_vec_pretty(prefs).map_err(|e| EncoderError::Internal(format!("failed to serialize preferences: {e}")))?;

    let mut tmp_file = File::create(&tmp_path).map_err(|e| EncoderError::Io(format!("failed to create temp preferences: {e}")))?;
    tmp_file.write_all(&json_bytes).map_err(|e| EncoderError::Io(format!("failed to write temp preferences: {e}")))?;
    tmp_file.sync_all().map_err(|e| EncoderError::Io(format!("failed to sync temp preferences: {e}")))?;

    std::fs::rename(&tmp_path, prefs_path).map_err(|e| EncoderError::Io(format!("failed to atomically commit preferences: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod prefs_tests {
    use super::*;

    #[test]
    fn unversioned_file_is_migrated_and_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.json");
        std::fs::write(&path, r#"{"max_parallel_jobs":99,"theme":"Neon","auto_start_queue":false}"#).unwrap();
        let p = load_preferences(&path).unwrap();
        assert_eq!(p.schema_version, PREFERENCES_SCHEMA_VERSION);
        assert_eq!(p.max_parallel_jobs, 8);
        assert_eq!(p.theme, "Dark");
        assert!(!p.auto_start_queue);
    }

    #[test]
    fn preferences_round_trip_and_corruption_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.json");
        let p = AppPreferences { max_parallel_jobs: 4, ..AppPreferences::default() };
        save_preferences_atomic(&path, &p).unwrap();
        assert_eq!(load_preferences(&path).unwrap(), p);
        std::fs::write(&path, "{broken").unwrap();
        assert!(load_preferences(&path).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, ContainerType, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_journal_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("queue-journal.json");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "job-1".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 100, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: PathBuf::from("in.mov"), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: PathBuf::from("out.mp4"),
                preset_name: "Web".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: None,
                custom_height: None,
                custom_frame_rate: None,
                target_bitrate_kbps: None,
                target_lufs: None,
                ..Default::default()
            }],
        };

        let mut journal = QueueJournal::default();
        journal.jobs.push(QueuedJob::new(spec));

        save_journal_atomic(&path, &journal).unwrap();
        assert!(path.exists());

        let loaded = load_journal(&path).unwrap();
        assert_eq!(loaded.jobs.len(), 1);
        assert_eq!(loaded.jobs[0].spec.job_id, "job-1");
    }
}
