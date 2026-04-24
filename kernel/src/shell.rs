use alloc::string::ToString;
use alloc::{string::String, vec::Vec};
use core::fmt::{self, Write};

pub struct Repl {
    prompt: &'static str,
    echo: bool,
}

impl Repl {
    pub const fn new() -> Self {
        Self {
            prompt: "NewOS> ",
            echo: true,
        }
    }

    pub fn handle_byte(&mut self, byte: u8) -> Option<Response> {
        match byte {
            b'\r' | b'\n' => Some(Response::Newline),
            0x08 | 0x7F => Some(Response::Backspace),
            b if (0x20..=0x7E).contains(&b) || (0xA0..=0xFF).contains(&b) => {
                Some(Response::Char(byte))
            }
            _ => None,
        }
    }

    pub fn process_command(&mut self, cmd: &str) -> CommandResult {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return CommandResult::Empty;
        }

        let parts: Vec<&str> = cmd.split_whitespace().collect();
        match parts[0] {
            "help" => CommandResult::Help,
            "info" => CommandResult::Info,
            "ls" => CommandResult::Ls,
            "cat" => {
                if parts.len() > 1 {
                    CommandResult::Cat(parts[1].to_string())
                } else {
                    CommandResult::Echo("Usage: cat <file>".to_string())
                }
            }
            "echo" => {
                if parts.len() > 1 {
                    CommandResult::Echo(parts[1..].join(" "))
                } else {
                    CommandResult::Echo("".to_string())
                }
            }
            "clear" => CommandResult::Clear,
            "version" => CommandResult::Version,
            "mem" => CommandResult::Memory,
            "tasks" => CommandResult::Tasks,
            "exit" => CommandResult::Exit,
            _ => CommandResult::Unknown(parts[0].to_string()),
        }
    }

    pub fn prompt_str(&self) -> &'static str {
        self.prompt
    }
}

pub enum Response {
    Char(u8),
    Backspace,
    Newline,
}

pub enum CommandResult {
    Empty,
    Help,
    Info,
    Ls,
    Cat(String),
    Echo(String),
    Clear,
    Version,
    Memory,
    Tasks,
    Exit,
    Unknown(String),
}

impl CommandResult {
    pub fn write_output<W: Write>(&self, writer: &mut W) -> fmt::Result {
        match self {
            CommandResult::Empty => Ok(()),
            CommandResult::Help => {
                writeln!(writer, "Available commands:")?;
                writeln!(writer, "  help    - Show this help message")?;
                writeln!(writer, "  info   - Show kernel information")?;
                writeln!(writer, "  ls     - List files")?;
                writeln!(writer, "  cat    - Show file contents")?;
                writeln!(writer, "  echo   - Echo text back")?;
                writeln!(writer, "  clear  - Clear the screen")?;
                writeln!(writer, "  version - Show version")?;
                writeln!(writer, "  mem    - Show memory info")?;
                writeln!(writer, "  tasks  - Show tasks")?;
                writeln!(writer, "  exit   - Exit (shutdown)")
            }
            CommandResult::Info => {
                writeln!(writer, "NewOS - A Rust-first operating system")?;
                writeln!(writer, "Phase 6: Terminal-first usability")?;
                writeln!(writer, "Built with Rust (nightly, no_std)")
            }
            CommandResult::Ls => {
                let vfs = crate::vfs::Vfs::new();
                let files = vfs.list_dir();
                for name in files.iter() {
                    let _ = writeln!(writer, "{}", name);
                }
                Ok(())
            }
            CommandResult::Cat(path) => {
                writeln!(writer, "cat: file '{}' - demo only", path)
            }
            CommandResult::Echo(text) => {
                writeln!(writer, "{}", text)
            }
            CommandResult::Clear => {
                writeln!(writer, "\x1b[2J\x1b[H")
            }
            CommandResult::Version => {
                writeln!(writer, "NewOS {}", crate::kernel_info().project_name)?;
                writeln!(writer, "ABI version: {}", crate::kernel_info().abi_version)
            }
            CommandResult::Memory => {
                writeln!(
                    writer,
                    "Heap: {} bytes at 0x{:016x}",
                    crate::memory::heap::HEAP_SIZE,
                    crate::memory::heap::HEAP_START
                )
            }
            CommandResult::Tasks => {
                writeln!(writer, "Scheduler: cooperative multitasking enabled")
            }
            CommandResult::Exit => {
                writeln!(writer, "Shutting down...")
            }
            CommandResult::Unknown(cmd) => {
                writeln!(
                    writer,
                    "Unknown command: {}. Type 'help' for available commands.",
                    cmd
                )
            }
        }
    }
}
