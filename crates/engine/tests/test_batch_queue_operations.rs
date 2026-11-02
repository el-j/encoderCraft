//! Integration tests verifying Adobe Media Encoder batch queue operations.

use std::path::PathBuf;

use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, JobStatus, OutputTarget, SourceDescriptor, StartPolicy};
use encodercraft_engine::QueueManager;

#[test]
fn test_batch_operations_workflow() {
    let queue = QueueManager::new_in_memory();

    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "batch-source-1".into(),
        client_info: ClientInfo { app_name: "AME Test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::Paused,
        source: SourceDescriptor::File { path: PathBuf::from("raw_footage.mov"), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget {
            target_id: "target-1".into(),
            output_path: PathBuf::from("export_4k.mp4"),
            preset_name: "YouTube 4K".into(),
            container: ContainerType::IsoBmffMp4,
            ..Default::default()
        }],
    };

    // 1. Submit initial job
    assert!(queue.submit_job(spec).is_ok());

    // 2. Add multiple output targets to the single source (Batch Multi-Target)
    let target_1080p = OutputTarget {
        target_id: "target-2".into(),
        output_path: PathBuf::from("export_1080p.mp4"),
        preset_name: "YouTube 1080p".into(),
        container: ContainerType::IsoBmffMp4,
        ..Default::default()
    };
    assert!(queue.add_output_target("batch-source-1", target_1080p).is_ok());

    let target_prores = OutputTarget {
        target_id: "target-3".into(),
        output_path: PathBuf::from("export_master.mov"),
        preset_name: "ProRes 422 HQ".into(),
        container: ContainerType::QuickTimeMov,
        ..Default::default()
    };
    assert!(queue.add_output_target("batch-source-1", target_prores).is_ok());

    let jobs = queue.list_jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].spec.targets.len(), 3);

    // 3. Duplicate output target
    assert!(queue.duplicate_output_target("batch-source-1", 1).is_ok());
    let jobs = queue.list_jobs();
    assert_eq!(jobs[0].spec.targets.len(), 4);

    // 4. Remove output target
    assert!(queue.remove_output_target("batch-source-1", 3).is_ok());
    let jobs = queue.list_jobs();
    assert_eq!(jobs[0].spec.targets.len(), 3);

    // 5. Reset status of a job
    assert!(queue.cancel_job("batch-source-1").is_ok());
    let jobs = queue.list_jobs();
    assert_eq!(jobs[0].status, JobStatus::Cancelled);

    assert!(queue.reset_job_status("batch-source-1").is_ok());
    let jobs = queue.list_jobs();
    assert_eq!(jobs[0].status, JobStatus::Queued);

    // 6. Test Multi-Clip Stitching
    let clips = vec![PathBuf::from("scene_01.mov"), PathBuf::from("scene_02.mov"), PathBuf::from("scene_03.mov")];
    let stitch_res = queue.stitch_sources_into_job(clips, PathBuf::from("stitched_movie.mp4"), "YouTube 1080p".into());
    assert!(stitch_res.is_ok());

    let jobs = queue.list_jobs();
    assert_eq!(jobs.len(), 2);
    assert!(matches!(jobs[1].spec.source, SourceDescriptor::StitchedClips { .. }));

    // 7. Clear completed jobs
    let removed = queue.clear_completed_jobs().unwrap();
    assert_eq!(removed, 0); // None completed yet
}
