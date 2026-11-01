//! Multi-job execution scheduler and queue controller.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use encodercraft_core::{CraftJobSpec, EncoderError, JobPriority, JobStatus};
use encodercraft_pipeline::TranscodePipeline;

use crate::state::{QueueJournal, QueuedJob, load_journal, save_journal_atomic};

/// Central queue manager orchestrating job submission, priority scheduling, and persistence.
pub struct QueueManager {
    journal_path: Option<PathBuf>,
    state: Mutex<QueueJournal>,
}

impl QueueManager {
    /// Creates an in-memory queue manager without filesystem persistence.
    pub fn new_in_memory() -> Self {
        Self { journal_path: None, state: Mutex::new(QueueJournal::default()) }
    }

    /// Creates a queue manager persisted to disk at the specified journal path.
    pub fn new_persisted(journal_path: PathBuf) -> Result<Self, EncoderError> {
        let journal = load_journal(&journal_path)?;
        Ok(Self { journal_path: Some(journal_path), state: Mutex::new(journal) })
    }

    /// Adds a new job to the queue, ordering by priority.
    pub fn submit_job(&self, spec: CraftJobSpec) -> Result<String, EncoderError> {
        spec.validate()?;
        let job_id = spec.job_id.clone();
        let queued = QueuedJob::new(spec);

        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);

        // Check for duplicates
        if lock.jobs.iter().any(|j| j.spec.job_id == job_id) {
            return Err(EncoderError::InvalidJobSpec(format!("job with ID '{job_id}' already exists in queue")));
        }

        lock.jobs.push(queued);
        self.sort_jobs_by_priority(&mut lock.jobs);
        drop(lock);

        self.persist_state()?;
        Ok(job_id)
    }

    /// Removes a job from the queue.
    pub fn remove_job(&self, job_id: &str) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let orig_len = lock.jobs.len();
        lock.jobs.retain(|j| j.spec.job_id != job_id);

        if lock.jobs.len() == orig_len {
            return Err(EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")));
        }
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Cancels an in-progress or queued job.
    pub fn cancel_job(&self, job_id: &str) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        job.status = JobStatus::Cancelled;
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Pauses an active or queued job.
    pub fn pause_job(&self, job_id: &str) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        job.status = JobStatus::Paused;
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Resumes or starts a job in the queue.
    pub fn start_job(&self, job_id: &str) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        job.status = JobStatus::Queued;
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Duplicates an existing job with a new generated ID.
    pub fn duplicate_job(&self, job_id: &str) -> Result<String, EncoderError> {
        let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        let mut new_spec = job.spec.clone();
        new_spec.job_id = format!("{}-copy", job.spec.job_id);
        drop(lock);

        self.submit_job(new_spec)
    }

    /// Reorders the queue to match the given list of job IDs.
    pub fn reorder_jobs(&self, ordered_ids: &[String]) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut new_jobs = Vec::with_capacity(lock.jobs.len());

        for id in ordered_ids {
            if let Some(pos) = lock.jobs.iter().position(|j| &j.spec.job_id == id) {
                new_jobs.push(lock.jobs.remove(pos));
            }
        }
        // Append any remaining jobs not mentioned in the order list
        new_jobs.append(&mut lock.jobs);
        lock.jobs = new_jobs;
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Processes the next available `Queued` job to completion synchronously.
    /// Returns true if a job was found and executed.
    pub fn process_next_job(&self) -> Result<bool, EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);

        let next_idx = lock.jobs.iter().position(|j| matches!(j.status, JobStatus::Queued));
        let job_idx = match next_idx {
            Some(idx) => idx,
            None => return Ok(false), // No jobs ready
        };

        let job_spec = if let Some(j) = lock.jobs.get_mut(job_idx) {
            j.status = JobStatus::Processing;
            j.spec.clone()
        } else {
            return Ok(false);
        };
        drop(lock);

        let pipeline = TranscodePipeline::new(job_spec);
        let result = pipeline.execute();

        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(j) = lock.jobs.get_mut(job_idx) {
            match result {
                Ok(()) => {
                    j.status = JobStatus::Completed;
                    j.progress_percent = 100.0;
                }
                Err(ref err) => {
                    j.status = JobStatus::Failed(err.to_string());
                    j.error_message = Some(err.to_string());
                }
            }
        }
        drop(lock);

        self.persist_state()?;
        result.map(|_| true)
    }

    /// Returns a snapshot of all jobs currently in the queue.
    pub fn list_jobs(&self) -> Vec<QueuedJob> {
        let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        lock.jobs.clone()
    }

    fn persist_state(&self) -> Result<(), EncoderError> {
        if let Some(ref path) = self.journal_path {
            let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            save_journal_atomic(path, &lock)?;
        }
        Ok(())
    }

    fn sort_jobs_by_priority(&self, jobs: &mut [QueuedJob]) {
        jobs.sort_by(|a, b| {
            let prio_a = priority_rank(a.spec.priority);
            let prio_b = priority_rank(b.spec.priority);
            prio_b.cmp(&prio_a) // Higher priority first
        });
    }
}

fn priority_rank(priority: JobPriority) -> u8 {
    match priority {
        JobPriority::Urgent => 4,
        JobPriority::High => 3,
        JobPriority::Normal => 2,
        JobPriority::Low => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, ContainerType, OutputTarget, SourceDescriptor, StartPolicy};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn make_test_spec(id: &str, priority: JobPriority) -> CraftJobSpec {
        CraftJobSpec {
            version: "1.0".into(),
            job_id: id.into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: PathBuf::from("in.wav"), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: PathBuf::from("out.wav"),
                preset_name: "WAV".into(),
                container: ContainerType::Wave,
                custom_width: None,
                custom_height: None,
                custom_frame_rate: None,
                target_bitrate_kbps: None,
                target_lufs: None,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn test_queue_priority_ordering() {
        let queue = QueueManager::new_in_memory();
        queue.submit_job(make_test_spec("low-1", JobPriority::Low)).unwrap();
        queue.submit_job(make_test_spec("urgent-1", JobPriority::Urgent)).unwrap();
        queue.submit_job(make_test_spec("normal-1", JobPriority::Normal)).unwrap();

        let jobs = queue.list_jobs();
        assert_eq!(jobs[0].spec.job_id, "urgent-1");
        assert_eq!(jobs[1].spec.job_id, "normal-1");
        assert_eq!(jobs[2].spec.job_id, "low-1");
    }

    #[test]
    fn test_queue_persisted_execution() {
        let dir = tempdir().unwrap();
        let journal_path = dir.path().join("journal.json");

        let queue = QueueManager::new_persisted(journal_path.clone()).unwrap();
        let target_path = dir.path().join("out.wav");

        let mut spec = make_test_spec("job-exec", JobPriority::High);
        spec.targets[0].output_path = target_path.clone();

        queue.submit_job(spec).unwrap();
        let processed = queue.process_next_job().unwrap();
        assert!(processed);

        let jobs = queue.list_jobs();
        assert_eq!(jobs[0].status, JobStatus::Completed);
        assert!(target_path.exists());
    }
}
