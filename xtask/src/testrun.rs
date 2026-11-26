//! `cargo xtask test`: builds every workspace test target once, then runs the test binaries side
//! by side.
//!
//! `cargo test --workspace` runs one test binary at a time, so a few binaries that mostly wait
//! (real-media QA conversions, watch-folder settle probes) set the pace for the whole suite. Each
//! binary still runs its own tests on several threads, as under `cargo test`; this only overlaps
//! the binaries, except the shared-memory ones in [`ONE_AT_A_TIME`]. A binary runs from its package
//! directory with `CARGO_MANIFEST_DIR` set, as `cargo test` does, and its output is printed in one
//! block when it finishes. Every binary runs even after one fails, and the failures are listed at
//! the end.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

struct TestBinary {
    /// Source file of the test target, relative to the workspace when possible.
    label: String,
    executable: PathBuf,
    package_dir: PathBuf,
}

enum Job {
    Binary(TestBinary),
    DocTests,
}

impl Job {
    fn label(&self) -> &str {
        match self {
            Self::Binary(binary) => &binary.label,
            Self::DocTests => "doc tests (cargo test --workspace --doc)",
        }
    }

    fn run(&self) -> std::io::Result<Output> {
        match self {
            Self::Binary(binary) => Command::new(&binary.executable)
                .current_dir(&binary.package_dir)
                .env("CARGO_MANIFEST_DIR", &binary.package_dir)
                .stdin(Stdio::null())
                .output(),
            Self::DocTests => Command::new("cargo").args(["test", "--workspace", "--doc"]).stdin(Stdio::null()).output(),
        }
    }
}

/// Test binaries that open shared-memory streams (iceoryx2), directly or through CLI processes they
/// start.
///
/// Side by side, two of them can create and drop iceoryx2 nodes at the same moment, and on Windows
/// iceoryx2's dead-node cleanup then panics in one of them. So these run one at a time, next to the
/// other binaries. A listed file that no longer builds a test binary is an error, so the list cannot
/// silently go stale.
const ONE_AT_A_TIME: &[&str] = &[
    "crates/shared-stream/src/lib.rs",
    "crates/source/src/lib.rs",
    "crates/pipeline/tests/conformance.rs",
    "apps/encodercraft-cli/tests/test_cli.rs",
];

pub fn run() -> Result<(), String> {
    let binaries = build_test_binaries()?;
    let binary_count = binaries.len();
    if let Some(missing) = ONE_AT_A_TIME.iter().find(|label| !binaries.iter().any(|binary| binary.label == **label)) {
        return Err(format!(
            "xtask/src/testrun.rs runs `{missing}` one at a time, but no test binary is built from it any more; update ONE_AT_A_TIME"
        ));
    }
    let (one_at_a_time, side_by_side): (Vec<_>, Vec<_>) = binaries.into_iter().partition(|binary| ONE_AT_A_TIME.contains(&binary.label.as_str()));
    let one_at_a_time: VecDeque<Job> = one_at_a_time.into_iter().map(Job::Binary).collect();
    let mut side_by_side: VecDeque<Job> = side_by_side.into_iter().map(Job::Binary).collect();
    // First, so compiling the doc tests overlaps the binaries even when there are few cores.
    side_by_side.push_front(Job::DocTests);

    let workers = std::thread::available_parallelism().map_or(1, usize::from).min(side_by_side.len());
    println!(
        "Running {binary_count} test binaries and the doc tests: {} shared-memory binaries one at a time, the rest {workers} at a time...",
        one_at_a_time.len()
    );
    let started = Instant::now();
    let failed = Mutex::new(Vec::new());
    let drain = |queue: &Mutex<VecDeque<Job>>| {
        // Take the job in its own statement: a guard in the `while let` condition would stay locked
        // for the whole loop body and run the jobs one at a time.
        while let Some(job) = next_job(queue) {
            if !run_and_report(&job) {
                failed.lock().unwrap_or_else(PoisonError::into_inner).push(job.label().to_owned());
            }
        }
    };
    let one_at_a_time = Mutex::new(one_at_a_time);
    let side_by_side = Mutex::new(side_by_side);
    std::thread::scope(|scope| {
        scope.spawn(|| drain(&one_at_a_time));
        for _ in 0..workers {
            scope.spawn(|| drain(&side_by_side));
        }
    });

    let failed = failed.into_inner().unwrap_or_else(PoisonError::into_inner);
    let elapsed = started.elapsed().as_secs_f64();
    if failed.is_empty() {
        println!("All {binary_count} test binaries and the doc tests passed in {elapsed:.1}s.");
        Ok(())
    } else {
        Err(format!("{} of {} test runs failed after {elapsed:.1}s:\n  {}", failed.len(), binary_count + 1, failed.join("\n  ")))
    }
}

fn next_job(queue: &Mutex<VecDeque<Job>>) -> Option<Job> {
    queue.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
}

/// Builds all test targets with `cargo test --no-run` and returns the test executables it reports.
fn build_test_binaries() -> Result<Vec<TestBinary>, String> {
    let output = Command::new("cargo")
        .args(["test", "--workspace", "--no-run", "--message-format=json-render-diagnostics"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("failed to spawn cargo test --no-run: {e}"))?;
    if !output.status.success() {
        return Err("cargo test --no-run failed".into());
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new(""));
    let binaries: Vec<TestBinary> = String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| test_binary(line, workspace)).collect();
    if binaries.is_empty() {
        return Err("cargo test --no-run reported no test executables".into());
    }
    Ok(binaries)
}

/// Reads one `--message-format=json` line; `Some` for a test executable artifact.
fn test_binary(line: &str, workspace: &Path) -> Option<TestBinary> {
    let message: serde_json::Value = serde_json::from_str(line).ok()?;
    if message.get("reason")?.as_str()? != "compiler-artifact" || !message.pointer("/profile/test")?.as_bool()? {
        return None;
    }
    let executable = PathBuf::from(message.get("executable")?.as_str()?);
    let package_dir = Path::new(message.get("manifest_path")?.as_str()?).parent()?.to_path_buf();
    let source = Path::new(message.pointer("/target/src_path")?.as_str()?);
    let label = source.strip_prefix(workspace).unwrap_or(source).display().to_string().replace('\\', "/");
    Some(TestBinary { label, executable, package_dir })
}

/// Runs one job and prints its output as a single block; `true` when it passed.
fn run_and_report(job: &Job) -> bool {
    let started = Instant::now();
    let result = job.run();
    let elapsed = started.elapsed().as_secs_f64();
    let (passed, body) = match result {
        Ok(output) => {
            let body = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            (output.status.success(), body)
        }
        Err(e) => (false, format!("could not start: {e}\n")),
    };
    let verdict = if passed { "ok" } else { "FAILED" };
    // One `print!` per job, so blocks from parallel jobs never interleave.
    print!("\n     Running {} ({verdict}, {elapsed:.2}s)\n{}\n", job.label(), body.trim_end_matches('\n'));
    passed
}
