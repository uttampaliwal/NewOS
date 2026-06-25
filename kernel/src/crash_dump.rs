use core::fmt::Write;
use spin::Mutex;

/// Maximum number of characters in the panic message.
const PANIC_MSG_MAX: usize = 512;

/// Maximum number of characters in the register dump.
const REG_DUMP_MAX: usize = 1024;

/// Maximum number of log entries to capture from the ring buffer.
const LOG_ENTRIES_TO_CAPTURE: usize = 32;

/// A captured panic report, stored in a static so it survives across
/// the panic path (and in real hardware could be read from reserved
/// memory after a warm reboot).
#[derive(Debug)]
pub struct PanicReport {
    /// The panic message (from `PanicInfo`).
    pub message: [u8; PANIC_MSG_MAX],
    pub message_len: usize,
    /// Register dump at the time of panic.
    pub registers: [u8; REG_DUMP_MAX],
    pub registers_len: usize,
    /// Uptime in ticks when the panic occurred.
    pub uptime_ticks: u64,
    /// Whether this report is valid (has been written).
    pub valid: bool,
}

impl PanicReport {
    const fn new() -> Self {
        PanicReport {
            message: [0; PANIC_MSG_MAX],
            message_len: 0,
            registers: [0; REG_DUMP_MAX],
            registers_len: 0,
            uptime_ticks: 0,
            valid: false,
        }
    }
}

static PANIC_REPORT: Mutex<PanicReport> = Mutex::new(PanicReport::new());

/// A ring buffer for formatting the register dump.
struct BufWriter {
    buf: [u8; REG_DUMP_MAX],
    pos: usize,
}

impl BufWriter {
    const fn new() -> Self {
        BufWriter {
            buf: [0; REG_DUMP_MAX],
            pos: 0,
        }
    }
}

impl Write for BufWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len() - self.pos;
        let to_write = bytes.len().min(remaining);
        self.buf[self.pos..self.pos + to_write].copy_from_slice(&bytes[..to_write]);
        self.pos += to_write;
        Ok(())
    }
}

/// Capture the register state from inline assembly and write a crash dump report.
///
/// This is called from the panic handler. It captures:
/// - The panic message
/// - CPU registers (RSP, RIP, RFLAGS, CR2 for page faults)
/// - Uptime ticks
/// - Recent kernel log entries
pub fn capture_panic(info: &core::panic::PanicInfo<'_>) {
    // Capture registers via inline assembly
    let rip: u64;
    let rsp: u64;
    let rbp: u64;
    let rflags: u64;
    let cr2: u64;
    let rax: u64;
    let rbx: u64;
    let rcx: u64;
    let rdx: u64;
    let rsi: u64;
    let rdi: u64;

    // Safety: reading register values has no memory side effects.
    unsafe {
        core::arch::asm!(
            "mov {rip}, rip",
            "mov {rsp}, rsp",
            "mov {rbp}, rbp",
            "pushfq",
            "pop {rflags}",
            "mov {cr2}, cr2",
            "mov {rax}, rax",
            "mov {rbx}, rbx",
            "mov {rcx}, rcx",
            "mov {rdx}, rdx",
            "mov {rsi}, rsi",
            "mov {rdi}, rdi",
            rip = out(reg) rip,
            rsp = out(reg) rsp,
            rbp = out(reg) rbp,
            rflags = out(reg) rflags,
            cr2 = out(reg) cr2,
            rax = out(reg) rax,
            rbx = out(reg) rbx,
            rcx = out(reg) rcx,
            rdx = out(reg) rdx,
            rsi = out(reg) rsi,
            rdi = out(reg) rdi,
            options(nomem, nostack, preserves_flags),
        );
    }

    let uptime = crate::task::scheduler::get_uptime_ticks();

    // Format the panic message
    let mut msg_buf = [0u8; PANIC_MSG_MAX];
    let mut msg_writer = BufWriter::new();
    let _ = write!(msg_writer, "{}", info);
    let msg_len = msg_writer.pos.min(PANIC_MSG_MAX);
    msg_buf[..msg_len].copy_from_slice(&msg_writer.buf[..msg_len]);

    // Format the register dump
    let mut reg_buf = [0u8; REG_DUMP_MAX];
    let mut reg_writer = BufWriter::new();
    let _ = writeln!(reg_writer, "=== Register Dump ===");
    let _ = writeln!(reg_writer, "RIP:    {:#018x}", rip);
    let _ = writeln!(reg_writer, "RSP:    {:#018x}", rsp);
    let _ = writeln!(reg_writer, "RBP:    {:#018x}", rbp);
    let _ = writeln!(reg_writer, "RFLAGS: {:#018x}", rflags);
    let _ = writeln!(reg_writer, "CR2:    {:#018x} (faulting address)", cr2);
    let _ = writeln!(reg_writer, "RAX:    {:#018x}", rax);
    let _ = writeln!(reg_writer, "RBX:    {:#018x}", rbx);
    let _ = writeln!(reg_writer, "RCX:    {:#018x}", rcx);
    let _ = writeln!(reg_writer, "RDX:    {:#018x}", rdx);
    let _ = writeln!(reg_writer, "RSI:    {:#018x}", rsi);
    let _ = writeln!(reg_writer, "RDI:    {:#018x}", rdi);

    // Best-effort stack trace via frame pointer walking
    let _ = writeln!(reg_writer);
    let _ = writeln!(reg_writer, "=== Stack Trace (frame pointer) ===");
    let mut frame_ptr = rbp;
    for i in 0..16 {
        if frame_ptr == 0 || frame_ptr < 0x1000 {
            break;
        }
        // Safety: frame_ptr comes from rbp which should point to a valid stack frame.
        // We read two u64 values: saved rbp and return address.
        let (saved_rbp, ret_addr) = unsafe {
            let rbp_ptr = frame_ptr as *const u64;
            let saved = core::ptr::read_unaligned(rbp_ptr);
            let ret = core::ptr::read_unaligned(rbp_ptr.add(1));
            (saved, ret)
        };
        let _ = writeln!(reg_writer, "  #{}: rbp={:#018x} ret={:#018x}", i, frame_ptr, ret_addr);
        if saved_rbp <= frame_ptr {
            break; // Prevent infinite loop on corrupt frame chains
        }
        frame_ptr = saved_rbp;
    }

    // Capture recent log entries
    let _ = writeln!(reg_writer);
    let _ = writeln!(reg_writer, "=== Recent Kernel Log ===");
    let log_entries = crate::log_ring::kernel_log_peek_n(LOG_ENTRIES_TO_CAPTURE);
    for entry in &log_entries {
        let _ = writeln!(
            reg_writer,
            "  [{:>10}] {:?}: {}",
            entry.timestamp_us,
            entry.level,
            entry.message()
        );
    }

    let reg_len = reg_writer.pos.min(REG_DUMP_MAX);
    reg_buf[..reg_len].copy_from_slice(&reg_writer.buf[..reg_len]);

    // Write the report
    let mut report = PANIC_REPORT.lock();
    report.message[..msg_len].copy_from_slice(&msg_buf[..msg_len]);
    report.message_len = msg_len;
    report.registers[..reg_len].copy_from_slice(&reg_buf[..reg_len]);
    report.registers_len = reg_len;
    report.uptime_ticks = uptime;
    report.valid = true;

    // Print the full crash dump to serial as well
    crate::serial::print(format_args!(
        "\n========== PANIC CRASH DUMP ==========\n{}\n{}\n======================================\n",
        core::str::from_utf8(&report.message[..report.message_len]).unwrap_or("<invalid utf8>"),
        core::str::from_utf8(&report.registers[..report.registers_len]).unwrap_or("<invalid utf8>"),
    ));
}

/// Check if a panic report is available (from a previous panic).
pub fn has_panic_report() -> bool {
    PANIC_REPORT.lock().valid
}

/// Read the panic report. Returns `None` if no report is available.
pub fn read_panic_report() -> Option<PanicReport> {
    let report = PANIC_REPORT.lock();
    if report.valid {
        // Return a copy of the report
        Some(PanicReport {
            message: report.message,
            message_len: report.message_len,
            registers: report.registers,
            registers_len: report.registers_len,
            uptime_ticks: report.uptime_ticks,
            valid: true,
        })
    } else {
        None
    }
}

/// Print a previously captured panic report to serial.
pub fn print_panic_report() {
    if let Some(report) = read_panic_report() {
        crate::serial::print(format_args!(
            "\n========== PREVIOUS PANIC REPORT ==========\n"
        ));
        if let Ok(msg) = core::str::from_utf8(&report.message[..report.message_len]) {
            crate::serial::print(format_args!("Panic: {}\n", msg));
        }
        if let Ok(regs) = core::str::from_utf8(&report.registers[..report.registers_len]) {
            crate::serial::print(format_args!("{}\n", regs));
        }
        crate::serial::print(format_args!(
            "Uptime at panic: {} ticks\n",
            report.uptime_ticks
        ));
        crate::serial::print(format_args!(
            "==========================================\n"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panic_report_initial_invalid() {
        let _guard = crate::test_serial::acquire();
        assert!(!has_panic_report());
    }

    #[test]
    fn test_panic_report_constants() {
        let _guard = crate::test_serial::acquire();
        assert!(PANIC_MSG_MAX >= 256);
        assert!(REG_DUMP_MAX >= 512);
        assert!(LOG_ENTRIES_TO_CAPTURE > 0);
    }

    #[test]
    fn test_buf_writer() {
        let _guard = crate::test_serial::acquire();
        let mut w = BufWriter::new();
        write!(w, "hello {}", 42).unwrap();
        assert_eq!(w.pos, 8);
        assert_eq!(&w.buf[..w.pos], b"hello 42");
    }

    #[test]
    fn test_buf_writer_truncation() {
        let _guard = crate::test_serial::acquire();
        let mut w = BufWriter::new();
        let long = "x".repeat(REG_DUMP_MAX + 100);
        write!(w, "{}", long).unwrap();
        assert_eq!(w.pos, REG_DUMP_MAX);
    }
}
