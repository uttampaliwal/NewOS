use crate::drivers::video::CONSOLE;
use alloc::collections::VecDeque;
use alloc::string::String;
use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    pub static ref TTY: Mutex<Tty> = Mutex::new(Tty::new());
}

pub struct Tty {
    line_buffer: String,
    input_queue: VecDeque<String>,
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

    pub fn read_line(&mut self) -> Option<String> {
        self.input_queue.pop_front()
    }

    pub fn write(&mut self, s: &str) {
        if let Some(ref mut console) = *CONSOLE.lock() {
            console.write_str(s);
        }
    }
}
