//! Définition et parsing du fichier de métadonnées `manifest.json` des skins.
//!
//! Un `manifest.json` provient d'un dossier de skin utilisateur : il est traité comme
//! une **entrée non fiable**. [`SkinManifest::from_json`] normalise puis valide
//! systématiquement les valeurs lues avant de les rendre disponibles (voir
//! [`crate::limits`] pour les bornes appliquées).

use crate::animation::{AnimationController, AnimationFrame, PlayMode, SpriteAnimation};
use crate::error::RenderError;
use crate::limits::{
    clamp_frame_duration_ms, DEFAULT_FRAME_DURATION_MS, MAX_ANCHOR_OFFSET, MAX_FRAME_DIMENSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::warn;

/// Durée d'affichage par défaut d'une frame lorsqu'un manifest ne la précise pas.
pub(crate) const fn default_frame_duration_ms() -> u64 {
    DEFAULT_FRAME_DURATION_MS
}

/// Point d'ancrage en coordonnées 2D (pixels) pour l'alignement des calques.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorPoint {
    /// Abscisse en pixels (peut être négative).
    pub x: i32,
    /// Ordonnée en pixels (peut être négative).
    pub y: i32,
}

/// Définition déclarative d'une animation au sein du `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationDef {
    /// Noms/clés des sprites composant l'animation dans l'ordre de lecture.
    pub frames: Vec<String>,
    /// Durée d'affichage d'une frame en millisecondes.
    ///
    /// Toujours ramenée dans `[MIN_FRAME_DURATION_MS, MAX_FRAME_DURATION_MS]` par
    /// [`SkinManifest::from_json`] : une valeur nulle empêcherait le contrôleur
    /// d'animation de converger.
    #[serde(default = "default_frame_duration_ms")]
    pub frame_duration_ms: u64,
    /// Mode de répétition de l'animation (`Loop`, `Once`, `PingPong`).
    #[serde(default)]
    pub mode: PlayMode,
}

impl AnimationDef {
    /// Durée d'affichage effective d'une frame, garantie non nulle et bornée.
    ///
    /// Cette méthode reborne la valeur même si la structure a été construite
    /// directement (littéral de struct) sans passer par le parsing.
    #[must_use]
    pub fn frame_duration(&self) -> Duration {
        let (ms, _) = clamp_frame_duration_ms(self.frame_duration_ms);
        Duration::from_millis(ms)
    }
}

/// Métadonnées complètes d'un pack de skin.
///
/// # Convention d'ancrage
///
/// Voir [`crate::layer::LayerCompositor`] : les sprites de calque sont dessinés sur un
/// canevas pleine taille et déjà positionnés. Le champ [`SkinManifest::anchors`] est
/// une **métadonnée descriptive** (point d'attache de référence pour les auteurs de
/// skins et l'outillage), il n'est jamais ajouté comme translation à la composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkinManifest {
    /// Nom du skin (ex: "Classic Gremlin").
    pub name: String,
    /// Auteur du pack.
    pub author: String,
    /// Version du format de manifest.
    pub version: String,
    /// Largeur d'une frame en pixels (ex: 64), bornée par [`MAX_FRAME_DIMENSION`].
    pub frame_width: u32,
    /// Hauteur d'une frame en pixels (ex: 64), bornée par [`MAX_FRAME_DIMENSION`].
    pub frame_height: u32,
    /// Points d'ancrage de référence pour chaque calque ("hat", "glasses", "held", etc.).
    ///
    /// Purement descriptif : consulté par l'outillage d'édition de skins, jamais
    /// appliqué comme décalage par le compositeur.
    #[serde(default)]
    pub anchors: BTreeMap<String, AnchorPoint>,
    /// Définitions des animations configurées dans le skin.
    #[serde(default)]
    pub animations: BTreeMap<String, AnimationDef>,
}

impl SkinManifest {
    /// Parse un manifest depuis une chaîne JSON, puis le normalise et le valide.
    ///
    /// Les durées de frames aberrantes (nulles ou démesurées) sont ramenées dans les
    /// bornes autorisées avec une trace d'avertissement ; les dimensions et ancrages
    /// hors bornes provoquent en revanche un rejet explicite du manifest.
    ///
    /// # Errors
    /// - `RenderError::InvalidManifest` si le JSON est malformé ou si un champ requis manque ;
    /// - `RenderError::InvalidManifestField` si une valeur est hors des bornes de sécurité.
    pub fn from_json(json_str: &str) -> Result<Self, RenderError> {
        let mut manifest: Self = serde_json::from_str(json_str)?;
        manifest.normalize();
        manifest.validate()?;
        Ok(manifest)
    }

    /// Ramène les valeurs corrigeables du manifest dans leurs bornes autorisées.
    ///
    /// Seules les durées de frames sont concernées : elles sont rabotées plutôt que
    /// de faire échouer tout un pack de skin pour une coquille.
    pub fn normalize(&mut self) {
        for (anim_name, def) in &mut self.animations {
            let (clamped, adjusted) = clamp_frame_duration_ms(def.frame_duration_ms);
            if adjusted {
                warn!(
                    animation = %anim_name,
                    raw = def.frame_duration_ms,
                    clamped,
                    "Durée de frame hors bornes dans le manifest : valeur rabotée"
                );
                def.frame_duration_ms = clamped;
            }
        }
    }

    /// Vérifie que toutes les valeurs issues du manifest respectent les bornes de sécurité.
    ///
    /// # Errors
    /// Renvoie `RenderError::InvalidManifestField` pour la première violation rencontrée.
    pub fn validate(&self) -> Result<(), RenderError> {
        validate_dimension("frame_width", self.frame_width)?;
        validate_dimension("frame_height", self.frame_height)?;

        for (name, anchor) in &self.anchors {
            if anchor.x.unsigned_abs() > MAX_ANCHOR_OFFSET.unsigned_abs()
                || anchor.y.unsigned_abs() > MAX_ANCHOR_OFFSET.unsigned_abs()
            {
                return Err(RenderError::invalid_field(
                    format!("anchors.{name}"),
                    format!(
                        "({}, {}) dépasse la borne de ±{MAX_ANCHOR_OFFSET} px",
                        anchor.x, anchor.y
                    ),
                ));
            }
        }

        Ok(())
    }

    /// Vérifie que les dimensions d'une image décodée correspondent à celles annoncées.
    ///
    /// Un PNG dont les dimensions ne collent pas au manifest trahit soit un pack
    /// corrompu, soit une tentative d'épuisement mémoire via une image démesurée.
    ///
    /// # Errors
    /// Renvoie `RenderError::InvalidManifestField` en cas de divergence.
    pub fn validate_frame_size(&self, width: u32, height: u32) -> Result<(), RenderError> {
        if width != self.frame_width || height != self.frame_height {
            return Err(RenderError::invalid_field(
                "frame_size",
                format!(
                    "image {width}x{height} incompatible avec la frame déclarée {}x{}",
                    self.frame_width, self.frame_height
                ),
            ));
        }
        Ok(())
    }

    /// Construit et initialise un `AnimationController` à partir des définitions du manifest.
    ///
    /// Les animations sans aucune frame sont ignorées (et tracées) : elles ne
    /// produiraient qu'un contrôleur inerte.
    #[must_use]
    pub fn build_animation_controller(&self) -> AnimationController {
        let mut controller = AnimationController::new();

        for (name, def) in &self.animations {
            if def.frames.is_empty() {
                warn!(
                    animation = %name,
                    skin = %self.name,
                    "Animation sans frame dans le manifest : entrée ignorée"
                );
                continue;
            }

            let duration = def.frame_duration();
            let frames = def
                .frames
                .iter()
                .map(|key| AnimationFrame::new(key.clone(), duration))
                .collect();

            let animation = SpriteAnimation::new(name.clone(), frames, def.mode);
            controller.register(animation);
        }

        controller
    }
}

/// Valide une dimension de frame issue d'un manifest non fiable.
fn validate_dimension(field: &str, value: u32) -> Result<(), RenderError> {
    if value == 0 {
        return Err(RenderError::invalid_field(
            field,
            "doit être strictement positif",
        ));
    }
    if value > MAX_FRAME_DIMENSION {
        return Err(RenderError::invalid_field(
            field,
            format!("{value} px dépasse la borne de {MAX_FRAME_DIMENSION} px"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{MAX_FRAME_DURATION_MS, MIN_FRAME_DURATION_MS};

    /// Gabarit de manifest valide dont les tests font varier un seul champ.
    fn manifest_json(body: &str) -> String {
        format!(
            r#"{{
                "name": "T",
                "author": "A",
                "version": "1.0.0",
                "frame_width": 64,
                "frame_height": 64,
                {body}
            }}"#
        )
    }

    #[test]
    fn test_manifest_parsing_with_animations() {
        let json = r#"{
            "name": "Classic Gremlin",
            "author": "Antigravity",
            "version": "1.0.0",
            "frame_width": 64,
            "frame_height": 64,
            "anchors": {
                "hat": { "x": 20, "y": 8 },
                "glasses": { "x": 18, "y": 24 }
            },
            "animations": {
                "idle": {
                    "frames": ["idle_0", "idle_1"],
                    "frame_duration_ms": 250,
                    "mode": "Loop"
                },
                "happy": {
                    "frames": ["happy_0", "happy_1", "happy_2"],
                    "frame_duration_ms": 150,
                    "mode": "PingPong"
                }
            }
        }"#;

        let manifest = match SkinManifest::from_json(json) {
            Ok(m) => m,
            Err(e) => panic!("Échec de parsing du manifest : {e}"),
        };
        assert_eq!(manifest.name, "Classic Gremlin");
        assert_eq!(manifest.frame_width, 64);
        assert_eq!(
            manifest.anchors.get("hat"),
            Some(&AnchorPoint { x: 20, y: 8 })
        );

        let mut controller = manifest.build_animation_controller();
        controller.play("idle", false);
        assert_eq!(controller.current_frame_key(), Some("idle_0"));
        controller.update(Duration::from_millis(250));
        assert_eq!(controller.current_frame_key(), Some("idle_1"));
    }

    // ---------------------------------------------------------------------
    // Manifests malformés / hostiles
    // ---------------------------------------------------------------------

    #[test]
    fn test_json_invalide_est_rejete() {
        let err = SkinManifest::from_json("{ pas du json ");
        assert!(matches!(err, Err(RenderError::InvalidManifest(_))));
    }

    #[test]
    fn test_champ_requis_manquant_est_rejete() {
        // `frame_height` absent : aucune valeur par défaut n'est prévue.
        let json = r#"{
            "name": "T", "author": "A", "version": "1.0.0", "frame_width": 64
        }"#;
        assert!(matches!(
            SkinManifest::from_json(json),
            Err(RenderError::InvalidManifest(_))
        ));
    }

    #[test]
    fn test_mode_de_lecture_inconnu_est_rejete() {
        let json =
            manifest_json(r#""animations": { "idle": { "frames": ["a"], "mode": "Reverse" } }"#);
        assert!(matches!(
            SkinManifest::from_json(&json),
            Err(RenderError::InvalidManifest(_))
        ));
    }

    #[test]
    fn test_duree_de_frame_nulle_est_rabotee() {
        let json = manifest_json(
            r#""animations": { "idle": { "frames": ["a", "b"], "frame_duration_ms": 0 } }"#,
        );
        let manifest = match SkinManifest::from_json(&json) {
            Ok(m) => m,
            Err(e) => panic!("le manifest aurait dû être accepté après rabotage : {e}"),
        };

        let Some(def) = manifest.animations.get("idle") else {
            panic!("animation 'idle' absente")
        };
        assert_eq!(def.frame_duration_ms, MIN_FRAME_DURATION_MS);
        assert!(!def.frame_duration().is_zero());
    }

    #[test]
    fn test_duree_de_frame_demesuree_est_rabotee() {
        let json = manifest_json(
            r#""animations": { "idle": { "frames": ["a"], "frame_duration_ms": 18446744073709551615 } }"#,
        );
        let manifest = match SkinManifest::from_json(&json) {
            Ok(m) => m,
            Err(e) => panic!("le manifest aurait dû être accepté après rabotage : {e}"),
        };
        assert_eq!(
            manifest.animations.get("idle").map(|d| d.frame_duration_ms),
            Some(MAX_FRAME_DURATION_MS)
        );
    }

    #[test]
    fn test_duree_de_frame_nulle_ne_bloque_pas_le_controleur() {
        // Régression : une durée nulle rendait la boucle de rattrapage de
        // `AnimationController::update` non convergente (DoS via manifest).
        let json = manifest_json(
            r#""animations": { "idle": { "frames": ["a", "b"], "frame_duration_ms": 0, "mode": "Loop" } }"#,
        );
        let manifest = match SkinManifest::from_json(&json) {
            Ok(m) => m,
            Err(e) => panic!("parsing inattendu : {e}"),
        };

        let mut controller = manifest.build_animation_controller();
        controller.play("idle", true);
        // Doit rendre la main immédiatement, quelle que soit l'amplitude du delta.
        controller.update(Duration::from_secs(3600));
        assert!(controller.current_frame_key().is_some());
    }

    #[test]
    fn test_dimensions_demesurees_sont_rejetees() {
        let json = r#"{
            "name": "T", "author": "A", "version": "1.0.0",
            "frame_width": 4294967295, "frame_height": 4294967295
        }"#;
        assert!(matches!(
            SkinManifest::from_json(json),
            Err(RenderError::InvalidManifestField { .. })
        ));
    }

    #[test]
    fn test_dimension_nulle_est_rejetee() {
        let json = r#"{
            "name": "T", "author": "A", "version": "1.0.0",
            "frame_width": 0, "frame_height": 64
        }"#;
        assert!(matches!(
            SkinManifest::from_json(json),
            Err(RenderError::InvalidManifestField { .. })
        ));
    }

    #[test]
    fn test_ancrage_negatif_est_accepte() {
        let json = manifest_json(r#""anchors": { "hat": { "x": -8, "y": -4 } }"#);
        let manifest = match SkinManifest::from_json(&json) {
            Ok(m) => m,
            Err(e) => panic!("un ancrage négatif est légitime : {e}"),
        };
        assert_eq!(
            manifest.anchors.get("hat"),
            Some(&AnchorPoint { x: -8, y: -4 })
        );
    }

    #[test]
    fn test_ancrage_hors_bornes_est_rejete() {
        let json = manifest_json(r#""anchors": { "hat": { "x": 999999, "y": 0 } }"#);
        assert!(matches!(
            SkinManifest::from_json(&json),
            Err(RenderError::InvalidManifestField { .. })
        ));
    }

    #[test]
    fn test_ancrage_hors_plage_entiere_est_rejete() {
        // 2^40 ne tient pas dans un i32 : erreur de désérialisation.
        let json = manifest_json(r#""anchors": { "hat": { "x": 1099511627776, "y": 0 } }"#);
        assert!(matches!(
            SkinManifest::from_json(&json),
            Err(RenderError::InvalidManifest(_))
        ));
    }

    #[test]
    fn test_animation_sans_frame_est_ignoree() {
        let json = manifest_json(
            r#""animations": { "vide": { "frames": [] }, "idle": { "frames": ["a"] } }"#,
        );
        let manifest = match SkinManifest::from_json(&json) {
            Ok(m) => m,
            Err(e) => panic!("parsing inattendu : {e}"),
        };

        let mut controller = manifest.build_animation_controller();
        controller.play("vide", true);
        assert_eq!(controller.current_animation_name(), None);
        controller.play("idle", true);
        assert_eq!(controller.current_frame_key(), Some("a"));
    }

    #[test]
    fn test_validate_frame_size() {
        let json = manifest_json(r#""animations": {}"#);
        let manifest = match SkinManifest::from_json(&json) {
            Ok(m) => m,
            Err(e) => panic!("parsing inattendu : {e}"),
        };
        assert!(manifest.validate_frame_size(64, 64).is_ok());
        assert!(matches!(
            manifest.validate_frame_size(65, 64),
            Err(RenderError::InvalidManifestField { .. })
        ));
    }
}
