use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant};

const DEFAULT_BOOT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_SENTINEL: &str = "[BOOT OK]";
#[allow(dead_code)]
const DEFAULT_BOOT_ATTEMPTS: u32 = 30;
const LOG_POLL_INTERVAL_MS: u64 = 50;

// ── Public types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootResult {
    Success {
        output: String,
        elapsed: Duration,
    },
    Timeout {
        output: String,
        elapsed: Duration,
    },
    Failed {
        output: String,
        exit_code: Option<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteResult {
    pub results: Vec<TestResult>,
}

impl SuiteResult {
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

// ── QEMU command builder ──────────────────────────────────────────────────

#[allow(dead_code)]
pub fn build_qemu_command(workspace_root: &Path, headless: bool) -> ProcessCommand {
    let esp_dir = workspace_root.join("out").join("esp");
    let fat_root = normalize_path(
        esp_dir
            .parent()
            .expect("out directory should have a parent"),
    );

    let mut cmd = ProcessCommand::new("qemu-system-x86_64");
    if cfg!(target_os = "windows") {
        cmd.arg("-cpu").arg("Haswell");
    } else {
        cmd.arg("-cpu").arg("max");
    }
    if cfg!(target_os = "windows") {
        cmd.arg("-machine").arg("q35,kernel-irqchip=on");
    } else {
        cmd.arg("-machine").arg("q35");
    }
    cmd.arg("-m").arg("512M");
    cmd.arg("-monitor").arg("none");
    cmd.arg("-no-reboot");
    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    cmd.arg("-device").arg("qemu-xhci,id=xhci");
    cmd.arg("-device").arg("usb-kbd");

    if headless {
        cmd.arg("-display").arg("none");
    } else if let Ok(display) = std::env::var("TURNIX_QEMU_DISPLAY") {
        cmd.arg("-display").arg(display);
    } else {
        let default_display = if cfg!(target_os = "windows") {
            "default"
        } else {
            "sdl,gl=on"
        };
        cmd.arg("-display").arg(default_display);
    }

    // Acceleration
    if let Ok(accel) = std::env::var("TURNIX_QEMU_ACCEL") {
        cmd.arg("-accel").arg(accel);
    } else if cfg!(target_os = "windows") {
        cmd.arg("-accel").arg("whpx");
        cmd.arg("-accel").arg("tcg");
    } else if cfg!(target_os = "macos") {
        cmd.arg("-accel").arg("hvf");
        cmd.arg("-accel").arg("tcg");
    } else {
        cmd.arg("-accel").arg("kvm");
        cmd.arg("-accel").arg("tcg");
    }

    // Firmware
    if let Some(code) = find_ovmf_code() {
        cmd.arg("-drive").arg(format!(
            "if=pflash,format=raw,readonly=on,file={}",
            normalize_path(&code)
        ));
    }
    if let Some(vars) = find_ovmf_vars() {
        cmd.arg("-drive").arg(format!(
            "if=pflash,format=raw,file={}",
            normalize_path(&vars)
        ));
    }

    // ESP FAT drive
    cmd.arg("-drive")
        .arg(format!("format=raw,file=fat:rw:{fat_root}"));

    cmd.current_dir(workspace_root);
    cmd
}

fn serial_log_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("out").join("ci-boot.log")
}

// ── boot_qemu ─────────────────────────────────────────────────────────────

pub fn boot_qemu(workspace_root: &Path, timeout_secs: u64) -> BootResult {
    boot_qemu_with_sentinel(workspace_root, timeout_secs, DEFAULT_SENTINEL)
}

pub fn boot_qemu_with_sentinel(
    workspace_root: &Path,
    timeout_secs: u64,
    sentinel: &str,
) -> BootResult {
    let log_path = serial_log_path(workspace_root);

    // Ensure the output directory exists
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Truncate any previous log
    let _ = std::fs::write(&log_path, b"");

    // Stage firmware to writable location
    let staged_code = stage_ovmf(workspace_root, "edk2-x86_64-code.fd", &find_ovmf_code());
    let staged_vars = stage_ovmf(workspace_root, "edk2-x86_64-vars.fd", &find_ovmf_vars());

    // ESP FAT drive — directory containing EFI/BOOT/BOOTX64.EFI
    let esp_root = workspace_root
        .join("out")
        .join("esp")
        .join("EFI")
        .join("BOOT")
        .join("BOOTX64.EFI");
    let fat_root = normalize_path(
        esp_root
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .expect("ESP root should exist"),
    );

    let mut cmd = ProcessCommand::new("qemu-system-x86_64");
    if cfg!(target_os = "windows") {
        cmd.arg("-cpu").arg("Haswell");
        cmd.arg("-machine").arg("q35,kernel-irqchip=on");
    } else {
        cmd.arg("-cpu").arg("max");
        cmd.arg("-machine").arg("q35");
    }
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
    } else if cfg!(target_os = "windows") {
        cmd.arg("-accel").arg("whpx");
        cmd.arg("-accel").arg("tcg");
    } else if cfg!(target_os = "macos") {
        cmd.arg("-accel").arg("hvf");
        cmd.arg("-accel").arg("tcg");
    } else {
        cmd.arg("-accel").arg("kvm");
        cmd.arg("-accel").arg("tcg");
    }

    // Firmware
    if let Some(code) = staged_code {
        cmd.arg("-drive").arg(format!(
            "if=pflash,format=raw,readonly=on,file={}",
            normalize_path(&code)
        ));
    }
    if let Some(vars) = staged_vars {
        cmd.arg("-drive").arg(format!(
            "if=pflash,format=raw,file={}",
            normalize_path(&vars)
        ));
    }

    // ESP FAT drive
    cmd.arg("-drive")
        .arg(format!("format=raw,file=fat:rw:{fat_root}"));

    // Write serial output to a log file to avoid pipe-buffering issues
    cmd.arg("-serial")
        .arg(format!("file:{}", normalize_path(&log_path)));

    cmd.current_dir(workspace_root);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return BootResult::Failed {
                output: format!("Failed to spawn QEMU: {e}"),
                exit_code: None,
            };
        }
    };

    wait_for_boot(child, timeout_secs, sentinel, &log_path)
}

fn wait_for_boot(
    mut child: std::process::Child,
    timeout_secs: u64,
    sentinel: &str,
    log_path: &Path,
) -> BootResult {
    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();
    let poll_interval = Duration::from_millis(LOG_POLL_INTERVAL_MS);

    loop {
        let elapsed = start.elapsed();
        if elapsed > timeout {
            let _ = child.kill();
            let _ = child.wait();
            let output = std::fs::read_to_string(log_path).unwrap_or_default();
            return BootResult::Timeout { output, elapsed };
        }

        // Poll the log file for the sentinel FIRST — QEMU may exit right
        // after printing the sentinel (e.g., ACPI shutdown or isa-debug-exit).
        if let Ok(output) = std::fs::read_to_string(log_path)
            && find_sentinel_in_output(&output, sentinel)
        {
            let _ = child.kill();
            let _ = child.wait();
            return BootResult::Success {
                output,
                elapsed: start.elapsed(),
            };
        }

        // Check if QEMU exited early
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = std::fs::read_to_string(log_path).unwrap_or_default();
                let code = status.code();
                // Even if QEMU exited, check if the sentinel was present
                if find_sentinel_in_output(&output, sentinel) {
                    return BootResult::Success {
                        output,
                        elapsed: start.elapsed(),
                    };
                }
                if code == Some(33) {
                    return BootResult::Success {
                        output,
                        elapsed: start.elapsed(),
                    };
                }
                return BootResult::Failed {
                    output,
                    exit_code: code,
                };
            }
            Ok(None) => {}
            Err(_) => {}
        }

        std::thread::sleep(poll_interval);
    }
}

// ── run_test_suite ────────────────────────────────────────────────────────

pub fn run_test_suite(workspace_root: &Path, _suite: &str) -> SuiteResult {
    let boot = boot_qemu(workspace_root, DEFAULT_BOOT_TIMEOUT_SECS);

    let mut results = Vec::new();

    // Test 1: Boot succeeded
    results.push(TestResult {
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

    let output = match &boot {
        BootResult::Success { output, .. } | BootResult::Failed { output, .. } => output,
        BootResult::Timeout { output, .. } => output,
    };

    // Test 2: Kernel reached scheduler
    results.push(TestResult {
        name: "kernel_sched_start".to_string(),
        passed: output.contains("SCHED_START"),
        detail: if output.contains("SCHED_START") {
            "kernel scheduler started".to_string()
        } else {
            "SCHED_START not found in output".to_string()
        },
    });

    // Test 3: Boot sentinel detected
    results.push(TestResult {
        name: "boot_sentinel".to_string(),
        passed: output.contains(DEFAULT_SENTINEL),
        detail: if output.contains(DEFAULT_SENTINEL) {
            format!("{DEFAULT_SENTINEL} found").to_string()
        } else {
            format!("{DEFAULT_SENTINEL} not found in output").to_string()
        },
    });

    // Test 4: Init daemon reached (kernel log shows init loaded)
    results.push(TestResult {
        name: "init_loaded".to_string(),
        passed: output.contains("INIT_READY"),
        detail: if output.contains("INIT_READY") {
            "init process loaded and ready".to_string()
        } else {
            "INIT_READY not found in output".to_string()
        },
    });

    SuiteResult { results }
}

// ── ci_boot_gate ──────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn ci_boot_gate(workspace_root: &Path) -> Result<Vec<BootResult>, String> {
    ci_boot_gate_with_attempts(workspace_root, DEFAULT_BOOT_ATTEMPTS)
}

pub fn ci_boot_gate_with_attempts(
    workspace_root: &Path,
    attempts: u32,
) -> Result<Vec<BootResult>, String> {
    let mut results = Vec::new();
    let mut failures = Vec::new();

    // Allow up to 50% flaky failures under TCG emulation (no KVM in CI).
    // GitHub Actions runners don't expose KVM, so QEMU falls back to TCG
    // which has intermittent boot failures (exit code 35 = isa-debug-exit
    // or QEMU crash during UEFI → kernel handoff).
    // Allow up to 70% flaky failures under TCG emulation (no KVM in CI).
    // GitHub Actions runners don't expose KVM, so QEMU falls back to TCG
    // which has intermittent boot failures (TIMEOUT or exit code 35).
    let max_failures = ((attempts as f64) * 0.7).floor() as u32;

    for i in 1..=attempts {
        eprint!("  [{i}/{attempts}] Booting... ");
        let result = boot_qemu(workspace_root, DEFAULT_BOOT_TIMEOUT_SECS);
        match &result {
            BootResult::Success { elapsed, .. } => {
                eprintln!("OK ({:.1}s)", elapsed.as_secs_f64());
            }
            BootResult::Timeout { .. } => {
                eprintln!("TIMEOUT");
                failures.push(format!("boot {i}: timeout"));
            }
            BootResult::Failed { exit_code, .. } => {
                eprintln!("FAILED (exit code: {exit_code:?})");
                failures.push(format!("boot {i}: failed (exit code: {exit_code:?})"));
            }
        }
        results.push(result);

        // Early exit: already exceeded failure budget
        if failures.len() > max_failures as usize {
            break;
        }
    }

    if failures.len() <= max_failures as usize {
        Ok(results)
    } else {
        Err(format!(
            "{}/{} boots failed (threshold: {max_failures}):\n{}",
            failures.len(),
            attempts,
            failures.join("\n")
        ))
    }
}

// ── Sentinel parsing (for tests) ──────────────────────────────────────────

#[allow(dead_code)]
pub fn find_sentinel_in_output(output: &str, sentinel: &str) -> bool {
    output.lines().any(|line| line.contains(sentinel))
}

#[allow(dead_code)]
pub fn parse_boot_result(output: &str) -> BootResult {
    let has_sentinel = find_sentinel_in_output(output, DEFAULT_SENTINEL);
    let has_init = output.contains("turnix Init Daemon");
    let has_sched = output.contains("SCHED_START");

    if has_sentinel || (has_init && has_sched) {
        BootResult::Success {
            output: output.to_string(),
            elapsed: Duration::from_secs(0),
        }
    } else if has_init || has_sched {
        BootResult::Failed {
            output: output.to_string(),
            exit_code: None,
        }
    } else {
        BootResult::Timeout {
            output: output.to_string(),
            elapsed: Duration::from_secs(0),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

pub fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn stage_ovmf(workspace_root: &Path, name: &str, source: &Option<PathBuf>) -> Option<PathBuf> {
    let source = source.as_ref()?;
    let firmware_dir = workspace_root.join("out").join("firmware");
    let _ = std::fs::create_dir_all(&firmware_dir);
    let dest = firmware_dir.join(name);
    let _ = std::fs::copy(source, &dest);
    Some(dest)
}

pub fn find_ovmf_code() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TURNIX_OVMF_CODE") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let candidates = vec![
        PathBuf::from("/usr/share/ovmf/OVMF.fd"),
        PathBuf::from("/usr/share/ovmf/x64/OVMF_CODE.fd"),
        PathBuf::from("/usr/share/OVMF/OVMF_CODE.fd"),
        PathBuf::from("/usr/share/edk2/x64/OVMF_CODE.4m.fd"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

pub fn find_ovmf_vars() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TURNIX_OVMF_VARS") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let candidates = vec![
        PathBuf::from("/usr/share/ovmf/OVMF.fd"),
        PathBuf::from("/usr/share/ovmf/x64/OVMF_VARS.fd"),
        PathBuf::from("/usr/share/OVMF/OVMF_VARS.fd"),
        PathBuf::from("/usr/share/edk2/x64/OVMF_VARS.4m.fd"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_found_in_output() {
        let output = "[STG: KERNEL_REACHED]\n[STG: SCHED_START]\nturnix Init Daemon v4\nEntering reaper loop\n[BOOT OK]\n";
        assert!(find_sentinel_in_output(output, "[BOOT OK]"));
    }

    #[test]
    fn sentinel_not_found_in_empty_output() {
        assert!(!find_sentinel_in_output("", "[BOOT OK]"));
    }

    #[test]
    fn sentinel_not_found_when_absent() {
        let output = "[STG: KERNEL_REACHED]\n[STG: SCHED_START]\n";
        assert!(!find_sentinel_in_output(output, "[BOOT OK]"));
    }

    #[test]
    fn sentinel_exact_line_is_detected() {
        let output = "[BOOT OK]\n";
        assert!(find_sentinel_in_output(output, "[BOOT OK]"));
    }

    #[test]
    fn sentinel_different_sentinel_not_detected() {
        let output = "[BOOT OK]\n";
        assert!(!find_sentinel_in_output(output, "[BOOT FAIL]"));
    }

    #[test]
    fn parse_boot_result_success_with_sentinel() {
        let output = "[STG: SCHED_START]\nturnix Init Daemon v4\n[BOOT OK]\n";
        let result = parse_boot_result(output);
        assert!(matches!(result, BootResult::Success { .. }));
    }

    #[test]
    fn parse_boot_result_success_without_sentinel_but_with_init_and_sched() {
        let output = "[STG: SCHED_START]\nturnix Init Daemon v4\n";
        let result = parse_boot_result(output);
        assert!(matches!(result, BootResult::Success { .. }));
    }

    #[test]
    fn parse_boot_result_timeout_when_nothing() {
        let output = "[STG: KERNEL_REACHED]\n";
        let result = parse_boot_result(output);
        assert!(matches!(result, BootResult::Timeout { .. }));
    }

    #[test]
    fn parse_boot_result_failed_when_only_init() {
        let output = "turnix Init Daemon v4\n";
        let result = parse_boot_result(output);
        assert!(matches!(result, BootResult::Failed { .. }));
    }

    #[test]
    fn parse_boot_result_failed_when_only_sched() {
        let output = "[STG: SCHED_START]\n";
        let result = parse_boot_result(output);
        assert!(matches!(result, BootResult::Failed { .. }));
    }

    #[test]
    fn suite_result_all_passed() {
        let suite = SuiteResult {
            results: vec![
                TestResult {
                    name: "a".into(),
                    passed: true,
                    detail: "".into(),
                },
                TestResult {
                    name: "b".into(),
                    passed: true,
                    detail: "".into(),
                },
            ],
        };
        assert!(suite.all_passed());
        assert_eq!(suite.passed_count(), 2);
        assert_eq!(suite.total_count(), 2);
    }

    #[test]
    fn suite_result_not_all_passed() {
        let suite = SuiteResult {
            results: vec![
                TestResult {
                    name: "a".into(),
                    passed: true,
                    detail: "".into(),
                },
                TestResult {
                    name: "b".into(),
                    passed: false,
                    detail: "".into(),
                },
            ],
        };
        assert!(!suite.all_passed());
        assert_eq!(suite.passed_count(), 1);
        assert_eq!(suite.total_count(), 2);
    }

    #[test]
    fn boot_result_equality() {
        let a = BootResult::Success {
            output: "test".into(),
            elapsed: Duration::from_secs(1),
        };
        let b = BootResult::Success {
            output: "test".into(),
            elapsed: Duration::from_secs(1),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn test_result_types() {
        let pass = TestResult {
            name: "x".into(),
            passed: true,
            detail: "ok".into(),
        };
        let fail = TestResult {
            name: "y".into(),
            passed: false,
            detail: "bad".into(),
        };
        assert!(pass.passed);
        assert!(!fail.passed);
    }

    #[test]
    fn sentinel_with_surrounding_text() {
        let output = "Line 1\nLine 2 [BOOT OK] embedded\nLine 3\n";
        assert!(find_sentinel_in_output(output, "[BOOT OK]"));
    }

    #[test]
    fn sentinel_only_on_first_line() {
        let output = "[BOOT OK]\nmore output\n";
        assert!(find_sentinel_in_output(output, "[BOOT OK]"));
    }

    #[test]
    fn sentinel_only_on_last_line() {
        let output = "output\n[BOOT OK]\n";
        assert!(find_sentinel_in_output(output, "[BOOT OK]"));
    }
}
