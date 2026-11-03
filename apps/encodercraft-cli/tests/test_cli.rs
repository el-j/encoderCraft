//! Integration tests verifying encoderCraft headless CLI subcommands and batch processing.

use std::process::Command;

#[test]
fn test_cli_help_and_presets() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");

    // 1. --help output
    let output = Command::new(bin).arg("--help").output().expect("failed to run cli");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("encoderCraft CLI"));
    assert!(stdout.contains("encode"));
    assert!(stdout.contains("probe"));
    assert!(stdout.contains("presets"));
    assert!(stdout.contains("watch"));

    // 2. presets output
    let p_output = Command::new(bin).arg("presets").output().expect("failed to run presets");
    assert!(p_output.status.success());
    let p_stdout = String::from_utf8_lossy(&p_output.stdout);
    assert!(p_stdout.contains("YouTube"));
    assert!(p_stdout.contains("Match Source"));
}

#[test]
fn test_cli_version_matches_manifest() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    for flag in ["--version", "-V"] {
        let output = Command::new(bin).arg(flag).output().expect("failed to run cli");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), format!("encodercraft-cli {}", env!("CARGO_PKG_VERSION")));
    }
}
