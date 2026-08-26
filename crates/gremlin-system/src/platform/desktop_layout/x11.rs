//! Zone de travail Linux/X11 via la propriété EWMH `_NET_WORKAREA`.
//!
//! `_NET_WORKAREA` est publiée par le gestionnaire de fenêtres sur la fenêtre
//! racine, sous forme de quadruplets `x, y, largeur, hauteur` — un par bureau
//! virtuel. Elle décrit une zone **globale**, pas une zone par moniteur : la
//! normalisation l'intersecte ensuite avec chaque écran.
//!
//! Tous les gestionnaires ne la publient pas (i3, sway sous X11, certains
//! environnements minimalistes). L'absence est alors une erreur explicite, que
//! l'appelant traduit en repli marqué [`WorkAreaAccuracy::BoundsFallback`].
//!
//! [`WorkAreaAccuracy::BoundsFallback`]: crate::desktop_layout::WorkAreaAccuracy::BoundsFallback

use crate::desktop_layout::{PhysicalRect, WorkAreaSource};
use crate::error::SystemError;
use std::env;
use std::ffi::CString;
use std::os::raw::{c_long, c_uchar, c_ulong};
use std::ptr;
use x11_dl::xlib;

/// Nombre de valeurs composant un rectangle de `_NET_WORKAREA`.
const WORKAREA_FIELDS: usize = 4;

/// Nombre maximal de bureaux virtuels dont la zone est lue.
///
/// La propriété peut en décrire des dizaines ; seul le premier nous sert, mais la
/// borne évite de demander au serveur X un transfert disproportionné.
const MAX_DESKTOPS_READ: c_long = 16;

/// Construit la source X11, ou explique pourquoi elle est indisponible.
pub(super) fn source() -> Result<Box<dyn WorkAreaSource + Send + Sync>, SystemError> {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    if session_type.eq_ignore_ascii_case("wayland") {
        return Err(SystemError::DesktopLayoutUnavailable(String::from(
            "Wayland ne publie ni la position des surfaces ni la zone de travail",
        )));
    }
    if env::var_os("DISPLAY").is_none() {
        return Err(SystemError::DesktopLayoutUnavailable(String::from(
            "variable DISPLAY absente",
        )));
    }

    let xlib = xlib::Xlib::open().map_err(|error| {
        SystemError::DesktopLayoutUnavailable(format!("chargement de Xlib impossible : {error}"))
    })?;
    Ok(Box::new(X11WorkArea { xlib }))
}

/// Source adossée à la propriété EWMH de la fenêtre racine.
struct X11WorkArea {
    xlib: xlib::Xlib,
}

// SAFETY: `Xlib` n'est qu'une table de pointeurs de fonctions chargée une fois,
// jamais mutée après construction. Chaque lecture ouvre et referme sa propre
// connexion : aucun état de connexion n'est partagé entre threads.
#[allow(unsafe_code)]
unsafe impl Send for X11WorkArea {}
// SAFETY: voir ci-dessus — la structure est immuable après construction.
#[allow(unsafe_code)]
unsafe impl Sync for X11WorkArea {}

impl WorkAreaSource for X11WorkArea {
    fn work_area(&self, bounds: PhysicalRect) -> Result<PhysicalRect, SystemError> {
        let global = self.read_global_work_area()?;
        // La zone est globale : seule sa portion recouvrant ce moniteur compte.
        global.intersection(bounds).ok_or_else(|| {
            SystemError::DesktopLayoutReadFailed(String::from(
                "_NET_WORKAREA ne recouvre pas ce moniteur",
            ))
        })
    }
}

impl X11WorkArea {
    /// Lit le premier quadruplet de `_NET_WORKAREA` sur la fenêtre racine.
    fn read_global_work_area(&self) -> Result<PhysicalRect, SystemError> {
        let Ok(property_name) = CString::new("_NET_WORKAREA") else {
            return Err(SystemError::DesktopLayoutReadFailed(String::from(
                "nom de propriété invalide",
            )));
        };

        // SAFETY: le nom nul demande à Xlib d'utiliser DISPLAY. Le pointeur est
        // vérifié puis refermé avant chaque sortie de cette fonction.
        #[allow(unsafe_code)]
        let display = unsafe { (self.xlib.XOpenDisplay)(ptr::null()) };
        if display.is_null() {
            return Err(SystemError::DesktopLayoutReadFailed(String::from(
                "connexion au serveur X11 impossible",
            )));
        }

        let result = self.query_property(display, &property_name);

        // SAFETY: `display` est une connexion valide ouverte juste au-dessus et
        // n'est refermée qu'ici, une seule fois.
        #[allow(unsafe_code)]
        unsafe {
            (self.xlib.XCloseDisplay)(display);
        }
        result
    }

    /// Interroge la propriété et convertit son contenu, connexion ouverte.
    fn query_property(
        &self,
        display: *mut xlib::Display,
        property_name: &CString,
    ) -> Result<PhysicalRect, SystemError> {
        let mut actual_type: xlib::Atom = 0;
        let mut actual_format: i32 = 0;
        let mut item_count: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut data: *mut c_uchar = ptr::null_mut();

        // SAFETY: `display` est une connexion valide ; `property_name` est un
        // `CString` vivant pendant tout l'appel. `XGetWindowProperty` n'écrit que
        // dans les variables locales dont l'adresse lui est passée, et alloue
        // `data`, libéré plus bas par `XFree` sur tous les chemins.
        #[allow(unsafe_code)]
        let status = unsafe {
            let root = (self.xlib.XDefaultRootWindow)(display);
            let atom = (self.xlib.XInternAtom)(display, property_name.as_ptr(), xlib::True);
            if atom == 0 {
                return Err(SystemError::DesktopLayoutReadFailed(String::from(
                    "_NET_WORKAREA non publiée par le gestionnaire de fenêtres",
                )));
            }
            (self.xlib.XGetWindowProperty)(
                display,
                root,
                atom,
                0,
                MAX_DESKTOPS_READ * WORKAREA_FIELDS as c_long,
                xlib::False,
                xlib::XA_CARDINAL,
                ptr::addr_of_mut!(actual_type),
                ptr::addr_of_mut!(actual_format),
                ptr::addr_of_mut!(item_count),
                ptr::addr_of_mut!(bytes_after),
                ptr::addr_of_mut!(data),
            )
        };

        let outcome = self.extract_first_rect(status, actual_type, actual_format, item_count, data);

        if !data.is_null() {
            // SAFETY: `data` a été alloué par `XGetWindowProperty` et n'est libéré
            // qu'ici, une seule fois, après la dernière lecture.
            #[allow(unsafe_code)]
            unsafe {
                (self.xlib.XFree)(data.cast());
            }
        }
        outcome
    }

    /// Convertit les quatre premières valeurs lues en rectangle validé.
    fn extract_first_rect(
        &self,
        status: i32,
        actual_type: xlib::Atom,
        actual_format: i32,
        item_count: c_ulong,
        data: *mut c_uchar,
    ) -> Result<PhysicalRect, SystemError> {
        if status != i32::from(xlib::Success)
            || data.is_null()
            || actual_type != xlib::XA_CARDINAL
            // Format 32 signifie « un `long` par valeur » dans la convention Xlib,
            // y compris sur les plateformes où `long` fait 64 bits.
            || actual_format != 32
            || (item_count as usize) < WORKAREA_FIELDS
        {
            return Err(SystemError::DesktopLayoutReadFailed(String::from(
                "_NET_WORKAREA absente ou de format inattendu",
            )));
        }

        // SAFETY: `data` est non nul, le format 32 garantit un tableau de `c_long`,
        // et `item_count` vient d'être vérifié comme couvrant au moins quatre
        // valeurs — les seules lues ici.
        #[allow(unsafe_code)]
        let values = unsafe { std::slice::from_raw_parts(data.cast::<c_long>(), WORKAREA_FIELDS) };

        let (Ok(x), Ok(y), Ok(width), Ok(height)) = (
            i32::try_from(values[0]),
            i32::try_from(values[1]),
            u32::try_from(values[2]),
            u32::try_from(values[3]),
        ) else {
            return Err(SystemError::DesktopLayoutReadFailed(String::from(
                "_NET_WORKAREA hors des bornes représentables",
            )));
        };

        let rect = PhysicalRect::new(x, y, width, height);
        if !rect.is_valid() {
            return Err(SystemError::DesktopLayoutReadFailed(String::from(
                "_NET_WORKAREA dégénérée",
            )));
        }
        Ok(rect)
    }
}
