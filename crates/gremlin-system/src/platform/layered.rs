//! Présentation par fenêtre en couches, pour une transparence par pixel réelle.
//!
//! # Pourquoi ce chemin existe
//!
//! La fenêtre du familier est déclarée transparente, mais c'est la *surface de
//! présentation* qui décide si le canal alpha est honoré. Sur Windows, une
//! swapchain attachée à un HWND classique ne sait pas composer d'alpha : mesuré
//! sur une RTX 3080, les trois backends — Vulkan, DX12 et OpenGL — n'annoncent
//! qu'un seul mode de composition, `Opaque`. Tout pixel laissé transparent est
//! donc aplati en noir, et le familier apparaît dans un carré noir de la taille
//! exacte de sa fenêtre.
//!
//! Ce n'est pas un défaut de configuration : aucun mode alternatif n'est proposé,
//! et le diagnostic est reproductible par
//! `cargo run -p gremlin-app --example probe_surface_alpha`.
//!
//! La voie standard sous Windows pour ce cas — un familier de bureau à alpha par
//! pixel — est la **fenêtre en couches** : on marque la fenêtre `WS_EX_LAYERED`
//! et on pousse l'image entière par `UpdateLayeredWindow`, qui accepte un canal
//! alpha et laisse le gestionnaire de fenêtres composer correctement. Elle nous
//! convient d'autant mieux que le familier est déjà composé dans un tampon CPU :
//! c'est précisément ce que cette interface attend.
//!
//! # Ce qui est testable, et ce qui ne l'est pas
//!
//! [`premultiplied_bgra_into`] — la conversion de format — est une fonction pure,
//! compilée et testée sur les trois systèmes. C'est là que se cachent les vraies
//! erreurs : inversion de canaux, alpha non prémultiplié, agrandissement décalé.
//! L'appel système, lui, est réduit à une enveloppe aussi mince que possible
//! autour d'elle.

/// Facteur d'agrandissement maximal accepté.
///
/// Borne l'empreinte du tampon intermédiaire : à 64×64 pixels natifs, un facteur
/// de huit demande déjà deux mégaoctets.
pub const MAX_PRESENTATION_SCALE: u32 = 8;

/// Convertit un tampon RGBA en BGRA prémultiplié, agrandi d'un facteur entier.
///
/// # Format attendu par Windows
///
/// Une section DIB de 32 bits en `BI_RGB` stocke chaque pixel comme un mot de
/// 32 bits `0xAARRGGBB`. En petit-boutien, l'ordre des octets en mémoire est donc
/// **B, G, R, A** — et non R, G, B, A. Inverser les deux teinte le familier en
/// bleu ; c'est l'erreur classique, invisible sur du gris.
///
/// `UpdateLayeredWindow` avec `AC_SRC_ALPHA` exige de plus un alpha
/// **prémultiplié** : chaque composante de couleur doit déjà être atténuée par
/// l'opacité du pixel. Fournir des couleurs non prémultipliées produit un halo
/// clair sur tout le contour du personnage.
///
/// # Agrandissement
///
/// Chaque pixel source devient un carré de `scale` par `scale` pixels. Un
/// agrandissement entier au plus proche voisin est le seul qui préserve le
/// pixel-art ; toute interpolation le rendrait flou.
///
/// Renvoie `false` si `out` est trop petit pour recevoir le résultat, sans rien
/// écrire de partiel.
#[must_use]
pub fn premultiplied_bgra_into(
    rgba: &[u8],
    width: u32,
    height: u32,
    scale: u32,
    out: &mut [u8],
) -> bool {
    let scale = scale.clamp(1, MAX_PRESENTATION_SCALE);
    let target_width = (width as usize).saturating_mul(scale as usize);
    let target_height = (height as usize).saturating_mul(scale as usize);
    let needed = target_width.saturating_mul(target_height).saturating_mul(4);

    if out.len() < needed {
        return false;
    }
    if rgba.len()
        < (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    {
        return false;
    }

    for source_y in 0..height as usize {
        for source_x in 0..width as usize {
            let source = ((source_y * width as usize) + source_x) * 4;
            let alpha = rgba[source + 3];

            // Prémultiplication avec arrondi au plus proche : la troncature
            // assombrissait visiblement les bords adoucis du sprite.
            let premultiply = |channel: u8| -> u8 {
                let product = u32::from(channel) * u32::from(alpha) + 127;
                ((product / 255) & 0xFF) as u8
            };
            let pixel = [
                premultiply(rgba[source + 2]), // bleu
                premultiply(rgba[source + 1]), // vert
                premultiply(rgba[source]),     // rouge
                alpha,
            ];

            for block_y in 0..scale as usize {
                let row = (source_y * scale as usize) + block_y;
                let row_start = row * target_width;
                for block_x in 0..scale as usize {
                    let column = (source_x * scale as usize) + block_x;
                    let destination = (row_start + column) * 4;
                    out[destination..destination + 4].copy_from_slice(&pixel);
                }
            }
        }
    }

    true
}

/// Nombre d'octets nécessaires pour présenter un tampon agrandi.
#[must_use]
pub fn presentation_buffer_len(width: u32, height: u32, scale: u32) -> usize {
    let scale = scale.clamp(1, MAX_PRESENTATION_SCALE) as usize;
    (width as usize)
        .saturating_mul(scale)
        .saturating_mul((height as usize).saturating_mul(scale))
        .saturating_mul(4)
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{premultiplied_bgra_into, MAX_PRESENTATION_SCALE};
    use crate::error::SystemError;
    use std::ptr;
    use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA,
        AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP,
        HDC, RGBQUAD,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, UpdateLayeredWindow, GWL_EXSTYLE, ULW_ALPHA,
        WS_EX_LAYERED,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    /// Surface de présentation en couches attachée à une fenêtre native.
    pub struct LayeredSurface {
        hwnd: HWND,
        memory_dc: HDC,
        bitmap: HBITMAP,
        /// Pixels de la section DIB, propriété de GDI.
        bits: *mut u8,
        /// Dimensions courantes de la section, en pixels.
        allocated: (u32, u32),
    }

    impl LayeredSurface {
        /// Marque la fenêtre comme fenêtre en couches et prépare son contexte GDI.
        ///
        /// # Errors
        /// Renvoie [`SystemError::WindowError`] si la fenêtre n'est pas une fenêtre
        /// Win32, ou si GDI refuse de créer le contexte mémoire.
        pub fn new(window: &Window) -> Result<Self, SystemError> {
            let handle = window
                .window_handle()
                .map_err(|e| SystemError::WindowError(format!("handle de fenêtre absent : {e}")))?;

            let RawWindowHandle::Win32(win32) = handle.as_raw() else {
                return Err(SystemError::WindowError(String::from(
                    "la fenêtre n'est pas une fenêtre Win32",
                )));
            };
            let hwnd = win32.hwnd.get() as HWND;

            // SAFETY : `hwnd` provient du handle que `winit` garantit valide tant
            // que la fenêtre vit, et l'appelant conserve cette fenêtre aussi
            // longtemps que la surface. Ajouter `WS_EX_LAYERED` à un style
            // existant est une opération documentée et sans effet de bord.
            #[allow(unsafe_code)]
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | (WS_EX_LAYERED as isize));
            }

            // SAFETY : `CreateCompatibleDC(null)` crée un contexte mémoire
            // compatible avec l'écran ; il est libéré dans `Drop`.
            #[allow(unsafe_code)]
            let memory_dc = unsafe { CreateCompatibleDC(ptr::null_mut()) };
            if memory_dc.is_null() {
                return Err(SystemError::WindowError(String::from(
                    "création du contexte GDI mémoire refusée",
                )));
            }

            Ok(Self {
                hwnd,
                memory_dc,
                bitmap: ptr::null_mut(),
                bits: ptr::null_mut(),
                allocated: (0, 0),
            })
        }

        /// Pousse une image RGBA vers la fenêtre, agrandie d'un facteur entier.
        ///
        /// La taille de la fenêtre est définie par cet appel : `UpdateLayeredWindow`
        /// est autoritaire sur les dimensions, il n'y a donc pas à les demander
        /// séparément.
        ///
        /// # Errors
        /// Renvoie [`SystemError::WindowError`] si la section DIB ne peut être
        /// allouée, si la conversion ne tient pas dans le tampon, ou si le
        /// gestionnaire de fenêtres rejette la mise à jour.
        pub fn present(
            &mut self,
            rgba: &[u8],
            width: u32,
            height: u32,
            scale: u32,
        ) -> Result<(), SystemError> {
            let scale = scale.clamp(1, MAX_PRESENTATION_SCALE);
            let target = (width.saturating_mul(scale), height.saturating_mul(scale));
            if target.0 == 0 || target.1 == 0 {
                return Ok(());
            }

            self.ensure_section(target)?;

            let length = (target.0 as usize) * (target.1 as usize) * 4;
            // SAFETY : `bits` pointe sur la section DIB que GDI vient d'allouer
            // pour exactement `target.0 * target.1` pixels de quatre octets, et
            // reste valide jusqu'à la destruction du bitmap. Aucun autre code ne
            // la lit pendant cette écriture.
            #[allow(unsafe_code)]
            let pixels = unsafe { std::slice::from_raw_parts_mut(self.bits, length) };

            if !premultiplied_bgra_into(rgba, width, height, scale, pixels) {
                return Err(SystemError::WindowError(String::from(
                    "conversion vers la section DIB impossible : tampon incompatible",
                )));
            }

            let size = SIZE {
                cx: target.0 as i32,
                cy: target.1 as i32,
            };
            let source_origin = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            // SAFETY : le bitmap est sélectionné dans le contexte mémoire, les
            // structures passées sont valides pour la durée de l'appel, et
            // `hdcDst`/`pptDst` nuls demandent explicitement de conserver la
            // position actuelle de la fenêtre.
            #[allow(unsafe_code)]
            let updated = unsafe {
                SelectObject(self.memory_dc, self.bitmap.cast());
                UpdateLayeredWindow(
                    self.hwnd,
                    ptr::null_mut(),
                    ptr::null(),
                    &raw const size,
                    self.memory_dc,
                    &raw const source_origin,
                    0,
                    &raw const blend,
                    ULW_ALPHA,
                )
            };

            if updated == 0 {
                return Err(SystemError::WindowError(String::from(
                    "UpdateLayeredWindow a été refusé par le gestionnaire de fenêtres",
                )));
            }

            Ok(())
        }

        /// Alloue la section DIB si les dimensions demandées ont changé.
        fn ensure_section(&mut self, target: (u32, u32)) -> Result<(), SystemError> {
            if self.allocated == target && !self.bits.is_null() {
                return Ok(());
            }

            self.release_section();

            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
                    biWidth: target.0 as i32,
                    // Hauteur négative : la section est orientée de haut en bas,
                    // comme notre tampon. Une hauteur positive retournerait
                    // l'image verticalement.
                    biHeight: -(target.1 as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                // Palette inutilisée en 32 bits, mais le champ doit exister.
                bmiColors: [RGBQUAD {
                    rgbBlue: 0,
                    rgbGreen: 0,
                    rgbRed: 0,
                    rgbReserved: 0,
                }],
            };

            let mut bits: *mut core::ffi::c_void = ptr::null_mut();
            // SAFETY : `info` décrit une section 32 bits valide, et `bits` reçoit
            // le pointeur vers les pixels alloués par GDI. Le bitmap obtenu est
            // libéré par `release_section` ou par `Drop`.
            #[allow(unsafe_code)]
            let bitmap = unsafe {
                CreateDIBSection(
                    self.memory_dc,
                    &raw const info,
                    DIB_RGB_COLORS,
                    &raw mut bits,
                    ptr::null_mut(),
                    0,
                )
            };

            if bitmap.is_null() || bits.is_null() {
                return Err(SystemError::WindowError(String::from(
                    "allocation de la section DIB refusée",
                )));
            }

            self.bitmap = bitmap;
            self.bits = bits.cast::<u8>();
            self.allocated = target;
            Ok(())
        }

        /// Libère la section DIB courante, s'il y en a une.
        fn release_section(&mut self) {
            if !self.bitmap.is_null() {
                // SAFETY : `bitmap` a été obtenu de `CreateDIBSection` et n'est
                // plus sélectionné nulle part au moment de sa destruction.
                #[allow(unsafe_code)]
                unsafe {
                    DeleteObject(self.bitmap.cast());
                }
            }
            self.bitmap = ptr::null_mut();
            self.bits = ptr::null_mut();
            self.allocated = (0, 0);
        }
    }

    impl Drop for LayeredSurface {
        fn drop(&mut self) {
            self.release_section();
            if !self.memory_dc.is_null() {
                // SAFETY : le contexte provient de `CreateCompatibleDC` et n'est
                // plus utilisé. Sa libération est obligatoire, GDI ne récupérant
                // pas les contextes à la fin du processus.
                #[allow(unsafe_code)]
                unsafe {
                    DeleteDC(self.memory_dc);
                }
                self.memory_dc = ptr::null_mut();
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::LayeredSurface;

/// Surface de présentation en couches, indisponible hors Windows.
///
/// Les autres systèmes n'en ont pas besoin : sur macOS comme sur les
/// environnements Linux avec compositeur, la surface graphique propose un mode de
/// composition honorant l'alpha, et le chemin GPU habituel suffit.
#[cfg(not(target_os = "windows"))]
pub struct LayeredSurface;

#[cfg(not(target_os = "windows"))]
impl LayeredSurface {
    /// Toujours en échec : ce chemin est propre à Windows.
    ///
    /// # Errors
    /// Renvoie toujours une erreur de fenêtre. Un faux succès ferait croire
    /// à l'appelant que le familier est présenté, alors que rien ne s'afficherait.
    pub fn new(_window: &winit::window::Window) -> Result<Self, crate::error::SystemError> {
        Err(crate::error::SystemError::WindowError(String::from(
            "la présentation en couches est spécifique à Windows",
        )))
    }

    /// Contrepartie inatteignable de la présentation Windows.
    ///
    /// [`Self::new`] échouant toujours ici, aucune surface n'existe hors Windows
    /// et cette méthode ne peut être appelée. Elle existe pour que l'appelant
    /// garde une seule écriture du chemin de présentation, sans `#[cfg]` dans la
    /// logique métier.
    ///
    /// # Errors
    /// Renvoie toujours une erreur de fenêtre, pour la même raison que
    /// [`Self::new`] : mieux vaut un échec bruyant qu'une image jamais affichée.
    pub fn present(
        &mut self,
        _rgba: &[u8],
        _width: u32,
        _height: u32,
        _scale: u32,
    ) -> Result<(), crate::error::SystemError> {
        Err(crate::error::SystemError::WindowError(String::from(
            "la présentation en couches est spécifique à Windows",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construit un tampon RGBA d'un seul pixel.
    fn pixel(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        vec![r, g, b, a]
    }

    #[test]
    fn test_channels_are_reordered_to_bgra() {
        // L'erreur classique : garder l'ordre RGBA et teindre le familier en bleu.
        let source = pixel(200, 100, 50, 255);
        let mut out = [0_u8; 4];
        assert!(premultiplied_bgra_into(&source, 1, 1, 1, &mut out));

        assert_eq!(out, [50, 100, 200, 255], "attendu B,G,R,A");
    }

    #[test]
    fn test_opaque_pixels_keep_their_colour() {
        let source = pixel(17, 34, 51, 255);
        let mut out = [0_u8; 4];
        assert!(premultiplied_bgra_into(&source, 1, 1, 1, &mut out));
        assert_eq!(out, [51, 34, 17, 255]);
    }

    #[test]
    fn test_transparent_pixels_become_fully_zero() {
        // Un pixel transparent dont les couleurs subsisteraient produirait le
        // halo que la prémultiplication est censée éviter.
        let source = pixel(255, 255, 255, 0);
        let mut out = [0_u8; 4];
        assert!(premultiplied_bgra_into(&source, 1, 1, 1, &mut out));
        assert_eq!(out, [0, 0, 0, 0]);
    }

    #[test]
    fn test_colours_are_premultiplied_by_the_alpha() {
        let source = pixel(255, 128, 64, 128);
        let mut out = [0_u8; 4];
        assert!(premultiplied_bgra_into(&source, 1, 1, 1, &mut out));

        // 255 * 128 / 255 = 128, 128 * 128 / 255 ≈ 64, 64 * 128 / 255 ≈ 32.
        assert_eq!(out[3], 128, "l'alpha lui-même n'est pas prémultiplié");
        assert_eq!(out[2], 128, "rouge");
        assert_eq!(out[1], 64, "vert");
        assert_eq!(out[0], 32, "bleu");
    }

    #[test]
    fn test_premultiplication_never_exceeds_the_alpha() {
        // Invariant du format prémultiplié : aucune composante ne peut dépasser
        // l'opacité du pixel, sinon le compositeur produit des couleurs aberrantes.
        for alpha in 0..=255_u8 {
            let source = pixel(255, 255, 255, alpha);
            let mut out = [0_u8; 4];
            assert!(premultiplied_bgra_into(&source, 1, 1, 1, &mut out));
            for (channel, value) in out.iter().take(3).enumerate() {
                assert!(
                    *value <= alpha,
                    "canal {channel} = {value} dépasse l'alpha {alpha}"
                );
            }
        }
    }

    #[test]
    fn test_integer_upscaling_replicates_each_pixel_as_a_block() {
        // Deux pixels côte à côte, agrandis trois fois : chacun doit occuper un
        // carré de 3×3 à la bonne place.
        let source = [255, 0, 0, 255, 0, 0, 255, 255];
        let mut out = vec![0_u8; presentation_buffer_len(2, 1, 3)];
        assert!(premultiplied_bgra_into(&source, 2, 1, 3, &mut out));

        let width = 6_usize;
        for y in 0..3 {
            for x in 0..3 {
                let index = ((y * width) + x) * 4;
                assert_eq!(
                    &out[index..index + 4],
                    &[0, 0, 255, 255],
                    "rouge en ({x},{y})"
                );
            }
            for x in 3..6 {
                let index = ((y * width) + x) * 4;
                assert_eq!(
                    &out[index..index + 4],
                    &[255, 0, 0, 255],
                    "bleu en ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn test_rows_are_not_shifted_by_the_upscaling() {
        // Un damier détecte tout décalage d'une ligne, symptôme d'un mauvais pas
        // de progression dans le tampon de destination.
        let source = [
            255, 255, 255, 255, // (0,0) blanc
            0, 0, 0, 255, // (1,0) noir
            0, 0, 0, 255, // (0,1) noir
            255, 255, 255, 255, // (1,1) blanc
        ];
        let mut out = vec![0_u8; presentation_buffer_len(2, 2, 2)];
        assert!(premultiplied_bgra_into(&source, 2, 2, 2, &mut out));

        let width = 4_usize;
        let at = |x: usize, y: usize| out[((y * width) + x) * 4];
        assert_eq!(at(0, 0), 255, "coin haut-gauche blanc");
        assert_eq!(at(3, 0), 0, "coin haut-droit noir");
        assert_eq!(at(0, 3), 0, "coin bas-gauche noir");
        assert_eq!(at(3, 3), 255, "coin bas-droit blanc");
    }

    #[test]
    fn test_undersized_destination_is_refused_without_partial_write() {
        let source = pixel(1, 2, 3, 255);
        let mut out = [0_u8; 3];
        assert!(!premultiplied_bgra_into(&source, 1, 1, 1, &mut out));
        assert_eq!(out, [0, 0, 0], "aucune écriture partielle attendue");
    }

    #[test]
    fn test_truncated_source_is_refused() {
        // Entrée hostile : un tampon annoncé plus grand qu'il ne l'est.
        let source = [1_u8, 2, 3];
        let mut out = vec![0_u8; 64];
        assert!(!premultiplied_bgra_into(&source, 4, 4, 1, &mut out));
    }

    #[test]
    fn test_degenerate_parameters_do_not_panic() {
        let mut out = vec![0_u8; 4096];
        for (width, height, scale) in [
            (0, 0, 1),
            (1, 1, 0),
            (1, 1, u32::MAX),
            (0, 8, 4),
            (8, 0, 4),
            (u32::MAX, u32::MAX, u32::MAX),
        ] {
            let _ = premultiplied_bgra_into(&[0; 4], width, height, scale, &mut out);
            let _ = presentation_buffer_len(width, height, scale);
        }
    }

    #[test]
    fn test_scale_is_clamped_to_the_documented_maximum() {
        // Un facteur absurde venu d'une configuration éditée à la main ne doit
        // pas demander une allocation démesurée.
        let clamped = presentation_buffer_len(64, 64, u32::MAX);
        let expected = presentation_buffer_len(64, 64, MAX_PRESENTATION_SCALE);
        assert_eq!(clamped, expected);
    }
}
