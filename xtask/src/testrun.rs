//! `cargo xtask test`: builds every workspace test target once, then runs the test binaries side
//! by side.
//!
//! `cargo test --workspace` runs one test binary at a time, so a few binaries that mostly wait
//! (real-media QA conversions, watch-folder settle probes) set the pace for the whole suite. This
//! overlaps the binaries, except the shared-memory ones in [`ONE_AT_A_TIME`], and runs each one's
//! tests on fewer threads than `cargo test` would ([`test_threads`]), so the suite as a whole keeps
//! about one test thread per core instead of one per core in every binary. A binary runs from its package
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

    fn run(&self, test_threads: usize) -> std::io::Result<Output> {
        let threads = format!("--test-threads={test_threads}");
        match self {
            Self::Binary(binary) => Command::new(&binary.executable)
                .arg(threads)
                .current_dir(&binary.package_dir)
                .env("CARGO_MANIFEST_DIR", &binary.package_dir)
                .stdin(Stdio::null())
                .output(),
            Self::DocTests => Command::new("cargo").args(["test", "--workspace", "--doc", "--"]).arg(threads).stdin(Stdio::null()).output(),
        }
    }
}

/// The threads each test binary runs its tests on, given `RUST_TEST_THREADS` (`set`) if any: the cores are split
/// evenly between the binaries running side by side and the tests inside each, so a binary of a few long tests
/// (the real-media QA matrix) still runs them together on a large machine. At least two, so a binary's waits
/// (settle timers, child processes) overlap.
fn test_threads(cores: usize, set: Option<usize>) -> usize {
    set.filter(|&n| n > 0).unwrap_or_else(|| cores.isqrt().max(2))
}

/// How many binaries run side by side on `cores` when each runs `test_threads` threads: enough to fill the cores,
/// at least one, and no more than there are `jobs`. The one-at-a-time lane runs next to them, so the suite runs
/// about `cores + test_threads` test threads: six on a 4-core CI runner, where each of the binaries running side by
/// side would otherwise have started four.
fn side_by_side_workers(cores: usize, test_threads: usize, jobs: usize) -> usize {
    (cores / test_threads.max(1)).max(1).min(jobs)
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

    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let test_threads = test_threads(cores, std::env::var("RUST_TEST_THREADS").ok().and_then(|n| n.parse().ok()));
    let workers = side_by_side_workers(cores, test_threads, side_by_side.len());
    println!(
        "Running {binary_count} test binaries and the doc tests on {cores} cores: {} shared-memory binaries one at a time, the rest {workers} at a time, {test_threads} test threads each...",
        one_at_a_time.len()
    );
    let started = Instant::now();
    let failed = Mutex::new(Vec::new());
    let drain = |queue: &Mutex<VecDeque<Job>>| {
        // Take the job in its own statement: a guard in the `while let` condition would stay locked
        // for the whole loop body and run the jobs one at a time.
        while let Some(job) = next_job(queue) {
            if !run_and_report(&job, test_threads) {
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
fn run_and_report(job: &Job, test_threads: usize) -> bool {
    let started = Instant::now();
    let result = job.run(test_threads);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The test threads the whole suite runs: the side-by-side binaries plus the one-at-a-time lane.
    fn suite_threads(cores: usize) -> usize {
        let threads = test_threads(cores, None);
        (side_by_side_workers(cores, threads, 60) + 1) * threads
    }

    #[test]
    fn the_suite_keeps_about_one_test_thread_per_core() {
        assert_eq!(suite_threads(4), 6, "a 4-core CI runner: two binaries side by side and the shared-memory lane, two threads each");
        assert_eq!(suite_threads(32), 35, "32 cores: six binaries side by side and the lane, five threads each");
        assert_eq!(side_by_side_workers(1, test_threads(1, None), 60), 1, "one core still runs the binaries");
        assert_eq!(side_by_side_workers(32, test_threads(32, None), 3), 3, "no more workers than jobs");
        assert_eq!(test_threads(32, Some(1)), 1, "RUST_TEST_THREADS wins");
    }
}
