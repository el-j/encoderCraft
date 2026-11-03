//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust. External tools (`cargo`) are invoked through `std::process::Command`.

mod bench;
mod ico;
mod layers;
mod version;

use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  layers          enforce the crate dependency layering (docs/architecture.md §2)
  test            run workspace tests
  bench           run automated transcoding performance benchmark suite
  version         print the workspace version; `version set <X.Y.Z[-pre]>` changes it
  ico             write a Windows .ico from PNGs (used by packaging/icons.sh)
  ci              fmt --check, clippy -D warnings, test, layers (stops at first failure)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("layers") => cmd_layers(),
        Some("test") => cmd_test(),
        Some("bench") => bench::run_benchmarks(),
        Some("ci") => cmd_ci(),
        Some("version") => version::run(args.get(1..).unwrap_or(&[])),
        Some("ico") => ico::run(args.get(1..).unwrap_or(&[])),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_layers() -> Result<(), String> {
    println!("Checking crate dependency layers...");
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|e| format!("failed to run cargo metadata: {e}"))?;

    if !output.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&output.stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    layers::check(&stdout)?;
    println!("All layers are compliant.");
    Ok(())
}

fn cmd_test() -> Result<(), String> {
    println!("Running workspace tests...");
    let status = Command::new("cargo").args(["test", "--workspace"]).status().map_err(|e| format!("failed to spawn cargo test: {e}"))?;

    if !status.success() {
        return Err("cargo test failed".into());
    }
    Ok(())
}

fn cmd_ci() -> Result<(), String> {
    println!("=== 1. Checking formatting ===");
    let status = Command::new("cargo").args(["fmt", "--all", "--", "--check"]).status().map_err(|e| format!("failed to run cargo fmt: {e}"))?;
    if !status.success() {
        return Err("cargo fmt --check failed".into());
    }

    println!("=== 2. Running clippy ===");
    let status = Command::new("cargo")
        .args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])
        .status()
        .map_err(|e| format!("failed to run cargo clippy: {e}"))?;
    if !status.success() {
        return Err("cargo clippy failed".into());
    }

    println!("=== 3. Running tests ===");
    cmd_test()?;

    println!("=== 4. Checking layers ===");
    cmd_layers()?;

    println!("=== All CI checks passed successfully! ===");
    Ok(())
}
