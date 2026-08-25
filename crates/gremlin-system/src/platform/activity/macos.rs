//! Mesure d'inactivité macOS via Quartz Event Services.

use std::time::Duration;

use super::IdleBackend;
use crate::error::SystemError;

const HID_SYSTEM_STATE: i32 = 1;
const ANY_INPUT_EVENT: u32 = u32::MAX;
const MAX_REPORTED_IDLE_SECS: f64 = 31_536_000.0;

#[link(name = "ApplicationServices", kind = "framework")]
#[allow(unsafe_code)]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(source_state_id: i32, event_type: u32) -> f64;
}

#[allow(clippy::unnecessary_wraps)]
pub fn default_backend() -> Result<Box<dyn IdleBackend>, SystemError> {
    Ok(Box::new(MacOsIdleBackend))
}

struct MacOsIdleBackend;

impl IdleBackend for MacOsIdleBackend {
    fn idle_for(&mut self) -> Result<Duration, SystemError> {
        // SAFETY: la fonction Quartz accepte les deux constantes documentées et
        // ne reçoit aucun pointeur dont Rust devrait garantir la validité.
        #[allow(unsafe_code)]
        let seconds =
            unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) };
        checked_duration(seconds)
    }
}

fn checked_duration(seconds: f64) -> Result<Duration, SystemError> {
    if !seconds.is_finite() || !(0.0..=MAX_REPORTED_IDLE_SECS).contains(&seconds) {
        return Err(SystemError::ActivityReadFailed(format!(
            "Quartz a renvoyé une durée invalide ({seconds})"
        )));
    }
    Ok(Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hostile_quartz_durations_are_rejected() {
        for value in [f64::NAN, f64::INFINITY, -1.0, MAX_REPORTED_IDLE_SECS + 1.0] {
            assert!(checked_duration(value).is_err());
        }
        assert_eq!(
            checked_duration(2.5).ok(),
            Some(Duration::from_millis(2_500))
        );
    }
}
