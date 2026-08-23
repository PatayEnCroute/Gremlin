//! Thème visuel et jetons de design inspirés de Raycast.

/// Palette de couleurs fidèle au style sombre et contrasté de Raycast.
pub struct RaycastTheme;

impl RaycastTheme {
    /// Fond principal de la fenêtre Raycast (Dark Slate).
    pub const BG_PRIMARY: [u8; 4] = [22, 22, 26, 250];

    /// Fond du panneau secondaire / inspection droite.
    pub const BG_INSPECTOR: [u8; 4] = [28, 28, 34, 255];

    /// Fond de la barre de recherche supérieure.
    pub const BG_SEARCH: [u8; 4] = [18, 18, 22, 255];

    /// Fond de l'élément actuellement sélectionné (Hover/Focus).
    pub const BG_SELECTED: [u8; 4] = [45, 45, 56, 255];

    /// Fond des badges de statut.
    pub const BG_BADGE: [u8; 4] = [38, 38, 48, 255];

    /// Bordure ultra-fine de séparation (1px).
    pub const BORDER: [u8; 4] = [48, 48, 60, 255];

    /// Couleur d'accentuation principale (Rouge corail Raycast).
    pub const ACCENT: [u8; 4] = [229, 72, 77, 255];

    /// Couleur d'accentuation secondaire (Vert Gremlin).
    pub const ACCENT_GREEN: [u8; 4] = [76, 175, 80, 255];

    /// Texte principal (haute lisibilité).
    pub const TEXT_PRIMARY: [u8; 4] = [245, 245, 247, 255];

    /// Texte secondaire / atténué (labels, raccourcis, descriptions).
    pub const TEXT_MUTED: [u8; 4] = [140, 140, 152, 255];

    /// Texte accentué pour les badges actifs.
    pub const TEXT_BADGE_ACTIVE: [u8; 4] = [255, 139, 139, 255];

    /// Fond de la zone d'aperçu Gremlin (Preview Box).
    pub const BG_PREVIEW_BOX: [u8; 4] = [16, 16, 20, 255];

    /// Bordure de l'encart d'aperçu.
    pub const BORDER_PREVIEW_BOX: [u8; 4] = [60, 60, 75, 255];

    // Jauges & Barres de progression pour le profil du Gremlin
    /// Fond des jauges de statistiques.
    pub const BAR_BG: [u8; 4] = [34, 34, 44, 255];
    /// Jauge de faim (Ambre / Jaune).
    pub const BAR_HUNGER: [u8; 4] = [245, 158, 11, 255];
    /// Jauge d'énergie (Bleu électrique).
    pub const BAR_ENERGY: [u8; 4] = [59, 130, 246, 255];
    /// Jauge de bonheur (Rose vif).
    pub const BAR_HAPPINESS: [u8; 4] = [236, 72, 153, 255];
    /// Jauge de santé (Vert émeraude).
    pub const BAR_HEALTH: [u8; 4] = [16, 185, 129, 255];
    /// Jauge d'expérience XP (Violet néon).
    pub const BAR_XP: [u8; 4] = [139, 92, 246, 255];
}

/// Métriques et dimensions de la fenêtre de paramètres style Raycast.
pub struct RaycastLayout;

impl RaycastLayout {
    /// Largeur totale en pixels de la fenêtre Raycast.
    pub const WIDTH: u32 = 480;

    /// Hauteur totale en pixels de la fenêtre Raycast.
    pub const HEIGHT: u32 = 300;

    /// Hauteur de la barre de recherche.
    pub const SEARCH_BAR_HEIGHT: i32 = 36;

    /// Largeur du panneau gauche (liste des commandes / accessoires).
    pub const LEFT_PANE_WIDTH: i32 = 290;

    /// Hauteur de la barre d'action clavier inférieure.
    pub const FOOTER_HEIGHT: i32 = 26;

    /// Hauteur d'un item dans la liste.
    pub const ITEM_ROW_HEIGHT: i32 = 24;

    /// Nombre maximal d'items affichés simultanément dans le panneau gauche.
    pub const MAX_VISIBLE_ITEMS: usize = 9;

    /// Marge horizontale du texte dans la liste de gauche.
    pub const LIST_TEXT_X: i32 = 16;

    /// Décalage vertical du haut de la liste sous la barre de recherche.
    pub const LIST_TOP_OFFSET: i32 = 8;

    /// Abscisse du texte saisi dans la barre de recherche.
    pub const SEARCH_TEXT_X: i32 = 28;

    /// Ordonnée de la ligne de texte de la barre de recherche.
    pub const SEARCH_TEXT_Y: i32 = 14;

    /// Nombre maximal de caractères affichés pour le titre d'un item.
    pub const ITEM_TITLE_MAX_CHARS: usize = 24;

    /// Nombre maximal de caractères affichés pour un message de commit.
    pub const COMMIT_MSG_MAX_CHARS: usize = 24;

    /// Nombre maximal de caractères affichés pour une description d'accessoire.
    pub const DESCRIPTION_MAX_CHARS: usize = 28;

    /// Retrait du badge par rapport au bord droit du panneau gauche.
    pub const BADGE_RIGHT_INSET: i32 = 64;

    /// Largeur du fond d'un badge.
    pub const BADGE_WIDTH: i32 = 52;

    /// Hauteur du fond d'un badge.
    pub const BADGE_HEIGHT: i32 = 12;

    /// Côté de la boîte carrée d'aperçu du familier.
    pub const PREVIEW_BOX_SIZE: i32 = 110;

    /// Côté du sprite natif du familier affiché dans l'aperçu.
    pub const PREVIEW_SPRITE_SIZE: i32 = 64;

    /// Largeur des jauges vitales du panneau d'inspection.
    pub const STAT_BAR_WIDTH: i32 = 110;

    /// Largeur réservée au libellé précédant une jauge.
    pub const STAT_LABEL_WIDTH: i32 = 44;

    /// Épaisseur d'une jauge vitale.
    pub const STAT_BAR_HEIGHT: i32 = 5;

    /// Interligne vertical entre deux jauges.
    pub const STAT_LINE_SPACING: i32 = 12;
}
