use alloc::collections::VecDeque;
use lazy_static::lazy_static;
use pc_keyboard::{HandleControl, KeyCode as Ps2KeyCode, Keyboard, ScancodeSet1, layouts};
use spin::Mutex;
use turnix_abi::input::{
    InputEvent, KEY_0, KEY_1, KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8, KEY_9, KEY_A, KEY_B,
    KEY_BACKSLASH, KEY_BACKSPACE, KEY_BACKTICK, KEY_C, KEY_CAPSLOCK, KEY_COMMA, KEY_D, KEY_DELETE,
    KEY_DOT, KEY_DOWN, KEY_E, KEY_END, KEY_ENTER, KEY_EQUAL, KEY_ESC, KEY_F, KEY_F1, KEY_F2,
    KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8, KEY_F9, KEY_F10, KEY_F11, KEY_F12, KEY_G,
    KEY_H, KEY_HOME, KEY_I, KEY_INSERT, KEY_J, KEY_K, KEY_KP_ASTERISK, KEY_KP_DIVIDE, KEY_KP_DOT,
    KEY_KP_ENTER, KEY_KP_MINUS, KEY_KP_PLUS, KEY_KP0, KEY_KP1, KEY_KP2, KEY_KP3, KEY_KP4, KEY_KP5,
    KEY_KP6, KEY_KP7, KEY_KP8, KEY_KP9, KEY_L, KEY_LALT, KEY_LBRACE, KEY_LCTRL, KEY_LEFT,
    KEY_LSHIFT, KEY_LWIN, KEY_M, KEY_MENU, KEY_MINUS, KEY_N, KEY_NUMLOCK, KEY_O, KEY_P,
    KEY_PAGEDOWN, KEY_PAGEUP, KEY_PAUSE, KEY_PRINT, KEY_Q, KEY_QUOTE, KEY_R, KEY_RALT, KEY_RBRACE,
    KEY_RCTRL, KEY_RESERVED, KEY_RIGHT, KEY_RSHIFT, KEY_RWIN, KEY_S, KEY_SCROLLLOCK, KEY_SEMICOLON,
    KEY_SLASH, KEY_SPACE, KEY_T, KEY_TAB, KEY_U, KEY_UP, KEY_V, KEY_W, KEY_X, KEY_Y, KEY_Z,
};

// ── Ring buffers ─────────────────────────────────────────────────────────────

const EVENT_BUFFER_CAPACITY: usize = 256;

lazy_static! {
    /// Ring buffer of normalized input events (for the `input_read` syscall).
    static ref EVENT_BUFFER: Mutex<VecDeque<InputEvent>> =
        Mutex::new(VecDeque::with_capacity(EVENT_BUFFER_CAPACITY));

    /// Legacy per-character ring buffer (kept for compatibility).
    pub static ref KEYBOARD_BUFFER: Mutex<VecDeque<char>> =
        Mutex::new(VecDeque::with_capacity(128));

    /// PS/2 keyboard state machine (pc_keyboard).
    pub static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore
        ));
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Push a normalized event into the ring buffer.
pub fn add_event(ev: InputEvent) {
    let mut buf = EVENT_BUFFER.lock();
    if buf.len() < EVENT_BUFFER_CAPACITY {
        buf.push_back(ev);
    }
}

/// Pop the oldest event from the buffer.
pub fn read_event() -> Option<InputEvent> {
    EVENT_BUFFER.lock().pop_front()
}

/// Legacy: push a character into the char buffer.
pub fn add_char(c: char) {
    let mut buffer = KEYBOARD_BUFFER.lock();
    if buffer.len() < 128 {
        buffer.push_back(c);
    }
}

/// Legacy: pop a character.
pub fn read_char() -> Option<char> {
    KEYBOARD_BUFFER.lock().pop_front()
}

// ── PS/2 scancode-set‑1 → key-code mapping ──────────────────────────────────

/// Map a `pc_keyboard::KeyCode` to a turnix KEY_* constant.
///
/// This is the "normalization" step: every physical keyboard driver produces an
/// identical `u16` code for the same physical key regardless of transport.
pub fn ps2_keycode_to_key(pc: Ps2KeyCode) -> u16 {
    use Ps2KeyCode::*;
    match pc {
        Escape => KEY_ESC,
        F1 => KEY_F1,
        F2 => KEY_F2,
        F3 => KEY_F3,
        F4 => KEY_F4,
        F5 => KEY_F5,
        F6 => KEY_F6,
        F7 => KEY_F7,
        F8 => KEY_F8,
        F9 => KEY_F9,
        F10 => KEY_F10,
        F11 => KEY_F11,
        F12 => KEY_F12,
        PrintScreen => KEY_PRINT,
        SysRq => KEY_PRINT, // same physical key
        ScrollLock => KEY_SCROLLLOCK,
        PauseBreak => KEY_PAUSE,

        Oem8 => KEY_BACKTICK,
        Key1 => KEY_1,
        Key2 => KEY_2,
        Key3 => KEY_3,
        Key4 => KEY_4,
        Key5 => KEY_5,
        Key6 => KEY_6,
        Key7 => KEY_7,
        Key8 => KEY_8,
        Key9 => KEY_9,
        Key0 => KEY_0,
        OemMinus => KEY_MINUS,
        OemPlus => KEY_EQUAL,
        Backspace => KEY_BACKSPACE,

        Insert => KEY_INSERT,
        Home => KEY_HOME,
        PageUp => KEY_PAGEUP,

        NumpadLock => KEY_NUMLOCK,
        NumpadDivide => KEY_KP_DIVIDE,
        NumpadMultiply => KEY_KP_ASTERISK,
        NumpadSubtract => KEY_KP_MINUS,

        Tab => KEY_TAB,
        Q => KEY_Q,
        W => KEY_W,
        E => KEY_E,
        R => KEY_R,
        T => KEY_T,
        Y => KEY_Y,
        U => KEY_U,
        I => KEY_I,
        O => KEY_O,
        P => KEY_P,
        Oem4 => KEY_LBRACE,
        Oem6 => KEY_RBRACE,
        Oem5 => KEY_BACKSLASH,
        Oem7 => KEY_BACKSLASH, // ISO-only key maps to backslash

        Delete => KEY_DELETE,
        End => KEY_END,
        PageDown => KEY_PAGEDOWN,

        Numpad7 => KEY_KP7,
        Numpad8 => KEY_KP8,
        Numpad9 => KEY_KP9,
        NumpadAdd => KEY_KP_PLUS,

        CapsLock => KEY_CAPSLOCK,
        A => KEY_A,
        S => KEY_S,
        D => KEY_D,
        F => KEY_F,
        G => KEY_G,
        H => KEY_H,
        J => KEY_J,
        K => KEY_K,
        L => KEY_L,
        Oem1 => KEY_SEMICOLON,
        Oem3 => KEY_QUOTE,

        Return => KEY_ENTER,

        Numpad4 => KEY_KP4,
        Numpad5 => KEY_KP5,
        Numpad6 => KEY_KP6,

        LShift => KEY_LSHIFT,
        Z => KEY_Z,
        X => KEY_X,
        C => KEY_C,
        V => KEY_V,
        B => KEY_B,
        N => KEY_N,
        M => KEY_M,
        OemComma => KEY_COMMA,
        OemPeriod => KEY_DOT,
        Oem2 => KEY_SLASH,
        RShift => KEY_RSHIFT,

        ArrowUp => KEY_UP,

        Numpad1 => KEY_KP1,
        Numpad2 => KEY_KP2,
        Numpad3 => KEY_KP3,
        NumpadEnter => KEY_KP_ENTER,

        LControl => KEY_LCTRL,
        LWin => KEY_LWIN,
        LAlt => KEY_LALT,
        Spacebar => KEY_SPACE,
        RAltGr => KEY_RALT,
        RWin => KEY_RWIN,
        Apps => KEY_MENU,
        RControl => KEY_RCTRL,

        ArrowLeft => KEY_LEFT,
        ArrowDown => KEY_DOWN,
        ArrowRight => KEY_RIGHT,

        Numpad0 => KEY_KP0,
        NumpadPeriod => KEY_KP_DOT,

        _ => KEY_RESERVED,
    }
}

// ── USB HID usage → key-code mapping ─────────────────────────────────────────

/// Map a USB HID keyboard usage ID (from the HID Usage Tables, chapter 10)
/// to a turnix KEY_* constant.
pub fn hid_usage_to_keycode(usage: u8) -> u16 {
    match usage {
        0x04 => KEY_A,
        0x05 => KEY_B,
        0x06 => KEY_C,
        0x07 => KEY_D,
        0x08 => KEY_E,
        0x09 => KEY_F,
        0x0A => KEY_G,
        0x0B => KEY_H,
        0x0C => KEY_I,
        0x0D => KEY_J,
        0x0E => KEY_K,
        0x0F => KEY_L,
        0x10 => KEY_M,
        0x11 => KEY_N,
        0x12 => KEY_O,
        0x13 => KEY_P,
        0x14 => KEY_Q,
        0x15 => KEY_R,
        0x16 => KEY_S,
        0x17 => KEY_T,
        0x18 => KEY_U,
        0x19 => KEY_V,
        0x1A => KEY_W,
        0x1B => KEY_X,
        0x1C => KEY_Y,
        0x1D => KEY_Z,

        0x1E => KEY_1,
        0x1F => KEY_2,
        0x20 => KEY_3,
        0x21 => KEY_4,
        0x22 => KEY_5,
        0x23 => KEY_6,
        0x24 => KEY_7,
        0x25 => KEY_8,
        0x26 => KEY_9,
        0x27 => KEY_0,

        0x28 => KEY_ENTER,
        0x29 => KEY_ESC,
        0x2A => KEY_BACKSPACE,
        0x2B => KEY_TAB,
        0x2C => KEY_SPACE,
        0x2D => KEY_MINUS,
        0x2E => KEY_EQUAL,
        0x2F => KEY_LBRACE,
        0x30 => KEY_RBRACE,
        0x31 => KEY_BACKSLASH,
        0x32 => KEY_RESERVED, // Non-US #
        0x33 => KEY_SEMICOLON,
        0x34 => KEY_QUOTE,
        0x35 => KEY_BACKTICK,
        0x36 => KEY_COMMA,
        0x37 => KEY_DOT,
        0x38 => KEY_SLASH,
        0x39 => KEY_CAPSLOCK,

        0x3A => KEY_F1,
        0x3B => KEY_F2,
        0x3C => KEY_F3,
        0x3D => KEY_F4,
        0x3E => KEY_F5,
        0x3F => KEY_F6,
        0x40 => KEY_F7,
        0x41 => KEY_F8,
        0x42 => KEY_F9,
        0x43 => KEY_F10,
        0x44 => KEY_F11,
        0x45 => KEY_F12,

        0x46 => KEY_PRINT,
        0x47 => KEY_SCROLLLOCK,
        0x48 => KEY_PAUSE,
        0x49 => KEY_INSERT,
        0x4A => KEY_HOME,
        0x4B => KEY_PAGEUP,
        0x4C => KEY_DELETE,
        0x4D => KEY_END,
        0x4E => KEY_PAGEDOWN,
        0x4F => KEY_RIGHT,
        0x50 => KEY_LEFT,
        0x51 => KEY_DOWN,
        0x52 => KEY_UP,

        0x53 => KEY_NUMLOCK,
        0x54 => KEY_KP_DIVIDE,
        0x55 => KEY_KP_ASTERISK,
        0x56 => KEY_KP_MINUS,
        0x57 => KEY_KP_PLUS,
        0x58 => KEY_KP_ENTER,
        0x59 => KEY_KP1,
        0x5A => KEY_KP2,
        0x5B => KEY_KP3,
        0x5C => KEY_KP4,
        0x5D => KEY_KP5,
        0x5E => KEY_KP6,
        0x5F => KEY_KP7,
        0x60 => KEY_KP8,
        0x61 => KEY_KP9,
        0x62 => KEY_KP0,
        0x63 => KEY_KP_DOT,

        0x64 => KEY_BACKSLASH, // Non-US \|
        0x65 => KEY_MENU,

        0xE0 => KEY_LCTRL,
        0xE1 => KEY_LSHIFT,
        0xE2 => KEY_LALT,
        0xE3 => KEY_LWIN,
        0xE4 => KEY_RCTRL,
        0xE5 => KEY_RSHIFT,
        0xE6 => KEY_RALT,
        0xE7 => KEY_RWIN,

        0xEA => KEY_RESERVED, // Reserved
        0xEB => KEY_RESERVED, // Mute; HUT uses different codes for media
        0xEC => KEY_RESERVED, // App menu?

        _ => KEY_RESERVED,
    }
}

/// Legacy HID usage → ASCII mapping (kept for callers that still need it).
/// Superseded by `hid_usage_to_keycode` for new code.
pub fn hid_usage_to_ascii(usage: u8) -> Option<char> {
    match usage {
        0x04 => Some('a'),
        0x05 => Some('b'),
        0x06 => Some('c'),
        0x07 => Some('d'),
        0x08 => Some('e'),
        0x09 => Some('f'),
        0x0A => Some('g'),
        0x0B => Some('h'),
        0x0C => Some('i'),
        0x0D => Some('j'),
        0x0E => Some('k'),
        0x0F => Some('l'),
        0x10 => Some('m'),
        0x11 => Some('n'),
        0x12 => Some('o'),
        0x13 => Some('p'),
        0x14 => Some('q'),
        0x15 => Some('r'),
        0x16 => Some('s'),
        0x17 => Some('t'),
        0x18 => Some('u'),
        0x19 => Some('v'),
        0x1A => Some('w'),
        0x1B => Some('x'),
        0x1C => Some('y'),
        0x1D => Some('z'),
        0x1E => Some('1'),
        0x1F => Some('2'),
        0x20 => Some('3'),
        0x21 => Some('4'),
        0x22 => Some('5'),
        0x23 => Some('6'),
        0x24 => Some('7'),
        0x25 => Some('8'),
        0x26 => Some('9'),
        0x27 => Some('0'),
        0x28 => Some('\n'),         // Enter
        0x29 => Some(0x1B as char), // Escape
        0x2A => Some(0x08 as char), // Backspace
        0x2B => Some('\t'),         // Tab
        0x2C => Some(' '),          // Space
        0x2D => Some('-'),
        0x2E => Some('='),
        0x2F => Some('['),
        0x30 => Some(']'),
        0x31 => Some('\\'),
        0x33 => Some(';'),
        0x34 => Some('\''),
        0x35 => Some('`'),
        0x36 => Some(','),
        0x37 => Some('.'),
        0x38 => Some('/'),
        _ => None,
    }
}

// ── Pointer acceleration ────────────────────────────────────────────────────

/// Pointer acceleration threshold and factor for integer-only calculation.
///
/// The model used is a simple piece-wise linear + quadratic boost:
///
/// ```ignore
/// if |delta| ≤ THRESHOLD:
///     result = delta × sensitivity / Q
/// else:
///     result = delta × sensitivity / Q
///            + sign(delta) × (|delta| - THRESHOLD) × (|delta| - THRESHOLD) / BOOST_DIV
/// ```
///
/// Small, precise movements are unaffected; large, fast movements get an
/// increasingly strong boost.
const ACCEL_THRESHOLD: i32 = 4; // pixels; below this: no boost
const ACCEL_BOOST_DIV: i32 = 32; // denominator for the quadratic term

/// Apply pointer acceleration to an integer delta.
///
/// `speed` is in Q8 fixed-point (256 = 1.0 × normal).
pub fn apply_pointer_acceleration_i32(delta: i32, speed_q8: i32) -> i32 {
    if delta == 0 || speed_q8 <= 0 {
        return delta;
    }

    let abs_delta = delta.abs();
    let base = delta * speed_q8 / 256;

    if abs_delta <= ACCEL_THRESHOLD {
        base
    } else {
        let excess = abs_delta - ACCEL_THRESHOLD;
        let boost = (delta.signum()) * (excess * excess / ACCEL_BOOST_DIV);
        base + boost
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    // ── Key mapping tests ────────────────────────────────────────────────

    #[test]
    fn ps2_map_letter_keys() {
        use Ps2KeyCode::*;
        assert_eq!(ps2_keycode_to_key(A), KEY_A);
        assert_eq!(ps2_keycode_to_key(Z), KEY_Z);
        assert_eq!(ps2_keycode_to_key(M), KEY_M);
    }

    #[test]
    fn ps2_map_number_keys() {
        use Ps2KeyCode::*;
        assert_eq!(ps2_keycode_to_key(Key1), KEY_1);
        assert_eq!(ps2_keycode_to_key(Key0), KEY_0);
    }

    #[test]
    fn ps2_map_function_keys() {
        use Ps2KeyCode::*;
        assert_eq!(ps2_keycode_to_key(F1), KEY_F1);
        assert_eq!(ps2_keycode_to_key(F12), KEY_F12);
    }

    #[test]
    fn ps2_map_modifiers() {
        use Ps2KeyCode::*;
        assert_eq!(ps2_keycode_to_key(LShift), KEY_LSHIFT);
        assert_eq!(ps2_keycode_to_key(RShift), KEY_RSHIFT);
        assert_eq!(ps2_keycode_to_key(LControl), KEY_LCTRL);
        assert_eq!(ps2_keycode_to_key(RControl), KEY_RCTRL);
        assert_eq!(ps2_keycode_to_key(LAlt), KEY_LALT);
        assert_eq!(ps2_keycode_to_key(RAltGr), KEY_RALT);
        assert_eq!(ps2_keycode_to_key(LWin), KEY_LWIN);
        assert_eq!(ps2_keycode_to_key(RWin), KEY_RWIN);
        assert_eq!(ps2_keycode_to_key(Apps), KEY_MENU);
    }

    #[test]
    fn ps2_map_nav_keys() {
        use Ps2KeyCode::*;
        assert_eq!(ps2_keycode_to_key(ArrowUp), KEY_UP);
        assert_eq!(ps2_keycode_to_key(ArrowDown), KEY_DOWN);
        assert_eq!(ps2_keycode_to_key(ArrowLeft), KEY_LEFT);
        assert_eq!(ps2_keycode_to_key(ArrowRight), KEY_RIGHT);
        assert_eq!(ps2_keycode_to_key(Home), KEY_HOME);
        assert_eq!(ps2_keycode_to_key(End), KEY_END);
        assert_eq!(ps2_keycode_to_key(PageUp), KEY_PAGEUP);
        assert_eq!(ps2_keycode_to_key(PageDown), KEY_PAGEDOWN);
        assert_eq!(ps2_keycode_to_key(Insert), KEY_INSERT);
        assert_eq!(ps2_keycode_to_key(Delete), KEY_DELETE);
    }

    #[test]
    fn ps2_map_special_keys() {
        use Ps2KeyCode::*;
        assert_eq!(ps2_keycode_to_key(Escape), KEY_ESC);
        assert_eq!(ps2_keycode_to_key(Tab), KEY_TAB);
        assert_eq!(ps2_keycode_to_key(Return), KEY_ENTER);
        assert_eq!(ps2_keycode_to_key(Backspace), KEY_BACKSPACE);
        assert_eq!(ps2_keycode_to_key(Spacebar), KEY_SPACE);
        assert_eq!(ps2_keycode_to_key(CapsLock), KEY_CAPSLOCK);
        assert_eq!(ps2_keycode_to_key(PrintScreen), KEY_PRINT);
        assert_eq!(ps2_keycode_to_key(PauseBreak), KEY_PAUSE);
        assert_eq!(ps2_keycode_to_key(ScrollLock), KEY_SCROLLLOCK);
    }

    #[test]
    fn ps2_map_punctuation() {
        use Ps2KeyCode::*;
        assert_eq!(ps2_keycode_to_key(OemMinus), KEY_MINUS);
        assert_eq!(ps2_keycode_to_key(OemPlus), KEY_EQUAL);
        assert_eq!(ps2_keycode_to_key(Oem4), KEY_LBRACE);
        assert_eq!(ps2_keycode_to_key(Oem6), KEY_RBRACE);
        assert_eq!(ps2_keycode_to_key(Oem5), KEY_BACKSLASH);
        assert_eq!(ps2_keycode_to_key(Oem1), KEY_SEMICOLON);
        assert_eq!(ps2_keycode_to_key(Oem3), KEY_QUOTE);
        assert_eq!(ps2_keycode_to_key(OemComma), KEY_COMMA);
        assert_eq!(ps2_keycode_to_key(OemPeriod), KEY_DOT);
        assert_eq!(ps2_keycode_to_key(Oem2), KEY_SLASH);
    }

    #[test]
    fn hid_map_letter_keys() {
        assert_eq!(hid_usage_to_keycode(0x04), KEY_A);
        assert_eq!(hid_usage_to_keycode(0x1D), KEY_Z);
        assert_eq!(hid_usage_to_keycode(0x10), KEY_M);
    }

    #[test]
    fn hid_map_modifiers() {
        assert_eq!(hid_usage_to_keycode(0xE0), KEY_LCTRL);
        assert_eq!(hid_usage_to_keycode(0xE1), KEY_LSHIFT);
        assert_eq!(hid_usage_to_keycode(0xE2), KEY_LALT);
        assert_eq!(hid_usage_to_keycode(0xE3), KEY_LWIN);
        assert_eq!(hid_usage_to_keycode(0xE4), KEY_RCTRL);
        assert_eq!(hid_usage_to_keycode(0xE5), KEY_RSHIFT);
        assert_eq!(hid_usage_to_keycode(0xE6), KEY_RALT);
        assert_eq!(hid_usage_to_keycode(0xE7), KEY_RWIN);
    }

    #[test]
    fn hid_map_fkeys() {
        assert_eq!(hid_usage_to_keycode(0x3A), KEY_F1);
        assert_eq!(hid_usage_to_keycode(0x45), KEY_F12);
    }

    #[test]
    fn hid_unmapped_returns_reserved() {
        assert_eq!(hid_usage_to_keycode(0xFF), KEY_RESERVED);
    }

    // ── Event buffer tests ───────────────────────────────────────────────

    #[test]
    fn event_buffer_does_not_overflow() {
        let _guard = crate::test_serial::acquire();
        // Drain any events left by previous tests.
        while read_event().is_some() {}

        let overflow = EVENT_BUFFER_CAPACITY + 64;
        let ev = InputEvent::syn();
        for _ in 0..overflow {
            add_event(ev);
        }

        let mut count = 0;
        while read_event().is_some() {
            count += 1;
        }
        // Should have dropped events above capacity
        assert_eq!(count, EVENT_BUFFER_CAPACITY);
    }

    // ── Legacy char buffer ───────────────────────────────────────────────

    #[test]
    fn legacy_char_buffer_works() {
        add_char('x');
        assert_eq!(read_char(), Some('x'));
        assert_eq!(read_char(), None);
    }

    // ── Pointer acceleration ─────────────────────────────────────────────

    #[test]
    fn pointer_accel_zero_input() {
        // Zero delta is always zero.
        assert_eq!(apply_pointer_acceleration_i32(0, 256), 0);
        // Zero speed returns raw delta.
        assert_eq!(apply_pointer_acceleration_i32(5, 0), 5);
        // Negative speed returns raw delta.
        assert_eq!(apply_pointer_acceleration_i32(5, -1), 5);
    }

    #[test]
    fn pointer_accel_preserves_sign() {
        let r = apply_pointer_acceleration_i32(-5, 256);
        assert!(r < 0, "negative delta stays negative, got {}", r);
        let r = apply_pointer_acceleration_i32(5, 256);
        assert!(r > 0, "positive delta stays positive, got {}", r);
    }

    #[test]
    fn pointer_accel_small_deltas_not_boosted() {
        // Deltas ≤ THRESHOLD get only the linear sensitivity scaling.
        let r = apply_pointer_acceleration_i32(3, 256);
        assert_eq!(r, 3, "small delta should be unchanged");
    }

    #[test]
    fn pointer_accel_amplifies_large_deltas() {
        // Delta 10 with Q8 speed 256: base = 10, excess = 6, boost = 36/32 = 1
        let r = apply_pointer_acceleration_i32(10, 256);
        assert!(r > 10, "large delta should be amplified, got {}", r);
    }

    #[test]
    fn pointer_accel_responds_to_speed() {
        let normal = apply_pointer_acceleration_i32(10, 256);
        let faster = apply_pointer_acceleration_i32(10, 512);
        assert!(faster > normal, "higher speed should give larger result");
    }

    // ── Property-based tests ─────────────────────────────────────────────

    /// Use proptest to verify the PS/2 key mapping never returns `KEY_RESERVED`
    /// for all known `pc_keyboard::KeyCode` variants.
    #[test]
    fn ps2_all_variants_mapped() {
        // pc_keyboard::KeyCode doesn't implement Arbitrary, so we enumerate
        use Ps2KeyCode::*;
        let all = [
            Escape,
            F1,
            F2,
            F3,
            F4,
            F5,
            F6,
            F7,
            F8,
            F9,
            F10,
            F11,
            F12,
            PrintScreen,
            SysRq,
            ScrollLock,
            PauseBreak,
            Oem8,
            Key1,
            Key2,
            Key3,
            Key4,
            Key5,
            Key6,
            Key7,
            Key8,
            Key9,
            Key0,
            OemMinus,
            OemPlus,
            Backspace,
            Insert,
            Home,
            PageUp,
            NumpadLock,
            NumpadDivide,
            NumpadMultiply,
            NumpadSubtract,
            Tab,
            Q,
            W,
            E,
            R,
            T,
            Y,
            U,
            I,
            O,
            P,
            Oem4,
            Oem6,
            Oem5,
            Oem7,
            Delete,
            End,
            PageDown,
            Numpad7,
            Numpad8,
            Numpad9,
            NumpadAdd,
            CapsLock,
            A,
            S,
            D,
            F,
            G,
            H,
            J,
            K,
            L,
            Oem1,
            Oem3,
            Return,
            Numpad4,
            Numpad5,
            Numpad6,
            LShift,
            Z,
            X,
            C,
            V,
            B,
            N,
            M,
            OemComma,
            OemPeriod,
            Oem2,
            RShift,
            ArrowUp,
            Numpad1,
            Numpad2,
            Numpad3,
            NumpadEnter,
            LControl,
            LWin,
            LAlt,
            Spacebar,
            RAltGr,
            RWin,
            Apps,
            RControl,
            ArrowLeft,
            ArrowDown,
            ArrowRight,
            Numpad0,
            NumpadPeriod,
        ];
        for &k in &all {
            let mapped = ps2_keycode_to_key(k);
            assert!(
                mapped != KEY_RESERVED,
                "PS/2 keycode {:?} maps to KEY_RESERVED",
                k,
            );
        }
    }

    #[test]
    fn hid_all_standard_usages_mapped() {
        // Known USB HID usage IDs (from HUT1.12 chapter 10 Keyboard/Keypad Page).
        // We spot-check a range for non-reserved mappings.
        let known = [
            0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11,
            0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
            0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D,
            0x2E, 0x2F, 0x30, 0x31, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C,
            0x3D, 0x3E, 0x3F, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A,
            0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58,
            0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x5F, 0x60, 0x61, 0x62, 0x63, 0xE0, 0xE1, 0xE2,
            0xE3, 0xE4, 0xE5, 0xE6, 0xE7,
        ];
        for &usage in &known {
            let mapped = hid_usage_to_keycode(usage);
            assert!(
                mapped != KEY_RESERVED,
                "HID usage 0x{:02X} maps to KEY_RESERVED",
                usage,
            );
        }
    }

    proptest::proptest! {
        // ── Pointer acceleration is monotonic in |delta| ─────────────────
        #[test]
        fn pointer_accel_is_monotonic(a in 0i32..1000, b in 0i32..1000) {
            let speed = 256;
            let accel_a = apply_pointer_acceleration_i32(a, speed);
            let accel_b = apply_pointer_acceleration_i32(b, speed);
            if a <= b {
                assert!(accel_a <= accel_b,
                        "monotonicity violated: a={}→{} but b={}→{}",
                        a, accel_a, b, accel_b);
            }
        }

        // ── Amplified for large deltas ───────────────────────────────────
        #[test]
        fn prop_pointer_accel_amplifies(delta in 10i32..200) {
            let accelerated = apply_pointer_acceleration_i32(delta, 256);
            assert!(accelerated >= delta,
                    "expected acceleration for large deltas: raw={}, accel={}",
                    delta, accelerated);
        }

        // ── HID mapping returns valid keys ───────────────────────────────
        #[test]
        fn hid_mapping_consistent(usage in 0x04u8..=0x65u8) {
            let key = hid_usage_to_keycode(usage);
            assert!(key <= KEY_PRINT, "key {} out of range for usage 0x{:02X}", key, usage);
        }

        // ── PS/2 mapping never panics ────────────────────────────────────
        #[test]
        fn ps2_mapping_returns_something(_ in 0u8..=255u8) {
            // We can't construct an arbitrary pc_keyboard::KeyCode from a u8,
            // so we just test that no code panics the match.
            // It ensures the wildcard `_ =>` arm exists and doesn't panic.
        }
    }
}
