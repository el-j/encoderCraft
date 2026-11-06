//! Multi-job execution scheduler and queue controller.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_core::{CraftJobSpec, EncoderError, JobPriority, JobStatus};
use encodercraft_pipeline::{JobProgress, PipelineLimits, TranscodePipeline};

use crate::conversion::{ConversionDecision, LossyConversion, LossyConversionKey, assess_lossy_conversions};
use crate::state::{AppPreferences, LogEntry, QueueJournal, QueuedJob, load_journal, load_preferences, save_journal_atomic, save_preferences_atomic};

/// Upper bound for concurrently running jobs.
const MAX_PARALLELISM: usize = 16;

/// Central queue manager orchestrating job submission, priority scheduling, and persistence.
pub struct QueueManager {
    journal_path: Option<PathBuf>,
    conversion_preferences_path: Mutex<Option<PathBuf>>,
    state: Mutex<QueueJournal>,
    active_progress: Mutex<HashMap<String, Arc<JobProgress>>>,
    parallelism: AtomicUsize,
    running_jobs: AtomicUsize,
    is_worker_running: Arc<AtomicBool>,
    stop_worker_signal: Arc<AtomicBool>,
    paused: AtomicBool,
    pipeline_limits: PipelineLimits,
}

impl QueueManager {
    /// Creates an in-memory queue manager without filesystem persistence.
    pub fn new_in_memory() -> Self {
        Self::new_in_memory_with_limits(PipelineLimits::default())
    }

    /// Creates an in-memory queue manager with explicit per-pipeline memory limits.
    pub fn new_in_memory_with_limits(pipeline_limits: PipelineLimits) -> Self {
        Self {
            journal_path: None,
            conversion_preferences_path: Mutex::new(None),
            state: Mutex::new(QueueJournal::default()),
            active_progress: Mutex::new(HashMap::new()),
            parallelism: AtomicUsize::new(MAX_PARALLELISM.min(2)),
            running_jobs: AtomicUsize::new(0),
            is_worker_running: Arc::new(AtomicBool::new(false)),
            stop_worker_signal: Arc::new(AtomicBool::new(false)),
            paused: AtomicBool::new(false),
            pipeline_limits,
        }
    }

    /// Creates a queue manager persisted to disk at the specified journal path.
    pub fn new_persisted(journal_path: PathBuf) -> Result<Self, EncoderError> {
        let mut journal = load_journal(&journal_path)?;
        // A job left `Processing` by a crash or forced exit is picked up again.
        for job in &mut journal.jobs {
            if matches!(job.status, JobStatus::Processing) {
                job.status = JobStatus::Queued;
                job.progress_percent = 0.0;
                job.current_fps = 0.0;
                job.target_eta_seconds = None;
            }
        }
        let parallelism = journal.active_parallelism.clamp(1, MAX_PARALLELISM);
        Ok(Self {
            journal_path: Some(journal_path),
            conversion_preferences_path: Mutex::new(None),
            state: Mutex::new(journal),
            active_progress: Mutex::new(HashMap::new()),
            parallelism: AtomicUsize::new(parallelism),
            running_jobs: AtomicUsize::new(0),
            is_worker_running: Arc::new(AtomicBool::new(false)),
            stop_worker_signal: Arc::new(AtomicBool::new(false)),
            paused: AtomicBool::new(false),
            pipeline_limits: PipelineLimits::default(),
        })
    }

    /// Number of jobs that may be encoded concurrently.
    pub fn parallelism(&self) -> usize {
        self.parallelism.load(Ordering::Acquire)
    }

    /// Changes the number of concurrent jobs (clamped to 1..=16); takes effect for the next claim.
    pub fn set_parallelism(&self, workers: usize) -> Result<(), EncoderError> {
        let workers = workers.clamp(1, MAX_PARALLELISM);
        self.parallelism.store(workers, Ordering::Release);
        self.state.lock().unwrap_or_else(PoisonError::into_inner).active_parallelism = workers;
        self.persist_state()
    }

    /// Adds a new job to the queue, ordering by priority.
    pub fn submit_job(&self, spec: CraftJobSpec) -> Result<String, EncoderError> {
        self.submit_job_with_approvals(spec, &[])
    }

    /// Adds a job after one-time approval of the listed exact conversion pairs.
    pub fn submit_job_with_approvals(&self, spec: CraftJobSpec, approved: &[LossyConversionKey]) -> Result<String, EncoderError> {
        spec.validate()?;
        self.check_conversion_consent(&spec, approved)?;
        let job_id = spec.job_id.clone();
        let mut queued = QueuedJob::new(spec);
        queued.conversion_approvals = approved.to_vec();

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

    /// Path to the shared preferences file used for saved conversion decisions.
    pub fn set_conversion_preferences_path(&self, path: PathBuf) {
        *self.conversion_preferences_path.lock().unwrap_or_else(PoisonError::into_inner) = Some(path);
    }

    /// Resolves an application configuration file beneath the configured per-user directory.
    pub fn config_file_path(&self, file_name: &str) -> Result<PathBuf, EncoderError> {
        let name = std::path::Path::new(file_name);
        if name.components().count() != 1 || name.file_name().and_then(|part| part.to_str()) != Some(file_name) {
            return Err(EncoderError::InvalidJobSpec("configuration file name must be a single path component".into()));
        }
        let config_dir = self
            .conversion_preferences_path
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .ok_or_else(|| EncoderError::Io("no configuration directory is set for this queue".into()))?;
        Ok(config_dir.join(name))
    }

    /// Loads shared application preferences, using defaults when no preferences file exists.
    pub fn app_preferences(&self) -> Result<AppPreferences, EncoderError> {
        match self.preferences_file_path() {
            Some(path) => load_preferences(&path),
            None => Ok(AppPreferences::default()),
        }
    }

    /// Saves preferences while preserving conversion decisions written by other engine commands.
    pub fn save_app_preferences(&self, mut preferences: AppPreferences) -> Result<(), EncoderError> {
        let path = self.preferences_file_path().ok_or_else(|| EncoderError::Io("no per-user preferences file is available".into()))?;
        let existing = load_preferences(&path)?;
        for decision in existing.lossy_conversion_decisions {
            if !preferences.lossy_conversion_decisions.iter().any(|saved| saved.key == decision.key) {
                preferences.lossy_conversion_decisions.push(decision);
            }
        }
        preferences.migrate();
        save_preferences_atomic(&path, &preferences)
    }

    fn preferences_file_path(&self) -> Option<PathBuf> {
        self.conversion_preferences_path.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Lossy conversions in `spec` that do not yet have a saved allow/deny decision.
    pub fn conversion_review(&self, spec: &CraftJobSpec) -> Result<Vec<LossyConversion>, EncoderError> {
        let resolved = self.resolve_presets(spec);
        let decisions = self.conversion_preferences()?;
        let conversions = assess_lossy_conversions(&resolved)?;
        for conversion in &conversions {
            if decisions.iter().any(|decision| decision.key == conversion.key && !decision.allowed) {
                return Err(EncoderError::InvalidJobSpec(format!("this lossy conversion was previously declined: {}", conversion.description())));
            }
        }
        Ok(conversions
            .into_iter()
            .filter(|conversion| !decisions.iter().any(|decision| decision.key == conversion.key && decision.allowed))
            .collect())
    }

    /// Saves an allow or deny choice for one exact source-to-target conversion pair.
    pub fn save_conversion_decision(&self, key: LossyConversionKey, allowed: bool) -> Result<(), EncoderError> {
        let path = self
            .preferences_file_path()
            .ok_or_else(|| EncoderError::InvalidJobSpec("cannot save a conversion decision without a preferences file".into()))?;
        let mut preferences = load_preferences(&path)?;
        if let Some(decision) = preferences.lossy_conversion_decisions.iter_mut().find(|decision| decision.key == key) {
            decision.allowed = allowed;
        } else {
            preferences.lossy_conversion_decisions.push(ConversionDecision { key, allowed });
        }
        save_preferences_atomic(&path, &preferences)
    }

    fn conversion_preferences(&self) -> Result<Vec<ConversionDecision>, EncoderError> {
        let path = self.conversion_preferences_path.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match path {
            Some(path) => Ok(load_preferences(&path)?.lossy_conversion_decisions),
            None => Ok(AppPreferences::default().lossy_conversion_decisions),
        }
    }

    fn conversion_approvals_for_job(&self, job_id: &str) -> Result<Vec<LossyConversionKey>, EncoderError> {
        let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        lock.jobs
            .iter()
            .find(|job| job.spec.job_id == job_id)
            .map(|job| job.conversion_approvals.clone())
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))
    }

    fn resolve_presets(&self, spec: &CraftJobSpec) -> CraftJobSpec {
        let mut resolved = spec.clone();
        let registry = encodercraft_presets::PresetRegistry::default();
        for target in &mut resolved.targets {
            registry.apply_to_target(target);
        }
        resolved
    }

    fn check_conversion_consent(&self, spec: &CraftJobSpec, approved: &[LossyConversionKey]) -> Result<(), EncoderError> {
        let resolved = self.resolve_presets(spec);
        let decisions = self.conversion_preferences()?;
        for conversion in assess_lossy_conversions(&resolved)? {
            match decisions.iter().find(|decision| decision.key == conversion.key) {
                Some(decision) if decision.allowed => {}
                Some(_) => {
                    return Err(EncoderError::InvalidJobSpec(format!("this lossy conversion was previously declined: {}", conversion.description())));
                }
                None if approved.contains(&conversion.key) => {}
                None => return Err(EncoderError::ConversionApprovalRequired(conversion.description())),
            }
        }
        Ok(())
    }

    /// Removes a job from the queue.
    pub fn remove_job(&self, job_id: &str) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if lock.jobs.iter().any(|j| j.spec.job_id == job_id && matches!(j.status, JobStatus::Processing)) {
            return Err(EncoderError::InvalidJobSpec(format!("job '{job_id}' is encoding; cancel it before removing")));
        }
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

        if matches!(job.status, JobStatus::Queued | JobStatus::Paused | JobStatus::Processing) {
            self.with_active(job_id, |p| p.cancel());
            job.status = JobStatus::Cancelled;
            job.target_eta_seconds = None;
        }
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

        if matches!(job.status, JobStatus::Queued | JobStatus::Processing) {
            self.with_active(job_id, |p| p.set_paused(true));
            job.status = JobStatus::Paused;
        }
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

        if !matches!(job.status, JobStatus::Processing) {
            if self.is_active(job_id) {
                self.with_active(job_id, |p| p.set_paused(false));
                job.status = JobStatus::Processing;
            } else {
                job.status = JobStatus::Queued;
            }
        }
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Duplicates an existing job with a new generated ID.
    pub fn duplicate_job(&self, job_id: &str) -> Result<String, EncoderError> {
        self.submit_job(self.duplicate_job_spec(job_id)?)
    }

    /// Builds a duplicate job specification without submitting it.
    pub fn duplicate_job_spec(&self, job_id: &str) -> Result<CraftJobSpec, EncoderError> {
        let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        let mut new_spec = job.spec.clone();
        new_spec.job_id = format!("{}-copy", job.spec.job_id);
        Ok(new_spec)
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

    fn is_active(&self, job_id: &str) -> bool {
        let p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
        p_lock.contains_key(job_id)
    }

    fn with_active(&self, job_id: &str, f: impl FnOnce(&JobProgress)) {
        let p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(prog) = p_lock.get(job_id) {
            f(prog);
        }
    }

    /// Atomically claims the next `Queued` job and marks it `Processing`.
    fn claim_next(&self) -> Option<(String, CraftJobSpec)> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock.jobs.iter_mut().find(|j| matches!(j.status, JobStatus::Queued))?;
        job.status = JobStatus::Processing;
        job.progress_percent = 0.0;
        job.current_fps = 0.0;
        job.target_eta_seconds = None;
        job.error_message = None;
        job.completed_at_unix_ms = None;
        Some((job.spec.job_id.clone(), job.spec.clone()))
    }

    /// Processes the next available `Queued` job to completion synchronously.
    /// Returns true if a job was found and executed.
    pub fn process_next_job(&self) -> Result<bool, EncoderError> {
        let Some((job_id, spec)) = self.claim_next() else {
            return Ok(false);
        };
        self.persist_state()?;
        match self.run_job(&job_id, &spec) {
            Ok(()) | Err(EncoderError::Cancelled) => Ok(true),
            Err(e) => Err(e),
        }
    }

    /// Executes a claimed job, mirrors live progress into the journal and records the outcome.
    fn run_job(&self, job_id: &str, spec: &CraftJobSpec) -> Result<(), EncoderError> {
        let start_time = std::time::Instant::now();
        let resolved = self.resolve_presets(spec);
        let approvals = self.conversion_approvals_for_job(job_id)?;
        let pipeline = TranscodePipeline::with_limits(resolved, self.pipeline_limits);
        let progress = pipeline.progress();
        {
            let mut p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
            p_lock.insert(job_id.to_string(), Arc::clone(&progress));
        }

        let stop_monitor = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut last_persist = std::time::Instant::now();
                while !stop_monitor.load(Ordering::Acquire) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    if last_persist.elapsed() >= std::time::Duration::from_secs(2) {
                        last_persist = std::time::Instant::now();
                        let _ = self.persist_state();
                    }
                    let pct = progress.percent_complete();
                    let fps = progress.current_fps();
                    let target_eta_seconds = progress.eta_seconds();
                    let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(j) = lock.jobs.iter_mut().find(|j| j.spec.job_id == job_id)
                        && matches!(j.status, JobStatus::Processing | JobStatus::Paused)
                    {
                        j.progress_percent = pct;
                        j.current_fps = fps;
                        j.target_eta_seconds = target_eta_seconds;
                    }
                }
            });
            let result = self.check_conversion_consent(spec, &approvals).and_then(|()| pipeline.execute());
            stop_monitor.store(true, Ordering::Release);
            result
        });

        {
            let mut p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
            p_lock.remove(job_id);
        }

        let outcome = if progress.is_cancelled() && result.is_err() { Err(EncoderError::Cancelled) } else { result };
        self.finish_job(job_id, spec, &outcome, start_time.elapsed().as_millis() as u64)?;
        outcome
    }

    /// Writes the final status of a job unless the job vanished or was already cancelled by the user.
    fn finish_job(&self, job_id: &str, spec: &CraftJobSpec, outcome: &Result<(), EncoderError>, elapsed_ms: u64) -> Result<(), EncoderError> {
        let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);

        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut status_str = "Completed".to_string();
        let mut err_msg = None;
        if let Some(j) = lock.jobs.iter_mut().find(|j| j.spec.job_id == job_id) {
            let already_cancelled = matches!(j.status, JobStatus::Cancelled);
            match outcome {
                Ok(()) if !already_cancelled => {
                    j.status = JobStatus::Completed;
                    j.progress_percent = 100.0;
                    j.target_eta_seconds = None;
                    j.completed_at_unix_ms = Some(now_unix);
                }
                Ok(()) => status_str = "Cancelled".to_string(),
                Err(EncoderError::Cancelled) => {
                    j.status = JobStatus::Cancelled;
                    j.target_eta_seconds = None;
                    j.completed_at_unix_ms = Some(now_unix);
                    status_str = "Cancelled".to_string();
                }
                Err(err) if !already_cancelled => {
                    j.status = JobStatus::Failed(err.to_string());
                    j.error_message = Some(err.to_string());
                    j.target_eta_seconds = None;
                    j.completed_at_unix_ms = Some(now_unix);
                    status_str = "Failed".to_string();
                    err_msg = Some(err.to_string());
                }
                Err(_) => status_str = "Cancelled".to_string(),
            }
        }

        let source_name = match &spec.source {
            encodercraft_core::SourceDescriptor::File { path, .. } => path.file_name().and_then(|s| s.to_str()).unwrap_or("File").to_string(),
            encodercraft_core::SourceDescriptor::StitchedClips { .. } => "Stitched Sequence".to_string(),
            encodercraft_core::SourceDescriptor::Pipe { endpoint, .. } => endpoint.clone(),
            encodercraft_core::SourceDescriptor::ProjectReference { composition_id, .. } => composition_id.clone(),
        };
        let first = spec.targets.first();
        lock.log_entries.push(LogEntry {
            timestamp_unix_ms: now_unix,
            job_id: job_id.to_string(),
            source_name,
            preset_name: first.map(|t| t.preset_name.clone()).unwrap_or_else(|| "Unknown".into()),
            output_path: first.map(|t| t.output_path.display().to_string()).unwrap_or_else(|| "output.mp4".into()),
            duration_ms: elapsed_ms,
            status: status_str,
            error: err_msg,
        });
        if lock.log_entries.len() > 500 {
            lock.log_entries.remove(0);
        }
        drop(lock);

        self.persist_state()
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

    /// Starts (or resumes) the whole queue: every `Queued` job is processed one after another
    /// until the queue is drained. Returns true if a worker is running afterwards.
    pub fn start_queue(self: &Arc<Self>) -> bool {
        self.stop_worker_signal.store(false, Ordering::Release);
        self.resume_worker();
        self.start_background_worker();
        self.is_worker_running()
    }

    /// Returns true if the background queue worker is currently actively transcoding.
    pub fn is_worker_running(&self) -> bool {
        self.is_worker_running.load(Ordering::Acquire)
    }

    /// Returns true if the last stop request has not been superseded by a start.
    pub fn is_stop_requested(&self) -> bool {
        self.stop_worker_signal.load(Ordering::Acquire)
    }

    /// Returns true if the queue is paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    /// Stops background processing and cancels the currently transcoding job.
    pub fn stop_worker(&self) {
        self.stop_worker_signal.store(true, Ordering::Release);
        self.paused.store(false, Ordering::Release);
        let p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
        for prog in p_lock.values() {
            prog.cancel();
        }
    }

    /// Pauses the queue: the current job is suspended and no further job is started until resumed.
    pub fn pause_worker(&self) {
        self.paused.store(true, Ordering::Release);
        let p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
        for prog in p_lock.values() {
            prog.set_paused(true);
        }
    }

    /// Resumes a paused queue and its suspended job.
    pub fn resume_worker(&self) {
        self.paused.store(false, Ordering::Release);
        let p_lock = self.active_progress.lock().unwrap_or_else(PoisonError::into_inner);
        for prog in p_lock.values() {
            prog.set_paused(false);
        }
    }

    fn has_queued_jobs(&self) -> bool {
        let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        lock.jobs.iter().any(|j| matches!(j.status, JobStatus::Queued))
    }

    /// Scheduler: keeps up to `parallelism()` jobs running until the queue is drained, stopped or paused.
    fn run_worker_loop(self: &Arc<Self>) {
        loop {
            if self.stop_worker_signal.load(Ordering::Acquire) {
                break;
            }
            let running = self.running_jobs.load(Ordering::Acquire);
            if self.paused.load(Ordering::Acquire) || running >= self.parallelism() {
                std::thread::sleep(std::time::Duration::from_millis(25));
                continue;
            }

            let Some((job_id, spec)) = self.claim_next() else {
                if running > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                    continue;
                }
                // Close the race with a job submitted while the worker was about to exit.
                self.is_worker_running.store(false, Ordering::Release);
                if self.has_queued_jobs() && !self.stop_worker_signal.load(Ordering::Acquire) && !self.is_worker_running.swap(true, Ordering::AcqRel)
                {
                    continue;
                }
                return;
            };

            let _ = self.persist_state();
            self.running_jobs.fetch_add(1, Ordering::AcqRel);
            let this = Arc::clone(self);
            let id = job_id.clone();
            let builder = std::thread::Builder::new().name(format!("encodercraft-job-{job_id}"));
            let spawned = builder.spawn(move || {
                // A failed job is recorded by `finish_job`; the batch continues with the next item.
                let _ = this.run_job(&id, &spec);
                this.running_jobs.fetch_sub(1, Ordering::AcqRel);
            });
            if spawned.is_err() {
                self.running_jobs.fetch_sub(1, Ordering::AcqRel);
                let err = Err(EncoderError::Io("failed to spawn encode thread".into()));
                if let Some((_, spec)) = self.job_spec(&job_id).map(|s| (job_id.clone(), s)) {
                    let _ = self.finish_job(&job_id, &spec, &err, 0);
                }
            }
        }
        // Wait for in-flight jobs (cancelled by stop) so the worker flag reflects reality.
        while self.running_jobs.load(Ordering::Acquire) > 0 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        self.is_worker_running.store(false, Ordering::Release);
    }

    fn job_spec(&self, job_id: &str) -> Option<CraftJobSpec> {
        let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        lock.jobs.iter().find(|j| j.spec.job_id == job_id).map(|j| j.spec.clone())
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
        job.target_eta_seconds = None;
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Adds an additional output target to an existing job in the queue.
    pub fn add_output_target(&self, job_id: &str, target: encodercraft_core::OutputTarget) -> Result<(), EncoderError> {
        let mut updated_spec = {
            let lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            lock.jobs
                .iter()
                .find(|job| job.spec.job_id == job_id)
                .map(|job| job.spec.clone())
                .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?
        };
        updated_spec.targets.push(target.clone());
        updated_spec.validate()?;
        let approvals = self.conversion_approvals_for_job(job_id)?;
        self.check_conversion_consent(&updated_spec, &approvals)?;

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

    /// Updates an existing output target at `target_idx` on `job_id`.
    pub fn update_output_target(&self, job_id: &str, target_idx: usize, target: encodercraft_core::OutputTarget) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        if let Some(existing) = job.spec.targets.get_mut(target_idx) {
            *existing = target;
        } else {
            return Err(EncoderError::InvalidJobSpec(format!("target index {target_idx} out of range")));
        }
        drop(lock);

        self.persist_state()?;
        Ok(())
    }

    /// Updates the job specification in-place for `job_id`.
    pub fn update_job_spec(&self, job_id: &str, spec: CraftJobSpec) -> Result<(), EncoderError> {
        self.update_job_spec_with_approvals(job_id, spec, &[])
    }

    /// Updates a job after one-time approval of the listed exact conversion pairs.
    pub fn update_job_spec_with_approvals(&self, job_id: &str, spec: CraftJobSpec, approved: &[LossyConversionKey]) -> Result<(), EncoderError> {
        if spec.job_id != job_id {
            return Err(EncoderError::InvalidJobSpec(format!("updated job ID '{}' does not match '{job_id}'", spec.job_id)));
        }
        spec.validate()?;
        let mut approvals = self.conversion_approvals_for_job(job_id)?;
        approvals.extend_from_slice(approved);
        self.check_conversion_consent(&spec, &approvals)?;

        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        job.spec = spec;
        for key in approved {
            if !job.conversion_approvals.contains(key) {
                job.conversion_approvals.push(key.clone());
            }
        }
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
        self.submit_job(Self::build_stitched_job_spec(paths, output_path, preset_name)?)
    }

    /// Builds a multi-clip stitched job specification without submitting it.
    pub fn build_stitched_job_spec(paths: Vec<PathBuf>, output_path: PathBuf, preset_name: String) -> Result<CraftJobSpec, EncoderError> {
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

        Ok(spec)
    }

    /// Stitches selected jobs into a single combined transcode job (AME Parity).
    pub fn stitch_selected_jobs(&self, job_ids: &[String], output_path: PathBuf, preset_name: String) -> Result<String, EncoderError> {
        self.submit_job(self.build_stitched_selected_job_spec(job_ids, output_path, preset_name)?)
    }

    /// Builds a stitched job specification from selected jobs without submitting it.
    pub fn build_stitched_selected_job_spec(
        &self,
        job_ids: &[String],
        output_path: PathBuf,
        preset_name: String,
    ) -> Result<CraftJobSpec, EncoderError> {
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

        Self::build_stitched_job_spec(file_paths, output_path, preset_name)
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

    /// Updates the In/Out trim range (`start_tick`, `duration_ticks`, `timebase`) on a queued job's file source.
    pub fn set_job_trim_range(&self, job_id: &str, in_frame: Option<u64>, out_frame: Option<u64>, fps: u64) -> Result<(), EncoderError> {
        let mut lock = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let job = lock
            .jobs
            .iter_mut()
            .find(|j| j.spec.job_id == job_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;

        if let encodercraft_core::SourceDescriptor::File { start_tick, duration_ticks, timebase, .. } = &mut job.spec.source {
            let frame_to_tick = |frame: u64| {
                i64::try_from(frame).map_err(|_| EncoderError::InvalidJobSpec("trim frame exceeds the supported signed tick range".into()))
            };
            let frame_rate =
                || i64::try_from(fps.max(1)).map_err(|_| EncoderError::InvalidJobSpec("frame rate exceeds the supported signed tick range".into()));
            match (in_frame, out_frame) {
                (None, None) => {
                    *start_tick = None;
                    *duration_ticks = None;
                }
                (Some(inf), None) => {
                    let start = frame_to_tick(inf)?;
                    let rate = frame_rate()?;
                    *start_tick = Some(start);
                    *duration_ticks = None;
                    *timebase = Some(encodercraft_core::Rational64::new(1, rate)?);
                }
                (None, Some(outf)) => {
                    let end = frame_to_tick(outf)?;
                    let duration = end
                        .checked_add(1)
                        .ok_or_else(|| EncoderError::InvalidJobSpec("trim duration exceeds the supported signed tick range".into()))?;
                    let rate = frame_rate()?;
                    *start_tick = Some(0);
                    *duration_ticks = Some(duration);
                    *timebase = Some(encodercraft_core::Rational64::new(1, rate)?);
                }
                (Some(inf), Some(outf)) => {
                    let in_tick = frame_to_tick(inf)?;
                    let out_tick = frame_to_tick(outf)?;
                    let lo = in_tick.min(out_tick);
                    let hi = in_tick.max(out_tick);
                    let duration = hi
                        .checked_sub(lo)
                        .and_then(|span| span.checked_add(1))
                        .ok_or_else(|| EncoderError::InvalidJobSpec("trim duration exceeds the supported signed tick range".into()))?;
                    let rate = frame_rate()?;
                    *start_tick = Some(lo);
                    *duration_ticks = Some(duration);
                    *timebase = Some(encodercraft_core::Rational64::new(1, rate)?);
                }
            }
        }
        drop(lock);

        self.persist_state()?;
        Ok(())
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
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use encodercraft_core::{ClientInfo, ContainerType, OutputTarget, SourceDescriptor, StartPolicy};
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// A queue with no configured directory must not fall back to the user's own preferences: tests
    /// and embedders that forget the path used to overwrite the real theme and settings.
    #[test]
    fn unconfigured_queue_never_touches_user_preferences() {
        let queue = QueueManager::new_in_memory();
        assert_eq!(queue.app_preferences().unwrap(), AppPreferences::default());
        assert!(queue.save_app_preferences(AppPreferences { theme: "High Contrast".into(), ..AppPreferences::default() }).is_err());
        assert!(queue.config_file_path("favorites.json").is_err());

        let dir = tempdir().unwrap();
        queue.set_conversion_preferences_path(dir.path().join("preferences.json"));
        queue.save_app_preferences(AppPreferences { theme: "Light".into(), ..AppPreferences::default() }).unwrap();
        assert_eq!(queue.app_preferences().unwrap().theme, "Light");
    }

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

    fn wait_until_idle(queue: &QueueManager) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while queue.is_worker_running() {
            assert!(std::time::Instant::now() < deadline, "worker did not drain the queue in time");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn is_terminal(status: &JobStatus) -> bool {
        matches!(status, JobStatus::Completed | JobStatus::Failed(_) | JobStatus::Cancelled)
    }

    #[test]
    fn conversion_decisions_persist_and_can_be_reversed() {
        let dir = tempdir().unwrap();
        let queue = QueueManager::new_in_memory();
        queue.set_conversion_preferences_path(dir.path().join("preferences.json"));
        let key = LossyConversionKey {
            source_pixel_format: "yuv422p10le".into(),
            source_bit_depth: 10,
            source_color_space: ColorSpaceTag::REC709,
            source_has_alpha: false,
            target_pixel_format: PixelFormat::Yuv420p,
            target_color: encodercraft_core::ColorTag::Bt709,
        };

        queue.save_conversion_decision(key.clone(), true).unwrap();
        let decisions = queue.conversion_preferences().unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].key, key);
        assert!(decisions[0].allowed);

        queue.save_conversion_decision(key.clone(), false).unwrap();
        let decisions = queue.conversion_preferences().unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].key, key);
        assert!(!decisions[0].allowed);
    }

    #[test]
    fn trim_range_rejects_unrepresentable_frames_without_mutating_the_job() {
        let queue = QueueManager::new_in_memory();
        queue.submit_job(make_test_spec("trim-overflow", JobPriority::Normal)).unwrap();

        for (in_frame, out_frame, fps) in
            [(Some(u64::MAX), None, 24), (None, Some(i64::MAX as u64), 24), (Some(0), Some(i64::MAX as u64), 24), (Some(0), Some(1), u64::MAX)]
        {
            assert!(queue.set_job_trim_range("trim-overflow", in_frame, out_frame, fps).is_err());
            let job = queue.list_jobs().into_iter().find(|job| job.spec.job_id == "trim-overflow").unwrap();
            assert!(matches!(job.spec.source, SourceDescriptor::File { start_tick: None, duration_ticks: None, .. }));
        }
    }

    #[test]
    fn lossy_video_submission_requires_saved_allow_and_honors_saved_refusal() {
        let dir = tempdir().unwrap();
        let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../source/tests/fixtures/prores.mov");
        let preferences_path = dir.path().join("preferences.json");
        let queue = QueueManager::new_in_memory();
        queue.set_conversion_preferences_path(preferences_path);

        let mut spec = make_test_spec("lossy-video", JobPriority::Normal);
        spec.source = SourceDescriptor::File { path: source_path, start_tick: None, duration_ticks: None, timebase: None };
        let target = spec.targets.first_mut().unwrap();
        target.container = ContainerType::IsoBmffMp4;
        target.output_path = dir.path().join("lossy-video.mp4");
        target.preset_name = "Match Source - High Bitrate".into();

        let conversions = queue.conversion_review(&spec).unwrap();
        let conversion = conversions.first().expect("the ProRes fixture has higher precision than the H.264 target");
        assert!(matches!(queue.submit_job(spec.clone()), Err(EncoderError::ConversionApprovalRequired(_))));

        queue.save_conversion_decision(conversion.key.clone(), true).unwrap();
        assert!(queue.submit_job(spec.clone()).is_ok());

        queue.save_conversion_decision(conversion.key.clone(), false).unwrap();
        assert!(queue.submit_job(spec).is_err());
    }

    #[test]
    fn editing_a_queued_job_requires_approval_without_replacing_its_settings() {
        let dir = tempdir().unwrap();
        let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../source/tests/fixtures/prores.mov");
        let queue = QueueManager::new_in_memory();
        queue.set_conversion_preferences_path(dir.path().join("preferences.json"));
        let mut original = make_test_spec("edit-lossy", JobPriority::Normal);
        original.source = SourceDescriptor::File { path: source_path, start_tick: None, duration_ticks: None, timebase: None };
        original.targets[0].output_path = dir.path().join("edit-lossy.wav");
        queue.submit_job(original.clone()).unwrap();

        let mut updated = original.clone();
        let target = updated.targets.first_mut().unwrap();
        target.container = ContainerType::IsoBmffMp4;
        target.output_path = dir.path().join("edit-lossy.mp4");
        target.preset_name = "Match Source - High Bitrate".into();

        assert!(matches!(queue.update_job_spec(&original.job_id, updated), Err(EncoderError::ConversionApprovalRequired(_))));
        assert_eq!(queue.list_jobs()[0].spec, original);
    }

    #[test]
    fn one_time_conversion_approval_survives_queue_restart() {
        let dir = tempdir().unwrap();
        let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../source/tests/fixtures/prores.mov");
        let journal_path = dir.path().join("queue.json");
        let mut spec = make_test_spec("restart-approved", JobPriority::Normal);
        spec.source = SourceDescriptor::File { path: source_path, start_tick: None, duration_ticks: None, timebase: None };
        let target = spec.targets.first_mut().unwrap();
        target.container = ContainerType::IsoBmffMp4;
        target.output_path = dir.path().join("restart-approved.mp4");
        target.preset_name = "Match Source - High Bitrate".into();

        let queue = QueueManager::new_persisted(journal_path.clone()).unwrap();
        queue.set_conversion_preferences_path(dir.path().join("preferences.json"));
        let conversions = queue.conversion_review(&spec).unwrap();
        let approvals: Vec<_> = conversions.into_iter().map(|conversion| conversion.key).collect();
        assert!(!approvals.is_empty());
        queue.submit_job_with_approvals(spec.clone(), &approvals).unwrap();
        drop(queue);

        let reloaded = QueueManager::new_persisted(journal_path).unwrap();
        reloaded.set_conversion_preferences_path(dir.path().join("preferences.json"));
        let saved_approvals = reloaded.conversion_approvals_for_job(&spec.job_id).unwrap();
        assert_eq!(saved_approvals, approvals);
        assert!(reloaded.check_conversion_consent(&spec, &saved_approvals).is_ok());
    }

    /// A real 0.1 s mono 16-bit 8 kHz WAV, so jobs decode actual audio.
    fn write_wav(dir: &std::path::Path) -> PathBuf {
        let samples: Vec<i16> = (0..800).map(|i| ((i as f32 * 0.3).sin() * 8000.0) as i16).collect();
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let path = dir.join("in.wav");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn spec_in(dir: &std::path::Path, id: &str) -> CraftJobSpec {
        let mut spec = make_test_spec(id, JobPriority::Normal);
        spec.source = SourceDescriptor::File { path: write_wav(dir), start_tick: None, duration_ticks: None, timebase: None };
        spec.targets[0].output_path = dir.join(format!("{id}.wav"));
        spec
    }

    #[test]
    fn test_start_queue_drains_every_queued_job() {
        let dir = tempdir().unwrap();
        let queue = Arc::new(QueueManager::new_in_memory());
        for i in 0..4 {
            queue.submit_job(spec_in(dir.path(), &format!("batch-{i}"))).unwrap();
        }
        assert!(queue.start_queue());
        wait_until_idle(&queue);
        let jobs = queue.list_jobs();
        assert_eq!(jobs.len(), 4);
        assert!(jobs.iter().all(|j| is_terminal(&j.status)), "all jobs must be finished after a single start");
    }

    #[test]
    fn test_parallel_pool_drains_and_journal_stays_consistent() {
        let dir = tempdir().unwrap();
        let journal = dir.path().join("queue.json");
        let queue = Arc::new(QueueManager::new_persisted(journal.clone()).unwrap());
        queue.set_parallelism(4).unwrap();
        for i in 0..8 {
            queue.submit_job(spec_in(dir.path(), &format!("par-{i}"))).unwrap();
        }
        assert!(queue.start_queue());
        wait_until_idle(&queue);
        assert!(queue.list_jobs().iter().all(|j| matches!(j.status, JobStatus::Completed)));

        let reloaded = QueueManager::new_persisted(journal).unwrap();
        assert_eq!(reloaded.parallelism(), 4);
        let jobs = reloaded.list_jobs();
        assert_eq!(jobs.len(), 8);
        assert!(jobs.iter().all(|j| matches!(j.status, JobStatus::Completed)));
    }

    #[test]
    fn test_set_parallelism_is_clamped() {
        let queue = QueueManager::new_in_memory();
        queue.set_parallelism(0).unwrap();
        assert_eq!(queue.parallelism(), 1);
        queue.set_parallelism(1000).unwrap();
        assert_eq!(queue.parallelism(), MAX_PARALLELISM);
    }

    #[test]
    fn test_failed_job_does_not_stop_the_batch() {
        let dir = tempdir().unwrap();
        let queue = Arc::new(QueueManager::new_in_memory());
        let mut bad = spec_in(dir.path(), "bad");
        bad.targets[0].output_path = dir.path().join("missing-dir").join("nested").join("x").join("bad.wav");
        queue.submit_job(bad).unwrap();
        queue.submit_job(spec_in(dir.path(), "good")).unwrap();
        queue.start_queue();
        wait_until_idle(&queue);
        assert!(queue.list_jobs().iter().all(|j| is_terminal(&j.status)));
    }

    #[test]
    fn test_paused_queue_holds_jobs_until_resumed() {
        let dir = tempdir().unwrap();
        let queue = Arc::new(QueueManager::new_in_memory());
        queue.submit_job(spec_in(dir.path(), "p1")).unwrap();
        queue.submit_job(spec_in(dir.path(), "p2")).unwrap();
        queue.pause_worker();
        assert!(queue.start_background_worker());
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(queue.list_jobs().iter().all(|j| matches!(j.status, JobStatus::Queued)));
        assert!(queue.is_worker_running(), "pausing must not kill the worker");
        queue.resume_worker();
        wait_until_idle(&queue);
        assert!(queue.list_jobs().iter().all(|j| is_terminal(&j.status)));
    }

    #[test]
    fn test_stop_worker_exits_without_running_jobs() {
        let dir = tempdir().unwrap();
        let queue = Arc::new(QueueManager::new_in_memory());
        queue.submit_job(spec_in(dir.path(), "s1")).unwrap();
        queue.pause_worker();
        assert!(queue.start_background_worker());
        queue.stop_worker();
        wait_until_idle(&queue);
        assert!(queue.is_stop_requested());
        assert!(queue.list_jobs().iter().all(|j| matches!(j.status, JobStatus::Queued)));
    }

    #[test]
    fn test_cancelled_queued_job_is_skipped() {
        let dir = tempdir().unwrap();
        let queue = Arc::new(QueueManager::new_in_memory());
        queue.submit_job(spec_in(dir.path(), "c1")).unwrap();
        queue.submit_job(spec_in(dir.path(), "c2")).unwrap();
        queue.cancel_job("c1").unwrap();
        queue.start_queue();
        wait_until_idle(&queue);
        let jobs = queue.list_jobs();
        let c1 = jobs.iter().find(|j| j.spec.job_id == "c1").unwrap();
        assert_eq!(c1.status, JobStatus::Cancelled);
        assert!(!dir.path().join("c1.wav").exists());
    }

    #[test]
    fn test_finished_job_status_is_not_overwritten_by_controls() {
        let dir = tempdir().unwrap();
        let queue = QueueManager::new_in_memory();
        queue.submit_job(spec_in(dir.path(), "done")).unwrap();
        queue.process_next_job().unwrap();
        let before = queue.list_jobs()[0].status.clone();
        queue.cancel_job("done").unwrap();
        queue.pause_job("done").unwrap();
        assert_eq!(queue.list_jobs()[0].status, before);
    }

    #[test]
    fn test_remove_rejects_processing_job() {
        let queue = QueueManager::new_in_memory();
        queue.submit_job(make_test_spec("busy", JobPriority::Normal)).unwrap();
        queue.claim_next().unwrap();
        assert!(queue.remove_job("busy").is_err());
        assert_eq!(queue.list_jobs().len(), 1);
    }

    #[test]
    fn test_cancel_active_job_marks_cancelled_via_progress() {
        let queue = QueueManager::new_in_memory();
        queue.submit_job(make_test_spec("act", JobPriority::Normal)).unwrap();
        let (id, spec) = queue.claim_next().unwrap();
        queue.cancel_job(&id).unwrap();
        queue.finish_job(&id, &spec, &Ok(()), 1).unwrap();
        assert_eq!(queue.list_jobs()[0].status, JobStatus::Cancelled);
    }

    #[test]
    fn test_persisted_processing_job_is_requeued_on_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("journal.json");
        let queue = QueueManager::new_persisted(path.clone()).unwrap();
        queue.submit_job(make_test_spec("crash", JobPriority::Normal)).unwrap();
        queue.claim_next().unwrap();
        queue.persist_state().unwrap();
        drop(queue);
        let reloaded = QueueManager::new_persisted(path).unwrap();
        assert_eq!(reloaded.list_jobs()[0].status, JobStatus::Queued);
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
        spec.source = SourceDescriptor::File { path: write_wav(dir.path()), start_tick: None, duration_ticks: None, timebase: None };
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
