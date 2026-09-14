use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use evdev::{Device, EventSummary, KeyCode, enumerate};
use tokio::sync::mpsc;

use crate::sound::Phase;

const RESCAN_INTERVAL: Duration = Duration::from_secs(2);
const IDLE_INTERVAL: Duration = Duration::from_millis(4);
const TAP_WINDOW: Duration = Duration::from_millis(800);
const TAP_GAP: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug)]
pub enum MonitorEvent {
    Key { code: u16, phase: Phase },
    ToggleMute,
}

#[derive(Clone, Copy, Debug)]
pub struct Hotkey {
    pub key: u16,
    pub taps: usize,
    pub ctrl: bool,
}

pub struct InputMonitor {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl InputMonitor {
    pub fn start(hotkey: Hotkey) -> io::Result<(Self, mpsc::Receiver<MonitorEvent>)> {
        let (sender, receiver) = mpsc::channel(256);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("keebyd-input".into())
            .spawn(move || {
                let mut hotkey_state = HotkeyState::default();
                while !worker_stop.load(Ordering::Relaxed) {
                    let mut devices = scan_devices();
                    let deadline = Instant::now() + RESCAN_INTERVAL;
                    while Instant::now() < deadline && !worker_stop.load(Ordering::Relaxed) {
                        for device in &mut devices {
                            let events = match device.fetch_events() {
                                Ok(events) => events.collect::<Vec<_>>(),
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                                Err(error) => {
                                    tracing::debug!(%error, "input device disappeared");
                                    break;
                                }
                            };
                            for event in events {
                                let EventSummary::Key(_, key, value) = event.destructure() else {
                                    continue;
                                };
                                let phase = match value {
                                    1 => Phase::Down,
                                    0 => Phase::Up,
                                    _ => continue,
                                };
                                if hotkey_state.accept(&hotkey, key.code(), phase) {
                                    let _ = sender.blocking_send(MonitorEvent::ToggleMute);
                                }
                                if sender
                                    .blocking_send(MonitorEvent::Key {
                                        code: key.code(),
                                        phase,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                        thread::sleep(IDLE_INTERVAL);
                    }
                }
            })?;
        Ok((
            Self {
                stop,
                worker: Some(worker),
            },
            receiver,
        ))
    }
}

impl Drop for InputMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[must_use]
pub fn devices() -> Vec<(String, String)> {
    enumerate()
        .map(|(path, device)| {
            (
                path.display().to_string(),
                device.name().unwrap_or("unnamed").to_owned(),
            )
        })
        .collect()
}

fn scan_devices() -> Vec<Device> {
    let devices = enumerate()
        .filter_map(|(path, device)| {
            if !is_keyboard_or_mouse(&device) {
                return None;
            }
            if let Err(error) = device.set_nonblocking(true) {
                tracing::warn!(path = %path.display(), %error, "could not monitor input device");
                return None;
            }
            Some(device)
        })
        .collect::<Vec<_>>();
    tracing::debug!(count = devices.len(), "input devices scanned");
    devices
}

fn is_keyboard_or_mouse(device: &Device) -> bool {
    let Some(keys) = device.supported_keys() else {
        return false;
    };
    let alpha_count = [
        KeyCode::KEY_Q,
        KeyCode::KEY_W,
        KeyCode::KEY_E,
        KeyCode::KEY_R,
        KeyCode::KEY_T,
        KeyCode::KEY_Y,
        KeyCode::KEY_U,
        KeyCode::KEY_I,
        KeyCode::KEY_O,
        KeyCode::KEY_P,
        KeyCode::KEY_A,
        KeyCode::KEY_S,
        KeyCode::KEY_D,
        KeyCode::KEY_F,
        KeyCode::KEY_G,
        KeyCode::KEY_H,
        KeyCode::KEY_J,
        KeyCode::KEY_K,
        KeyCode::KEY_L,
    ]
    .into_iter()
    .filter(|key| keys.contains(*key))
    .count();
    let keyboard = alpha_count >= 10 && keys.contains(KeyCode::KEY_SPACE);
    let mouse = keys.contains(KeyCode::BTN_LEFT) && !keys.contains(KeyCode::KEY_A);
    keyboard || mouse
}

#[derive(Default)]
struct HotkeyState {
    ctrl_down: bool,
    taps: VecDeque<Instant>,
}

impl HotkeyState {
    fn accept(&mut self, hotkey: &Hotkey, code: u16, phase: Phase) -> bool {
        if code == KeyCode::KEY_LEFTCTRL.code() || code == KeyCode::KEY_RIGHTCTRL.code() {
            self.ctrl_down = matches!(phase, Phase::Down);
            return false;
        }
        if code != hotkey.key || !matches!(phase, Phase::Down) || hotkey.ctrl && !self.ctrl_down {
            return false;
        }
        let now = Instant::now();
        if self
            .taps
            .back()
            .is_some_and(|last| now.duration_since(*last) > TAP_GAP)
        {
            self.taps.clear();
        }
        self.taps.push_back(now);
        let taps = hotkey.taps.max(1);
        while self.taps.len() > taps {
            self.taps.pop_front();
        }
        let matched = self.taps.len() == taps
            && now.duration_since(*self.taps.front().expect("tap exists")) <= TAP_WINDOW;
        if matched {
            self.taps.clear();
        }
        matched
    }
}
