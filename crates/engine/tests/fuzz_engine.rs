use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
use encodercraft_engine::{CommandRegistry, QueueManager};
use proptest::prelude::*;
use std::path::PathBuf;

proptest! {
    #[test]
    fn test_queue_fuzz_no_panic(
        job_count in 1usize..20usize,
        prio_val in 0u8..4u8,
    ) {
        let queue = QueueManager::new_in_memory();
        let registry = CommandRegistry::new();

        let prio = match prio_val {
            0 => JobPriority::Low,
            1 => JobPriority::Normal,
            2 => JobPriority::High,
            _ => JobPriority::Urgent,
        };

        for i in 0..job_count {
            let spec = CraftJobSpec {
                version: "1.0".into(),
                job_id: format!("fuzz-job-{i}"),
                client_info: ClientInfo {
                    app_name: "fuzz".into(),
                    app_version: "1.0".into(),
                    pid: 1,
                    user_notes: None,
                },
                priority: prio,
                start_policy: StartPolicy::AutoStart,
                source: SourceDescriptor::File {
                    path: PathBuf::from("in.mov"),
                    start_tick: None,
                    duration_ticks: None,
                    timebase: None,
                },
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

            let _ = registry.dispatch(&queue, "queue.add", serde_json::to_value(&spec).unwrap());
        }

        let _ = registry.dispatch(&queue, "queue.list", serde_json::json!({}));
        let _ = queue.list_jobs();
    }
}
