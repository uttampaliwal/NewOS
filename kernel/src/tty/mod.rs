use crate::drivers::video::CONSOLE;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    pub static ref TTY: Mutex<Tty> = Mutex::new(Tty::new());
}

pub struct Tty {
    line_buffer: String,
    input_queue: VecDeque<String>,
    /// Partially consumed line (byte-at-a-time reads).
    pending_line: Vec<u8>,
    pending_offset: usize,
}

impl Default for Tty {
    fn default() -> Self {
        Self::new()
    }
}

impl Tty {
    pub fn new() -> Self {
        Self {
            line_buffer: String::new(),
            input_queue: VecDeque::new(),
            pending_line: Vec::new(),
            pending_offset: 0,
        }
    }

    pub fn handle_input(&mut self, c: char) {
        match c {
            '\n' | '\r' => {
                let mut line = self.line_buffer.clone();
                line.push('\n');
                self.input_queue.push_back(line);
                self.line_buffer.clear();

                if let Some(ref mut console) = *CONSOLE.lock() {
                    console.write_char('\n');
                }
            }
            '\x08' | '\x7f' => {
                // Backspace
                if self.line_buffer.pop().is_some()
                    && let Some(ref mut console) = *CONSOLE.lock()
                {
                    console.backspace();
                }
            }
            _ => {
                if !c.is_control() {
                    self.line_buffer.push(c);
                    if let Some(ref mut console) = *CONSOLE.lock() {
                        console.write_char(c);
                    }
                }
            }
        }
    }

    /// Return the next byte from the TTY input, blocking if necessary.
    /// Returns `None` only if there is truly no data (queue empty and no pending).
    pub fn read_byte(&mut self) -> Option<u8> {
        // First, drain any partially consumed line.
        if self.pending_offset < self.pending_line.len() {
            let b = self.pending_line[self.pending_offset];
            self.pending_offset += 1;
            // If we consumed the entire pending line, reset.
            if self.pending_offset >= self.pending_line.len() {
                self.pending_line.clear();
                self.pending_offset = 0;
            }
            return Some(b);
        }

        // No pending data — pop the next complete line from the queue.
        if let Some(line) = self.input_queue.pop_front() {
            self.pending_line = line.into_bytes();
            self.pending_offset = 1; // We're about to return byte 0.
            Some(self.pending_line[0])
        } else {
            None
        }
    }

    /// Legacy line-buffered read (used by kernel-internal callers).
    pub fn read_line(&mut self) -> Option<String> {
        self.input_queue.pop_front()
    }

    pub fn write(&mut self, s: &str) {
        if let Some(ref mut console) = *CONSOLE.lock() {
            console.write_str(s);
        }
    }
}
