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
    println!("Turnix workspace is ready at {}.", workspace_root.display());
    println!("Current milestone: stabilized higher-half kernel bring-up.");
    println!("Useful commands: cargo xtask doctor, cargo xtask build-uefi, cargo xtask run-uefi");
    println!("Compatibility alias: cargo xtask uefi-loader");
}

fn print_doctor() {
    print_check(
        "qemu-system-x86_64 on PATH",
        command_works("qemu-system-x86_64", ["--version"]),
    );
    print_check(
        "nightly toolchain available",
        output_contains("rustup", ["toolchain", "list"], "nightly"),
    );
    print_check(
        "x86_64-unknown-uefi target installed",
        output_contains(
            "rustup",
            ["target", "list", "--installed", "--toolchain", "nightly"],
            "x86_64-unknown-uefi",
        ),
    );
    print_check(
        "x86_64-unknown-none target installed",
        output_contains(
            "rustup",
            ["target", "list", "--installed", "--toolchain", "nightly"],
            "x86_64-unknown-none",
        ),
    );
    match find_ovmf_code() {
        Some(path) => println!("[ok] EDK2 firmware found at {}", path.display()),
        None => println!("[missing] EDK2 firmware image not found. Set TURNIX_OVMF_CODE if needed."),
    }
    match find_ovmf_vars() {
        Some(path) => println!("[ok] EDK2 vars image found at {}", path.display()),
        None => println!("[missing] EDK2 vars image not found. Set TURNIX_OVMF_VARS if needed."),
    }
}

fn parse_command(raw: Option<&str>) -> Command {
    match raw {
        Some("doctor") => Command::Doctor,
        Some("build-uefi") => Command::BuildUefi,
        Some("run-uefi") => Command::RunUefi,
        Some("uefi-loader") => {
            println!(
                "`cargo xtask uefi-loader` is kept as a compatibility alias for `cargo xtask run-uefi`."
            );
            Command::RunUefi
        }
        Some("status") | None => Command::Status,
        Some(other) => {
            eprintln!("Unknown xtask command: {other}");
            eprintln!("Available commands: status, doctor, build-uefi, run-uefi, uefi-loader");
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
            "turnix-uefi-loader",
            "--target",
            "x86_64-unknown-uefi",
        ],
        workspace_root,
    );

    let built_image = workspace_root
        .join("target")
        .join("x86_64-unknown-uefi")
        .join("debug")
        .join("turnix-uefi-loader.efi");

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

fn build_userland(workspace_root: &Path) -> PathBuf {
    run_or_die_with_env(
        "cargo",
        &[
            "+nightly",
            "build",
            "-p",
            "init",
            "--target",
            "x86_64-unknown-none",
            "--release",
        ],
        workspace_root,
        &[("RUSTFLAGS", "-C link-arg=-Tuserland/init/linker.ld")],
    );

    let built_bin = workspace_root
        .join("target")
        .join("x86_64-unknown-none")
        .join("release")
        .join("init");

    if !built_bin.exists() {
        eprintln!(
            "Expected userland init binary was not produced: {}",
            built_bin.display()
        );
        std::process::exit(1);
    }
    built_bin
}

fn build_shell(workspace_root: &Path) -> PathBuf {
    run_or_die_with_env(
        "cargo",
        &[
            "+nightly",
            "build",
            "-p",
            "shell",
            "--target",
            "x86_64-unknown-none",
            "--release",
        ],
        workspace_root,
        &[("RUSTFLAGS", "-C link-arg=-Tuserland/init/linker.ld")],
    );

    let built_bin = workspace_root
        .join("target")
        .join("x86_64-unknown-none")
        .join("release")
        .join("shell");

    if !built_bin.exists() {
        eprintln!(
            "Expected userland shell binary was not produced: {}",
            built_bin.display()
        );
        std::process::exit(1);
    }
    built_bin
}

fn build_fault_tester(workspace_root: &Path) -> PathBuf {
    run_or_die_with_env(
        "cargo",
        &[
            "+nightly",
            "build",
            "-p",
            "fault-tester",
            "--target",
            "x86_64-unknown-none",
            "--release",
        ],
        workspace_root,
        &[("RUSTFLAGS", "-C link-arg=-Tuserland/init/linker.ld")],
    );

    let built_bin = workspace_root
        .join("target")
        .join("x86_64-unknown-none")
        .join("release")
        .join("fault-tester");

    if !built_bin.exists() {
        eprintln!(
            "Expected userland fault-tester binary was not produced: {}",
            built_bin.display()
        );
        std::process::exit(1);
    }
    built_bin
}

fn build_kernel_image(workspace_root: &Path) -> PathBuf {
    run_or_die_with_env(
        "cargo",
        &[
            "+nightly",
            "build",
            "-p",
            "turnix-kernel",
            "--bin",
            "turnix-kernel-image",
            "--target",
            "x86_64-unknown-none",
        ],
        workspace_root,
        &[(
            "RUSTFLAGS",
            "-C link-arg=-Tkernel/linker.ld -C link-arg=-z -C link-arg=max-page-size=0x1000 -C relocation-model=static",
        )],
    );

    let built_image = workspace_root
        .join("target")
        .join("x86_64-unknown-none")
        .join("debug")
        .join("turnix-kernel-image");

    if !built_image.exists() {
        eprintln!(
            "Expected freestanding kernel image was not produced: {}",
            built_image.display()
        );
        std::process::exit(1);
    }

    let staged_dir = workspace_root.join("out").join("esp").join("turnix");
    fs::create_dir_all(&staged_dir).expect("creating kernel staging directory should succeed");

    let staged_image = staged_dir.join("kernel.elf");
    fs::copy(&built_image, &staged_image).expect("copying kernel image should succeed");

    // Package initramfs
    let init_bin = build_userland(workspace_root);
    let init_data = fs::read(&init_bin).expect("failed to read init binary");

    let shell_bin = build_shell(workspace_root);
    let shell_data = fs::read(&shell_bin).expect("failed to read shell binary");

    let fault_tester_bin = build_fault_tester(workspace_root);
    let fault_tester_data =
        fs::read(&fault_tester_bin).expect("failed to read fault-tester binary");

    let mut ramdisk = Vec::new();

    // Helper to add a "file" to our simple ramdisk
    let mut add_file = |name: &str, data: &[u8]| {
        let mut header = [0u8; 64];
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len().min(63);
        header[..name_len].copy_from_slice(&name_bytes[..name_len]);
        ramdisk.extend_from_slice(&header);
        ramdisk.extend_from_slice(&(data.len() as u64).to_le_bytes());
        ramdisk.extend_from_slice(data);
    };

    add_file(
        "initramfs.txt",
        b"Hello from Initramfs!\nThis is a kernel experiment.\n",
    );
    add_file("init", &init_data);
    add_file("shell", &shell_data);
    add_file("fault-tester", &fault_tester_data);

    // Create and add a minimal PSF2 font for the terminal
    let font_data = create_minimal_psf2_font();
    add_file("font.psf", &font_data);

    let initramfs_path = staged_dir.join("initramfs.img");
    fs::write(&initramfs_path, &ramdisk).expect("creating initramfs should succeed");
    println!(
        "Initramfs created at {} ({} bytes, including 'init')",
        initramfs_path.display(),
        ramdisk.len()
    );

    staged_image
}

fn create_minimal_psf2_font() -> Vec<u8> {
    let mut data = Vec::new();

    // PSF2 Header
    data.extend_from_slice(&[0x72, 0xb5, 0x4a, 0x86]); // Magic
    data.extend_from_slice(&0u32.to_le_bytes()); // Version
    data.extend_from_slice(&32u32.to_le_bytes()); // Header size
    data.extend_from_slice(&0u32.to_le_bytes()); // Flags
    data.extend_from_slice(&256u32.to_le_bytes()); // Length (number of glyphs)
    data.extend_from_slice(&16u32.to_le_bytes()); // Char size (bytes per glyph)
    data.extend_from_slice(&16u32.to_le_bytes()); // Height
    data.extend_from_slice(&8u32.to_le_bytes()); // Width

    // Glyph data (256 glyphs * 16 bytes each)
    for i in 0..256 {
        if i == 32 {
            // Space (empty)
            data.extend_from_slice(&[0u8; 16]);
        } else {
            // A simple box border for every other character
            data.push(0xFF); // Top bar
            for _ in 0..14 {
                data.push(0x81); // Side bars
            }
            data.push(0xFF); // Bottom bar
        }
    }

    data
}

fn run_uefi(workspace_root: &Path) {
    let staged_image = build_uefi(workspace_root);
    let ovmf_code = find_ovmf_code().unwrap_or_else(|| {
        eprintln!("Could not find an EDK2 firmware image. Set TURNIX_OVMF_CODE to point to it.");
        std::process::exit(1);
    });
    let ovmf_vars = find_ovmf_vars().unwrap_or_else(|| {
        eprintln!("Could not find an EDK2 vars image. Set TURNIX_OVMF_VARS to point to it.");
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
    let mut qemu = ProcessCommand::new("qemu-system-x86_64");
    qemu.arg("-machine")
        .arg("q35")
        .arg("-m")
        .arg("512M")
        .arg("-serial")
        .arg("stdio")
        .arg("-monitor")
        .arg("none")
        .arg("-display")
        .arg("sdl,gl=on")
        .arg("-no-reboot")
        .arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04")
        .arg("-device")
        .arg("qemu-xhci,id=xhci")
        .arg("-device")
        .arg("usb-kbd");

    if let Ok(accel) = env::var("TURNIX_QEMU_ACCEL") {
        qemu.arg("-accel").arg(accel);
    } else {
        if cfg!(target_os = "windows") {
            qemu.arg("-accel").arg("whpx");
        } else if cfg!(target_os = "macos") {
            qemu.arg("-accel").arg("hvf");
        } else {
            qemu.arg("-accel").arg("kvm");
        }
        qemu.arg("-accel").arg("tcg");
    }

    let status = qemu
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
        Some(33) => println!("QEMU exited after the Turnix success path."),
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
    if let Ok(path) = env::var("TURNIX_OVMF_CODE") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let mut candidates = Vec::new();
    if let Some(qemu_path) = find_command_path("qemu-system-x86_64") {
        if let Some(base) = qemu_path.parent().and_then(Path::parent) {
            candidates.push(base.join("share").join("qemu").join("edk2-x86_64-code.fd"));
            candidates.push(base.join("share").join("ovmf").join("OVMF.fd"));
        }
    }

    candidates.push(PathBuf::from("/usr/share/ovmf/OVMF.fd"));
    candidates.push(PathBuf::from("/usr/share/ovmf/x64/OVMF_CODE.fd"));
    candidates.push(PathBuf::from("/usr/share/OVMF/OVMF_CODE.fd"));
    candidates.push(PathBuf::from(
        r"C:\msys64\ucrt64\share\qemu\edk2-x86_64-code.fd",
    ));
    candidates.push(PathBuf::from(
        r"C:\Program Files\qemu\share\qemu\edk2-x86_64-code.fd",
    ));

    candidates.into_iter().find(|path| path.exists())
}

fn find_ovmf_vars() -> Option<PathBuf> {
    if let Ok(path) = env::var("TURNIX_OVMF_VARS") {
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
            let ovmf = base.join("share").join("ovmf");
            candidates.push(ovmf.join("OVMF.fd"));
        }
    }

    candidates.push(PathBuf::from("/usr/share/ovmf/OVMF.fd"));
    candidates.push(PathBuf::from("/usr/share/ovmf/x64/OVMF_VARS.fd"));
    candidates.push(PathBuf::from("/usr/share/OVMF/OVMF_VARS.fd"));
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
    let program = if cfg!(target_os = "windows") {
        "where.exe"
    } else {
        "which"
    };
    let mut cmd = ProcessCommand::new(program);
    cmd.arg(command);

    let output = match cmd.output_safe() {
        Ok(o) => o,
        Err(_) => return None,
    };

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

trait CommandExt {
    fn output_safe(&mut self) -> std::io::Result<std::process::Output>;
}

impl CommandExt for ProcessCommand {
    fn output_safe(&mut self) -> std::io::Result<std::process::Output> {
        use std::io::Read;
        use std::process::Stdio;

        self.stdout(Stdio::piped());
        self.stderr(Stdio::piped());

        let mut child = self.spawn()?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        if let Some(mut out) = child.stdout.take() {
            out.read_to_end(&mut stdout).ok();
        }
        if let Some(mut err) = child.stderr.take() {
            err.read_to_end(&mut stderr).ok();
        }

        let status = child.wait()?;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    }
}

fn command_works<I, S>(program: &str, args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = ProcessCommand::new(program);
    cmd.args(args);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

fn output_contains<I, S>(program: &str, args: I, expected: &str) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = ProcessCommand::new(program);
    cmd.args(args);
    match cmd.output_safe() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).contains(expected),
        Err(_) => false,
    }
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

fn run_or_die_with_env<I, S, K, V>(
    program: &str,
    args: I,
    workspace_root: &Path,
    env_vars: &[(K, V)],
) where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let mut cmd = ProcessCommand::new(program);
    cmd.args(args).current_dir(workspace_root);
    for (k, v) in env_vars {
        cmd.env(k, v);
    }

    let status = cmd.status().expect("subprocess should start");

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}
