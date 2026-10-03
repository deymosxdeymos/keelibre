//! OS adapters emit physical keys in the shared evdev numbering scheme.

use crate::sound::Phase;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorEvent {
    pub code: u16,
    pub phase: Phase,
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{InputMonitor, devices};

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod native;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use native::{InputMonitor, devices};
