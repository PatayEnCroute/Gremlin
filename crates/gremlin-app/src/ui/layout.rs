//! Métriques d'affichage et géométrie du panneau de paramètres.
//!
//! # Pourquoi ce module existe
//!
//! Le panneau était auparavant dessiné en coordonnées de pixels fixes dans un
//! tampon de 480×300 déclaré en unités *logiques*. Sur tout écran à 125 % ou
//! 150 %, la surface de présentation suivait le facteur du système alors que le
//! tampon restait à sa taille nominale : l'image était donc réétirée d'un
//! facteur non entier, ce qui dédoublait irrégulièrement les lignes de glyphes.
//!
//! La correction repose sur une séparation stricte :
//!
//! * la **géométrie** (marges, hauteurs de ligne, largeurs de panneau) est
//!   exprimée en points de conception (`dp`) puis convertie en pixels par un
//!   facteur *fractionnaire*, avec arrondi. Elle suit donc exactement le
//!   facteur d'échelle du système, et tombe toujours sur des pixels entiers ;
//! * les **glyphes** sont des images bitmap : ils ne peuvent être agrandis que
//!   par un facteur *entier* sans se déformer. On choisit donc, parmi les corps
//!   réellement dessinés, celui dont la hauteur approche le mieux la cible.
//!
//! Le tampon est ensuite alloué à la taille physique exacte de la fenêtre, si
//! bien qu'aucun rééchantillonnage n'a lieu à la présentation.

/// Facteur d'échelle minimal accepté, tous facteurs confondus.
///
/// Une valeur plus basse produirait un panneau illisible ; un facteur nul ou
/// négatif lu depuis le système donnerait une fenêtre de dimension nulle.
const MIN_SCALE: f32 = 0.75;

/// Facteur d'échelle maximal accepté, tous facteurs confondus.
///
/// Borne l'empreinte mémoire du tampon : au-delà, un écran 4K à 300 % et une
/// préférence « Grand » demanderaient une allocation démesurée.
const MAX_SCALE: f32 = 4.0;

/// Facteur d'échelle retenu lorsque le système fournit une valeur inutilisable.
const FALLBACK_SCALE: f32 = 1.0;

/// Pénalité, en pixels, appliquée à chaque doublement entier d'un glyphe.
///
/// Sans elle, une cible de 22 px choisirait le petit corps doublé (11 × 2 = 22,
/// écart nul) plutôt que le grand corps natif (20 px, écart de 2 px) : le
/// résultat serait mathématiquement plus proche mais visuellement plus grossier,
/// puisqu'un glyphe doublé a des contours en marches d'escalier. La pénalité
/// exprime cette préférence pour le dessin natif.
const GLYPH_UPSCALE_PENALTY_PX: i32 = 3;

/// Corps de police réellement dessinés dans les tables de glyphes.
///
/// Trois corps suffisent : combinés au doublement entier, ils couvrent une
/// échelle utile de 11 à 60 pixels, ce qui englobe les facteurs système
/// courants (100 %, 125 %, 150 %, 175 %, 200 %) pour les trois préférences de
/// taille de texte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontSize {
    /// Corps 6×11 : en-têtes de section, badges, pied de page.
    Small,
    /// Corps 8×15 : titres d'items et texte courant.
    Medium,
    /// Corps 11×20 : titres de haute densité et affichages agrandis.
    Large,
}

impl FontSize {
    /// Tous les corps dessinés, du plus petit au plus grand.
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    /// Hauteur de la cellule du glyphe, en pixels, à l'échelle 1.
    #[must_use]
    pub const fn cell_height(self) -> i32 {
        match self {
            Self::Small => 11,
            Self::Medium => 15,
            Self::Large => 20,
        }
    }

    /// Largeur de la cellule du glyphe, en pixels, à l'échelle 1.
    #[must_use]
    pub const fn cell_width(self) -> i32 {
        match self {
            Self::Small => 6,
            Self::Medium => 8,
            Self::Large => 11,
        }
    }
}

/// Préférence de taille de texte exposée à l'utilisateur.
///
/// Une police bitmap ne suit pas l'échelle du système en continu : ce réglage
/// rend la main à l'utilisateur là où la mise à l'échelle automatique ne peut
/// atteindre qu'un palier voisin.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum TextSize {
    /// Densité maximale : plus d'items visibles simultanément.
    Compact,
    /// Réglage par défaut, calé sur le facteur d'échelle du système.
    #[default]
    Normal,
    /// Confort de lecture accru, pour vision basse ou grand écran distant.
    Large,
}

impl TextSize {
    /// Multiplicateur appliqué au facteur d'échelle du système.
    #[must_use]
    pub const fn factor(self) -> f32 {
        match self {
            Self::Compact => 0.85,
            Self::Normal => 1.0,
            Self::Large => 1.3,
        }
    }
}

/// Dimensions du panneau et de ses zones, en points de conception.
///
/// Ces valeurs sont indépendantes de l'écran : elles décrivent la maquette, que
/// [`UiMetrics::px`] projette ensuite en pixels réels.
pub struct PanelDp;

impl PanelDp {
    /// Largeur totale du panneau.
    pub const WIDTH: i32 = 720;
    /// Hauteur totale du panneau.
    pub const HEIGHT: i32 = 480;
    /// Hauteur de la barre de recherche supérieure.
    pub const SEARCH_BAR_HEIGHT: i32 = 52;
    /// Hauteur de la barre de raccourcis inférieure.
    pub const FOOTER_HEIGHT: i32 = 34;
    /// Largeur du panneau gauche portant la liste.
    pub const LEFT_PANE_WIDTH: i32 = 440;
    /// Hauteur d'une ligne d'item, titre et sous-titre compris.
    pub const ROW_HEIGHT: i32 = 44;
    /// Hauteur d'une ligne d'en-tête de section.
    pub const SECTION_HEADER_HEIGHT: i32 = 26;
    /// Marge horizontale intérieure des deux panneaux.
    pub const PANE_PADDING: i32 = 14;
    /// Largeur de l'ascenseur de la liste.
    pub const SCROLLBAR_WIDTH: i32 = 4;
    /// Hauteur du texte courant, qui pilote le choix du corps de police.
    pub const BODY_TEXT_HEIGHT: i32 = 15;
    /// Hauteur du texte secondaire (sous-titres, badges, en-têtes).
    pub const CAPTION_TEXT_HEIGHT: i32 = 11;
    /// Épaisseur du liseré d'accent marquant la ligne sélectionnée.
    pub const SELECTION_MARKER_WIDTH: i32 = 3;
    /// Marge intérieure horizontale d'un badge, de chaque côté du texte.
    pub const BADGE_PADDING_X: i32 = 6;
    /// Marge intérieure verticale d'un badge.
    pub const BADGE_PADDING_Y: i32 = 3;
    /// Côté de la zone réservée à l'aperçu du familier.
    ///
    /// Zone, et non boîte : rien n'y est peint, le familier reposant directement
    /// sur le fond de l'inspecteur.
    pub const PREVIEW_AREA: i32 = 132;
    /// Côté du sprite natif du familier.
    pub const PREVIEW_SPRITE: i32 = 64;
    /// Longueur d'une jauge vitale.
    pub const STAT_BAR_WIDTH: i32 = 96;
    /// Épaisseur d'une jauge vitale.
    pub const STAT_BAR_HEIGHT: i32 = 6;
    /// Largeur réservée au libellé précédant une jauge.
    pub const STAT_LABEL_WIDTH: i32 = 52;
    /// Interligne entre deux jauges vitales.
    pub const STAT_LINE_SPACING: i32 = 16;
    /// Interligne du texte du panneau d'inspection.
    pub const INSPECTOR_LINE_SPACING: i32 = 15;
    /// Abscisse du texte saisi dans la barre de recherche.
    pub const SEARCH_TEXT_X: i32 = 40;
}

/// Métriques résolues pour un écran et une préférence donnés.
///
/// Construite une fois par ouverture du panneau et à chaque changement de
/// facteur d'échelle, puis consultée par le moteur de rendu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiMetrics {
    scale: f32,
    body: GlyphChoice,
    caption: GlyphChoice,
}

/// Corps de police retenu et son facteur d'agrandissement entier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphChoice {
    /// Corps dessiné à utiliser.
    pub face: FontSize,
    /// Facteur d'agrandissement entier appliqué au bitmap.
    pub upscale: u32,
}

impl GlyphChoice {
    /// Hauteur effective du texte rendu, en pixels.
    #[must_use]
    pub const fn height_px(self) -> i32 {
        self.face.cell_height() * (self.upscale as i32)
    }

    /// Largeur effective d'une cellule de glyphe, en pixels.
    #[must_use]
    pub const fn width_px(self) -> i32 {
        self.face.cell_width() * (self.upscale as i32)
    }
}

/// Agrandissement entier maximal d'un glyphe.
///
/// Au-delà, les marches d'escalier du bitmap dominent le dessin de la lettre.
const MAX_GLYPH_UPSCALE: u32 = 3;

impl UiMetrics {
    /// Résout les métriques pour un facteur d'échelle système et une préférence.
    ///
    /// `dpi_scale` provient de `winit::window::Window::scale_factor`. Toute
    /// valeur non finie, nulle ou négative est neutralisée : la frontière de
    /// confiance s'applique aussi aux valeurs venues du système de fenêtrage,
    /// et `f32::clamp` propagerait un `NaN` sans le signaler.
    #[must_use]
    pub fn for_display(dpi_scale: f64, preference: TextSize) -> Self {
        let scale = sanitize_scale(dpi_scale * f64::from(preference.factor()));

        Self {
            scale,
            body: pick_glyph(scaled_px(PanelDp::BODY_TEXT_HEIGHT, scale)),
            caption: pick_glyph(scaled_px(PanelDp::CAPTION_TEXT_HEIGHT, scale)),
        }
    }

    /// Facteur d'échelle effectivement appliqué, bornes comprises.
    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// Convertit une dimension en points de conception vers des pixels.
    ///
    /// L'arrondi garantit que toute coordonnée tombe sur un pixel entier :
    /// c'est ce qui évite les bordures d'un demi-pixel et les lignes floues.
    #[must_use]
    pub fn px(&self, dp: i32) -> i32 {
        scaled_px(dp, self.scale)
    }

    /// Corps de police du texte courant.
    #[must_use]
    pub const fn body_glyph(&self) -> GlyphChoice {
        self.body
    }

    /// Corps de police du texte secondaire.
    #[must_use]
    pub const fn caption_glyph(&self) -> GlyphChoice {
        self.caption
    }

    /// Dimensions du tampon de pixels à allouer pour le panneau.
    ///
    /// Le tampon est dimensionné en pixels *physiques* : la présentation est
    /// alors un transfert un pour un, sans le moindre rééchantillonnage.
    #[must_use]
    pub fn buffer_size(&self) -> (u32, u32) {
        let width = self.px(PanelDp::WIDTH).max(1);
        let height = self.px(PanelDp::HEIGHT).max(1);
        (width as u32, height as u32)
    }

    /// Nombre de lignes d'items affichables dans le panneau gauche.
    ///
    /// Calculé depuis la place réellement disponible, et non figé par une
    /// constante : le panneau montrait auparavant neuf lignes quelle que soit
    /// sa hauteur.
    #[must_use]
    pub fn visible_rows(&self) -> usize {
        let list_height = self.px(PanelDp::HEIGHT)
            - self.px(PanelDp::SEARCH_BAR_HEIGHT)
            - self.px(PanelDp::FOOTER_HEIGHT);
        let row_height = self.px(PanelDp::ROW_HEIGHT).max(1);

        (list_height / row_height).max(1) as usize
    }

    /// Ordonnée, en pixels, du haut de la ligne d'indice `row`.
    #[must_use]
    pub fn row_top(&self, row: usize) -> i32 {
        let offset = i32::try_from(row).unwrap_or(i32::MAX / 2);
        self.px(PanelDp::SEARCH_BAR_HEIGHT)
            .saturating_add(offset.saturating_mul(self.px(PanelDp::ROW_HEIGHT)))
    }

    /// Indice de la ligne visible située sous le point `(x, y)`, s'il y en a une.
    ///
    /// Renvoie `None` hors du panneau gauche, dans la barre de recherche ou
    /// dans le pied de page : le survol et le clic partagent ainsi exactement
    /// la même géométrie que le rendu.
    #[must_use]
    pub fn row_at(&self, x: i32, y: i32, visible_rows: usize) -> Option<usize> {
        let top = self.px(PanelDp::SEARCH_BAR_HEIGHT);
        let bottom = self.px(PanelDp::HEIGHT) - self.px(PanelDp::FOOTER_HEIGHT);

        if x < 0 || x >= self.px(PanelDp::LEFT_PANE_WIDTH) || y < top || y >= bottom {
            return None;
        }

        let row_height = self.px(PanelDp::ROW_HEIGHT).max(1);
        let row = ((y - top) / row_height) as usize;

        (row < visible_rows).then_some(row)
    }
}

/// Neutralise un facteur d'échelle venu du système, puis le borne.
fn sanitize_scale(raw: f64) -> f32 {
    if !raw.is_finite() || raw <= 0.0 {
        return FALLBACK_SCALE;
    }

    let scale = raw as f32;
    if scale.is_finite() {
        scale.clamp(MIN_SCALE, MAX_SCALE)
    } else {
        FALLBACK_SCALE
    }
}

/// Projette une dimension en points de conception vers des pixels entiers.
///
/// La conversion vers `f32` est exacte pour toutes les valeurs manipulées ici :
/// les points de conception décrivent une maquette de 720×480 et restent très
/// en dessous du seuil de 2^24 au-delà duquel `f32` perd des entiers.
#[allow(clippy::cast_precision_loss)]
fn scaled_px(dp: i32, scale: f32) -> i32 {
    let value = (dp as f32) * scale;
    if value.is_finite() {
        value.round() as i32
    } else {
        dp
    }
}

/// Choisit le corps dessiné et l'agrandissement les plus proches de `target_px`.
///
/// Le dessin natif est privilégié : voir [`GLYPH_UPSCALE_PENALTY_PX`].
fn pick_glyph(target_px: i32) -> GlyphChoice {
    let target = target_px.max(1);
    let mut best = GlyphChoice {
        face: FontSize::Small,
        upscale: 1,
    };
    let mut best_cost = i32::MAX;

    for face in FontSize::ALL {
        for upscale in 1..=MAX_GLYPH_UPSCALE {
            let height = face.cell_height().saturating_mul(upscale as i32);
            let penalty = (upscale as i32 - 1).saturating_mul(GLYPH_UPSCALE_PENALTY_PX);
            let cost = (height - target).abs().saturating_add(penalty);

            if cost < best_cost {
                best_cost = cost;
                best = GlyphChoice { face, upscale };
            }
        }
    }

    best
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_hostile_scale_factors_are_neutralised() {
        // La frontière de confiance couvre aussi le système de fenêtrage : un
        // facteur nul produirait une fenêtre de dimension nulle, et `NaN` se
        // propagerait silencieusement à travers `clamp`.
        for hostile in [
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            f64::MIN,
        ] {
            let metrics = UiMetrics::for_display(hostile, TextSize::Normal);
            assert!(
                metrics.scale().is_finite(),
                "facteur non fini accepté depuis {hostile}"
            );
            assert!(
                (MIN_SCALE..=MAX_SCALE).contains(&metrics.scale()),
                "facteur hors bornes depuis {hostile} : {}",
                metrics.scale()
            );

            let (w, h) = metrics.buffer_size();
            assert!(w > 0 && h > 0, "tampon de dimension nulle depuis {hostile}");
        }
    }

    #[test]
    fn test_geometry_follows_the_system_scale_exactly() {
        // Le correctif central : à 150 %, la géométrie suit le facteur au
        // pixel près au lieu de laisser la présentation réétirer l'image.
        let at_100 = UiMetrics::for_display(1.0, TextSize::Normal);
        let at_150 = UiMetrics::for_display(1.5, TextSize::Normal);

        assert_eq!(at_100.px(PanelDp::WIDTH), 720);
        assert_eq!(at_150.px(PanelDp::WIDTH), 1080);
        assert_eq!(at_150.buffer_size(), (1080, 720));
    }

    #[test]
    fn test_native_face_is_preferred_over_a_doubled_one() {
        // Régression de conception : à 150 %, la cible du texte courant tombe
        // à 22,5 px. Le petit corps doublé (11 × 2 = 22) est numériquement plus
        // proche, mais visuellement grossier ; le grand corps natif (20 px)
        // doit gagner.
        let glyph = UiMetrics::for_display(1.5, TextSize::Normal).body_glyph();
        assert_eq!(glyph.face, FontSize::Large);
        assert_eq!(glyph.upscale, 1);
    }

    #[test]
    fn test_chosen_glyph_height_grows_with_the_scale() {
        let mut previous = 0;
        for scale in [0.75_f64, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0] {
            let height = UiMetrics::for_display(scale, TextSize::Normal)
                .body_glyph()
                .height_px();
            assert!(
                height >= previous,
                "la hauteur du texte régresse à l'échelle {scale} : {height} < {previous}"
            );
            previous = height;
        }
    }

    #[test]
    fn test_text_size_preference_orders_the_result() {
        let compact = UiMetrics::for_display(1.0, TextSize::Compact);
        let normal = UiMetrics::for_display(1.0, TextSize::Normal);
        let large = UiMetrics::for_display(1.0, TextSize::Large);

        assert!(compact.scale() < normal.scale());
        assert!(normal.scale() < large.scale());
        assert!(compact.visible_rows() >= large.visible_rows());
    }

    #[test]
    fn test_hit_test_round_trips_with_the_rendered_geometry() {
        // Le survol et le clic doivent partager la géométrie du rendu : un
        // décalage d'un pixel suffirait à activer la mauvaise ligne.
        for scale in [1.0_f64, 1.5, 2.0] {
            let metrics = UiMetrics::for_display(scale, TextSize::Normal);
            let rows = metrics.visible_rows();

            for row in 0..rows {
                let top = metrics.row_top(row);
                let middle = top + metrics.px(PanelDp::ROW_HEIGHT) / 2;
                assert_eq!(
                    metrics.row_at(metrics.px(PanelDp::PANE_PADDING), middle, rows),
                    Some(row),
                    "aller-retour rompu à l'échelle {scale}, ligne {row}"
                );
            }
        }
    }

    #[test]
    fn test_hit_test_rejects_the_chrome_and_the_inspector() {
        let metrics = UiMetrics::for_display(1.0, TextSize::Normal);
        let rows = metrics.visible_rows();
        let inside_x = metrics.px(PanelDp::PANE_PADDING);
        let list_top = metrics.px(PanelDp::SEARCH_BAR_HEIGHT);

        // Barre de recherche, pied de page, panneau d'inspection, hors cadre.
        assert_eq!(metrics.row_at(inside_x, list_top - 1, rows), None);
        assert_eq!(
            metrics.row_at(inside_x, metrics.px(PanelDp::HEIGHT) - 1, rows),
            None
        );
        assert_eq!(
            metrics.row_at(metrics.px(PanelDp::LEFT_PANE_WIDTH), list_top + 1, rows),
            None
        );
        assert_eq!(metrics.row_at(-1, list_top + 1, rows), None);
    }

    #[test]
    fn test_visible_rows_is_never_degenerate() {
        for scale in [0.75_f64, 1.0, 2.0, 4.0] {
            for pref in [TextSize::Compact, TextSize::Normal, TextSize::Large] {
                let metrics = UiMetrics::for_display(scale, pref);
                assert!(
                    metrics.visible_rows() >= 1,
                    "aucune ligne affichable à l'échelle {scale} pour {pref:?}"
                );
            }
        }
    }

    #[test]
    fn test_row_top_does_not_overflow_on_absurd_indices() {
        let metrics = UiMetrics::for_display(1.0, TextSize::Normal);
        // Un indice absurde ne doit pas déborder : `panic = "abort"` en release
        // transformerait un dépassement arithmétique en arrêt du processus.
        let _ = metrics.row_top(usize::MAX);
        let _ = metrics.row_at(i32::MAX, i32::MAX, usize::MAX);
        let _ = metrics.row_at(i32::MIN, i32::MIN, 0);
    }
}
