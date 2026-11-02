//! Multi-job execution scheduler and queue controller.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_core::{CraftJobSpec, EncoderError, JobPriority, JobStatus};
use encodercraft_pipeline::{JobProgress, TranscodePipeline};

use crate::state::{LogEntry, QueueJournal, QueuedJob, load_journal, save_journal_atomic};

/// Central queue manager orchestrating job submission, priority scheduling, and persistence.
pub struct QueueManager {
    journal_path: Option<PathBuf>,
    state: Mutex<QueueJournal>,
    active_progress: Mutex<Option<(String, Arc<JobProgress>)>>,
    is_worker_running: Arc<AtomicBool>,
    stop_worker_signal: Arc<AtomicBool>,
}

impl QueueManager {
    /// Creates an in-memory queue manager without filesystem persistence.
    pub fn new_in_memory() -> Self {
        Self {
            journal_path: None,
            state: Mutex::new(QueueJournal::default()),
            active_progress: Mutex::new(None),
            is_worker_running: Arc::new(AtomicBool::new(false)),
            stop_worker_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Creates a queue manager persisted to disk at the specified journal path.
    pub fn new_persisted(journal_path: PathBuf) -> Result<Self, EncoderError> {
        let journal = load_journal(&journal_path)?;
        Ok(Self {
            journal_path: Some(journal_path),
            state: Mutex::new(journal),
            active_progress: Mutex::new(None),
            is_worker_running: Arc::new(AtomicBool::new(false)),
            stop_worker_signal: Arc::new(AtomicBool::new(false)),
        })
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

    /// Moves a job up by one position in the queue (higher priority).
    pub fn move_job_up(&self, job_id: &str) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pos) = lock.jobs.iter().position(|j| j.spec.job_id == job_id)
            && pos > 0
        {
            lock.jobs.swap(pos, pos - 1);
            drop(lock);
            self.persist_state()?;
        }
        Ok(())
    }

    /// Moves a job down by one position in the queue (lower priority).
    pub fn move_job_down(&self, job_id: &str) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pos) = lock.jobs.iter().position(|j| j.spec.job_id == job_id)
            && pos + 1 < lock.jobs.len()
        {
            lock.jobs.swap(pos, pos + 1);
            drop(lock);
            self.persist_state()?;
        }
        Ok(())
    }

    /// Processes the next available `Queued` job to completion synchronously.
    /// Returns true if a job was found and executed.
    pub fn process_next_job(&self) -> Result<bool, EncoderError> {
        let start_time = std::time::Instant::now();
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

        let pipeline = TranscodePipeline::new(job_spec.clone());
        let result = pipeline.execute();

        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);

        let (status_str, err_msg) = match &result {
            Ok(()) => ("Completed".to_string(), None),
            Err(e) => ("Failed".to_string(), Some(e.to_string())),
        };

        let source_display = match &job_spec.source {
            encodercraft_core::SourceDescriptor::File { path, .. } => path.file_name().and_then(|s| s.to_str()).unwrap_or("File").to_string(),
            encodercraft_core::SourceDescriptor::StitchedClips { .. } => "Stitched Sequence".to_string(),
            encodercraft_core::SourceDescriptor::Pipe { endpoint, .. } => endpoint.clone(),
            encodercraft_core::SourceDescriptor::ProjectReference { composition_id, .. } => composition_id.clone(),
        };

        let preset_display = job_spec.targets.first().map(|t| t.preset_name.clone()).unwrap_or_else(|| "Unknown".into());
        let output_display = job_spec.targets.first().map(|t| t.output_path.display().to_string()).unwrap_or_else(|| "output.mp4".into());

        let log_entry = LogEntry {
            timestamp_unix_ms: now_unix,
            job_id: job_spec.job_id.clone(),
            source_name: source_display,
            preset_name: preset_display,
            output_path: output_display,
            duration_ms: elapsed_ms,
            status: status_str,
            error: err_msg,
        };

        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(j) = lock.jobs.get_mut(job_idx) {
            match result {
                Ok(()) => {
                    j.status = JobStatus::Completed;
                    j.progress_percent = 100.0;
                    j.completed_at_unix_ms = Some(now_unix);
                }
                Err(ref err) => {
                    j.status = JobStatus::Failed(err.to_string());
                    j.error_message = Some(err.to_string());
                    j.completed_at_unix_ms = Some(now_unix);
                }
            }
        }
        lock.log_entries.push(log_entry);
        if lock.log_entries.len() > 500 {
            lock.log_entries.remove(0);
        }
        drop(lock);

        self.persist_state()?;
        result.map(|_| true)
    }

    /// Starts background asynchronous processing of queued jobs in a dedicated worker thread (AME Parity).
    pub fn start_background_worker(self: &Arc<Self>) -> bool {
        if self.is_worker_running.swap(true, Ordering::AcqRel) {
            return false; // Already active
        }
        self.stop_worker_signal.store(false, Ordering::Release);

        let this = Arc::clone(self);
        let builder = std::thread::Builder::new().name("encodercraft-worker".into());
        let spawn_res = builder.spawn(move || {
            this.run_worker_loop();
        });

        if spawn_res.is_err() {
            self.is_worker_running.store(false, Ordering::Release);
            return false;
        }
        true
    }

    /// Returns true if the background queue worker is currently actively transcoding.
    pub fn is_worker_running(&self) -> bool {
        self.is_worker_running.load(Ordering::Acquire)
    }

    /// Stops background processing and cancels the currently transcoding job.
    pub fn stop_worker(&self) {
        self.stop_worker_signal.store(true, Ordering::Release);
        let p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, ref prog)) = *p_lock {
            prog.cancel();
        }
    }

    /// Pauses the active worker and current job.
    pub fn pause_worker(&self) {
        self.stop_worker_signal.store(true, Ordering::Release);
        let p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, ref prog)) = *p_lock {
            prog.set_paused(true);
        }
    }

    fn run_worker_loop(self: &Arc<Self>) {
        loop {
            if self.stop_worker_signal.load(Ordering::Acquire) {
                break;
            }

            let next_job = {
                let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(pos) = lock.jobs.iter().position(|j| matches!(j.status, JobStatus::Queued)) {
                    if let Some(j) = lock.jobs.get_mut(pos) {
                        j.status = JobStatus::Processing;
                        j.progress_percent = 0.0;
                        j.current_fps = 0.0;
                        Some((j.spec.job_id.clone(), j.spec.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            let Some((job_id, spec)) = next_job else {
                break;
            };

            let _ = self.persist_state();

            let pipeline = TranscodePipeline::new(spec.clone());
            let progress = pipeline.progress();

            {
                let mut p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
                *p_lock = Some((job_id.clone(), Arc::clone(&progress)));
            }

            let start_time = std::time::Instant::now();

            let stop_monitor = Arc::new(AtomicBool::new(false));
            let m_stop_clone = Arc::clone(&stop_monitor);
            let p_clone = Arc::clone(&progress);
            let m_this = Arc::clone(self);
            let m_job_id = job_id.clone();

            let monitor_handle = std::thread::Builder::new().name("encodercraft-progress".into()).spawn(move || {
                while !m_stop_clone.load(Ordering::Acquire) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let pct = p_clone.percent_complete();
                    let fps = p_clone.current_fps();
                    let mut lock = m_this.state.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(j) = lock.jobs.iter_mut().find(|j| j.spec.job_id == m_job_id) {
                        j.progress_percent = pct;
                        j.current_fps = fps;
                    }
                }
            });

            let exec_result = pipeline.execute();

            stop_monitor.store(true, Ordering::Release);
            if let Ok(handle) = monitor_handle {
                let _ = handle.join();
            }

            {
                let mut p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
                *p_lock = None;
            }

            let elapsed_ms = start_time.elapsed().as_millis() as u64;
            let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);

            let (status_str, err_msg) = match &exec_result {
                Ok(()) => ("Completed".to_string(), None),
                Err(e) => ("Failed".to_string(), Some(e.to_string())),
            };

            let source_display = match &spec.source {
                encodercraft_core::SourceDescriptor::File { path, .. } => path.file_name().and_then(|s| s.to_str()).unwrap_or("File").to_string(),
                encodercraft_core::SourceDescriptor::StitchedClips { .. } => "Stitched Sequence".to_string(),
                encodercraft_core::SourceDescriptor::Pipe { endpoint, .. } => endpoint.clone(),
                encodercraft_core::SourceDescriptor::ProjectReference { composition_id, .. } => composition_id.clone(),
            };

            let preset_display = spec.targets.first().map(|t| t.preset_name.clone()).unwrap_or_else(|| "Unknown".into());
            let output_display = spec.targets.first().map(|t| t.output_path.display().to_string()).unwrap_or_else(|| "output.mp4".into());

            let log_entry = LogEntry {
                timestamp_unix_ms: now_unix,
                job_id: job_id.clone(),
                source_name: source_display,
                preset_name: preset_display,
                output_path: output_display,
                duration_ms: elapsed_ms,
                status: status_str,
                error: err_msg.clone(),
            };

            {
                let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(j) = lock.jobs.iter_mut().find(|j| j.spec.job_id == job_id) {
                    match exec_result {
                        Ok(()) => {
                            j.status = JobStatus::Completed;
                            j.progress_percent = 100.0;
                            j.completed_at_unix_ms = Some(now_unix);
                        }
                        Err(err) => {
                            j.status = JobStatus::Failed(err.to_string());
                            j.error_message = Some(err.to_string());
                            j.completed_at_unix_ms = Some(now_unix);
                        }
                    }
                }
                lock.log_entries.push(log_entry);
                if lock.log_entries.len() > 500 {
                    lock.log_entries.remove(0);
                }
            }

            let _ = self.persist_state();
        }

        self.is_worker_running.store(false, Ordering::Release);
    }

    /// Resets a completed, failed, or cancelled job back to Queued state.
    pub fn reset_job_status(&self, job_id: &str) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        job.status = JobStatus::Queued;
        job.progress_percent = 0.0;
        job.error_message = None;
        job.current_fps = 0.0;
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Adds an additional output target to an existing job in the queue.
    pub fn add_output_target(&self, job_id: &str, target: encodercraft_core::OutputTarget) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        job.spec.targets.push(target);
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Removes a specific output target from a job.
    pub fn remove_output_target(&self, job_id: &str, target_idx: usize) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        if job.spec.targets.len() <= 1 {
            return Err(EncoderError::InvalidJobSpec("cannot remove the last output target of a job".into()));
        }

        if target_idx < job.spec.targets.len() {
            job.spec.targets.remove(target_idx);
        } else {
            return Err(EncoderError::InvalidJobSpec(format!("target index {target_idx} out of range")));
        }
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Duplicates an output target on a job.
    pub fn duplicate_output_target(&self, job_id: &str, target_idx: usize) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        let target = job
            .spec
            .targets
            .get(target_idx)
            .cloned()
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("target index {target_idx} out of range")))?;

        let mut dup = target;
        let mut out_path = dup.output_path;
        let stem = out_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        let ext = out_path.extension().and_then(|s| s.to_str()).unwrap_or("mp4");
        out_path.set_file_name(format!("{stem}_copy.{ext}"));
        dup.output_path = out_path;
        dup.target_id = format!("{}_copy", dup.target_id);

        job.spec.targets.push(dup);
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Clears all completed jobs from the queue, returning the count of removed jobs.
    pub fn clear_completed_jobs(&self) -> Result<usize, EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let orig_len = lock.jobs.len();
        lock.jobs.retain(|j| j.status != JobStatus::Completed);
        let removed = orig_len.saturating_sub(lock.jobs.len());
        drop(lock);

        self.persist_state()?;
        Ok(removed)
    }

    /// Submits a multi-clip stitched sequence as a single batch job.
    pub fn stitch_sources_into_job(&self, paths: Vec<PathBuf>, output_path: PathBuf, preset_name: String) -> Result<String, EncoderError> {
        if paths.is_empty() {
            return Err(EncoderError::InvalidJobSpec("no clips provided for stitching".into()));
        }

        let clips =
            paths.into_iter().map(|path| encodercraft_core::StitchedClipSource { path, in_point_ticks: None, out_point_ticks: None }).collect();

        let job_id = format!("stitch-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0));

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: job_id.clone(),
            client_info: encodercraft_core::ClientInfo {
                app_name: "Stitch Clips".into(),
                app_version: "0.1.0".into(),
                pid: std::process::id(),
                user_notes: None,
            },
            priority: JobPriority::Normal,
            start_policy: encodercraft_core::StartPolicy::AutoStart,
            source: encodercraft_core::SourceDescriptor::StitchedClips { clips },
            targets: vec![encodercraft_core::OutputTarget {
                target_id: "stitch-target-1".into(),
                output_path,
                preset_name,
                container: encodercraft_core::ContainerType::IsoBmffMp4,
                ..Default::default()
            }],
        };

        self.submit_job(spec)
    }

    /// Stitches selected jobs into a single combined transcode job (AME Parity).
    pub fn stitch_selected_jobs(&self, job_ids: &[String], output_path: PathBuf, preset_name: String) -> Result<String, EncoderError> {
        if job_ids.len() < 2 {
            return Err(EncoderError::InvalidJobSpec("at least two jobs required to stitch".into()));
        }

        let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut file_paths = Vec::new();

        for id in job_ids {
            if let Some(j) = lock.jobs.iter().find(|j| &j.spec.job_id == id) {
                match &j.spec.source {
                    encodercraft_core::SourceDescriptor::File { path, .. } => {
                        file_paths.push(path.clone());
                    }
                    encodercraft_core::SourceDescriptor::StitchedClips { clips } => {
                        for clip in clips {
                            file_paths.push(clip.path.clone());
                        }
                    }
                    _ => return Err(EncoderError::InvalidJobSpec("only file sources can be stitched".into())),
                }
            } else {
                return Err(EncoderError::InvalidJobSpec(format!("job '{id}' not found")));
            }
        }
        drop(lock);

        self.stitch_sources_into_job(file_paths, output_path, preset_name)
    }

    /// Sets the destination directory across all specified jobs and their output targets (AME Parity).
    pub fn set_jobs_output_dir(&self, job_ids: &[String], output_dir: PathBuf) -> Result<usize, EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut modified = 0;

        for id in job_ids {
            if let Some(j) = lock.jobs.iter_mut().find(|j| &j.spec.job_id == id) {
                for target in &mut j.spec.targets {
                    let filename = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
                    target.output_path = output_dir.join(filename);
                }
                modified += 1;
            }
        }
        drop(lock);

        if modified > 0 {
            self.persist_state()?;
        }
        Ok(modified)
    }

    /// Returns a list of all historical transcode log entries.
    pub fn list_log_entries(&self) -> Vec<LogEntry> {
        let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        lock.log_entries.clone()
    }

    /// Clears historical transcode log entries.
    pub fn clear_log_entries(&self) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        lock.log_entries.clear();
        drop(lock);
        self.persist_state()?;
        Ok(())
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

        // Verify log entries recorded
        let logs = queue.list_log_entries();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].job_id, "job-exec");
        assert_eq!(logs[0].status, "Completed");

        // Verify clear log
        queue.clear_log_entries().unwrap();
        assert!(queue.list_log_entries().is_empty());

        // Test set_jobs_output_dir
        let mut spec2 = make_test_spec("job-dir", JobPriority::Normal);
        spec2.targets[0].output_path = dir.path().join("dir1").join("out.wav");
        queue.submit_job(spec2).unwrap();

        let new_dir = dir.path().join("dir2");
        let modified = queue.set_jobs_output_dir(&["job-dir".to_string()], new_dir.clone()).unwrap();
        assert_eq!(modified, 1);
        let jobs2 = queue.list_jobs();
        let target_job = jobs2.iter().find(|j| j.spec.job_id == "job-dir").unwrap();
        assert_eq!(target_job.spec.targets[0].output_path, new_dir.join("out.wav"));

        // Test stitch_selected_jobs
        let s1 = make_test_spec("stitch-a", JobPriority::Normal);
        let s2 = make_test_spec("stitch-b", JobPriority::Normal);
        queue.submit_job(s1).unwrap();
        queue.submit_job(s2).unwrap();

        let stitch_id = queue
            .stitch_selected_jobs(
                &["stitch-a".to_string(), "stitch-b".to_string()],
                dir.path().join("stitched.mp4"),
                "Match Source - High Bitrate".into(),
            )
            .unwrap();
        assert!(stitch_id.starts_with("stitch-"));

        // Verify reload from disk
        let reloaded = QueueManager::new_persisted(journal_path).unwrap();
        assert!(reloaded.list_jobs().iter().any(|j| j.spec.job_id == stitch_id));
    }

    #[test]
    fn test_move_job_up_down() {
        let queue = QueueManager::new_in_memory();
        queue.submit_job(make_test_spec("job-1", JobPriority::Normal)).unwrap();
        queue.submit_job(make_test_spec("job-2", JobPriority::Normal)).unwrap();

        queue.move_job_up("job-2").unwrap();
        let jobs = queue.list_jobs();
        assert_eq!(jobs[0].spec.job_id, "job-2");
        assert_eq!(jobs[1].spec.job_id, "job-1");

        queue.move_job_down("job-2").unwrap();
        let jobs2 = queue.list_jobs();
        assert_eq!(jobs2[0].spec.job_id, "job-1");
        assert_eq!(jobs2[1].spec.job_id, "job-2");
    }
}
