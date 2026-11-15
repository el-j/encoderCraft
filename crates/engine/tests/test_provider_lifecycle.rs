//! End-to-end integration tests for codec provider management, resolution precedence, and lifecycle.

use std::sync::{Arc, PoisonError};

use encodercraft_color::PixelFormat;
use encodercraft_core::{CURRENT_PROTOCOL_VERSION, ContainerType, ProviderManifest, ProviderOperation, ProviderOperationKind};
use encodercraft_engine::QueueManager;
use encodercraft_engine::cmd::CommandRegistry;
use encodercraft_engine::provider::{ProviderRegistry, ResolvedCodec};

#[test]
fn test_provider_command_lifecycle() {
    let queue = Arc::new(QueueManager::new_in_memory());
    let reg = CommandRegistry::new();

    // Register dummy provider into queue's provider registry
    let manifest = ProviderManifest {
        id: "test.x264.plugin".to_string(),
        name: "Test x264 Provider".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: CURRENT_PROTOCOL_VERSION,
        encodercraft_version: None,
        description: Some("Community x264 bridge".to_string()),
        author: Some("ArtCraft".to_string()),
        license: "GPL-2.0-or-later".to_string(),
        homepage: None,
        executable: std::path::PathBuf::from("bin/x264-worker"),
        operations: vec![ProviderOperation {
            operation_kind: ProviderOperationKind::VideoEncode,
            codec_id: "ext-h264".to_string(),
            display_name: "External x264".to_string(),
            aliases: vec!["custom-264".to_string()],
            profiles: vec!["high".to_string()],
            pixel_formats: vec![PixelFormat::Yuv420p],
            sample_formats: vec![],
            sample_rates: vec![],
            containers: vec![ContainerType::IsoBmffMp4],
            parameters: vec![],
        }],
    };

    {
        let lock = queue.provider_registry();
        let mut r = lock.lock().unwrap_or_else(PoisonError::into_inner);
        r.register(manifest, std::path::PathBuf::from("/tmp/provider"), std::path::PathBuf::from("/tmp/provider/bin/x264-worker")).unwrap();
    }

    // 1. provider.list
    let list_val = reg.dispatch(&queue, "provider.list", serde_json::json!({})).unwrap();
    let list = list_val.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "test.x264.plugin");
    assert_eq!(list[0]["enabled"], false);

    // 2. provider.inspect
    let inspect_val = reg.dispatch(&queue, "provider.inspect", serde_json::json!({ "providerId": "test.x264.plugin" })).unwrap();
    assert_eq!(inspect_val["name"], "Test x264 Provider");
    assert_eq!(inspect_val["license"], "GPL-2.0-or-later");

    // 3. provider.enable
    let enable_val = reg.dispatch(&queue, "provider.enable", serde_json::json!({ "providerId": "test.x264.plugin" })).unwrap();
    assert_eq!(enable_val["success"], true);
    assert_eq!(enable_val["enabled"], true);

    // Verify registry updated
    {
        let lock = queue.provider_registry();
        let r = lock.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(r.get("test.x264.plugin").unwrap().enabled);
    }

    // 4. provider.disable
    let disable_val = reg.dispatch(&queue, "provider.disable", serde_json::json!({ "providerId": "test.x264.plugin" })).unwrap();
    assert_eq!(disable_val["success"], true);
    assert_eq!(disable_val["enabled"], false);

    // Verify disabled
    {
        let lock = queue.provider_registry();
        let r = lock.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(!r.get("test.x264.plugin").unwrap().enabled);
    }
}

#[test]
fn test_provider_resolution_precedence() {
    let mut registry = ProviderRegistry::new();
    let manifest = ProviderManifest {
        id: "rogue.h264".to_string(),
        name: "Shadowing H264".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: CURRENT_PROTOCOL_VERSION,
        encodercraft_version: None,
        description: None,
        author: None,
        license: "MIT".to_string(),
        homepage: None,
        executable: std::path::PathBuf::from("worker"),
        operations: vec![
            ProviderOperation {
                operation_kind: ProviderOperationKind::VideoEncode,
                codec_id: "h264".to_string(), // Tries to shadow built-in!
                display_name: "Shadow H264".to_string(),
                aliases: vec![],
                profiles: vec![],
                pixel_formats: vec![],
                sample_formats: vec![],
                sample_rates: vec![],
                containers: vec![ContainerType::IsoBmffMp4],
                parameters: vec![],
            },
            ProviderOperation {
                operation_kind: ProviderOperationKind::VideoEncode,
                codec_id: "svt-av1-custom".to_string(),
                display_name: "Custom AV1".to_string(),
                aliases: vec![],
                profiles: vec![],
                pixel_formats: vec![],
                sample_formats: vec![],
                sample_rates: vec![],
                containers: vec![ContainerType::MatroskaMkv],
                parameters: vec![],
            },
        ],
    };

    registry.register(manifest, std::path::PathBuf::from("/prov"), std::path::PathBuf::from("/prov/worker")).unwrap();
    registry.enable("rogue.h264").unwrap();

    // 1. Generic "h264" MUST resolve to BuiltIn, NEVER the provider!
    let res = registry.resolve_video_codec("h264", ContainerType::IsoBmffMp4).unwrap();
    assert!(matches!(res, ResolvedCodec::BuiltIn(_)));

    // 2. Custom codec resolves to External when enabled
    let res_custom = registry.resolve_video_codec("svt-av1-custom", ContainerType::MatroskaMkv).unwrap();
    assert!(matches!(res_custom, ResolvedCodec::External { .. }));

    // 3. Container incompatibility is rejected
    let res_bad_container = registry.resolve_video_codec("svt-av1-custom", ContainerType::Wave);
    assert!(res_bad_container.is_err());
}
