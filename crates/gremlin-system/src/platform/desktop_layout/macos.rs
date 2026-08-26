//! Zone de travail macOS via `NSScreen::visibleFrame`.
//!
//! Deux conversions sont indispensables et faciles à manquer :
//!
//! * **origine** — AppKit place l'origine en bas à gauche de l'écran principal et
//!   fait croître `y` vers le haut ; le reste de Gremlin raisonne en haut à
//!   gauche avec `y` croissant vers le bas. Le retournement se fait autour du
//!   bord haut de l'écran **principal**, pas de l'écran courant, sans quoi un
//!   moniteur placé au-dessus se retrouverait en dessous ;
//! * **unité** — `frame` et `visibleFrame` sont en points logiques ; les limites
//!   fournies par `winit` sont en pixels physiques. Le rapport
//!   `backingScaleFactor` fait la conversion.

use crate::desktop_layout::{PhysicalRect, WorkAreaSource};
use crate::error::SystemError;
use objc2_app_kit::NSScreen;
use objc2_foundation::MainThreadMarker;

/// Écart maximal toléré, en pixels, entre les limites `winit` et un cadre AppKit.
///
/// Les deux piles arrondissent différemment le produit points × échelle ; exiger
/// l'égalité stricte ferait échouer l'appariement sur un écran à 1,5×.
const MATCH_TOLERANCE_PX: i64 = 4;

/// Construit la source macOS.
///
/// Le `Result` est imposé par la signature commune aux plateformes : X11 et les
/// systèmes non couverts, eux, ont de vraies raisons d'échouer.
#[allow(clippy::unnecessary_wraps)]
pub(super) fn source() -> Result<Box<dyn WorkAreaSource + Send + Sync>, SystemError> {
    Ok(Box::new(MacWorkArea))
}

/// Source adossée à AppKit.
struct MacWorkArea;

impl WorkAreaSource for MacWorkArea {
    fn work_area(&self, bounds: PhysicalRect) -> Result<PhysicalRect, SystemError> {
        // `NSScreen` n'est interrogeable que depuis le thread principal ; le
        // fournisseur est appelé depuis la boucle d'événements, qui y vit.
        let Some(marker) = MainThreadMarker::new() else {
            return Err(SystemError::DesktopLayoutReadFailed(String::from(
                "NSScreen n'est interrogeable que depuis le thread principal",
            )));
        };

        let screens = NSScreen::screens(marker);
        let Some(primary) = screens.iter().next() else {
            return Err(SystemError::DesktopLayoutReadFailed(String::from(
                "aucun écran rapporté par AppKit",
            )));
        };
        // Le premier écran de la liste porte l'origine du repère AppKit.
        let flip_origin = primary.frame().origin.y + primary.frame().size.height;

        for screen in &screens {
            let scale = screen.backingScaleFactor();
            let frame = to_physical(
                screen.frame().origin.x,
                screen.frame().origin.y,
                screen.frame().size.width,
                screen.frame().size.height,
                flip_origin,
                scale,
            );
            let Some(frame) = frame else {
                continue;
            };
            if !is_same_display(frame, bounds) {
                continue;
            }

            let visible = to_physical(
                screen.visibleFrame().origin.x,
                screen.visibleFrame().origin.y,
                screen.visibleFrame().size.width,
                screen.visibleFrame().size.height,
                flip_origin,
                scale,
            );
            return visible.ok_or_else(|| {
                SystemError::DesktopLayoutReadFailed(String::from(
                    "visibleFrame hors des bornes représentables",
                ))
            });
        }

        Err(SystemError::DesktopLayoutReadFailed(String::from(
            "aucun NSScreen ne correspond aux limites rapportées",
        )))
    }
}

/// Convertit un cadre AppKit en rectangle physique à origine haut-gauche.
///
/// Extraite pour être testable sans AppKit : c'est ici que se joue le
/// retournement vertical et la mise à l'échelle.
fn to_physical(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    flip_origin: f64,
    scale: f64,
) -> Option<PhysicalRect> {
    if ![x, y, width, height, flip_origin, scale]
        .iter()
        .all(|value| value.is_finite())
        || scale <= 0.0
        || width <= 0.0
        || height <= 0.0
    {
        return None;
    }

    // Retournement : le bord *haut* du cadre, mesuré depuis le haut du repère.
    let top = (flip_origin - (y + height)) * scale;
    let left = x * scale;
    let scaled_width = width * scale;
    let scaled_height = height * scale;

    let (Ok(x), Ok(y)) = (
        i32::try_from(left.round() as i64),
        i32::try_from(top.round() as i64),
    ) else {
        return None;
    };
    let (Ok(width), Ok(height)) = (
        u32::try_from(scaled_width.round() as i64),
        u32::try_from(scaled_height.round() as i64),
    ) else {
        return None;
    };

    Some(PhysicalRect::new(x, y, width, height))
}

/// Indique que deux rectangles décrivent le même écran, aux arrondis près.
fn is_same_display(frame: PhysicalRect, bounds: PhysicalRect) -> bool {
    let close = |a: i64, b: i64| (a - b).abs() <= MATCH_TOLERANCE_PX;
    close(i64::from(frame.x), i64::from(bounds.x))
        && close(i64::from(frame.y), i64::from(bounds.y))
        && close(i64::from(frame.width), i64::from(bounds.width))
        && close(i64::from(frame.height), i64::from(bounds.height))
}

#[cfg(test)]
mod tests {
    use super::{is_same_display, to_physical};
    use crate::desktop_layout::PhysicalRect;

    #[test]
    fn test_primary_screen_keeps_its_origin() {
        // Écran principal 1440×900 à 2×, sans Dock : le cadre couvre tout.
        let rect = to_physical(0.0, 0.0, 1440.0, 900.0, 900.0, 2.0);
        assert_eq!(rect, Some(PhysicalRect::new(0, 0, 2880, 1800)));
    }

    #[test]
    fn test_dock_reduces_the_visible_frame_from_the_bottom() {
        // Dock de 70 points en bas : `visibleFrame` commence à y = 70 en repère
        // AppKit, ce qui se traduit par une hauteur réduite en repère écran.
        let rect = to_physical(0.0, 70.0, 1440.0, 830.0, 900.0, 2.0);
        assert_eq!(rect, Some(PhysicalRect::new(0, 0, 2880, 1660)));
    }

    #[test]
    fn test_menu_bar_reduces_the_visible_frame_from_the_top() {
        // Barre de menus de 25 points en haut : le cadre visible commence plus bas.
        let rect = to_physical(0.0, 0.0, 1440.0, 875.0, 900.0, 1.0);
        assert_eq!(rect, Some(PhysicalRect::new(0, 25, 1440, 875)));
    }

    #[test]
    fn test_screen_above_the_primary_gets_a_negative_top() {
        // Un écran placé au-dessus a un `y` AppKit supérieur au bord haut du
        // principal : après retournement, son `y` écran doit être négatif.
        let rect = to_physical(0.0, 900.0, 1440.0, 900.0, 900.0, 1.0);
        assert_eq!(rect, Some(PhysicalRect::new(0, -900, 1440, 900)));
    }

    #[test]
    fn test_screen_left_of_the_primary_keeps_a_negative_x() {
        let rect = to_physical(-1920.0, 0.0, 1920.0, 1080.0, 1080.0, 1.0);
        assert_eq!(rect, Some(PhysicalRect::new(-1920, 0, 1920, 1080)));
    }

    #[test]
    fn test_hostile_values_produce_no_rect() {
        assert_eq!(to_physical(f64::NAN, 0.0, 100.0, 100.0, 100.0, 1.0), None);
        assert_eq!(to_physical(0.0, 0.0, 0.0, 100.0, 100.0, 1.0), None);
        assert_eq!(to_physical(0.0, 0.0, 100.0, 100.0, 100.0, 0.0), None);
        assert_eq!(
            to_physical(0.0, 0.0, f64::INFINITY, 100.0, 100.0, 1.0),
            None
        );
        assert_eq!(to_physical(0.0, 0.0, 1e18, 1e18, 1e18, 1.0), None);
    }

    #[test]
    fn test_matching_tolerates_rounding_but_not_a_different_screen() {
        let bounds = PhysicalRect::new(0, 0, 2880, 1800);
        assert!(is_same_display(PhysicalRect::new(0, 0, 2880, 1800), bounds));
        assert!(is_same_display(PhysicalRect::new(2, 0, 2879, 1801), bounds));
        assert!(!is_same_display(
            PhysicalRect::new(0, 0, 1920, 1080),
            bounds
        ));
        assert!(!is_same_display(
            PhysicalRect::new(2880, 0, 2880, 1800),
            bounds
        ));
    }
}
