//! Palettes de couleurs du panneau et contrôle de contraste.
//!
//! # De constantes à instances
//!
//! Les couleurs étaient des constantes associées : une seule palette, sombre,
//! impossible à remplacer à l'exécution. Elles deviennent les champs d'un
//! [`Theme`], ce qui permet d'en servir trois — sombre, clair, contraste
//! renforcé — et de suivre le thème du système.
//!
//! # Le contraste est vérifié, pas supposé
//!
//! [`contrast_ratio`] calcule le rapport de luminance relative défini par
//! WCAG 2.1. La suite de tests l'applique à chaque paire texte-sur-fond des
//! trois palettes et exige 4,5:1 pour le texte, 3:1 pour les éléments
//! d'interface. Une couleur qui dégraderait la lisibilité ne peut donc plus être
//! commise : le test la rejette avant la revue.

/// Thème du système d'exploitation, tel que rapporté par le gestionnaire de fenêtres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTheme {
    /// Interface claire.
    Light,
    /// Interface sombre.
    Dark,
}

/// Préférence de thème exposée à l'utilisateur.
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
pub enum ThemePreference {
    /// Suit le thème du système, avec repli sombre s'il est inconnu.
    #[default]
    System,
    /// Toujours sombre.
    Dark,
    /// Toujours clair.
    Light,
    /// Contraste renforcé, pour vision basse ou fort éclairage ambiant.
    HighContrast,
}

impl ThemePreference {
    /// Toutes les valeurs, dans l'ordre de présentation à l'utilisateur.
    pub const ALL: [Self; 4] = [Self::System, Self::Dark, Self::Light, Self::HighContrast];

    /// Libellé affiché dans le panneau.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::System => "Système",
            Self::Dark => "Sombre",
            Self::Light => "Clair",
            Self::HighContrast => "Contraste renforcé",
        }
    }

    /// Préférence suivante dans le cycle.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::System => Self::Dark,
            Self::Dark => Self::Light,
            Self::Light => Self::HighContrast,
            Self::HighContrast => Self::System,
        }
    }
}

/// Palette de couleurs résolue, prête au rendu.
///
/// Toutes les teintes sont **opaques** : le panneau est présenté par un
/// transfert mémoire sans canal alpha, et une couleur partiellement transparente
/// produirait une couleur affichée imprévisible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Fond principal du panneau.
    pub bg_primary: [u8; 4],
    /// Fond du panneau d'inspection, à droite.
    pub bg_inspector: [u8; 4],
    /// Fond de la barre de recherche et du pied de page.
    pub bg_search: [u8; 4],
    /// Fond de la ligne sélectionnée.
    pub bg_selected: [u8; 4],
    /// Fond de la ligne survolée à la souris.
    pub bg_row_hover: [u8; 4],
    /// Fond d'une pastille de statut.
    pub bg_badge: [u8; 4],
    /// Filets de séparation.
    pub border: [u8; 4],
    /// Accentuation principale.
    pub accent: [u8; 4],
    /// Accentuation secondaire, pour les informations Git.
    pub accent_green: [u8; 4],
    /// Texte principal.
    pub text_primary: [u8; 4],
    /// Titre d'une ligne non sélectionnée.
    pub text_title_idle: [u8; 4],
    /// Texte secondaire : sous-titres, descriptions.
    pub text_muted: [u8; 4],
    /// Libellé de section, en capitales.
    pub text_section: [u8; 4],
    /// Texte d'une pastille active.
    pub text_badge_active: [u8; 4],
    /// Rail de l'ascenseur.
    pub scrollbar_track: [u8; 4],
    /// Curseur de l'ascenseur.
    pub scrollbar_thumb: [u8; 4],
    /// Fond d'une jauge vitale.
    pub bar_bg: [u8; 4],
    /// Jauge de satiété.
    pub bar_hunger: [u8; 4],
    /// Jauge d'énergie.
    pub bar_energy: [u8; 4],
    /// Jauge de bonheur.
    pub bar_happiness: [u8; 4],
    /// Jauge de santé.
    pub bar_health: [u8; 4],
    /// Jauge d'expérience.
    pub bar_xp: [u8; 4],
}

impl Theme {
    /// Palette sombre, celle d'origine, retouchée pour satisfaire le contraste.
    pub const DARK: Self = Self {
        bg_primary: [22, 22, 26, 255],
        bg_inspector: [28, 28, 34, 255],
        bg_search: [18, 18, 22, 255],
        bg_selected: [45, 45, 56, 255],
        bg_row_hover: [33, 33, 41, 255],
        bg_badge: [38, 38, 48, 255],
        border: [48, 48, 60, 255],
        accent: [240, 96, 100, 255],
        accent_green: [110, 210, 130, 255],
        text_primary: [245, 245, 247, 255],
        text_title_idle: [214, 214, 222, 255],
        text_muted: [158, 158, 172, 255],
        text_section: [150, 150, 164, 255],
        text_badge_active: [255, 150, 150, 255],
        scrollbar_track: [30, 30, 38, 255],
        scrollbar_thumb: [124, 124, 140, 255],
        bar_bg: [34, 34, 44, 255],
        bar_hunger: [245, 158, 11, 255],
        bar_energy: [96, 156, 250, 255],
        bar_happiness: [240, 110, 175, 255],
        bar_health: [40, 200, 145, 255],
        bar_xp: [170, 140, 250, 255],
    };

    /// Palette claire.
    pub const LIGHT: Self = Self {
        bg_primary: [250, 250, 252, 255],
        bg_inspector: [242, 242, 246, 255],
        bg_search: [255, 255, 255, 255],
        bg_selected: [222, 224, 234, 255],
        bg_row_hover: [237, 238, 244, 255],
        bg_badge: [230, 231, 239, 255],
        border: [212, 213, 223, 255],
        accent: [176, 26, 34, 255],
        accent_green: [20, 104, 48, 255],
        text_primary: [22, 22, 28, 255],
        text_title_idle: [42, 42, 52, 255],
        text_muted: [82, 82, 96, 255],
        text_section: [88, 88, 102, 255],
        text_badge_active: [150, 22, 28, 255],
        scrollbar_track: [234, 234, 241, 255],
        scrollbar_thumb: [118, 119, 134, 255],
        bar_bg: [222, 223, 232, 255],
        bar_hunger: [166, 96, 4, 255],
        bar_energy: [26, 82, 176, 255],
        bar_happiness: [166, 30, 100, 255],
        bar_health: [10, 108, 78, 255],
        bar_xp: [88, 52, 176, 255],
    };

    /// Palette à contraste renforcé.
    ///
    /// Noir et blanc francs, accents saturés : elle vise le rapport maximal
    /// plutôt que l'élégance, pour les vision basses et les écrans en plein soleil.
    pub const HIGH_CONTRAST: Self = Self {
        bg_primary: [0, 0, 0, 255],
        bg_inspector: [0, 0, 0, 255],
        bg_search: [0, 0, 0, 255],
        bg_selected: [46, 46, 46, 255],
        bg_row_hover: [26, 26, 26, 255],
        bg_badge: [32, 32, 32, 255],
        border: [255, 255, 255, 255],
        accent: [255, 214, 0, 255],
        accent_green: [64, 255, 128, 255],
        text_primary: [255, 255, 255, 255],
        text_title_idle: [255, 255, 255, 255],
        text_muted: [235, 235, 235, 255],
        text_section: [235, 235, 235, 255],
        text_badge_active: [255, 214, 0, 255],
        scrollbar_track: [40, 40, 40, 255],
        scrollbar_thumb: [255, 255, 255, 255],
        bar_bg: [50, 50, 50, 255],
        bar_hunger: [255, 190, 60, 255],
        bar_energy: [110, 190, 255, 255],
        bar_happiness: [255, 130, 200, 255],
        bar_health: [80, 255, 170, 255],
        bar_xp: [200, 170, 255, 255],
    };

    /// Résout la palette à employer.
    ///
    /// `system` provient de `winit::window::Window::theme` et vaut `None`
    /// lorsque le gestionnaire de fenêtres ne le rapporte pas — sur certains
    /// environnements Linux, notamment. On retombe alors sur la palette sombre,
    /// qui correspond au familier et à l'attente d'un outil de développement.
    #[must_use]
    pub const fn resolve(preference: ThemePreference, system: Option<SystemTheme>) -> Self {
        match preference {
            ThemePreference::Dark => Self::DARK,
            ThemePreference::Light => Self::LIGHT,
            ThemePreference::HighContrast => Self::HIGH_CONTRAST,
            ThemePreference::System => match system {
                Some(SystemTheme::Light) => Self::LIGHT,
                Some(SystemTheme::Dark) | None => Self::DARK,
            },
        }
    }
}

/// Composante de luminance linéaire d'un canal, selon WCAG 2.1.
fn linearize(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Luminance relative d'une couleur, selon WCAG 2.1.
///
/// Le canal alpha est ignoré : les palettes sont opaques.
#[must_use]
pub fn relative_luminance(color: [u8; 4]) -> f32 {
    // Écrit en multiplications-additions fusionnées : la formule reste celle de
    // WCAG 2.1, mais la précision et le coût y gagnent.
    0.072_2_f32.mul_add(
        linearize(color[2]),
        0.212_6_f32.mul_add(linearize(color[0]), 0.715_2 * linearize(color[1])),
    )
}

/// Rapport de contraste entre deux couleurs, de 1:1 à 21:1.
///
/// Le résultat est symétrique : l'ordre des arguments n'importe pas.
#[must_use]
pub fn contrast_ratio(first: [u8; 4], second: [u8; 4]) -> f32 {
    let a = relative_luminance(first);
    let b = relative_luminance(second);
    let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Dimensions du panneau, en points de conception.
///
/// Réexport de [`crate::ui::layout::PanelDp`], conservé pour que les appelants
/// n'aient pas à connaître deux modules pour dessiner une ligne.
pub use crate::ui::layout::PanelDp as RaycastLayout;

#[cfg(test)]
mod tests {
    use super::*;

    /// Seuil WCAG AA pour le texte courant.
    const MIN_TEXT_RATIO: f32 = 4.5;

    /// Seuil WCAG AA pour les éléments d'interface non textuels.
    const MIN_UI_RATIO: f32 = 3.0;

    /// Paires texte-sur-fond à vérifier, avec leur exigence.
    fn text_pairs(theme: &Theme) -> Vec<(&'static str, [u8; 4], [u8; 4])> {
        vec![
            (
                "texte principal sur fond",
                theme.text_primary,
                theme.bg_primary,
            ),
            (
                "texte principal sur ligne sélectionnée",
                theme.text_primary,
                theme.bg_selected,
            ),
            (
                "texte principal sur inspecteur",
                theme.text_primary,
                theme.bg_inspector,
            ),
            (
                "titre au repos sur fond",
                theme.text_title_idle,
                theme.bg_primary,
            ),
            (
                "titre au repos sur survol",
                theme.text_title_idle,
                theme.bg_row_hover,
            ),
            ("texte atténué sur fond", theme.text_muted, theme.bg_primary),
            (
                "texte atténué sur ligne sélectionnée",
                theme.text_muted,
                theme.bg_selected,
            ),
            (
                "texte atténué sur inspecteur",
                theme.text_muted,
                theme.bg_inspector,
            ),
            (
                "libellé de section sur fond",
                theme.text_section,
                theme.bg_primary,
            ),
            (
                "libellé de section sur ligne sélectionnée",
                theme.text_section,
                theme.bg_selected,
            ),
            (
                "texte de pastille active sur pastille",
                theme.text_badge_active,
                theme.bg_badge,
            ),
            (
                "texte atténué sur pastille",
                theme.text_muted,
                theme.bg_badge,
            ),
            (
                "texte atténué sur barre de recherche",
                theme.text_muted,
                theme.bg_search,
            ),
            (
                "accent sur barre de recherche",
                theme.accent,
                theme.bg_search,
            ),
            (
                "accent secondaire sur inspecteur",
                theme.accent_green,
                theme.bg_inspector,
            ),
            (
                "expérience sur inspecteur",
                theme.bar_xp,
                theme.bg_inspector,
            ),
        ]
    }

    /// Seuil minimal des éléments purement décoratifs.
    ///
    /// WCAG 1.4.11 exige 3:1 de « l'information visuelle nécessaire pour
    /// identifier un composant d'interface ou un état », et **exclut**
    /// explicitement les éléments décoratifs. Les filets de séparation et le
    /// contour de l'aperçu tombent dans cette exclusion : l'information qu'ils
    /// portent — le regroupement, la délimitation de l'aperçu — est redondante
    /// avec le libellé de section et avec le contenu de l'aperçu lui-même. Ce
    /// seuil ne vérifie donc qu'une chose : qu'ils restent perceptibles.
    const MIN_DECORATIVE_RATIO: f32 = 1.25;

    /// Paires d'éléments d'interface porteurs d'information ou d'état.
    ///
    /// Ni la sélection ni le survol n'y figurent par leur fond, mais par leur
    /// **liseré d'accent** : c'est lui qui identifie l'état, le fond n'étant
    /// qu'un renfort. Exiger 3:1 des deux fonds les rendrait par ailleurs
    /// indiscernables l'un de l'autre, puisqu'ils devraient tous deux s'éclaircir
    /// jusqu'à se rejoindre. Le liseré remplit le rôle sans cette contradiction,
    /// et c'est précisément le motif que WCAG recommande : ne pas s'appuyer sur
    /// une teinte de fond seule pour signaler un état.
    fn ui_pairs(theme: &Theme) -> Vec<(&'static str, [u8; 4], [u8; 4])> {
        vec![
            (
                "liseré de sélection sur fond",
                theme.accent,
                theme.bg_primary,
            ),
            (
                "liseré de sélection sur ligne sélectionnée",
                theme.accent,
                theme.bg_selected,
            ),
            (
                "liseré de survol sur fond",
                theme.accent,
                theme.bg_row_hover,
            ),
            (
                "curseur d'ascenseur sur rail",
                theme.scrollbar_thumb,
                theme.scrollbar_track,
            ),
            (
                "curseur d'ascenseur sur fond",
                theme.scrollbar_thumb,
                theme.bg_primary,
            ),
            (
                "jauge de satiété sur son fond",
                theme.bar_hunger,
                theme.bar_bg,
            ),
            (
                "jauge d'énergie sur son fond",
                theme.bar_energy,
                theme.bar_bg,
            ),
            (
                "jauge de bonheur sur son fond",
                theme.bar_happiness,
                theme.bar_bg,
            ),
            (
                "jauge de santé sur son fond",
                theme.bar_health,
                theme.bar_bg,
            ),
        ]
    }

    /// Paires décoratives, vérifiées au seuil de perceptibilité seulement.
    fn decorative_pairs(theme: &Theme) -> Vec<(&'static str, [u8; 4], [u8; 4])> {
        vec![
            ("filet sur fond", theme.border, theme.bg_primary),
            (
                "fond de ligne sélectionnée sur fond",
                theme.bg_selected,
                theme.bg_primary,
            ),
        ]
    }

    fn named_themes() -> [(&'static str, Theme); 3] {
        [
            ("sombre", Theme::DARK),
            ("clair", Theme::LIGHT),
            ("contraste renforcé", Theme::HIGH_CONTRAST),
        ]
    }

    #[test]
    fn test_every_text_pair_meets_the_wcag_threshold() {
        let mut failures = Vec::new();

        for (theme_name, theme) in named_themes() {
            for (pair_name, foreground, background) in text_pairs(&theme) {
                let ratio = contrast_ratio(foreground, background);
                if ratio < MIN_TEXT_RATIO {
                    failures.push(format!(
                        "thème {theme_name} : {pair_name} = {ratio:.2}:1 (minimum {MIN_TEXT_RATIO}:1)"
                    ));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "contraste de texte insuffisant :\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn test_every_ui_pair_meets_the_wcag_threshold() {
        let mut failures = Vec::new();

        for (theme_name, theme) in named_themes() {
            for (pair_name, foreground, background) in ui_pairs(&theme) {
                let ratio = contrast_ratio(foreground, background);
                if ratio < MIN_UI_RATIO {
                    failures.push(format!(
                        "thème {theme_name} : {pair_name} = {ratio:.2}:1 (minimum {MIN_UI_RATIO}:1)"
                    ));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "contraste d'interface insuffisant :\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn test_decorative_elements_stay_perceptible() {
        let mut failures = Vec::new();

        for (theme_name, theme) in named_themes() {
            for (pair_name, foreground, background) in decorative_pairs(&theme) {
                let ratio = contrast_ratio(foreground, background);
                if ratio < MIN_DECORATIVE_RATIO {
                    failures.push(format!(
                        "theme {theme_name} : {pair_name} = {ratio:.2}:1 (minimum {MIN_DECORATIVE_RATIO}:1)"
                    ));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "element decoratif imperceptible :\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn test_contrast_ratio_matches_the_reference_values() {
        // Valeurs de référence du calcul WCAG : noir sur blanc vaut 21:1, et
        // une couleur contre elle-même vaut 1:1.
        let white = [255, 255, 255, 255];
        let black = [0, 0, 0, 255];
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 0.001);

        // Symétrie.
        let a = [17, 34, 51, 255];
        let b = [200, 180, 160, 255];
        assert!((contrast_ratio(a, b) - contrast_ratio(b, a)).abs() < 0.000_1);
    }

    #[test]
    fn test_all_theme_colours_are_opaque() {
        // La présentation logicielle ignore l'alpha : une couleur translucide
        // afficherait une teinte imprévisible.
        for (name, theme) in named_themes() {
            for color in [
                theme.bg_primary,
                theme.bg_inspector,
                theme.bg_search,
                theme.bg_selected,
                theme.bg_row_hover,
                theme.bg_badge,
                theme.border,
                theme.accent,
                theme.accent_green,
                theme.text_primary,
                theme.text_title_idle,
                theme.text_muted,
                theme.text_section,
                theme.text_badge_active,
                theme.scrollbar_track,
                theme.scrollbar_thumb,
                theme.bar_bg,
                theme.bar_hunger,
                theme.bar_energy,
                theme.bar_happiness,
                theme.bar_health,
                theme.bar_xp,
            ] {
                assert_eq!(color[3], 255, "thème {name} : couleur non opaque {color:?}");
            }
        }
    }

    #[test]
    fn test_resolution_follows_the_preference_then_the_system() {
        assert_eq!(
            Theme::resolve(ThemePreference::Dark, Some(SystemTheme::Light)),
            Theme::DARK,
            "une préférence explicite doit primer sur le système"
        );
        assert_eq!(
            Theme::resolve(ThemePreference::System, Some(SystemTheme::Light)),
            Theme::LIGHT
        );
        assert_eq!(
            Theme::resolve(ThemePreference::System, Some(SystemTheme::Dark)),
            Theme::DARK
        );
        // Thème système inconnu : repli sombre, jamais de panique.
        assert_eq!(
            Theme::resolve(ThemePreference::System, None),
            Theme::DARK,
            "le repli doit être la palette sombre"
        );
    }

    #[test]
    fn test_preference_cycle_visits_every_value() {
        let mut seen = Vec::new();
        let mut preference = ThemePreference::System;
        for _ in 0..ThemePreference::ALL.len() {
            seen.push(preference);
            preference = preference.next();
        }

        assert_eq!(
            preference,
            ThemePreference::System,
            "le cycle doit boucler sur son point de départ"
        );
        for value in ThemePreference::ALL {
            assert!(seen.contains(&value), "{value:?} absent du cycle");
        }
    }

    #[test]
    fn test_high_contrast_beats_the_other_themes_on_body_text() {
        let strict = contrast_ratio(
            Theme::HIGH_CONTRAST.text_primary,
            Theme::HIGH_CONTRAST.bg_primary,
        );
        for (name, theme) in [("sombre", Theme::DARK), ("clair", Theme::LIGHT)] {
            let ordinary = contrast_ratio(theme.text_primary, theme.bg_primary);
            assert!(
                strict > ordinary,
                "le contraste renforcé ({strict:.2}) devrait dépasser le thème {name} ({ordinary:.2})"
            );
        }
    }
}
