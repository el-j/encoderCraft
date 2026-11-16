#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
#![no_main]

use encodercraft_dispatch::IpcRequest;
use encodercraft_engine::{EngineIpcHandler, QueueManager};
use encodercraft_ipc::IpcHandler;
use encodercraft_presets::PresetRegistry;
use libfuzzer_sys::fuzz_target;
use std::sync::{Arc, Mutex};

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let Ok(request) = serde_json::from_slice::<IpcRequest>(data) else {
        return;
    };
    if !matches!(
        request.method.as_str(),
        "app.ping"
            | "app.version"
            | "queue.list"
            | "queue.status"
            | "queue.cancel"
            | "queue.pause"
            | "queue.duplicate"
            | "queue.move_up"
            | "queue.move_down"
            | "queue.reset"
            | "queue.clear_finished"
    ) {
        return;
    }

    let queue = Arc::new(QueueManager::new_in_memory());
    let presets = Arc::new(Mutex::new(PresetRegistry::default()));
    let handler = EngineIpcHandler::new(queue, presets);
    let _ = handler.handle(&request);
});
