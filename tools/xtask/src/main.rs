use std::env;

enum Command {
    Status,
    Doctor,
}

fn main() {
    match parse_command(env::args().nth(1).as_deref()) {
        Command::Status => print_status(),
        Command::Doctor => print_doctor(),
    }
}

fn print_status() {
    println!("NewOS Phase 0 workspace is in place.");
    println!("Next milestone: bootable kernel bring-up.");
}

fn print_doctor() {
    println!("Host checks are not automated yet.");
    println!("Planned checks: QEMU, Limine, Rust nightly, freestanding target.");
}

fn parse_command(raw: Option<&str>) -> Command {
    match raw {
        Some("doctor") => Command::Doctor,
        Some("status") | None => Command::Status,
        Some(other) => {
            eprintln!("Unknown xtask command: {other}");
            eprintln!("Available commands: status, doctor");
            std::process::exit(2);
        }
    }
}
