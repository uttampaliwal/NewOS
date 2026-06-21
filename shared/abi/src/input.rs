//! Input event types shared between kernel and userspace.
//!
//! Events use an evdev-inspired three-field structure (`kind`, `code`, `value`)
//! so that userspace can read them with a single `input_read` syscall.

use core::fmt;

// ── Event kinds ───────────────────────────────────────────────────────────────

pub const INPUT_KIND_SYN: u16 = 0; // synchronisation / separator
pub const INPUT_KIND_KEY: u16 = 1; // key press / release / repeat
pub const INPUT_KIND_REL: u16 = 2; // relative axis (pointer motion, wheel)

// ── Relative axes (code field for INPUT_KIND_REL) ─────────────────────────────

pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const REL_WHEEL: u16 = 0x08;
pub const REL_HWHEEL: u16 = 0x0a;

// ── Key codes (code field for INPUT_KIND_KEY) ─────────────────────────────────
// These are loosely based on Linux KEY_* constants from input-event-codes.h.

pub const KEY_RESERVED: u16 = 0;
pub const KEY_ESC: u16 = 1;
pub const KEY_1: u16 = 2;
pub const KEY_2: u16 = 3;
pub const KEY_3: u16 = 4;
pub const KEY_4: u16 = 5;
pub const KEY_5: u16 = 6;
pub const KEY_6: u16 = 7;
pub const KEY_7: u16 = 8;
pub const KEY_8: u16 = 9;
pub const KEY_9: u16 = 10;
pub const KEY_0: u16 = 11;
pub const KEY_MINUS: u16 = 12;
pub const KEY_EQUAL: u16 = 13;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_Q: u16 = 16;
pub const KEY_W: u16 = 17;
pub const KEY_E: u16 = 18;
pub const KEY_R: u16 = 19;
pub const KEY_T: u16 = 20;
pub const KEY_Y: u16 = 21;
pub const KEY_U: u16 = 22;
pub const KEY_I: u16 = 23;
pub const KEY_O: u16 = 24;
pub const KEY_P: u16 = 25;
pub const KEY_LBRACE: u16 = 26;
pub const KEY_RBRACE: u16 = 27;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LCTRL: u16 = 29;
pub const KEY_A: u16 = 30;
pub const KEY_S: u16 = 31;
pub const KEY_D: u16 = 32;
pub const KEY_F: u16 = 33;
pub const KEY_G: u16 = 34;
pub const KEY_H: u16 = 35;
pub const KEY_J: u16 = 36;
pub const KEY_K: u16 = 37;
pub const KEY_L: u16 = 38;
pub const KEY_SEMICOLON: u16 = 39;
pub const KEY_QUOTE: u16 = 40;
pub const KEY_BACKTICK: u16 = 41;
pub const KEY_LSHIFT: u16 = 42;
pub const KEY_BACKSLASH: u16 = 43;
pub const KEY_Z: u16 = 44;
pub const KEY_X: u16 = 45;
pub const KEY_C: u16 = 46;
pub const KEY_V: u16 = 47;
pub const KEY_B: u16 = 48;
pub const KEY_N: u16 = 49;
pub const KEY_M: u16 = 50;
pub const KEY_COMMA: u16 = 51;
pub const KEY_DOT: u16 = 52;
pub const KEY_SLASH: u16 = 53;
pub const KEY_RSHIFT: u16 = 54;
pub const KEY_KP_ASTERISK: u16 = 55;
pub const KEY_LALT: u16 = 56;
pub const KEY_SPACE: u16 = 57;
pub const KEY_CAPSLOCK: u16 = 58;
pub const KEY_F1: u16 = 59;
pub const KEY_F2: u16 = 60;
pub const KEY_F3: u16 = 61;
pub const KEY_F4: u16 = 62;
pub const KEY_F5: u16 = 63;
pub const KEY_F6: u16 = 64;
pub const KEY_F7: u16 = 65;
pub const KEY_F8: u16 = 66;
pub const KEY_F9: u16 = 67;
pub const KEY_F10: u16 = 68;
pub const KEY_NUMLOCK: u16 = 69;
pub const KEY_SCROLLLOCK: u16 = 70;
pub const KEY_KP7: u16 = 71;
pub const KEY_KP8: u16 = 72;
pub const KEY_KP9: u16 = 73;
pub const KEY_KP_MINUS: u16 = 74;
pub const KEY_KP4: u16 = 75;
pub const KEY_KP5: u16 = 76;
pub const KEY_KP6: u16 = 77;
pub const KEY_KP_PLUS: u16 = 78;
pub const KEY_KP1: u16 = 79;
pub const KEY_KP2: u16 = 80;
pub const KEY_KP3: u16 = 81;
pub const KEY_KP0: u16 = 82;
pub const KEY_KP_DOT: u16 = 83;
pub const KEY_F11: u16 = 87;
pub const KEY_F12: u16 = 88;
pub const KEY_KP_ENTER: u16 = 96;
pub const KEY_RCTRL: u16 = 97;
pub const KEY_KP_DIVIDE: u16 = 98;
pub const KEY_RALT: u16 = 100;
pub const KEY_HOME: u16 = 102;
pub const KEY_UP: u16 = 103;
pub const KEY_PAGEUP: u16 = 104;
pub const KEY_LEFT: u16 = 105;
pub const KEY_RIGHT: u16 = 106;
pub const KEY_END: u16 = 107;
pub const KEY_DOWN: u16 = 108;
pub const KEY_PAGEDOWN: u16 = 109;
pub const KEY_INSERT: u16 = 110;
pub const KEY_DELETE: u16 = 111;
pub const KEY_MUTE: u16 = 113;
pub const KEY_VOLUMEDOWN: u16 = 114;
pub const KEY_VOLUMEUP: u16 = 115;
pub const KEY_POWER: u16 = 116;
pub const KEY_KP_EQUAL: u16 = 117;
pub const KEY_PAUSE: u16 = 119;
pub const KEY_KP_COMMA: u16 = 121;
pub const KEY_LWIN: u16 = 125;
pub const KEY_RWIN: u16 = 126;
pub const KEY_MENU: u16 = 127;
pub const KEY_PRINT: u16 = 129;

// ── Event value constants ────────────────────────────────────────────────────

pub const KEY_STATE_RELEASED: i32 = 0;
pub const KEY_STATE_PRESSED: i32 = 1;
pub const KEY_STATE_REPEAT: i32 = 2;

// ── Fixed-size input event (8 bytes) ─────────────────────────────────────────

/// A single input event, modelled after Linux input_event.
///
/// Layout
/// ──────
/// ```text
/// offset  size  field
///  0       2    kind   – INPUT_KIND_*  (SYN, KEY, REL)
///  2       2    code   – KEY_*, REL_*, …
///  4       4    value  – key state / delta / …
/// ```
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InputEvent {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

impl InputEvent {
    pub const fn new(kind: u16, code: u16, value: i32) -> Self {
        Self { kind, code, value }
    }

    /// Synchronisation marker that separates a batch of events.
    pub const fn syn() -> Self {
        Self::new(INPUT_KIND_SYN, 0, 0)
    }

    /// Key press event.
    pub const fn key_press(code: u16) -> Self {
        Self::new(INPUT_KIND_KEY, code, KEY_STATE_PRESSED)
    }

    /// Key release event.
    pub const fn key_release(code: u16) -> Self {
        Self::new(INPUT_KIND_KEY, code, KEY_STATE_RELEASED)
    }

    /// Relative pointer motion.
    pub const fn rel_motion(axis: u16, delta: i32) -> Self {
        Self::new(INPUT_KIND_REL, axis, delta)
    }
}

impl fmt::Debug for InputEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind_str = match self.kind {
            INPUT_KIND_SYN => "SYN",
            INPUT_KIND_KEY => "KEY",
            INPUT_KIND_REL => "REL",
            _ => "?",
        };
        let code = self.code;
        let value = self.value;
        f.debug_struct("InputEvent")
            .field("kind", &kind_str)
            .field("code", &code)
            .field("value", &value)
            .finish()
    }
}

impl PartialEq for InputEvent {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.code == other.code && self.value == other.value
    }
}

impl Eq for InputEvent {}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn input_event_size() {
        assert_eq!(size_of::<InputEvent>(), 8);
    }

    #[test]
    fn syn_event() {
        let e = InputEvent::syn();
        assert_eq!(e.kind, INPUT_KIND_SYN);
        assert_eq!(e.code, 0);
        assert_eq!(e.value, 0);
    }

    #[test]
    fn key_press_event() {
        let e = InputEvent::key_press(KEY_A);
        assert_eq!(e.kind, INPUT_KIND_KEY);
        assert_eq!(e.code, KEY_A);
        assert_eq!(e.value, KEY_STATE_PRESSED);
    }

    #[test]
    fn key_release_event() {
        let e = InputEvent::key_release(KEY_ENTER);
        assert_eq!(e.kind, INPUT_KIND_KEY);
        assert_eq!(e.code, KEY_ENTER);
        assert_eq!(e.value, KEY_STATE_RELEASED);
    }

    #[test]
    fn rel_motion_event() {
        let e = InputEvent::rel_motion(REL_X, -5);
        assert_eq!(e.kind, INPUT_KIND_REL);
        assert_eq!(e.code, REL_X);
        assert_eq!(e.value, -5);
    }

    #[test]
    fn constant_consistency() {
        // Spot-check some key code constants.
        assert_eq!(KEY_ESC, 1);
        assert_eq!(KEY_A, 30);
        assert_eq!(KEY_ENTER, 28);
        assert_eq!(KEY_SPACE, 57);
        assert_eq!(KEY_LSHIFT, 42);
        assert_eq!(KEY_UP, 103);
        assert_eq!(KEY_F1, 59);
        assert_eq!(KEY_F12, 88);
        assert_eq!(KEY_PRINT, 129);
    }
}
