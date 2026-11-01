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

/// Persistent snapshot of the queue state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueJournal {
    pub version: u32,
    pub active_parallelism: usize,
    pub jobs: Vec<QueuedJob>,
}

impl Default for QueueJournal {
    fn default() -> Self {
        Self { version: 1, active_parallelism: 2, jobs: Vec::new() }
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
