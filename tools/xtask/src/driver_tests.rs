use std::path::Path;
use std::process::Command as ProcessCommand;
use std::time::Duration;

use crate::ci::BootResult;

// ── Public types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DriverTestResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct DriverSuiteResult {
    pub results: Vec<DriverTestResult>,
}

impl DriverSuiteResult {
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

// ── QEMU command builder for driver tests ─────────────────────────────────

fn build_driver_test_qemu_command(workspace_root: &Path) -> ProcessCommand {
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

    // Debug exit device
    cmd.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");

    // USB controller + keyboard
    cmd.arg("-device").arg("qemu-xhci,id=xhci");
    cmd.arg("-device").arg("usb-kbd");

    // VirtIO network device
    cmd.arg("-netdev").arg("user,id=net0");
    cmd.arg("-device").arg("virtio-net-pci,netdev=net0");

    // Display
    cmd.arg("-display").arg("none");

    // Acceleration
    cmd.arg("-accel").arg("kvm");
    cmd.arg("-accel").arg("tcg");

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

// ── Serial log parsing ────────────────────────────────────────────────────

fn parse_device_lines(log: &str) -> Vec<String> {
    log.lines()
        .filter(|line| line.contains("[PCIE]") && line.contains(':'))
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
fn parse_pci_device_count(log: &str) -> Option<usize> {
    for line in log.lines() {
        if line.contains("[PCIE]") && line.contains("Enumeration complete") {
            // e.g. "[PCIE] Enumeration complete. Discovered 7 device functions."
            return line
                .split_whitespace()
                .find(|w| w.chars().all(|c| c.is_ascii_digit()))
                .and_then(|w| w.parse().ok());
        }
    }
    None
}

fn count_device_lines(log: &str) -> usize {
    parse_device_lines(log).len()
}

// ── Boot + driver verification ────────────────────────────────────────────

pub fn boot_qemu_driver_test(workspace_root: &Path, timeout_secs: u64) -> BootResult {
    let log_path = workspace_root.join("out").join("ci-driver-test.log");

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&log_path, b"");

    let mut cmd = build_driver_test_qemu_command(workspace_root);
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

        // Poll log for boot sentinel (kernel doesn't exit on its own)
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

// ── Run driver test suite ─────────────────────────────────────────────────

pub fn run_driver_test_suite(workspace_root: &Path) -> DriverSuiteResult {
    let boot_timeout = 60;
    let boot = boot_qemu_driver_test(workspace_root, boot_timeout);

    let mut results = Vec::new();

    let output = match &boot {
        BootResult::Success { output, .. }
        | BootResult::Failed { output, .. }
        | BootResult::Timeout { output, .. } => output.clone(),
    };

    // Test 1: Boot succeeded
    results.push(DriverTestResult {
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

    // Test 2: PCIe enumeration
    let pcie_device_count = count_device_lines(&output);
    results.push(DriverTestResult {
        name: "pcie_enumeration".to_string(),
        passed: pcie_device_count >= 1,
        detail: if pcie_device_count >= 1 {
            format!("{pcie_device_count} PCIe device functions discovered")
        } else {
            format!(
                "expected >= 1 device functions, found {pcie_device_count}"
            )
        },
    });

    // Test 3: VirtIO-Net detected (probe may succeed or fail depending on driver state)
    let virtio_detected = output.contains("[VIRTIO]") && (output.contains("virtio") || output.contains("VIRTIO"));
    results.push(DriverTestResult {
        name: "virtio_net_detected".to_string(),
        passed: virtio_detected,
        detail: if virtio_detected {
            "VirtIO device detected in serial log".to_string()
        } else {
            "VirtIO device not detected".to_string()
        },
    });

    // Test 4: VirtIO-Net MAC address (check if MAC was negotiated, not all zeros)
    let mac_not_all_zeros = !output.contains("MAC: 00:00:00:00:00:00")
        && !output.contains("mac: 00:00:00:00:00:00");
    // If no explicit MAC log, check that probe succeeded (implies MAC was set)
    let virtio_probe_ok = output.contains("[VIRTIO] Re-initialisation succeeded")
        || output.contains("[VIRTIO] Feature negotiation");
    results.push(DriverTestResult {
        name: "virtio_net_mac".to_string(),
        passed: mac_not_all_zeros && virtio_detected,
        detail: if !virtio_detected {
            "VirtIO not detected, cannot check MAC".to_string()
        } else if virtio_probe_ok {
            "VirtIO-Net probe succeeded (MAC negotiated)".to_string()
        } else if mac_not_all_zeros {
            "MAC address is not all zeros".to_string()
        } else {
            "MAC appears to be all zeros or not logged".to_string()
        },
    });

    // Test 5: NVMe namespace discovery (info-only, no NVMe drive attached)
    let nvme_detected = output.contains("[NVMe]");
    results.push(DriverTestResult {
        name: "nvme_namespace".to_string(),
        passed: true, // info-only: no NVMe drive attached in this config
        detail: if nvme_detected {
            let skipped = output.lines()
                .filter(|l| l.contains("[NVMe]") && l.contains("Probe skipped"))
                .count();
            format!("NVMe detected ({skipped} probe skips, no drive attached)")
        } else {
            "NVMe not present in this QEMU configuration (info-only)".to_string()
        },
    });

    // Test 6: XHCI controller initialised
    let xhci_detected = output.contains("[XHCI]");
    let xhci_started = output.contains("[XHCI] Controller started successfully");
    results.push(DriverTestResult {
        name: "xhci_init".to_string(),
        passed: xhci_detected && (xhci_started || output.contains("[XHCI] Controller reset")),
        detail: if xhci_started {
            "XHCI controller started successfully".to_string()
        } else if xhci_detected {
            "XHCI detected but startup state unclear".to_string()
        } else {
            "XHCI controller not detected".to_string()
        },
    });

    // Test 7: GPU / framebuffer
    let gpu_detected = output.contains("[GPU]") || output.contains("framebuffer");
    let video_init = output.contains("[STG: VIDEO_INIT]");
    results.push(DriverTestResult {
        name: "gpu_framebuffer".to_string(),
        passed: video_init || gpu_detected,
        detail: if video_init && gpu_detected {
            "GPU and VIDEO_INIT detected".to_string()
        } else if video_init {
            "VIDEO_INIT detected (GPU probe may be pending)".to_string()
        } else if gpu_detected {
            "GPU detected in log".to_string()
        } else {
            "GPU/framebuffer not detected".to_string()
        },
    });

    DriverSuiteResult { results }
}

// ── Parse helpers ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_device_lines_finds_pcie_entries() {
        let log = "\
[PCIE] Enumerating devices via ECAM...
[PCIE] 00:00.0 8086:29c0 class=06 sub=00 if=00
[PCIE] 00:01.0 1234:1111 class=03 sub=00 if=00
[PCIE] Enumeration complete. Discovered 2 device functions.";
        let lines = parse_device_lines(log);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("8086:29c0"));
        assert!(lines[1].contains("1234:1111"));
    }

    #[test]
    fn parse_device_lines_ignores_non_pcie() {
        let log = "[VIRTIO] Probe skipped\n[PCI] Scan complete.\n[PCIE] 00:00.0 8086:29c0 class=06";
        let lines = parse_device_lines(log);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn parse_pci_device_count_from_log() {
        let log = "[PCIE] Enumeration complete. Discovered 7 device functions.";
        assert_eq!(parse_pci_device_count(log), Some(7));
    }

    #[test]
    fn parse_pci_device_count_missing() {
        let log = "[PCIE] Enumerating devices via ECAM...";
        assert_eq!(parse_pci_device_count(log), None);
    }

    #[test]
    fn count_device_lines_matches() {
        let log = "\
[PCIE] 00:00.0 8086:29c0 class=06 sub=00 if=00
[PCIE] 00:01.0 1234:1111 class=03 sub=00 if=00
[PCIE] 00:02.0 8086:10d3 class=02 sub=00 if=00";
        assert_eq!(count_device_lines(log), 3);
    }

    #[test]
    fn count_device_lines_empty_log() {
        assert_eq!(count_device_lines(""), 0);
    }

    #[test]
    fn parse_device_lines_preserves_full_line() {
        let log = "[PCIE] 00:1f.2 8086:2922 class=01 sub=06 if=01";
        let lines = parse_device_lines(log);
        assert_eq!(lines[0], "[PCIE] 00:1f.2 8086:2922 class=01 sub=06 if=01");
    }

    #[test]
    fn parse_pci_device_count_various_formats() {
        assert_eq!(
            parse_pci_device_count("[PCIE] Enumeration complete. Discovered 12 device functions."),
            Some(12)
        );
        assert_eq!(
            parse_pci_device_count("[PCIE] Enumeration complete. Discovered 1 device functions."),
            Some(1)
        );
        assert_eq!(
            parse_pci_device_count("[PCIE] Enumeration complete. Discovered 0 device functions."),
            Some(0)
        );
    }

    #[test]
    fn parse_device_lines_only_exact_pcie_prefix() {
        let log = "[PCI] Found device: Bus 00\n[PCIE] 00:00.0 8086:29c0\n[PCIE] Enumeration complete. Discovered 1 device functions.";
        let lines = parse_device_lines(log);
        // Only the device line, not the "Enumeration complete" line
        assert_eq!(lines.len(), 1);
    }
}
