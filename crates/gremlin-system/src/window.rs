//! Abstraction et configuration pour fenêtre transparente sans bordure et icônes d'application.

use image::GenericImageView;
use std::sync::OnceLock;
use winit::dpi::LogicalSize;
use winit::window::{Icon, WindowAttributes, WindowLevel};

/// Dimension (en pixels) de l'icône d'application produite par [`load_app_icon`].
const ICON_SIZE: u32 = 64;

/// Octets PNG intégrés du logo officiel de Gremlin (garantit la présence de
/// l'icône sans dépendre d'un fichier externe au moment de l'exécution).
///
/// # Couplage avec l'espace de travail
/// Le fichier source vit dans `assets/` **à la racine du dépôt**, en dehors de
/// cette caisse : `gremlin-system` n'est donc pas publiable seule sans ce
/// répertoire. Le chemin est résolu à partir de `CARGO_MANIFEST_DIR` plutôt
/// que relativement à ce fichier source, afin que déplacer ou renommer un
/// module ne casse pas l'inclusion.
pub const EMBEDDED_APP_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/icon_64.png"
));

/// Cache de l'icône décodée : le décodage PNG et le rééchantillonnage Lanczos3
/// sont coûteux et l'icône ne change jamais pendant la vie du processus.
static APP_ICON: OnceLock<Option<Icon>> = OnceLock::new();

/// Charge l'icône officielle de l'application Gremlin (Logo) pour la fenêtre,
/// la barre des tâches et le systray.
///
/// Le résultat est mémorisé au premier appel : les appels suivants ne font
/// qu'un clonage bon marché.
///
/// # Sécurité
/// En build *release*, seuls les octets embarqués sont utilisés. Aucune lecture
/// disque relative au répertoire courant n'a lieu : lancer l'application depuis
/// un dossier piégé ne peut donc pas substituer l'icône de l'application ni du
/// systray.
#[must_use]
pub fn load_app_icon() -> Option<Icon> {
    APP_ICON.get_or_init(build_app_icon).clone()
}

/// Construit l'icône (appelé une seule fois, sous [`OnceLock`]).
fn build_app_icon() -> Option<Icon> {
    // En développement uniquement, un artiste peut remplacer le logo sans
    // recompiler : les sources sont cherchées dans l'arborescence du dépôt,
    // jamais dans le répertoire de lancement.
    #[cfg(debug_assertions)]
    if let Some(icon) = load_icon_from_workspace_sources() {
        return icon.into();
    }

    decode_icon(EMBEDDED_APP_ICON_PNG)
}

/// Décode un PNG en icône `winit` au gabarit attendu.
fn decode_icon(bytes: &[u8]) -> Option<Icon> {
    let img = image::load_from_memory(bytes).ok()?;
    let (width, height) = img.dimensions();

    let rgba = if width == ICON_SIZE && height == ICON_SIZE {
        img.to_rgba8()
    } else {
        img.resize_exact(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3)
            .to_rgba8()
    };

    Icon::from_rgba(rgba.into_raw(), ICON_SIZE, ICON_SIZE).ok()
}

/// Cherche une source de logo dans l'arborescence du dépôt (builds de debug).
#[cfg(debug_assertions)]
fn load_icon_from_workspace_sources() -> Option<Icon> {
    /// Sources d'authoring du logo, ancrées sur la racine de l'espace de
    /// travail (`CARGO_MANIFEST_DIR`), par ordre de priorité.
    const WORKSPACE_ICON_SOURCES: [&str; 2] = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../Icon/Logo.png"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/icon.png"),
    ];

    WORKSPACE_ICON_SOURCES
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .find_map(|bytes| decode_icon(&bytes))
}

/// Configuration standard de la fenêtre flottante de Gremlin.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub transparent: bool,
    pub decorations: bool,
    pub always_on_top: bool,
    pub resizable: bool,
    /// Visible dès sa création.
    ///
    /// Le panneau de paramètres naît **invisible** : l'adaptateur
    /// d'accessibilité doit être installé avant le premier affichage de la
    /// fenêtre, sans quoi certaines plateformes servent un arbre vide au lecteur
    /// d'écran pour toute la durée de vie de la fenêtre.
    pub visible: bool,
    pub icon: Option<Icon>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: String::from("Gremlin"),
            width: 128,
            height: 128,
            transparent: true,
            decorations: false,
            always_on_top: true,
            resizable: false,
            visible: true,
            icon: load_app_icon(),
        }
    }
}

impl WindowConfig {
    /// Préréglage de la fenêtre du panneau de paramètres.
    ///
    /// Elle diffère de la fenêtre du familier sur trois points qui comptent :
    ///
    /// * **opaque** — elle est présentée par un transfert mémoire sans canal
    ///   alpha, et un panneau de réglages n'a pas à laisser voir le bureau ;
    /// * **titrée** — « Paramètres Gremlin » est le nom que reprennent la barre
    ///   des tâches et, surtout, les lecteurs d'écran. La fenêtre du familier
    ///   n'a pas cette exigence, elle n'accueille pas de texte ;
    /// * **sans décoration mais focalisable** — l'aspect Raycast exige l'absence
    ///   de barre de titre, alors que la saisie clavier et l'accessibilité
    ///   exigent qu'elle puisse recevoir le focus.
    #[must_use]
    pub fn settings_panel(width: u32, height: u32) -> Self {
        Self {
            title: String::from("Paramètres Gremlin"),
            width,
            height,
            transparent: false,
            decorations: false,
            always_on_top: true,
            resizable: false,
            visible: false,
            icon: load_app_icon(),
        }
    }

    /// Construit les attributs de fenêtre pour `winit` avec le logo officiel de Gremlin.
    #[must_use]
    pub fn to_window_attributes(&self) -> WindowAttributes {
        let mut attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(LogicalSize::new(
                f64::from(self.width),
                f64::from(self.height),
            ))
            .with_transparent(self.transparent)
            .with_decorations(self.decorations)
            .with_resizable(self.resizable)
            .with_visible(self.visible);

        if let Some(icon) = self.icon.clone().or_else(load_app_icon) {
            attrs = attrs.with_window_icon(Some(icon));
        }

        if self.always_on_top {
            attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop);
        }

        attrs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_config_defaults() {
        let config = WindowConfig::default();
        assert_eq!(config.title, "Gremlin");
        assert_eq!(config.width, 128);
        assert_eq!(config.height, 128);
        assert!(config.transparent);
        assert!(!config.decorations);
        assert!(config.always_on_top);
        assert!(config.visible);
        assert!(config.icon.is_some());
    }

    #[test]
    fn test_settings_panel_starts_hidden() {
        // L'adaptateur d'accessibilité s'installe entre la création et le premier
        // affichage : la fenêtre doit donc naître invisible.
        let config = WindowConfig::settings_panel(720, 480);
        assert!(!config.visible);
        assert!(!config.transparent, "le panneau est opaque");
        assert!(!config.decorations);
        assert_eq!(config.title, "Paramètres Gremlin");
    }

    #[test]
    fn test_load_app_icon_loads_valid_icon() {
        let icon = load_app_icon();
        assert!(
            icon.is_some(),
            "L'icône officielle de Gremlin doit être chargée avec succès"
        );
    }

    #[test]
    fn test_load_app_icon_is_cached_and_stable() {
        // Le second appel doit servir la valeur mémorisée : même verdict,
        // sans nouvelle lecture disque ni nouveau décodage PNG.
        assert!(load_app_icon().is_some());
        assert!(load_app_icon().is_some());
        assert!(APP_ICON.get().is_some(), "le cache doit être initialisé");
    }

    #[test]
    fn test_embedded_icon_is_a_decodable_png() {
        assert!(
            EMBEDDED_APP_ICON_PNG.starts_with(&[0x89, b'P', b'N', b'G']),
            "les octets embarqués doivent être un PNG"
        );
        assert!(decode_icon(EMBEDDED_APP_ICON_PNG).is_some());
    }

    #[test]
    fn test_decode_icon_rejects_garbage() {
        assert!(decode_icon(b"ceci n'est pas une image").is_none());
        assert!(decode_icon(&[]).is_none());
    }
}
