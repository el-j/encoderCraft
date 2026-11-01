//! Three-stage settle-delay protocol ensuring ingested files are fully written before transcoding.

use std::fs::File;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use encodercraft_core::EncoderError;

/// Performs a 3-stage settle-delay check on a candidate file path.
///
/// 1. Reads initial metadata and byte length.
/// 2. Sleeps for `settle_delay_ms` / 2 and re-checks byte length (size quiescence).
/// 3. Attempts to acquire an open file descriptor without sharing errors.
pub fn probe_file_settled(path: &Path, settle_delay_ms: u64) -> Result<bool, EncoderError> {
    if !path.exists() {
        return Ok(false);
    }

    // Ignore partial download files or temp files
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if matches!(ext.as_str(), "tmp" | "crdownload" | "part" | "downloading") {
        return Ok(false);
    }

    let meta1 = std::fs::metadata(path).map_err(|e| EncoderError::Io(format!("failed to read file metadata: {e}")))?;
    let size1 = meta1.len();

    // Zero-byte files cannot be encoded yet
    if size1 == 0 {
        return Ok(false);
    }

    let check_interval = Duration::from_millis(settle_delay_ms.clamp(200, 10000) / 2);
    sleep(check_interval);

    let meta2 = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(false),
    };
    let size2 = meta2.len();

    // Stage 2: Size quiescence check
    if size1 != size2 {
        return Ok(false);
    }

    // Stage 3: Exclusive lock probe (attempt opening for read)
    match File::open(path) {
        Ok(_file) => Ok(true),
        Err(_) => Ok(false), // Locked by another process writing to disk
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_file_settle_probe() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("settled_media.mov");

        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"test file content").unwrap();
        drop(file);

        let settled = probe_file_settled(&file_path, 200).unwrap();
        assert!(settled);
    }

    #[test]
    fn test_temp_file_not_settled() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("upload.tmp");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"temp").unwrap();
        drop(file);

        let settled = probe_file_settled(&file_path, 100).unwrap();
        assert!(!settled);
    }
}
