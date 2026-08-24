//! Mesure d'inactivité Windows via `GetLastInputInfo`.

use std::mem::size_of;
use std::time::Duration;

use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

use super::IdleBackend;
use crate::error::SystemError;

#[allow(clippy::unnecessary_wraps)]
pub fn default_backend() -> Result<Box<dyn IdleBackend>, SystemError> {
    Ok(Box::new(WindowsIdleBackend))
}

struct WindowsIdleBackend;

impl IdleBackend for WindowsIdleBackend {
    fn idle_for(&mut self) -> Result<Duration, SystemError> {
        let cb_size = u32::try_from(size_of::<LASTINPUTINFO>()).map_err(|error| {
            SystemError::ActivityReadFailed(format!("taille LASTINPUTINFO invalide : {error}"))
        })?;
        let mut info = LASTINPUTINFO {
            cbSize: cb_size,
            dwTime: 0,
        };

        // SAFETY: `info` pointe vers une structure initialisée avec la taille
        // exigée par Win32 et reste valide pendant toute la durée de l'appel.
        #[allow(unsafe_code)]
        let succeeded = unsafe { GetLastInputInfo(&raw mut info) } != 0;
        if !succeeded {
            return Err(SystemError::ActivityReadFailed(
                "GetLastInputInfo a échoué".to_owned(),
            ));
        }

        // SAFETY: `GetTickCount` ne prend aucun pointeur et n'impose aucun invariant.
        #[allow(unsafe_code)]
        let now = unsafe { GetTickCount() };
        Ok(Duration::from_millis(idle_millis(now, info.dwTime)))
    }
}

fn idle_millis(now: u32, last_input: u32) -> u64 {
    u64::from(now.wrapping_sub(last_input))
}

#[cfg(test)]
mod tests {
    use super::idle_millis;

    #[test]
    fn test_tick_counter_wrap_is_handled() {
        assert_eq!(idle_millis(25, u32::MAX - 24), 50);
    }
}
