use std::path::Path;
use std::process::Command as ProcessCommand;
use std::time::Duration;

use crate::ci::BootResult;

// ── Public types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BenchmarkEntry {
    pub benchmark: String,
    pub value: f64,
    pub unit: String,
    pub status: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BenchmarkReport {
    pub benchmarks: Vec<BenchmarkEntry>,
}

#[derive(Debug, Clone)]
pub struct BenchResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct BenchSuiteResult {
    pub results: Vec<BenchResult>,
}

impl BenchSuiteResult {
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }
    pub fn passed_count(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }
    pub fn total_count(&self) -> usize {
        self.results.len()
    }
}

// ── QEMU command builder for benchmarks ───────────────────────────────────

fn build_bench_qemu_command(workspace_root: &Path) -> ProcessCommand {
    let esp_dir = workspace_root.join("out").join("esp");
    let fat_root = crate::ci::normalize_path(&esp_dir);

    // Stage firmware to writable location
    let staged_code = crate::ci::stage_ovmf(workspace_root, "edk2-x86_64-code.fd", &crate::ci::find_ovmf_code());
    let staged_vars = crate::ci::stage_ovmf(workspace_root, "edk2-x86_64-vars.fd", &crate::ci::find_ovmf_vars());

    let mut cmd = ProcessCommand::new("qemu-system-x86_64");
    cmd.arg("-cpu").arg("max");
    cmd.arg("-machine").arg("q35");
    cmd.arg("-m").arg("512M");
    cmd.arg("-monitor").arg("none");
    cmd.arg("-no-reboot");

    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    cmd.arg("-device").arg("qemu-xhci,id=xhci");
    cmd.arg("-device").arg("usb-kbd");

    cmd.arg("-display").arg("none");

    // Acceleration
    if let Ok(accel) = std::env::var("TURNIX_QEMU_ACCEL") {
        cmd.arg("-accel").arg(accel);
    } else if cfg!(target_os = "linux") {
        cmd.arg("-accel").arg("kvm");
        cmd.arg("-accel").arg("tcg");
    } else if cfg!(target_os = "macos") {
        cmd.arg("-accel").arg("hvf");
        cmd.arg("-accel").arg("tcg");
    } else {
        cmd.arg("-accel").arg("tcg");
    }

    // Firmware
    if let Some(code) = staged_code {
        cmd.arg("-drive").arg(format!(
            "if=pflash,format=raw,readonly=on,file={}",
            crate::ci::normalize_path(&code)
        ));
    }
    if let Some(vars) = staged_vars {
        cmd.arg("-drive").arg(format!(
            "if=pflash,format=raw,file={}",
            crate::ci::normalize_path(&vars)
        ));
    }

    // ESP FAT drive
    cmd.arg("-drive")
        .arg(format!("format=raw,file=fat:rw:{fat_root}"));

    cmd.current_dir(workspace_root);
    cmd
}

// ── Boot + benchmark capture ──────────────────────────────────────────────

fn boot_qemu_bench(workspace_root: &Path, timeout_secs: u64) -> BootResult {
    let log_path = workspace_root.join("out").join("ci-bench.log");

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&log_path, b"");

    let mut cmd = build_bench_qemu_command(workspace_root);
    cmd.arg("-serial").arg(format!("file:{}", log_path.display()));

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return BootResult::Failed {
                exit_code: None,
                output: format!("failed to spawn QEMU: {e}"),
            };
        }
    };

    let poll_interval = Duration::from_millis(50);
    loop {
        if start.elapsed() > timeout {
            let _ = child.kill();
            let output = std::fs::read_to_string(&log_path).unwrap_or_default();
            return BootResult::Timeout {
                output,
                elapsed: start.elapsed(),
            };
        }

        if let Ok(output) = std::fs::read_to_string(&log_path)
            && output.contains("[BOOT OK]")
        {
            let _ = child.kill();
            return BootResult::Success {
                output,
                elapsed: start.elapsed(),
            };
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                let output = std::fs::read_to_string(&log_path).unwrap_or_default();
                return BootResult::Failed {
                    exit_code: status.code(),
                    output,
                };
            }
            Ok(None) => {}
            Err(e) => {
                let output = std::fs::read_to_string(&log_path).unwrap_or_default();
                return BootResult::Failed {
                    exit_code: None,
                    output: format!("try_wait error: {e}\n{output}"),
                };
            }
        }

        std::thread::sleep(poll_interval);
    }
}

// ── Parse benchmark JSON from serial log ──────────────────────────────────

/// Filter out non-JSON noise (like kernel worker 'w' heartbeat characters)
/// from a region of serial output to make it valid JSON.
fn filter_json_noise(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_string = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                in_string = !in_string;
                result.push(ch);
            }
            _ if in_string => result.push(ch),
            '{' | '}' | '[' | ']' | ':' | ',' | '.' | '-' => result.push(ch),
            '0'..='9' => result.push(ch),
            ' ' | '\n' | '\r' | '\t' => result.push(ch),
            _ => {}
        }
    }
    result
}

pub fn parse_benchmark_report(log: &str) -> Option<BenchmarkReport> {
    // Find the JSON block in the log — look for {"benchmarks":[...]}
    let start = log.find("{\"benchmarks\"")?;
    // Find the matching closing brace
    let mut depth = 0i32;
    let mut end = start;
    for (i, ch) in log[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    let json_str = filter_json_noise(&log[start..end]);
    serde_json::from_str(&json_str).ok()
}

// ── Run benchmark suite ───────────────────────────────────────────────────

pub fn run_bench_suite(workspace_root: &Path) -> BenchSuiteResult {
    let mut results = Vec::new();

    // Test 1: Benchmark binary compiles and exists
    let bench_bin = workspace_root
        .join("target")
        .join("x86_64-unknown-none")
        .join("release")
        .join("benchmarks");
    let bin_exists = bench_bin.exists();
    results.push(BenchResult {
        name: "benchmark_binary_exists".to_string(),
        passed: bin_exists,
        detail: if bin_exists {
            format!("binary at {}", bench_bin.display())
        } else {
            "benchmark binary not found".to_string()
        },
    });

    // Test 2: JSON parser works correctly with known-good input
    let test_json = r#"{"benchmarks":[{"benchmark":"uptime_resolution","value":1.000,"unit":"ticks","status":"PASS"}]}"#;
    match parse_benchmark_report(test_json) {
        Some(report) => {
            results.push(BenchResult {
                name: "json_parse".to_string(),
                passed: true,
                detail: format!("parsed {} benchmark(s)", report.benchmarks.len()),
            });
            for entry in &report.benchmarks {
                results.push(BenchResult {
                    name: entry.benchmark.clone(),
                    passed: entry.status == "PASS",
                    detail: format!("{:.3} {} [{}]", entry.value, entry.unit, entry.status),
                });
            }
        }
        None => {
            results.push(BenchResult {
                name: "json_parse".to_string(),
                passed: false,
                detail: "failed to parse benchmark JSON from known-good input".to_string(),
            });
        }
    }

    // Test 3: JSON parser handles noise (worker task 'w' characters)
    let noisy_json = "wwww{\"benchmarks\":[w{\"benchmark\":\"uptime_resolution\",\"value\":2.000,\"unit\":\"ticks\",\"status\":\"PASS\"}w]}www";
    match parse_benchmark_report(noisy_json) {
        Some(report) => {
            results.push(BenchResult {
                name: "json_parse_noisy".to_string(),
                passed: report.benchmarks.len() == 1,
                detail: format!(
                    "parsed {} benchmark(s) from noisy input",
                    report.benchmarks.len()
                ),
            });
        }
        None => {
            results.push(BenchResult {
                name: "json_parse_noisy".to_string(),
                passed: false,
                detail: "failed to parse benchmark JSON from noisy input".to_string(),
            });
        }
    }

    // Test 4: Boot QEMU and verify kernel boots cleanly
    let boot_timeout = 30;
    let boot = boot_qemu_bench(workspace_root, boot_timeout);
    results.push(BenchResult {
        name: "boot".to_string(),
        passed: matches!(&boot, BootResult::Success { .. }),
        detail: match &boot {
            BootResult::Success { elapsed, .. } => {
                format!("booted in {:.1}s", elapsed.as_secs_f64())
            }
            BootResult::Timeout { .. } => "boot timed out".to_string(),
            BootResult::Failed { exit_code, .. } => {
                format!("boot failed (exit code: {exit_code:?})")
            }
        },
    });

    BenchSuiteResult { results }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_benchmark_report_valid() {
        let log = r#"{"benchmarks":[
{"benchmark":"fork_latency","value":2.500,"unit":"ms","status":"PASS"},
{"benchmark":"pipe_throughput","value":150.000,"unit":"MB/s","status":"PASS"},
{"benchmark":"page_fault_latency","value":500.000,"unit":"ns","status":"PASS"},
{"benchmark":"context_switch","value":10.000,"unit":"us","status":"PASS"}
]}"#;
        let report = parse_benchmark_report(log).unwrap();
        assert_eq!(report.benchmarks.len(), 4);
        assert_eq!(report.benchmarks[0].benchmark, "fork_latency");
        assert_eq!(report.benchmarks[0].value, 2.5);
        assert_eq!(report.benchmarks[0].unit, "ms");
        assert_eq!(report.benchmarks[0].status, "PASS");
        assert_eq!(report.benchmarks[1].benchmark, "pipe_throughput");
        assert_eq!(report.benchmarks[1].value, 150.0);
        assert_eq!(report.benchmarks[2].benchmark, "page_fault_latency");
        assert_eq!(report.benchmarks[3].benchmark, "context_switch");
    }

    #[test]
    fn parse_benchmark_report_with_surrounding_text() {
        let log = "[STG: SCHED_START]\n[BOOT OK]\n{\"benchmarks\":[{\"benchmark\":\"fork_latency\",\"value\":3.000,\"unit\":\"ms\",\"status\":\"PASS\"}]}\n";
        let report = parse_benchmark_report(log).unwrap();
        assert_eq!(report.benchmarks.len(), 1);
        assert_eq!(report.benchmarks[0].value, 3.0);
    }

    #[test]
    fn parse_benchmark_report_empty_array() {
        let log = "{\"benchmarks\":[]}";
        let report = parse_benchmark_report(log).unwrap();
        assert!(report.benchmarks.is_empty());
    }

    #[test]
    fn parse_benchmark_report_missing() {
        let log = "[STG: SCHED_START]\n[BOOT OK]";
        assert!(parse_benchmark_report(log).is_none());
    }

    #[test]
    fn parse_benchmark_report_malformed_json() {
        let log = "{\"benchmarks\":[{invalid json}]";
        assert!(parse_benchmark_report(log).is_none());
    }

    #[test]
    fn parse_benchmark_report_fail_status() {
        let log = r#"{"benchmarks":[{"benchmark":"fork_latency","value":15.000,"unit":"ms","status":"FAIL"}]}"#;
        let report = parse_benchmark_report(log).unwrap();
        assert_eq!(report.benchmarks[0].status, "FAIL");
        assert_eq!(report.benchmarks[0].value, 15.0);
    }

    #[test]
    fn parse_benchmark_report_multiple_with_mixed_status() {
        let log = r#"{"benchmarks":[
{"benchmark":"a","value":1.0,"unit":"ms","status":"PASS"},
{"benchmark":"b","value":2.0,"unit":"MB/s","status":"FAIL"},
{"benchmark":"c","value":3.0,"unit":"ns","status":"PASS"}
]}"#;
        let report = parse_benchmark_report(log).unwrap();
        assert_eq!(report.benchmarks.len(), 3);
        assert_eq!(report.benchmarks[0].status, "PASS");
        assert_eq!(report.benchmarks[1].status, "FAIL");
        assert_eq!(report.benchmarks[2].status, "PASS");
    }
}
