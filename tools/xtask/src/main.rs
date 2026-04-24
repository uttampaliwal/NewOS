use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus};

enum Command {
    Status,
    Doctor,
    BuildUefi,
    RunUefi,
}

fn main() {
    let workspace_root = workspace_root();

    match parse_command(env::args().nth(1).as_deref()) {
        Command::Status => print_status(&workspace_root),
        Command::Doctor => print_doctor(),
        Command::BuildUefi => {
            let image = build_uefi(&workspace_root);
            println!("UEFI image staged at {}", image.display());
        }
        Command::RunUefi => run_uefi(&workspace_root),
    }
}

fn print_status(workspace_root: &Path) {
    println!("NewOS workspace is ready at {}.", workspace_root.display());
    println!("Current milestone: freestanding kernel handoff.");
    println!("Useful commands: cargo xtask doctor, cargo xtask build-uefi, cargo xtask run-uefi");
}

fn print_doctor() {
    print_check(
        "qemu-system-x86_64 on PATH",
        command_works("qemu-system-x86_64", ["--version"]),
    );
    print_check(
        "nightly toolchain available",
        output_contains(
            "rustup",
            ["toolchain", "list"],
            "nightly-x86_64-pc-windows-msvc",
        ),
    );
    print_check(
        "x86_64-unknown-uefi target installed",
        output_contains(
            "rustup",
            [
                "target",
                "list",
                "--installed",
                "--toolchain",
                "nightly-x86_64-pc-windows-msvc",
            ],
            "x86_64-unknown-uefi",
        ),
    );
    print_check(
        "x86_64-unknown-none target installed",
        output_contains(
            "rustup",
            [
                "target",
                "list",
                "--installed",
                "--toolchain",
                "nightly-x86_64-pc-windows-msvc",
            ],
            "x86_64-unknown-none",
        ),
    );
    match find_ovmf_code() {
        Some(path) => println!("[ok] EDK2 firmware found at {}", path.display()),
        None => println!("[missing] EDK2 firmware image not found. Set NEWOS_OVMF_CODE if needed."),
    }
    match find_ovmf_vars() {
        Some(path) => println!("[ok] EDK2 vars image found at {}", path.display()),
        None => println!("[missing] EDK2 vars image not found. Set NEWOS_OVMF_VARS if needed."),
    }
}

fn parse_command(raw: Option<&str>) -> Command {
    match raw {
        Some("doctor") => Command::Doctor,
        Some("build-uefi") => Command::BuildUefi,
        Some("run-uefi") => Command::RunUefi,
        Some("status") | None => Command::Status,
        Some(other) => {
            eprintln!("Unknown xtask command: {other}");
            eprintln!("Available commands: status, doctor, build-uefi, run-uefi");
            std::process::exit(2);
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root should exist")
        .to_path_buf()
}

fn build_uefi(workspace_root: &Path) -> PathBuf {
    let staged_kernel = build_kernel_image(workspace_root);
    run_or_die(
        "cargo",
        [
            "+nightly",
            "build",
            "-p",
            "newos-uefi-loader",
            "--target",
            "x86_64-unknown-uefi",
        ],
        workspace_root,
    );

    let built_image = workspace_root
        .join("target")
        .join("x86_64-unknown-uefi")
        .join("debug")
        .join("newos-uefi-loader.efi");

    if !built_image.exists() {
        eprintln!(
            "Expected EFI image was not produced: {}",
            built_image.display()
        );
        std::process::exit(1);
    }

    let esp_boot_dir = workspace_root
        .join("out")
        .join("esp")
        .join("EFI")
        .join("BOOT");
    fs::create_dir_all(&esp_boot_dir).expect("creating EFI boot directory should succeed");

    let staged_image = esp_boot_dir.join("BOOTX64.EFI");
    fs::copy(&built_image, &staged_image).expect("copying EFI image should succeed");
    println!("Kernel image staged at {}", staged_kernel.display());
    staged_image
}

fn build_kernel_image(workspace_root: &Path) -> PathBuf {
    run_or_die(
        "cargo",
        [
            "+nightly",
            "build",
            "-p",
            "newos-kernel",
            "--bin",
            "newos-kernel-image",
            "--target",
            "x86_64-unknown-none",
        ],
        workspace_root,
    );

    let built_image = workspace_root
        .join("target")
        .join("x86_64-unknown-none")
        .join("debug")
        .join("newos-kernel-image");

    if !built_image.exists() {
        eprintln!(
            "Expected freestanding kernel image was not produced: {}",
            built_image.display()
        );
        std::process::exit(1);
    }

    let staged_dir = workspace_root.join("out").join("esp").join("newos");
    fs::create_dir_all(&staged_dir).expect("creating kernel staging directory should succeed");

    let staged_image = staged_dir.join("kernel.elf");
    fs::copy(&built_image, &staged_image).expect("copying kernel image should succeed");
    staged_image
}

fn run_uefi(workspace_root: &Path) {
    let staged_image = build_uefi(workspace_root);
    let ovmf_code = find_ovmf_code().unwrap_or_else(|| {
        eprintln!("Could not find an EDK2 firmware image. Set NEWOS_OVMF_CODE to point to it.");
        std::process::exit(1);
    });
    let ovmf_vars = find_ovmf_vars().unwrap_or_else(|| {
        eprintln!("Could not find an EDK2 vars image. Set NEWOS_OVMF_VARS to point to it.");
        std::process::exit(1);
    });
    let staged_ovmf_code = stage_ovmf_code(workspace_root, &ovmf_code);
    let staged_ovmf_vars = stage_ovmf_vars(workspace_root, &ovmf_vars);

    let fat_root = normalize_for_qemu_path(
        staged_image
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .expect("ESP root should exist"),
    );
    let acceleration = env::var("NEWOS_QEMU_ACCEL").unwrap_or_else(|_| "whpx".to_string());

    let status = ProcessCommand::new("qemu-system-x86_64")
        .arg("-machine")
        .arg("q35")
        .arg("-accel")
        .arg(acceleration)
        .arg("-m")
        .arg("512M")
        .arg("-serial")
        .arg("stdio")
        .arg("-monitor")
        .arg("none")
        .arg("-display")
        .arg("none")
        .arg("-no-reboot")
        .arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04")
        .arg("-drive")
        .arg(format!(
            "if=pflash,format=raw,readonly=on,file={}",
            normalize_for_qemu_path(&staged_ovmf_code)
        ))
        .arg("-drive")
        .arg(format!(
            "if=pflash,format=raw,file={}",
            normalize_for_qemu_path(&staged_ovmf_vars)
        ))
        .arg("-drive")
        .arg(format!("format=raw,file=fat:rw:{fat_root}"))
        .current_dir(workspace_root)
        .status()
        .expect("running QEMU should succeed");

    handle_qemu_status(status);
}

fn handle_qemu_status(status: ExitStatus) {
    match status.code() {
        Some(33) => println!("QEMU exited after the NewOS success path."),
        Some(code) => {
            eprintln!("QEMU exited with code {code}.");
            std::process::exit(code);
        }
        None => {
            eprintln!("QEMU terminated without an exit code.");
            std::process::exit(1);
        }
    }
}

fn find_ovmf_code() -> Option<PathBuf> {
    if let Ok(path) = env::var("NEWOS_OVMF_CODE") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let mut candidates = Vec::new();
    if let Some(qemu_path) = find_command_path("qemu-system-x86_64") {
        if let Some(base) = qemu_path.parent().and_then(Path::parent) {
            candidates.push(base.join("share").join("qemu").join("edk2-x86_64-code.fd"));
        }
    }

    candidates.push(PathBuf::from(
        r"C:\msys64\ucrt64\share\qemu\edk2-x86_64-code.fd",
    ));
    candidates.push(PathBuf::from(
        r"C:\Program Files\qemu\share\qemu\edk2-x86_64-code.fd",
    ));

    candidates.into_iter().find(|path| path.exists())
}

fn find_ovmf_vars() -> Option<PathBuf> {
    if let Ok(path) = env::var("NEWOS_OVMF_VARS") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let mut candidates = Vec::new();
    if let Some(qemu_path) = find_command_path("qemu-system-x86_64") {
        if let Some(base) = qemu_path.parent().and_then(Path::parent) {
            let share = base.join("share").join("qemu");
            candidates.push(share.join("edk2-x86_64-vars.fd"));
            candidates.push(share.join("edk2-i386-vars.fd"));
        }
    }

    candidates.push(PathBuf::from(
        r"C:\msys64\ucrt64\share\qemu\edk2-x86_64-vars.fd",
    ));
    candidates.push(PathBuf::from(
        r"C:\msys64\ucrt64\share\qemu\edk2-i386-vars.fd",
    ));
    candidates.push(PathBuf::from(
        r"C:\Program Files\qemu\share\qemu\edk2-x86_64-vars.fd",
    ));
    candidates.push(PathBuf::from(
        r"C:\Program Files\qemu\share\qemu\edk2-i386-vars.fd",
    ));

    candidates.into_iter().find(|path| path.exists())
}

fn find_command_path(command: &str) -> Option<PathBuf> {
    let output = ProcessCommand::new("where").arg(command).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let first_line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();

    Some(PathBuf::from(first_line))
}

fn normalize_for_qemu_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn stage_ovmf_code(workspace_root: &Path, source: &Path) -> PathBuf {
    let firmware_dir = workspace_root.join("out").join("firmware");
    fs::create_dir_all(&firmware_dir).expect("creating firmware staging directory should succeed");

    let destination = firmware_dir.join("edk2-x86_64-code.fd");
    fs::copy(source, &destination).expect("copying the EDK2 firmware image should succeed");
    destination
}

fn stage_ovmf_vars(workspace_root: &Path, source: &Path) -> PathBuf {
    let firmware_dir = workspace_root.join("out").join("firmware");
    fs::create_dir_all(&firmware_dir).expect("creating firmware staging directory should succeed");

    let destination = firmware_dir.join("edk2-x86_64-vars.fd");
    fs::copy(source, &destination).expect("copying the EDK2 vars image should succeed");
    destination
}

fn command_works<I, S>(program: &str, args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    ProcessCommand::new(program)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn output_contains<I, S>(program: &str, args: I, expected: &str) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    ProcessCommand::new(program)
        .args(args)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(expected))
        .unwrap_or(false)
}

fn print_check(label: &str, ok: bool) {
    let state = if ok { "ok" } else { "missing" };
    println!("[{state}] {label}");
}

fn run_or_die<I, S>(program: &str, args: I, workspace_root: &Path)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = ProcessCommand::new(program)
        .args(args)
        .current_dir(workspace_root)
        .status()
        .expect("subprocess should start");

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}
