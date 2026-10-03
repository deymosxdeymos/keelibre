use std::{
    collections::HashSet,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use rdev::{Button, EventType, Key};
use tokio::sync::mpsc;

use super::MonitorEvent;
use crate::sound::Phase;

pub struct InputMonitor {
    stop: Arc<AtomicBool>,
}

impl InputMonitor {
    pub fn start() -> io::Result<(Self, mpsc::Receiver<MonitorEvent>)> {
        let (sender, receiver) = mpsc::channel(256);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        // rdev has no stop API. This one listener lives until process exit;
        // dropping the monitor disables forwarding without joining its run loop.
        thread::Builder::new()
            .name("keebyd-input".into())
            .spawn(move || {
                let mut pressed = HashSet::new();
                if let Err(error) = rdev::listen(move |event| {
                    if stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Some(event) = transition(event.event_type, &mut pressed) {
                        // Never wait inside a native OS hook.
                        let _ = sender.try_send(event);
                    }
                }) {
                    tracing::error!(?error, "input listener failed; check OS input permissions");
                }
            })?;
        #[cfg(target_os = "macos")]
        tracing::info!(
            "enable Keebyd in System Settings > Privacy & Security > Accessibility and Input Monitoring, then restart"
        );
        Ok((Self { stop }, receiver))
    }
}

impl Drop for InputMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[must_use]
pub fn devices() -> Vec<(String, String)> {
    vec![(
        "system".into(),
        "Global keyboard and mouse hook (individual devices are not exposed)".into(),
    )]
}

fn transition(event: EventType, pressed: &mut HashSet<u16>) -> Option<MonitorEvent> {
    let (code, phase) = match event {
        EventType::KeyPress(key) => (key_code(key)?, Phase::Down),
        EventType::KeyRelease(key) => (key_code(key)?, Phase::Up),
        EventType::ButtonPress(button) => (button_code(button)?, Phase::Down),
        EventType::ButtonRelease(button) => (button_code(button)?, Phase::Up),
        EventType::MouseMove { .. } | EventType::Wheel { .. } => return None,
    };
    let changed = match phase {
        Phase::Down => pressed.insert(code),
        Phase::Up => pressed.remove(&code),
    };
    changed.then_some(MonitorEvent { code, phase })
}

const fn button_code(button: Button) -> Option<u16> {
    match button {
        Button::Left => Some(272),
        Button::Right => Some(273),
        Button::Middle => Some(274),
        Button::Unknown(_) => None,
    }
}

// Native unknown codes are intentionally not treated as portable keycodes.
const fn key_code(key: Key) -> Option<u16> {
    Some(match key {
        Key::Escape => 1,
        Key::Num1 => 2,
        Key::Num2 => 3,
        Key::Num3 => 4,
        Key::Num4 => 5,
        Key::Num5 => 6,
        Key::Num6 => 7,
        Key::Num7 => 8,
        Key::Num8 => 9,
        Key::Num9 => 10,
        Key::Num0 => 11,
        Key::Minus => 12,
        Key::Equal => 13,
        Key::Backspace => 14,
        Key::Tab => 15,
        Key::KeyQ => 16,
        Key::KeyW => 17,
        Key::KeyE => 18,
        Key::KeyR => 19,
        Key::KeyT => 20,
        Key::KeyY => 21,
        Key::KeyU => 22,
        Key::KeyI => 23,
        Key::KeyO => 24,
        Key::KeyP => 25,
        Key::LeftBracket => 26,
        Key::RightBracket => 27,
        Key::Return => 28,
        Key::ControlLeft => 29,
        Key::KeyA => 30,
        Key::KeyS => 31,
        Key::KeyD => 32,
        Key::KeyF => 33,
        Key::KeyG => 34,
        Key::KeyH => 35,
        Key::KeyJ => 36,
        Key::KeyK => 37,
        Key::KeyL => 38,
        Key::SemiColon => 39,
        Key::Quote => 40,
        Key::BackQuote => 41,
        Key::ShiftLeft => 42,
        Key::BackSlash => 43,
        Key::KeyZ => 44,
        Key::KeyX => 45,
        Key::KeyC => 46,
        Key::KeyV => 47,
        Key::KeyB => 48,
        Key::KeyN => 49,
        Key::KeyM => 50,
        Key::Comma => 51,
        Key::Dot => 52,
        Key::Slash => 53,
        Key::ShiftRight => 54,
        Key::KpMultiply => 55,
        Key::Alt => 56,
        Key::Space => 57,
        Key::CapsLock => 58,
        Key::F1 => 59,
        Key::F2 => 60,
        Key::F3 => 61,
        Key::F4 => 62,
        Key::F5 => 63,
        Key::F6 => 64,
        Key::F7 => 65,
        Key::F8 => 66,
        Key::F9 => 67,
        Key::F10 => 68,
        Key::NumLock => 69,
        Key::ScrollLock => 70,
        Key::Kp7 => 71,
        Key::Kp8 => 72,
        Key::Kp9 => 73,
        Key::KpMinus => 74,
        Key::Kp4 => 75,
        Key::Kp5 => 76,
        Key::Kp6 => 77,
        Key::KpPlus => 78,
        Key::Kp1 => 79,
        Key::Kp2 => 80,
        Key::Kp3 => 81,
        Key::Kp0 => 82,
        Key::KpDelete => 83,
        Key::IntlBackslash => 86,
        Key::F11 => 87,
        Key::F12 => 88,
        Key::KpReturn => 96,
        Key::ControlRight => 97,
        Key::KpDivide => 98,
        Key::PrintScreen => 99,
        Key::AltGr => 100,
        Key::Home => 102,
        Key::UpArrow => 103,
        Key::PageUp => 104,
        Key::LeftArrow => 105,
        Key::RightArrow => 106,
        Key::End => 107,
        Key::DownArrow => 108,
        Key::PageDown => 109,
        Key::Insert => 110,
        Key::Delete => 111,
        Key::Pause => 119,
        Key::MetaLeft => 125,
        Key::MetaRight => 126,
        Key::Function => 464,
        Key::Unknown(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_are_suppressed_but_repress_is_delivered() {
        let mut pressed = HashSet::new();
        let events = [
            EventType::KeyPress(Key::KeyA),
            EventType::KeyPress(Key::KeyA),
            EventType::KeyPress(Key::KeyL),
            EventType::KeyRelease(Key::KeyA),
            EventType::KeyPress(Key::KeyA),
        ];
        let actual: Vec<_> = events
            .into_iter()
            .filter_map(|event| transition(event, &mut pressed))
            .map(|event| (event.code, event.phase))
            .collect();
        assert_eq!(
            actual,
            [
                (30, Phase::Down),
                (38, Phase::Down),
                (30, Phase::Up),
                (30, Phase::Down)
            ]
        );
    }

    #[test]
    fn native_codes_do_not_leak_into_shared_mapping() {
        assert_eq!(key_code(Key::Unknown(28)), None);
        assert_eq!(key_code(Key::KpReturn), Some(96));
        assert_eq!(key_code(Key::ControlRight), Some(97));
        assert_eq!(button_code(Button::Right), Some(273));
        assert_eq!(button_code(Button::Unknown(1)), None);
    }
}
