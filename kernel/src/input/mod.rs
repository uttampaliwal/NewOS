use alloc::collections::VecDeque;
use lazy_static::lazy_static;
use pc_keyboard::{HandleControl, Keyboard, ScancodeSet1, layouts};
use spin::Mutex;

lazy_static! {
    pub static ref KEYBOARD_BUFFER: Mutex<VecDeque<char>> =
        Mutex::new(VecDeque::with_capacity(128));
    pub static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore
        ));
}

pub fn add_char(c: char) {
    let mut buffer = KEYBOARD_BUFFER.lock();
    if buffer.len() < 128 {
        buffer.push_back(c);
    }
}

pub fn read_char() -> Option<char> {
    KEYBOARD_BUFFER.lock().pop_front()
}
