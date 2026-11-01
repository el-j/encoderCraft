//! Fuzz and property tests for watch folder configuration and settle probe.

use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use encodercraft_presets::PresetRegistry;
use encodercraft_watch::{PostProcessAction, WatchFolderConfig, WatchFolderService, probe_file_settled};
use proptest::prelude::*;

#[test]
fn test_nonexistent_file_settle() {
    let dir = tempdir().unwrap();
    let phantom = dir.path().join("does_not_exist.mp4");
    let res = probe_file_settled(&phantom, 50).unwrap();
    assert!(!res);
}

#[test]
fn test_zero_byte_file_not_settled() {
    let dir = tempdir().unwrap();
    let empty = dir.path().join("empty.mov");
    let _ = File::create(&empty).unwrap();
    let res = probe_file_settled(&empty, 50).unwrap();
    assert!(!res);
}

proptest! {
    #[test]
    fn test_config_serde_roundtrip(
        id in "[a-zA-Z0-9_-]{1,16}",
        preset in "[a-zA-Z0-9 ]{1,32}",
        settle_delay in 100u64..5000u64,
        enabled in any::<bool>(),
    ) {
        let cfg = WatchFolderConfig {
            id,
            watch_dir: "/tmp/watch".into(),
            output_dir: "/tmp/output".into(),
            preset_name: preset,
            post_action: PostProcessAction::DeleteSource,
            settle_delay_ms: settle_delay,
            enabled,
        };

        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: WatchFolderConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cfg, decoded);
    }

    #[test]
    fn test_fuzz_random_file_bytes(
        data in proptest::collection::vec(any::<u8>(), 1..1024),
        ext in "(mov|mp4|mkv|wav|png|jpg|tmp|crdownload|part)"
    ) {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join(format!("test_input.{ext}"));
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(&data).unwrap();
        }

        // Must never panic on any bytes
        let is_settled = probe_file_settled(&file_path, 20).unwrap();

        if matches!(ext.as_str(), "tmp" | "crdownload" | "part") {
            prop_assert!(!is_settled);
        } else {
            prop_assert!(is_settled);
        }
    }
}

#[test]
fn test_service_error_handling_post_action() {
    let dir = tempdir().unwrap();
    let watch_dir = dir.path().join("watch");
    let out_dir = dir.path().join("out");
    let err_dir = dir.path().join("errors");

    std::fs::create_dir_all(&watch_dir).unwrap();
    std::fs::create_dir_all(&out_dir).unwrap();

    let mut service = WatchFolderService::new(PresetRegistry::default());
    let mut cfg = WatchFolderConfig::new("wf-err", watch_dir, out_dir, "YouTube 1080p Full HD");
    cfg.post_action = PostProcessAction::MoveToErrors(err_dir.clone());
    service.add_config(cfg);

    let test_file = dir.path().join("failed.mp4");
    let mut f = File::create(&test_file).unwrap();
    f.write_all(b"bad content").unwrap();
    drop(f);

    // Fail transcode -> file moves to error dir
    service.handle_post_process("wf-err", &test_file, false).unwrap();
    assert!(!test_file.exists());
    assert!(err_dir.join("failed.mp4").exists());
}
