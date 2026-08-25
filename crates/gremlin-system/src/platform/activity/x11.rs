//! Mesure d'inactivité Linux/X11 via l'extension `XScreenSaver`.

use std::env;
use std::ptr;
use std::time::Duration;

use x11_dl::{xlib, xss};

use super::IdleBackend;
use crate::error::SystemError;

pub fn default_backend() -> Result<Box<dyn IdleBackend>, SystemError> {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    validate_session(&session_type, env::var_os("DISPLAY").is_some())?;

    let xlib = xlib::Xlib::open().map_err(|error| {
        SystemError::ActivityUnavailable(format!("chargement de Xlib impossible : {error}"))
    })?;
    let xss = xss::Xss::open().map_err(|error| {
        SystemError::ActivityUnavailable(format!("chargement de XScreenSaver impossible : {error}"))
    })?;

    // SAFETY: le nom nul demande à Xlib d'utiliser DISPLAY. Le pointeur obtenu
    // est vérifié puis possédé exclusivement par ce backend jusqu'à `Drop`.
    #[allow(unsafe_code)]
    let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
    if display.is_null() {
        return Err(SystemError::ActivityUnavailable(
            "connexion au serveur X11 impossible".to_owned(),
        ));
    }

    // SAFETY: XScreenSaver alloue la structure, vérifiée ci-dessous et libérée
    // avec XFree dans `Drop`.
    #[allow(unsafe_code)]
    let info = unsafe { (xss.XScreenSaverAllocInfo)() };
    if info.is_null() {
        // SAFETY: `display` est une connexion Xlib valide possédée ici.
        #[allow(unsafe_code)]
        unsafe {
            (xlib.XCloseDisplay)(display);
        }
        return Err(SystemError::ActivityUnavailable(
            "allocation XScreenSaver impossible".to_owned(),
        ));
    }

    Ok(Box::new(X11IdleBackend {
        xlib,
        xss,
        display,
        info,
    }))
}

fn validate_session(session_type: &str, display_available: bool) -> Result<(), SystemError> {
    if session_type.eq_ignore_ascii_case("wayland") {
        return Err(SystemError::ActivityUnavailable(
            "Wayland n'expose pas de compteur global portable".to_owned(),
        ));
    }
    if !session_type.is_empty() && !session_type.eq_ignore_ascii_case("x11") {
        return Err(SystemError::ActivityUnavailable(format!(
            "session graphique non prise en charge ({session_type})"
        )));
    }
    if !display_available {
        return Err(SystemError::ActivityUnavailable(
            "variable DISPLAY absente".to_owned(),
        ));
    }
    Ok(())
}

struct X11IdleBackend {
    xlib: xlib::Xlib,
    xss: xss::Xss,
    display: *mut xlib::Display,
    info: *mut xss::XScreenSaverInfo,
}

impl IdleBackend for X11IdleBackend {
    fn idle_for(&mut self) -> Result<Duration, SystemError> {
        // SAFETY: la connexion, la fenêtre racine et la structure sont toutes
        // valides et utilisées exclusivement sur le thread du moniteur.
        #[allow(unsafe_code)]
        let status = unsafe {
            let root = (self.xlib.XDefaultRootWindow)(self.display);
            (self.xss.XScreenSaverQueryInfo)(self.display, root, self.info)
        };
        if status == 0 {
            return Err(SystemError::ActivityReadFailed(
                "XScreenSaverQueryInfo a échoué".to_owned(),
            ));
        }

        // SAFETY: `info` reste alloué et la requête réussie vient de remplir ses champs.
        #[allow(unsafe_code)]
        let idle_millis = unsafe { (*self.info).idle };
        Ok(Duration::from_millis(idle_millis))
    }
}

impl Drop for X11IdleBackend {
    fn drop(&mut self) {
        // SAFETY: les deux pointeurs ont été créés par ces bibliothèques, sont
        // encore valides et ne sont libérés qu'une fois, ici.
        #[allow(unsafe_code)]
        unsafe {
            (self.xlib.XFree)(self.info.cast());
            (self.xlib.XCloseDisplay)(self.display);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_session;

    #[test]
    fn test_wayland_and_headless_sessions_are_explicitly_unavailable() {
        assert!(validate_session("wayland", true).is_err());
        assert!(validate_session("x11", false).is_err());
        assert!(validate_session("", false).is_err());
        assert!(validate_session("x11", true).is_ok());
    }
}
