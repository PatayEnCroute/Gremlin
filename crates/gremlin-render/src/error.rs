//! Types d'erreurs pour le moteur de rendu 2D et le chargement des assets.

use thiserror::Error;

/// Erreurs de rendu et de gestion des textures.
#[derive(Debug, Error)]
pub enum RenderError {
    /// Échec du décodage d'une image PNG.
    #[error("erreur de décodage d'image : {0}")]
    ImageDecode(#[from] image::ImageError),

    /// Échec de lecture ou de parsing du manifest.json.
    #[error("manifest de skin invalide : {0}")]
    InvalidManifest(#[from] serde_json::Error),

    /// Champ de manifest syntaxiquement valide mais sémantiquement hors bornes.
    ///
    /// Utilisé pour toutes les valeurs issues d'une source non fiable qui échouent
    /// à la validation (dimensions démesurées, ancrages aberrants, etc.).
    #[error("champ de manifest invalide « {field} » : {reason}")]
    InvalidManifestField {
        /// Nom du champ fautif.
        field: String,
        /// Explication de la contrainte violée.
        reason: String,
    },

    /// Erreur d'accès I/O à un fichier d'asset.
    #[error("erreur I/O d'asset : {0}")]
    Io(#[from] std::io::Error),

    /// Clé de sprite absente de l'atlas de textures.
    #[error("sprite « {key} » introuvable dans l'atlas")]
    MissingSprite {
        /// Clé recherchée.
        key: String,
    },

    /// Coordonnées de dessin hors limites.
    #[error("coordonnées de rendu hors limites ({x}, {y}) pour un buffer ({width}x{height})")]
    OutOfBounds {
        /// Abscisse fautive.
        x: u32,
        /// Ordonnée fautive.
        y: u32,
        /// Largeur disponible.
        width: u32,
        /// Hauteur disponible.
        height: u32,
    },

    /// Taille du tampon de pixels invalide par rapport aux dimensions spécifiées.
    #[error("taille de buffer de pixels invalide : attendu {expected} octets, reçu {actual}")]
    InvalidBufferSize {
        /// Taille attendue en octets.
        expected: usize,
        /// Taille réellement fournie.
        actual: usize,
    },
}

impl RenderError {
    /// Raccourci de construction d'une erreur de champ de manifest.
    pub(crate) fn invalid_field(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidManifestField {
            field: field.into(),
            reason: reason.into(),
        }
    }
}
