use std::{
    collections::HashMap,
    io,
    os::fd::AsFd,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
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
    let mut devices = HashMap::new();
    scan_devices(&mut devices);
    let mut last_scan = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if last_scan.elapsed() >= Duration::from_secs(2) {
            scan_devices(&mut devices);
            last_scan = Instant::now();
        }
        let readiness = {
            let mut descriptors = devices
                .values()
                .map(|device| PollFd::new(device.as_fd(), PollFlags::POLLIN))
                .collect::<Vec<_>>();
            match poll(&mut descriptors, PollTimeout::from(500_u16)) {
                Ok(_) => Some(descriptors.iter().map(PollFd::revents).collect::<Vec<_>>()),
                Err(error) => {
                    tracing::debug!(%error, "input polling failed");
                    None
                }
            }
        };
        let Some(readiness) = readiness else {
            devices.clear();
            thread::sleep(Duration::from_millis(500));
            scan_devices(&mut devices);
            last_scan = Instant::now();
            continue;
        };
        let mut failed = Vec::new();
        for ((path, device), events) in devices.iter_mut().zip(readiness) {
            let Some(events) = events else { continue };
            if events.intersects(PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL) {
                failed.push(path.clone());
                continue;
            }
            if !events.contains(PollFlags::POLLIN) {
                continue;
            }
            match device.fetch_events() {
                Ok(events) => {
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
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => {
                    tracing::debug!(%error, "input device disappeared");
                    failed.push(path.clone());
                }
            }
        }
        if !failed.is_empty() {
            for path in failed {
                devices.remove(&path);
            }
            scan_devices(&mut devices);
            last_scan = Instant::now();
        }
    }
}

const fn is_sound_input(code: u16) -> bool {
    code < 0x100 || matches!(code, 272..=276)
}

fn scan_devices(devices: &mut HashMap<PathBuf, Device>) {
    let mut seen = std::collections::HashSet::new();
    for (path, device) in enumerate() {
        if !is_keyboard_or_mouse(&device) {
            continue;
        }
        seen.insert(path.clone());
        if devices.contains_key(&path) {
            continue;
        }
        if let Err(error) = device.set_nonblocking(true) {
            tracing::warn!(path = %path.display(), %error, "could not monitor input device");
            continue;
        }
        devices.insert(path, device);
    }
    devices.retain(|path, _| seen.contains(path));
    tracing::debug!(count = devices.len(), "input devices scanned");
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
