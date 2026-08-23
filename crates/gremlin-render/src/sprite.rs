//! Découpage, décodage PNG, découpage de spritesheets et cache d'atlas de textures.
//!
//! Toutes les images décodées ici proviennent potentiellement de packs de skins
//! utilisateur : le décodage est plafonné par [`crate::limits::decode_limits`] et
//! l'arithmétique de découpe est faite en `u64`/`usize` pour rester correcte quelles
//! que soient les dimensions annoncées.

use crate::draw::{blank_canvas, fill_rect, set_px};
use crate::error::RenderError;
use crate::limits::{decode_limits, CANVAS_SIZE};
use crate::manifest::SkinManifest;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use tracing::warn;

/// Une frame de sprite individuelle décodée en mémoire (RGBA 8 bits par canal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteFrame {
    /// Largeur en pixels.
    pub width: u32,
    /// Hauteur en pixels.
    pub height: u32,
    /// Données brutes de pixels (4 octets par pixel : R, G, B, A).
    pub rgba: Vec<u8>,
}

impl SpriteFrame {
    /// Crée une frame à partir de données brutes RGBA après vérification de cohérence de taille.
    ///
    /// # Errors
    /// Renvoie `RenderError::InvalidBufferSize` si la taille du vecteur ne correspond
    /// pas à `width * height * 4`.
    pub fn from_raw(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, RenderError> {
        let expected_size = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected_size {
            return Err(RenderError::InvalidBufferSize {
                expected: expected_size,
                actual: rgba.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// Charge et décode une image PNG depuis un fichier sur le disque.
    ///
    /// Le décodeur est plafonné par [`crate::limits::decode_limits`] : une image
    /// démesurée est rejetée avant allocation plutôt que de saturer la mémoire.
    ///
    /// # Errors
    /// Renvoie `RenderError` si le fichier est inaccessible, corrompu, ou si ses
    /// dimensions dépassent les bornes de décodage.
    pub fn from_png_file<P: AsRef<Path>>(path: P) -> Result<Self, RenderError> {
        let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
        reader.limits(decode_limits());
        Ok(Self::from_dynamic(&reader.decode()?))
    }

    /// Décode une image PNG depuis un tampon d'octets en mémoire.
    ///
    /// Applique les mêmes plafonds de décodage que [`SpriteFrame::from_png_file`].
    ///
    /// # Errors
    /// Renvoie `RenderError` si les octets ne correspondent pas à un format d'image
    /// valide ou si les dimensions dépassent les bornes de décodage.
    pub fn from_png_bytes(bytes: &[u8]) -> Result<Self, RenderError> {
        let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        reader.limits(decode_limits());
        Ok(Self::from_dynamic(&reader.decode()?))
    }

    /// Convertit une image décodée en frame RGBA8.
    fn from_dynamic(img: &image::DynamicImage) -> Self {
        let rgba_img = img.to_rgba8();
        let (width, height) = rgba_img.dimensions();
        Self {
            width,
            height,
            rgba: rgba_img.into_raw(),
        }
    }

    /// Découpe une sous-région rectangulaire de la frame.
    ///
    /// Les bornes sont vérifiées en arithmétique `u64` : aucune combinaison de
    /// `x`/`y`/`width`/`height` ne peut déborder pour contourner le contrôle.
    ///
    /// # Errors
    /// Renvoie `RenderError::OutOfBounds` si la région demandée dépasse les limites
    /// de la frame.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Result<Self, RenderError> {
        let right = u64::from(x) + u64::from(width);
        let bottom = u64::from(y) + u64::from(height);

        if right > u64::from(self.width) || bottom > u64::from(self.height) {
            return Err(RenderError::OutOfBounds {
                x: right.min(u64::from(u32::MAX)) as u32,
                y: bottom.min(u64::from(u32::MAX)) as u32,
                width: self.width,
                height: self.height,
            });
        }

        let row_bytes = (width as usize) * 4;
        let mut cropped_rgba = Vec::with_capacity(row_bytes * (height as usize));

        for row in 0..height {
            // Indices calculés en `usize` : `self.rgba` existe déjà en mémoire, donc
            // `self.width * self.height * 4` tient nécessairement dans un `usize`.
            let src_y = (y as usize) + (row as usize);
            let start_idx = (src_y * (self.width as usize) + (x as usize)) * 4;
            let end_idx = start_idx + row_bytes;

            let Some(slice) = self.rgba.get(start_idx..end_idx) else {
                return Err(RenderError::InvalidBufferSize {
                    expected: end_idx,
                    actual: self.rgba.len(),
                });
            };
            cropped_rgba.extend_from_slice(slice);
        }

        Ok(Self {
            width,
            height,
            rgba: cropped_rgba,
        })
    }
}

/// Cache de textures et sprites indexés par clé d'identification.
#[derive(Debug, Default, Clone)]
pub struct SpriteAtlas {
    frames: HashMap<String, SpriteFrame>,
}

impl SpriteAtlas {
    /// Crée un nouvel atlas vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Nombre de sprites actuellement chargés.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Indique si l'atlas est vide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Enregistre une frame dans l'atlas.
    pub fn insert(&mut self, key: impl Into<String>, frame: SpriteFrame) {
        self.frames.insert(key.into(), frame);
    }

    /// Récupère une référence vers une frame si elle existe dans le cache.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&SpriteFrame> {
        self.frames.get(key)
    }

    /// Vérifie si une clé est présente dans l'atlas.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.frames.contains_key(key)
    }

    /// Charge une image PNG individuelle et l'insère dans l'atlas.
    ///
    /// # Errors
    /// Renvoie `RenderError` si la lecture ou le décodage échoue.
    pub fn load_from_png_file<P: AsRef<Path>>(
        &mut self,
        key: impl Into<String>,
        path: P,
    ) -> Result<(), RenderError> {
        let frame = SpriteFrame::from_png_file(path)?;
        self.insert(key, frame);
        Ok(())
    }

    /// Charge une image PNG et refuse de l'insérer si ses dimensions ne correspondent
    /// pas à la frame déclarée par le manifest du skin.
    ///
    /// À préférer à [`SpriteAtlas::load_from_png_file`] lors du chargement d'un pack
    /// de skin : un PNG dont les dimensions divergent du manifest est soit corrompu,
    /// soit hostile.
    ///
    /// # Errors
    /// Renvoie `RenderError` si le décodage échoue ou si les dimensions divergent du manifest.
    pub fn load_from_png_file_checked<P: AsRef<Path>>(
        &mut self,
        key: impl Into<String>,
        path: P,
        manifest: &SkinManifest,
    ) -> Result<(), RenderError> {
        let frame = SpriteFrame::from_png_file(path)?;
        manifest.validate_frame_size(frame.width, frame.height)?;
        self.insert(key, frame);
        Ok(())
    }

    /// Sauvegarde une frame de l'atlas en fichier PNG sur le disque.
    ///
    /// # Errors
    /// Renvoie `RenderError::MissingSprite` si la clé est absente de l'atlas, ou
    /// `RenderError::ImageDecode` si l'encodage/écriture échoue.
    pub fn save_frame_to_png<P: AsRef<Path>>(&self, key: &str, path: P) -> Result<(), RenderError> {
        let Some(frame) = self.get(key) else {
            return Err(RenderError::MissingSprite {
                key: key.to_string(),
            });
        };

        image::save_buffer(
            path,
            &frame.rgba,
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(())
    }

    /// Découpe une spritesheet en grille régulière de sous-images et les enregistre
    /// avec le préfixe donné (ex: `idle_0`, `idle_1`, `idle_2` ...).
    ///
    /// Le découpage se fait en grille stricte : si les dimensions de la planche ne
    /// sont pas des multiples exacts de la taille de frame, la bande restante à
    /// droite et/ou en bas est **ignorée** (avec une trace d'avertissement).
    ///
    /// # Errors
    /// Renvoie `RenderError` si le découpage échoue.
    pub fn load_spritesheet_from_frame(
        &mut self,
        key_prefix: &str,
        full_frame: &SpriteFrame,
        frame_width: u32,
        frame_height: u32,
    ) -> Result<Vec<String>, RenderError> {
        if frame_width == 0 || frame_height == 0 {
            warn!(
                key_prefix,
                frame_width, frame_height, "Taille de frame nulle : spritesheet ignorée"
            );
            return Ok(Vec::new());
        }

        let cols = full_frame.width / frame_width;
        let rows = full_frame.height / frame_height;

        let remainder_x = full_frame.width % frame_width;
        let remainder_y = full_frame.height % frame_height;
        if remainder_x != 0 || remainder_y != 0 {
            warn!(
                key_prefix,
                sheet_width = full_frame.width,
                sheet_height = full_frame.height,
                frame_width,
                frame_height,
                remainder_x,
                remainder_y,
                "Spritesheet non multiple de la taille de frame : bande résiduelle ignorée"
            );
        }

        let mut generated_keys = Vec::with_capacity((cols as usize) * (rows as usize));

        let mut idx = 0;
        for r in 0..rows {
            for c in 0..cols {
                let x = c * frame_width;
                let y = r * frame_height;
                let sub_frame = full_frame.crop(x, y, frame_width, frame_height)?;
                let key = format!("{key_prefix}_{idx}");
                self.insert(key.clone(), sub_frame);
                generated_keys.push(key);
                idx += 1;
            }
        }

        Ok(generated_keys)
    }

    /// Découpe une spritesheet depuis un fichier PNG.
    ///
    /// # Errors
    /// Renvoie `RenderError` si l'ouverture ou le découpage échoue.
    pub fn load_spritesheet_from_png_file<P: AsRef<Path>>(
        &mut self,
        key_prefix: &str,
        path: P,
        frame_width: u32,
        frame_height: u32,
    ) -> Result<Vec<String>, RenderError> {
        let full_frame = SpriteFrame::from_png_file(path)?;
        self.load_spritesheet_from_frame(key_prefix, &full_frame, frame_width, frame_height)
    }

    /// Découpe une spritesheet depuis des octets PNG en mémoire.
    ///
    /// # Errors
    /// Renvoie `RenderError` si le décodage ou le découpage échoue.
    pub fn load_spritesheet_from_png_bytes(
        &mut self,
        key_prefix: &str,
        bytes: &[u8],
        frame_width: u32,
        frame_height: u32,
    ) -> Result<Vec<String>, RenderError> {
        let full_frame = SpriteFrame::from_png_bytes(bytes)?;
        self.load_spritesheet_from_frame(key_prefix, &full_frame, frame_width, frame_height)
    }

    /// Dessine un canevas [`CANVAS_SIZE`] x [`CANVAS_SIZE`] et l'insère sous `key`.
    ///
    /// Centralise l'allocation, la construction de la frame et le traitement d'erreur
    /// pour tous les sprites procéduraux.
    fn insert_procedural<F: FnOnce(&mut [u8])>(&mut self, key: &str, paint: F) {
        let mut pixels = blank_canvas();
        paint(&mut pixels);

        match SpriteFrame::from_raw(CANVAS_SIZE, CANVAS_SIZE, pixels) {
            Ok(frame) => self.insert(key, frame),
            Err(err) => warn!(
                sprite_key = key,
                error = %err,
                "Génération procédurale incohérente : sprite ignoré"
            ),
        }
    }

    /// Génère et insère le set de sprites procéduraux par défaut pour Gremlin.
    ///
    /// Garantit que l'application dispose toujours de visuels pixel art complets
    /// (Idle, Happy, Hungry, Sleep, Sick, Dead, Dragged) même en l'absence de fichiers
    /// sur disque. Tous les sprites sont peints sur un canevas pleine taille, déjà
    /// positionnés (voir la convention de [`crate::layer::LayerCompositor`]).
    pub fn load_default_procedural_sprites(&mut self) {
        let size = CANVAS_SIZE as usize;

        // Palette de couleurs Gremlin
        let c_body = [76, 175, 80, 255]; // Vert Gremlin vif
        let c_body_dark = [56, 142, 60, 255]; // Vert ombre
        let c_belly = [200, 230, 201, 255]; // Ventre clair
        let c_eyes = [33, 33, 33, 255]; // Noir yeux
        let c_white = [255, 255, 255, 255]; // Blanc
        let c_blush = [255, 138, 128, 255]; // Joues roses
        let c_blush_happy = [255, 64, 129, 255]; // Joues vives
        let c_sick = [139, 195, 74, 255]; // Vert maladif
        let c_ghost = [178, 223, 219, 200]; // Fantôme translucide

        // 1. Idle 0 & 1 (repos, puis léger rebond / respiration)
        self.insert_procedural("idle_0", |buf| {
            draw_pixel_gremlin_body(buf, size, c_body, c_body_dark, c_belly, 0);
            draw_eyes(buf, size, c_eyes, c_white, 0, false);
            draw_blush(buf, size, c_blush, 0);
        });
        self.insert_procedural("idle_1", |buf| {
            draw_pixel_gremlin_body(buf, size, c_body, c_body_dark, c_belly, -1);
            draw_eyes(buf, size, c_eyes, c_white, -1, true);
            draw_blush(buf, size, c_blush, -1);
        });

        // 2. Happy 0 & 1 (saut de joie, grands yeux brillants)
        for (key, offset) in [("happy_0", -3), ("happy_1", -5)] {
            self.insert_procedural(key, |buf| {
                draw_pixel_gremlin_body(buf, size, c_body, c_body_dark, c_belly, offset);
                draw_happy_eyes(buf, size, c_eyes, offset);
                draw_blush(buf, size, c_blush_happy, offset);
            });
        }

        // 3. Hungry 0 & 1
        for (key, offset) in [("hungry_0", 1), ("hungry_1", 2)] {
            self.insert_procedural(key, |buf| {
                draw_pixel_gremlin_body(buf, size, c_body, c_body_dark, c_belly, offset);
                draw_sad_eyes(buf, size, c_eyes, offset);
            });
        }

        // 4. Sleep 0 & 1 (yeux fermés + bulle Zzz animée)
        for (key, phase) in [("sleep_0", 0), ("sleep_1", 1)] {
            self.insert_procedural(key, |buf| {
                draw_pixel_gremlin_body(buf, size, c_body_dark, c_body_dark, c_belly, 2);
                draw_closed_eyes(buf, size, c_eyes, 2);
                draw_zzz(buf, size, c_white, phase);
            });
        }

        // 5. Sick 0 & 1 (deux frames identiques : l'animation reste statique)
        for key in ["sick_0", "sick_1"] {
            self.insert_procedural(key, |buf| {
                draw_pixel_gremlin_body(buf, size, c_sick, c_body_dark, [220, 237, 200, 255], 1);
                draw_dizzy_eyes(buf, size, c_eyes, 1);
            });
        }

        // 6. Dead (fantôme avec yeux en croix)
        self.insert_procedural("dead", |buf| {
            draw_pixel_gremlin_body(buf, size, c_ghost, c_ghost, [224, 242, 241, 180], -2);
            draw_cross_eyes(buf, size, [69, 90, 100, 255], -2);
        });

        // 7. Dragged 0 & 1 (pattes ballantes lors du déplacement souris)
        for (key, swing) in [("dragged_0", 0), ("dragged_1", 1)] {
            self.insert_procedural(key, |buf| {
                draw_pixel_gremlin_body(buf, size, c_body, c_body_dark, c_belly, -4);
                draw_surprised_eyes(buf, size, c_eyes, c_white, -4);
                draw_dangling_feet(buf, size, c_body_dark, swing);
            });
        }
    }

    /// Clés des sprites générés par [`SpriteAtlas::load_default_procedural_sprites`].
    ///
    /// Exposé pour l'outillage d'export (voir `examples/export_default_sprites.rs`).
    pub const DEFAULT_PROCEDURAL_KEYS: [&str; 13] = [
        "idle_0",
        "idle_1",
        "happy_0",
        "happy_1",
        "hungry_0",
        "hungry_1",
        "sleep_0",
        "sleep_1",
        "sick_0",
        "sick_1",
        "dead",
        "dragged_0",
        "dragged_1",
    ];
}

// Helpers de dessin procédural pour le sprite de base, sur canevas pleine taille.

fn draw_pixel_gremlin_body(
    buf: &mut [u8],
    size: usize,
    body: [u8; 4],
    shadow: [u8; 4],
    belly: [u8; 4],
    offset_y: i32,
) {
    let cy = 34 + offset_y;
    // Oreilles
    fill_rect(buf, size, 16, cy - 14, 6, 10, body);
    fill_rect(buf, size, 42, cy - 14, 6, 10, body);
    fill_rect(buf, size, 18, cy - 12, 2, 6, shadow);
    fill_rect(buf, size, 44, cy - 12, 2, 6, shadow);

    // Tête et corps rondouillard
    fill_rect(buf, size, 18, cy - 8, 28, 26, body);
    fill_rect(buf, size, 14, cy - 4, 36, 20, body);

    // Ombre sous le corps
    fill_rect(buf, size, 20, cy + 18, 24, 2, shadow);

    // Ventre
    fill_rect(buf, size, 24, cy + 4, 16, 12, belly);

    // Pieds
    fill_rect(buf, size, 20, cy + 18, 6, 4, shadow);
    fill_rect(buf, size, 38, cy + 18, 6, 4, shadow);
}

fn draw_eyes(
    buf: &mut [u8],
    size: usize,
    eyes: [u8; 4],
    white: [u8; 4],
    offset_y: i32,
    blink: bool,
) {
    let cy = 30 + offset_y;
    if blink {
        fill_rect(buf, size, 22, cy + 2, 5, 2, eyes);
        fill_rect(buf, size, 37, cy + 2, 5, 2, eyes);
    } else {
        fill_rect(buf, size, 22, cy, 6, 6, eyes);
        fill_rect(buf, size, 36, cy, 6, 6, eyes);
        set_px(buf, size, 23, cy + 1, white);
        set_px(buf, size, 37, cy + 1, white);
    }
}

fn draw_happy_eyes(buf: &mut [u8], size: usize, eyes: [u8; 4], offset_y: i32) {
    let cy = 28 + offset_y;
    // Yeux en arches joyeuses ^^
    fill_rect(buf, size, 22, cy, 6, 2, eyes);
    set_px(buf, size, 21, cy + 1, eyes);
    set_px(buf, size, 28, cy + 1, eyes);

    fill_rect(buf, size, 36, cy, 6, 2, eyes);
    set_px(buf, size, 35, cy + 1, eyes);
    set_px(buf, size, 42, cy + 1, eyes);
}

fn draw_sad_eyes(buf: &mut [u8], size: usize, eyes: [u8; 4], offset_y: i32) {
    let cy = 32 + offset_y;
    fill_rect(buf, size, 22, cy, 6, 4, eyes);
    fill_rect(buf, size, 36, cy, 6, 4, eyes);
}

fn draw_closed_eyes(buf: &mut [u8], size: usize, eyes: [u8; 4], offset_y: i32) {
    let cy = 32 + offset_y;
    fill_rect(buf, size, 22, cy, 6, 2, eyes);
    fill_rect(buf, size, 36, cy, 6, 2, eyes);
}

fn draw_dizzy_eyes(buf: &mut [u8], size: usize, eyes: [u8; 4], offset_y: i32) {
    let cy = 31 + offset_y;
    // Spirales / yeux tourbillonnants
    fill_rect(buf, size, 23, cy, 4, 4, eyes);
    set_px(buf, size, 24, cy + 1, [255, 255, 255, 255]);
    fill_rect(buf, size, 37, cy, 4, 4, eyes);
    set_px(buf, size, 38, cy + 1, [255, 255, 255, 255]);
}

fn draw_cross_eyes(buf: &mut [u8], size: usize, color: [u8; 4], offset_y: i32) {
    let cy = 30 + offset_y;
    // Croix X pour oeil gauche
    set_px(buf, size, 22, cy, color);
    set_px(buf, size, 26, cy, color);
    set_px(buf, size, 24, cy + 2, color);
    set_px(buf, size, 22, cy + 4, color);
    set_px(buf, size, 26, cy + 4, color);

    // Croix X pour oeil droit
    set_px(buf, size, 36, cy, color);
    set_px(buf, size, 40, cy, color);
    set_px(buf, size, 38, cy + 2, color);
    set_px(buf, size, 36, cy + 4, color);
    set_px(buf, size, 40, cy + 4, color);
}

fn draw_surprised_eyes(buf: &mut [u8], size: usize, eyes: [u8; 4], white: [u8; 4], offset_y: i32) {
    let cy = 26 + offset_y;
    // Grands yeux ronds écarquillés
    fill_rect(buf, size, 20, cy, 8, 8, white);
    fill_rect(buf, size, 36, cy, 8, 8, white);
    fill_rect(buf, size, 22, cy + 2, 4, 4, eyes);
    fill_rect(buf, size, 38, cy + 2, 4, 4, eyes);
}

fn draw_blush(buf: &mut [u8], size: usize, blush: [u8; 4], offset_y: i32) {
    let cy = 34 + offset_y;
    fill_rect(buf, size, 16, cy, 4, 2, blush);
    fill_rect(buf, size, 44, cy, 4, 2, blush);
}

fn draw_zzz(buf: &mut [u8], size: usize, color: [u8; 4], phase: i32) {
    let base_x = 46 + phase;
    let base_y = 16 - (phase * 2);
    // Petit Z
    fill_rect(buf, size, base_x, base_y, 4, 1, color);
    set_px(buf, size, base_x + 2, base_y + 1, color);
    set_px(buf, size, base_x + 1, base_y + 2, color);
    fill_rect(buf, size, base_x, base_y + 3, 4, 1, color);
}

fn draw_dangling_feet(buf: &mut [u8], size: usize, color: [u8; 4], swing: u8) {
    let base_y = 50;
    if swing == 0 {
        fill_rect(buf, size, 18, base_y, 5, 6, color);
        fill_rect(buf, size, 41, base_y + 2, 5, 6, color);
    } else {
        fill_rect(buf, size, 19, base_y + 2, 5, 6, color);
        fill_rect(buf, size, 40, base_y, 5, 6, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_frame_crop() {
        let mut raw = vec![0u8; 4 * 4 * 4];
        // Remplir le pixel (1, 1) en rouge
        let idx = (4 + 1) * 4;
        raw[idx] = 255;
        raw[idx + 3] = 255;

        let frame = match SpriteFrame::from_raw(4, 4, raw) {
            Ok(f) => f,
            Err(e) => panic!("Création de frame invalide : {e}"),
        };
        let cropped = match frame.crop(1, 1, 2, 2) {
            Ok(c) => c,
            Err(e) => panic!("Crop invalide : {e}"),
        };
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(&cropped.rgba[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn test_crop_hors_limites_est_rejete() {
        let frame = match SpriteFrame::from_raw(4, 4, vec![0u8; 4 * 4 * 4]) {
            Ok(f) => f,
            Err(e) => panic!("frame de test invalide : {e}"),
        };
        assert!(matches!(
            frame.crop(3, 0, 2, 1),
            Err(RenderError::OutOfBounds { .. })
        ));
        assert!(matches!(
            frame.crop(0, 3, 1, 2),
            Err(RenderError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn test_crop_ne_deborde_pas_sur_les_grandes_valeurs() {
        // Régression : `x + width > self.width` bouclait en `u32` en release, ce qui
        // laissait passer le contrôle de bornes pour des valeurs proches de u32::MAX.
        let frame = match SpriteFrame::from_raw(4, 4, vec![0u8; 4 * 4 * 4]) {
            Ok(f) => f,
            Err(e) => panic!("frame de test invalide : {e}"),
        };
        assert!(matches!(
            frame.crop(u32::MAX, 0, 8, 1),
            Err(RenderError::OutOfBounds { .. })
        ));
        assert!(matches!(
            frame.crop(2, 2, u32::MAX, u32::MAX),
            Err(RenderError::OutOfBounds { .. })
        ));
        assert!(matches!(
            frame.crop(0, 0, u32::MAX, 1),
            Err(RenderError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn test_crop_pleine_frame() {
        let frame = match SpriteFrame::from_raw(2, 2, vec![7u8; 2 * 2 * 4]) {
            Ok(f) => f,
            Err(e) => panic!("frame de test invalide : {e}"),
        };
        let full = match frame.crop(0, 0, 2, 2) {
            Ok(c) => c,
            Err(e) => panic!("crop plein invalide : {e}"),
        };
        assert_eq!(full.rgba, frame.rgba);
    }

    #[test]
    fn test_load_default_procedural_sprites() {
        let mut atlas = SpriteAtlas::new();
        atlas.load_default_procedural_sprites();

        for key in SpriteAtlas::DEFAULT_PROCEDURAL_KEYS {
            assert!(
                atlas.contains_key(key),
                "sprite procédural manquant : {key}"
            );
        }

        let Some(idle_frame) = atlas.get("idle_0") else {
            panic!("Frame manquante dans l'atlas procédural");
        };
        assert_eq!(idle_frame.width, CANVAS_SIZE);
        assert_eq!(idle_frame.height, CANVAS_SIZE);
        assert_eq!(
            idle_frame.rgba.len(),
            (CANVAS_SIZE as usize) * (CANVAS_SIZE as usize) * 4
        );
    }

    #[test]
    fn test_sprites_proceduraux_sont_distincts() {
        let mut atlas = SpriteAtlas::new();
        atlas.load_default_procedural_sprites();

        let (Some(idle), Some(happy), Some(dead)) =
            (atlas.get("idle_0"), atlas.get("happy_0"), atlas.get("dead"))
        else {
            panic!("sprites procéduraux manquants");
        };
        assert_ne!(idle.rgba, happy.rgba);
        assert_ne!(idle.rgba, dead.rgba);
    }

    #[test]
    fn test_save_frame_to_png_cle_absente() {
        let atlas = SpriteAtlas::new();
        let path = std::env::temp_dir().join("gremlin_render_absent.png");
        assert!(matches!(
            atlas.save_frame_to_png("inexistant", &path),
            Err(RenderError::MissingSprite { .. })
        ));
        assert!(!path.exists());
    }

    #[test]
    fn test_export_puis_relecture_dun_sprite() {
        let mut atlas = SpriteAtlas::new();
        atlas.load_default_procedural_sprites();

        // Répertoire temporaire du système : aucun effet de bord dans le dépôt.
        let dir = std::env::temp_dir().join("gremlin_render_export_test");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            panic!("création du répertoire temporaire impossible : {e}");
        }
        let path = dir.join("idle_0.png");

        if let Err(e) = atlas.save_frame_to_png("idle_0", &path) {
            panic!("échec d'export PNG : {e}");
        }

        let reloaded = match SpriteFrame::from_png_file(&path) {
            Ok(f) => f,
            Err(e) => panic!("échec de relecture du PNG exporté : {e}"),
        };
        assert_eq!(reloaded.width, CANVAS_SIZE);
        assert_eq!(reloaded.height, CANVAS_SIZE);
        assert_eq!(
            atlas.get("idle_0").map(|f| f.rgba.clone()),
            Some(reloaded.rgba)
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_spritesheet_non_multiple_ignore_le_reliquat() {
        // Planche de 5x3 découpée en frames de 2x2 : une colonne et une ligne
        // résiduelles sont volontairement abandonnées.
        let frame = match SpriteFrame::from_raw(5, 3, vec![0u8; 5 * 3 * 4]) {
            Ok(f) => f,
            Err(e) => panic!("frame de test invalide : {e}"),
        };
        let mut atlas = SpriteAtlas::new();
        let keys = match atlas.load_spritesheet_from_frame("anim", &frame, 2, 2) {
            Ok(k) => k,
            Err(e) => panic!("découpage invalide : {e}"),
        };
        assert_eq!(keys, vec!["anim_0".to_string(), "anim_1".to_string()]);
    }

    #[test]
    fn test_spritesheet_taille_de_frame_nulle() {
        let frame = match SpriteFrame::from_raw(4, 4, vec![0u8; 4 * 4 * 4]) {
            Ok(f) => f,
            Err(e) => panic!("frame de test invalide : {e}"),
        };
        let mut atlas = SpriteAtlas::new();
        let keys = match atlas.load_spritesheet_from_frame("anim", &frame, 0, 2) {
            Ok(k) => k,
            Err(e) => panic!("découpage invalide : {e}"),
        };
        assert!(keys.is_empty());
        assert!(atlas.is_empty());
    }

    #[test]
    fn test_png_corrompu_est_rejete() {
        assert!(SpriteFrame::from_png_bytes(&[0, 1, 2, 3, 4, 5, 6, 7]).is_err());
    }
}
