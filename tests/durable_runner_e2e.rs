use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[test]
fn caches_a_real_command_and_reports_latency() {
    let workspace = unique_temp_dir();
    let diary_path = workspace.join("diary.tsv");
    let counter_path = workspace.join("wrapped-command-count.txt");

    let fresh_started = Instant::now();
    let fresh = run_anvil(&diary_path, &counter_path);
    let fresh_run_ms = fresh_started.elapsed().as_secs_f64() * 1_000.0;
    assert_success(&fresh);
    assert_eq!(String::from_utf8_lossy(&fresh.stdout), "rendered invoice\n");

    let cached_started = Instant::now();
    let cached = run_anvil(&diary_path, &counter_path);
    let cache_hit_ms = cached_started.elapsed().as_secs_f64() * 1_000.0;
    assert_success(&cached);
    assert_eq!(String::from_utf8_lossy(&cached.stdout), "rendered invoice\n");

    // The shell command appends to this file only when Anvil actually starts it.
    // One line proves the second identical request was served from the diary.
    assert_eq!(fs::read_to_string(&counter_path).unwrap().lines().count(), 1);
    assert_eq!(fs::read_to_string(&diary_path).unwrap().lines().count(), 2);

    let status = Command::new(anvil_binary())
        .arg("status")
        .arg(&diary_path)
        .output()
        .expect("run anvil status");
    assert_success(&status);
    assert_eq!(
        String::from_utf8_lossy(&status.stdout),
        "completed_steps=1\nrender:invoice\n"
    );

    println!(
        "metrics: fresh_run_ms={fresh_run_ms:.3} cache_hit_ms={cache_hit_ms:.3}"
    );

    fs::remove_dir_all(workspace).unwrap();
}

fn run_anvil(diary_path: &Path, counter_path: &Path) -> Output {
    Command::new(anvil_binary())
        .args(["run", "--diary"])
        .arg(diary_path)
        .args([
            "render:invoice",
            "invoice-v1",
            "--",
            "sh",
            "-c",
            "printf 'rendered invoice\\n'; printf 'executed\\n' >> \"$1\"",
            "anvil-e2e",
        ])
        .arg(counter_path)
        .output()
        .expect("run anvil")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn anvil_binary() -> &'static str {
    env!("CARGO_BIN_EXE_anvil")
}

fn unique_temp_dir() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("anvil-e2e-{}-{timestamp}", process::id()));
    fs::create_dir(&path).expect("create isolated test directory");
    path
}
