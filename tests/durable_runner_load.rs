use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_COLD_STEPS: usize = 100;
const DEFAULT_CACHE_HITS: usize = 1_000;

#[test]
#[ignore = "load benchmark; run with --ignored --nocapture"]
fn measures_real_cli_throughput() {
    let cold_steps = configured_count("ANVIL_LOAD_STEPS", DEFAULT_COLD_STEPS);
    let cache_hits = configured_count("ANVIL_CACHE_HITS", DEFAULT_CACHE_HITS);
    let workspace = unique_temp_dir();
    let diary_path = workspace.join("diary.tsv");
    let counter_path = workspace.join("wrapped-command-count.txt");

    let cold_started = Instant::now();
    for index in 0..cold_steps {
        let output = run_anvil(
            &diary_path,
            &counter_path,
            &format!("load:step-{index}"),
            &format!("payload-{index}"),
        );
        assert_success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "load output\n");
    }
    let cold_elapsed = cold_started.elapsed();

    let cache_started = Instant::now();
    for _ in 0..cache_hits {
        let output = run_anvil(&diary_path, &counter_path, "load:step-0", "payload-0");
        assert_success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "load output\n");
    }
    let cache_elapsed = cache_started.elapsed();

    // The wrapped command runs only for the unique cold steps. Cache hits still
    // create CLI processes and replay the diary, so their rate is end-to-end.
    assert_eq!(fs::read_to_string(&counter_path).unwrap().lines().count(), cold_steps);
    assert_eq!(
        fs::read_to_string(&diary_path).unwrap().lines().count(),
        cold_steps * 2
    );

    println!(
        "benchmark: cold_steps_per_second={:.1} cached_steps_per_second={:.1} cold_steps={cold_steps} cache_hits={cache_hits}",
        operations_per_second(cold_steps, cold_elapsed),
        operations_per_second(cache_hits, cache_elapsed),
    );

    fs::remove_dir_all(workspace).unwrap();
}

fn run_anvil(diary_path: &Path, counter_path: &Path, step_id: &str, input: &str) -> Output {
    Command::new(anvil_binary())
        .args(["run", "--diary"])
        .arg(diary_path)
        .args([step_id, input, "--", "sh", "-c"])
        .arg("printf 'load output\\n'; printf 'executed\\n' >> \"$1\"")
        .arg("anvil-load")
        .arg(counter_path)
        .output()
        .expect("run anvil")
}

fn configured_count(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|count: &usize| *count > 0)
        .unwrap_or(default)
}

fn operations_per_second(operations: usize, elapsed: Duration) -> f64 {
    operations as f64 / elapsed.as_secs_f64().max(f64::EPSILON)
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
    let path = std::env::temp_dir().join(format!("anvil-load-{}-{timestamp}", process::id()));
    fs::create_dir(&path).expect("create isolated load-test directory");
    path
}
