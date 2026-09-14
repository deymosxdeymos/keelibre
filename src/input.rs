use std::{
    io,
    os::fd::AsFd,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use evdev::{Device, EventSummary, KeyCode, enumerate};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use tokio::sync::mpsc;

use crate::sound::Phase;

#[derive(Clone, Copy, Debug)]
pub struct MonitorEvent {
    pub code: u16,
    pub phase: Phase,
}

pub struct InputMonitor {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl InputMonitor {
    pub fn start() -> io::Result<(Self, mpsc::Receiver<MonitorEvent>)> {
        let (sender, receiver) = mpsc::channel(256);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("keebyd-input".into())
            .spawn(move || monitor_devices(&worker_stop, &sender))?;
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

fn monitor_devices(stop: &AtomicBool, sender: &mpsc::Sender<MonitorEvent>) {
    while !stop.load(Ordering::Relaxed) {
        let mut devices = scan_devices();
        while !stop.load(Ordering::Relaxed) {
            let readiness = {
                let mut descriptors = devices
                    .iter()
                    .map(|device| PollFd::new(device.as_fd(), PollFlags::POLLIN))
                    .collect::<Vec<_>>();
                match poll(&mut descriptors, PollTimeout::from(500_u16)) {
                    Ok(_) => descriptors.iter().map(PollFd::revents).collect::<Vec<_>>(),
                    Err(error) => {
                        tracing::debug!(%error, "input polling failed");
                        break;
                    }
                }
            };
            let mut rescan = false;
            for (index, events) in readiness.into_iter().enumerate() {
                let Some(events) = events else {
                    continue;
                };
                if events.intersects(PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL)
                {
                    rescan = true;
                    break;
                }
                if !events.contains(PollFlags::POLLIN) {
                    continue;
                }
                let events = match devices[index].fetch_events() {
                    Ok(events) => events.collect::<Vec<_>>(),
                    Err(error) => {
                        tracing::debug!(%error, "input device disappeared");
                        rescan = true;
                        break;
                    }
                };
                for event in events {
                    let EventSummary::Key(_, key, value) = event.destructure() else {
                        continue;
                    };
                    let code = key.code();
                    if !is_sound_input(code) {
                        continue;
                    }
                    let phase = match value {
                        1 => Phase::Down,
                        0 => Phase::Up,
                        _ => continue,
                    };
                    if sender.blocking_send(MonitorEvent { code, phase }).is_err() {
                        return;
                    }
                }
            }
            if rescan {
                break;
            }
        }
    }
}

const fn is_sound_input(code: u16) -> bool {
    code < 0x100 || matches!(code, 272..=276)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_touch_contacts_but_keeps_mouse_clicks() {
        assert!(is_sound_input(KeyCode::KEY_A.code()));
        assert!(is_sound_input(KeyCode::BTN_LEFT.code()));
        assert!(is_sound_input(KeyCode::BTN_EXTRA.code()));
        assert!(!is_sound_input(KeyCode::BTN_TOUCH.code()));
        assert!(!is_sound_input(KeyCode::BTN_TOOL_FINGER.code()));
    }
}
