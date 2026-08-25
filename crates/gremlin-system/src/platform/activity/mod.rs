//! Backend natif de mesure du temps écoulé depuis la dernière saisie.

use std::time::Duration;

use crate::error::SystemError;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod x11;

pub trait IdleBackend {
    fn idle_for(&mut self) -> Result<Duration, SystemError>;
}

#[cfg(target_os = "macos")]
pub use macos::default_backend;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub use unsupported::default_backend;
#[cfg(target_os = "windows")]
pub use windows::default_backend;
#[cfg(target_os = "linux")]
pub use x11::default_backend;
