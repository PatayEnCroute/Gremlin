//! Zone de travail Windows via `GetMonitorInfoW`.
//!
//! `MONITORINFO::rcWork` est la seule source fiable de la zone amputée de la
//! barre des tâches : elle suit sa position (bas, haut, côté), son masquage
//! automatique et les barres d'outils tierces enregistrées comme appbars.

use crate::desktop_layout::{PhysicalRect, WorkAreaSource};
use crate::error::SystemError;
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONULL,
};

/// Construit la source Windows.
///
/// Le `Result` est imposé par la signature commune aux plateformes : X11 et les
/// systèmes non couverts, eux, ont de vraies raisons d'échouer.
#[allow(clippy::unnecessary_wraps)]
pub(super) fn source() -> Result<Box<dyn WorkAreaSource + Send + Sync>, SystemError> {
    Ok(Box::new(WindowsWorkArea))
}

/// Source adossée à l'API moniteur de l'interface graphique Windows.
struct WindowsWorkArea;

impl WorkAreaSource for WindowsWorkArea {
    fn work_area(&self, bounds: PhysicalRect) -> Result<PhysicalRect, SystemError> {
        // Le centre du moniteur désigne celui-ci sans ambiguïté, y compris en
        // coordonnées négatives (écran à gauche ou au-dessus du principal).
        let (center_x, center_y) = bounds.center();
        let point = POINT {
            x: center_x,
            y: center_y,
        };

        // SAFETY: `POINT` est passé par valeur et l'appel ne fait que localiser un
        // moniteur. `MONITOR_DEFAULTTONULL` demande explicitement un pointeur nul
        // plutôt qu'un moniteur voisin : sans quoi un rectangle hors écran
        // renverrait la zone de travail d'un moniteur qui n'est pas le sien.
        #[allow(unsafe_code)]
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) };
        if monitor.is_null() {
            return Err(SystemError::DesktopLayoutReadFailed(format!(
                "aucun moniteur au point ({center_x}, {center_y})"
            )));
        }

        let mut info = MONITORINFO {
            // `cbSize` doit être renseigné avant l'appel : c'est ainsi que l'API
            // distingue `MONITORINFO` de `MONITORINFOEXW`.
            cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(0),
            ..unsafe_zeroed_monitor_info()
        };

        // SAFETY: `monitor` vient d'être obtenu et validé non nul ; `info` est une
        // structure locale entièrement initialisée dont `cbSize` annonce la taille
        // réelle. L'API n'écrit que dans ces octets.
        #[allow(unsafe_code)]
        let ok = unsafe { GetMonitorInfoW(monitor, std::ptr::addr_of_mut!(info)) };
        if ok == 0 {
            return Err(SystemError::DesktopLayoutReadFailed(String::from(
                "GetMonitorInfoW a échoué",
            )));
        }

        rect_from_win32(
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right,
            info.rcWork.bottom,
        )
    }
}

/// `MONITORINFO` entièrement à zéro, sans champ oublié si l'API évolue.
fn unsafe_zeroed_monitor_info() -> MONITORINFO {
    // SAFETY: `MONITORINFO` est un agrégat de types entiers et de `RECT`, sans
    // pointeur ni référence ni invariant de validité : le motif tout-à-zéro en est
    // une valeur licite. Les champs utiles sont écrasés juste après.
    #[allow(unsafe_code)]
    unsafe {
        std::mem::zeroed()
    }
}

/// Convertit un `RECT` Win32 (bords inclus/exclus) en rectangle du domaine.
///
/// Extraite pour être testable sans appel système : c'est là que se joue le
/// passage de deux coins à une origine et une taille.
fn rect_from_win32(
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> Result<PhysicalRect, SystemError> {
    let width = i64::from(right) - i64::from(left);
    let height = i64::from(bottom) - i64::from(top);
    if width <= 0 || height <= 0 {
        return Err(SystemError::DesktopLayoutReadFailed(format!(
            "zone de travail dégénérée : ({left}, {top}) - ({right}, {bottom})"
        )));
    }

    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(SystemError::DesktopLayoutReadFailed(String::from(
            "zone de travail hors bornes",
        )));
    };

    // Le rectangle est soumis aux mêmes bornes que le reste du domaine, ici
    // plutôt qu'en aval : l'erreur désigne alors l'appel natif fautif.
    let rect = PhysicalRect::new(left, top, width, height);
    if !rect.is_valid() {
        return Err(SystemError::DesktopLayoutReadFailed(format!(
            "zone de travail invraisemblable : {width}×{height}"
        )));
    }
    Ok(rect)
}

#[cfg(test)]
mod tests {
    use super::rect_from_win32;
    use crate::desktop_layout::PhysicalRect;

    #[test]
    fn test_win32_rect_becomes_origin_and_size() {
        let rect = rect_from_win32(0, 0, 1920, 1040);
        assert_eq!(rect.ok(), Some(PhysicalRect::new(0, 0, 1920, 1040)));
    }

    #[test]
    fn test_negative_origins_are_preserved() {
        let rect = rect_from_win32(-2560, -200, 0, 1240);
        assert_eq!(rect.ok(), Some(PhysicalRect::new(-2560, -200, 2560, 1440)));
    }

    #[test]
    fn test_degenerate_and_inverted_rects_are_refused() {
        assert!(rect_from_win32(100, 0, 100, 100).is_err());
        assert!(rect_from_win32(0, 100, 100, 100).is_err());
        assert!(rect_from_win32(200, 0, 100, 100).is_err());
    }

    #[test]
    fn test_extreme_coordinates_do_not_overflow() {
        // La soustraction se fait en `i64` : ce cas déborderait en `i32`.
        let rect = rect_from_win32(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        assert!(
            rect.is_err(),
            "une largeur supérieure à u32::MAX doit être refusée"
        );
    }
}
