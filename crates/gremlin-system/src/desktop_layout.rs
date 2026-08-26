//! Topologie des écrans et zones de travail, normalisée et bornée.
//!
//! Cette façade expose des **faits** : où sont les écrans, quelle portion reste
//! libre de barres système, et à quelle densité. Elle ne choisit ni ancre ni
//! trajectoire — c'est `gremlin-app` qui décide, à partir d'un moteur purement
//! géométrique.
//!
//! ## Ce que le système de fenêtrage donne, et ce qu'il ne donne pas
//!
//! `winit` sait énumérer les moniteurs et leurs limites sur les trois systèmes.
//! Il ne connaît en revanche pas la **zone de travail** — les limites amputées de
//! la barre des tâches, du Dock ou d'un panneau. Seul l'OS la publie, et chacun
//! à sa façon. Ce module part donc des limites livrées par `winit` et n'appelle
//! le natif que pour cette information-là, ce qui réduit la surface de FFI à une
//! seule fonction par plateforme.
//!
//! ## Wayland
//!
//! Un client Wayland ordinaire ne connaît ni sa position globale, ni celle des
//! autres surfaces, et ne choisit pas librement où sa fenêtre apparaît. La façade
//! renvoie alors [`DesktopLayoutState::Unavailable`] et l'interface désactive le
//! magnétisme, plutôt que d'annoncer un support qui n'existe pas.

use crate::error::SystemError;
use serde::{Deserialize, Serialize};

/// Nombre maximal d'écrans retenus.
///
/// Au-delà, la liste vient d'une lecture corrompue plutôt que d'un poste réel.
pub const MAX_DISPLAYS: usize = 16;

/// Longueur maximale d'un nom d'écran, **en caractères** et non en octets.
pub const MAX_DISPLAY_NAME_CHARS: usize = 64;

/// Dimension maximale acceptée pour un écran, en pixels physiques.
const MAX_DISPLAY_DIMENSION: u32 = 65_536;

/// Facteur d'échelle minimal accepté, en millièmes (0,25×).
const MIN_SCALE_FACTOR_MILLI: u32 = 250;

/// Facteur d'échelle maximal accepté, en millièmes (8×).
const MAX_SCALE_FACTOR_MILLI: u32 = 8_000;

/// Facteur d'échelle de repli lorsqu'une valeur non finie est lue.
const DEFAULT_SCALE_FACTOR_MILLI: u32 = 1_000;

/// Rectangle en pixels physiques, à origine signée.
///
/// L'origine peut être négative : sous Windows comme sous X11, un écran placé à
/// gauche ou au-dessus de l'écran principal a des coordonnées négatives. Tous les
/// calculs de bord passent par des `i64` pour que `x + width` ne déborde jamais.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhysicalRect {
    /// Abscisse du coin supérieur gauche.
    pub x: i32,
    /// Ordonnée du coin supérieur gauche.
    pub y: i32,
    /// Largeur en pixels physiques.
    pub width: u32,
    /// Hauteur en pixels physiques.
    pub height: u32,
}

impl PhysicalRect {
    /// Construit un rectangle depuis une origine et une taille.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Abscisse du bord droit, exclue.
    #[must_use]
    pub const fn right(self) -> i64 {
        self.x as i64 + self.width as i64
    }

    /// Ordonnée du bord bas, exclue.
    #[must_use]
    pub const fn bottom(self) -> i64 {
        self.y as i64 + self.height as i64
    }

    /// Indique que le rectangle a une surface non nulle et des bords représentables.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= MAX_DISPLAY_DIMENSION
            && self.height <= MAX_DISPLAY_DIMENSION
            && self.right() <= i32::MAX as i64
            && self.bottom() <= i32::MAX as i64
    }

    /// Indique que le point physique tombe dans le rectangle.
    #[must_use]
    pub const fn contains(self, x: i32, y: i32) -> bool {
        (x as i64) >= self.x as i64
            && (x as i64) < self.right()
            && (y as i64) >= self.y as i64
            && (y as i64) < self.bottom()
    }

    /// Centre du rectangle, arrondi vers le bas.
    #[must_use]
    pub const fn center(self) -> (i32, i32) {
        (
            (self.x as i64 + self.width as i64 / 2) as i32,
            (self.y as i64 + self.height as i64 / 2) as i32,
        )
    }

    /// Intersection avec un autre rectangle, ou `None` si elle est vide.
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        if right <= i64::from(left) || bottom <= i64::from(top) {
            return None;
        }
        Some(Self {
            x: left,
            y: top,
            // Les deux différences sont positives et bornées par les dimensions
            // d'origine : la conversion ne peut pas tronquer.
            width: (right - i64::from(left)) as u32,
            height: (bottom - i64::from(top)) as u32,
        })
    }

    /// Surface de l'intersection avec un autre rectangle, nulle si disjointe.
    ///
    /// Sert à choisir l'écran d'une fenêtre : celui qu'elle recouvre le plus.
    #[must_use]
    pub fn intersection_area(self, other: Self) -> u64 {
        self.intersection(other)
            .map_or(0, |rect| u64::from(rect.width) * u64::from(rect.height))
    }
}

/// Précision de la zone de travail retenue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkAreaAccuracy {
    /// Zone publiée par le système : barres et panneaux réellement exclus.
    Native,
    /// Le système n'a rien publié : les limites du moniteur servent de repli.
    ///
    /// Le familier peut alors se poser derrière une barre des tâches. L'écart est
    /// marqué explicitement plutôt que présenté comme un résultat natif.
    BoundsFallback,
}

/// Empreinte servant à retrouver un écran d'une session à l'autre.
///
/// Ni un identifiant OS — instable entre redémarrages et pilotes — ni une
/// coordonnée brute : un couple nom + définition, suffisamment discriminant pour
/// un poste réel et sans prétention d'unicité absolue.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayFingerprint {
    /// Nom rapporté par le système, tronqué en caractères.
    pub name: String,
    /// Largeur en pixels physiques.
    pub width: u32,
    /// Hauteur en pixels physiques.
    pub height: u32,
}

impl DisplayFingerprint {
    /// Construit une empreinte en bornant nom et dimensions.
    #[must_use]
    pub fn new(name: Option<&str>, width: u32, height: u32) -> Self {
        Self {
            name: normalize_display_name(name),
            width: width.min(MAX_DISPLAY_DIMENSION),
            height: height.min(MAX_DISPLAY_DIMENSION),
        }
    }

    /// Indique que deux empreintes désignent vraisemblablement le même écran.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.name == other.name
    }
}

/// Un écran, ses limites, sa zone de travail et sa densité.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayArea {
    /// Empreinte de restauration.
    pub fingerprint: DisplayFingerprint,
    /// Limites complètes du moniteur, en pixels physiques.
    pub bounds: PhysicalRect,
    /// Portion libre de barres système, toujours incluse dans `bounds`.
    pub work_area: PhysicalRect,
    /// Facteur d'échelle en millièmes (1 500 = 150 %).
    ///
    /// Entier : un flottant persisté ou comparé provoquerait des écarts d'un
    /// pixel imprévisibles d'une session à l'autre.
    pub scale_factor_milli: u32,
    /// Écran principal de la session.
    pub is_primary: bool,
    /// Précision réelle de `work_area`.
    pub accuracy: WorkAreaAccuracy,
}

/// Description brute d'un moniteur, telle que le système de fenêtrage la livre.
///
/// `gremlin-app` la remplit depuis `winit` — seul détenteur de la boucle
/// d'événements — ce qui garde `winit` hors de la signature du fournisseur et
/// rend celui-ci testable sur des écrans synthétiques.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorProbe {
    /// Nom rapporté par le système, s'il en donne un.
    pub name: Option<String>,
    /// Coin supérieur gauche en pixels physiques.
    pub position: (i32, i32),
    /// Définition en pixels physiques.
    pub size: (u32, u32),
    /// Facteur d'échelle rapporté.
    pub scale_factor: f64,
    /// Écran principal de la session.
    pub is_primary: bool,
}

/// État de la topologie du bureau.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopLayoutState {
    /// Écrans connus et normalisés, jamais vide.
    Available(Vec<DisplayArea>),
    /// La plateforme ne permet pas de connaître la topologie.
    Unavailable {
        /// Raison affichable dans le panneau.
        reason: String,
    },
}

impl DesktopLayoutState {
    /// Écrans disponibles, ou une tranche vide.
    #[must_use]
    pub fn displays(&self) -> &[DisplayArea] {
        match self {
            Self::Available(displays) => displays,
            Self::Unavailable { .. } => &[],
        }
    }

    /// Indique que le placement natif est exploitable.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available(_))
    }

    /// Écran principal, ou le premier de la liste.
    #[must_use]
    pub fn primary(&self) -> Option<&DisplayArea> {
        let displays = self.displays();
        displays
            .iter()
            .find(|display| display.is_primary)
            .or_else(|| displays.first())
    }

    /// Écran recouvrant le plus la fenêtre décrite par `window`.
    ///
    /// Trois critères en cascade : la plus grande intersection, puis l'écran
    /// contenant le centre, puis l'écran principal. Une fenêtre entièrement hors
    /// écran — après débranchement d'un moniteur — trouve ainsi toujours un hôte.
    #[must_use]
    pub fn display_for_window(&self, window: PhysicalRect) -> Option<&DisplayArea> {
        let displays = self.displays();
        let best = displays
            .iter()
            .map(|display| (display, window.intersection_area(display.bounds)))
            .filter(|(_, area)| *area > 0)
            .max_by_key(|(_, area)| *area)
            .map(|(display, _)| display);
        if best.is_some() {
            return best;
        }

        let (center_x, center_y) = window.center();
        displays
            .iter()
            .find(|display| display.bounds.contains(center_x, center_y))
            .or_else(|| self.primary())
    }

    /// Écran correspondant à une empreinte persistée.
    #[must_use]
    pub fn display_matching(&self, fingerprint: &DisplayFingerprint) -> Option<&DisplayArea> {
        self.displays()
            .iter()
            .find(|display| display.fingerprint.matches(fingerprint))
    }
}

/// Source injectable de la topologie du bureau.
pub trait DesktopLayoutProvider: Send + Sync {
    /// Complète des moniteurs bruts par leurs zones de travail.
    ///
    /// Ne renvoie jamais d'erreur : l'indisponibilité est un **état** que
    /// l'interface doit afficher, pas un incident ponctuel.
    fn resolve(&self, monitors: &[MonitorProbe]) -> DesktopLayoutState;
}

/// Source native de la zone de travail d'un écran.
///
/// Un seul point de FFI par plateforme, alimenté par les limites déjà connues.
pub(crate) trait WorkAreaSource {
    /// Zone de travail de l'écran dont les limites sont `bounds`.
    ///
    /// # Errors
    /// Renvoie [`SystemError::DesktopLayoutUnavailable`] si la plateforme ne
    /// publie rien d'exploitable, ou [`SystemError::DesktopLayoutReadFailed`] si
    /// l'appel natif échoue. Dans les deux cas l'appelant se replie sur les
    /// limites du moniteur, en marquant l'écart.
    fn work_area(&self, bounds: PhysicalRect) -> Result<PhysicalRect, SystemError>;
}

/// Fournisseur adossé aux capacités natives de la plateforme courante.
pub struct SystemDesktopLayout {
    /// Source native, ou la raison de son absence.
    source: Result<Box<dyn WorkAreaSource + Send + Sync>, SystemError>,
}

impl SystemDesktopLayout {
    /// Construit le fournisseur de la plateforme courante.
    #[must_use]
    pub fn new() -> Self {
        Self {
            source: crate::platform::desktop_layout::default_work_area_source(),
        }
    }

    /// Raison pour laquelle le placement natif est indisponible, s'il l'est.
    #[must_use]
    pub fn unavailable_reason(&self) -> Option<String> {
        self.source.as_ref().err().map(ToString::to_string)
    }
}

impl Default for SystemDesktopLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopLayoutProvider for SystemDesktopLayout {
    fn resolve(&self, monitors: &[MonitorProbe]) -> DesktopLayoutState {
        let source = match &self.source {
            Ok(source) => source.as_ref(),
            Err(error) => {
                return DesktopLayoutState::Unavailable {
                    reason: error.to_string(),
                }
            }
        };
        normalize_layout(monitors, |bounds| source.work_area(bounds).ok())
    }
}

/// Fournisseur figé, destiné aux tests et aux scénarios reproductibles.
#[derive(Debug, Clone, Default)]
pub struct FixedDesktopLayout {
    /// Écrans renvoyés tels quels, sans normalisation supplémentaire.
    pub displays: Vec<DisplayArea>,
}

impl DesktopLayoutProvider for FixedDesktopLayout {
    fn resolve(&self, _monitors: &[MonitorProbe]) -> DesktopLayoutState {
        if self.displays.is_empty() {
            return DesktopLayoutState::Unavailable {
                reason: String::from("aucun écran configuré"),
            };
        }
        DesktopLayoutState::Available(self.displays.clone())
    }
}

/// Fournisseur toujours indisponible, image du cas Wayland.
#[derive(Debug, Clone, Default)]
pub struct UnavailableDesktopLayout {
    /// Raison affichée par l'interface.
    pub reason: String,
}

impl DesktopLayoutProvider for UnavailableDesktopLayout {
    fn resolve(&self, _monitors: &[MonitorProbe]) -> DesktopLayoutState {
        DesktopLayoutState::Unavailable {
            reason: if self.reason.is_empty() {
                String::from("placement natif indisponible")
            } else {
                self.reason.clone()
            },
        }
    }
}

/// Normalise une liste de moniteurs bruts en topologie exploitable.
///
/// Publique parce qu'elle est **pure** : elle prend des faits et une fonction de
/// zone de travail, sans toucher au système. Les tests s'en servent pour
/// fabriquer des topologies synthétiques — deux écrans, origines négatives,
/// densités différentes — qu'aucun poste réel ne fournirait à la demande.
///
/// Écarte les rectangles nuls ou débordants, borne le nombre d'écrans, intersecte
/// chaque zone de travail avec les limites de son moniteur et garantit qu'un
/// écran principal existe.
pub fn normalize_layout(
    monitors: &[MonitorProbe],
    mut work_area_of: impl FnMut(PhysicalRect) -> Option<PhysicalRect>,
) -> DesktopLayoutState {
    let mut displays: Vec<DisplayArea> = Vec::new();

    for monitor in monitors.iter().take(MAX_DISPLAYS) {
        let bounds = PhysicalRect::new(
            monitor.position.0,
            monitor.position.1,
            monitor.size.0,
            monitor.size.1,
        );
        if !bounds.is_valid() {
            continue;
        }

        // La zone publiée par l'OS est *proposée*, jamais crue sur parole : une
        // zone hors limites ou vide retombe sur le moniteur, écart marqué.
        let (work_area, accuracy) = work_area_of(bounds)
            .filter(|area| area.is_valid())
            .and_then(|area| area.intersection(bounds))
            .map_or((bounds, WorkAreaAccuracy::BoundsFallback), |area| {
                (area, WorkAreaAccuracy::Native)
            });

        displays.push(DisplayArea {
            fingerprint: DisplayFingerprint::new(
                monitor.name.as_deref(),
                bounds.width,
                bounds.height,
            ),
            bounds,
            work_area,
            scale_factor_milli: normalize_scale_factor(monitor.scale_factor),
            is_primary: monitor.is_primary,
            accuracy,
        });
    }

    if displays.is_empty() {
        return DesktopLayoutState::Unavailable {
            reason: String::from("aucun écran exploitable rapporté par le système"),
        };
    }

    // Un poste sans écran principal déclaré existe (certaines topologies X11) :
    // sans ce repli, la restauration de placement n'aurait aucun point de chute.
    if !displays.iter().any(|display| display.is_primary) {
        if let Some(first) = displays.first_mut() {
            first.is_primary = true;
        }
    }

    DesktopLayoutState::Available(displays)
}

/// Convertit un facteur d'échelle flottant en millièmes bornés.
fn normalize_scale_factor(scale_factor: f64) -> u32 {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return DEFAULT_SCALE_FACTOR_MILLI;
    }
    let milli = scale_factor * 1_000.0;
    if milli >= f64::from(MAX_SCALE_FACTOR_MILLI) {
        return MAX_SCALE_FACTOR_MILLI;
    }
    // La borne haute vient d'être écartée et la valeur est positive : la
    // conversion ne peut ni déborder ni changer de signe.
    (milli.round() as u32).clamp(MIN_SCALE_FACTOR_MILLI, MAX_SCALE_FACTOR_MILLI)
}

/// Rogne et tronque un nom d'écran sur une frontière de caractère.
fn normalize_display_name(name: Option<&str>) -> String {
    let trimmed = name.unwrap_or_default().trim();
    if trimmed.is_empty() {
        return String::from("Écran");
    }
    trimmed.chars().take(MAX_DISPLAY_NAME_CHARS).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn probe(name: &str, x: i32, y: i32, width: u32, height: u32, primary: bool) -> MonitorProbe {
        MonitorProbe {
            name: Some(name.to_owned()),
            position: (x, y),
            size: (width, height),
            scale_factor: 1.0,
            is_primary: primary,
        }
    }

    /// Topologie à deux écrans : le secondaire est à gauche, en coordonnées négatives.
    fn dual_screen() -> Vec<MonitorProbe> {
        vec![
            probe("Principal", 0, 0, 1920, 1080, true),
            MonitorProbe {
                scale_factor: 1.5,
                ..probe("Gauche", -2560, -200, 2560, 1440, false)
            },
        ]
    }

    #[test]
    fn test_rect_edges_use_wide_arithmetic() {
        let extreme = PhysicalRect::new(i32::MAX - 10, 0, 100, 100);
        assert_eq!(extreme.right(), i64::from(i32::MAX - 10) + 100);
        assert!(!extreme.is_valid(), "un bord hors i32 doit être refusé");

        let valid = PhysicalRect::new(-2560, -200, 2560, 1440);
        assert!(valid.is_valid());
        assert_eq!(valid.right(), 0);
        assert_eq!(valid.bottom(), 1240);
    }

    #[test]
    fn test_degenerate_rects_are_invalid() {
        assert!(!PhysicalRect::new(0, 0, 0, 1080).is_valid());
        assert!(!PhysicalRect::new(0, 0, 1920, 0).is_valid());
        assert!(!PhysicalRect::new(0, 0, u32::MAX, 1080).is_valid());
    }

    #[test]
    fn test_intersection_and_area() {
        let a = PhysicalRect::new(0, 0, 100, 100);
        let b = PhysicalRect::new(50, 50, 100, 100);
        assert_eq!(a.intersection(b), Some(PhysicalRect::new(50, 50, 50, 50)));
        assert_eq!(a.intersection_area(b), 2_500);

        let disjoint = PhysicalRect::new(200, 200, 10, 10);
        assert_eq!(a.intersection(disjoint), None);
        assert_eq!(a.intersection_area(disjoint), 0);

        // Contact par le bord : surface nulle, pas d'intersection.
        let touching = PhysicalRect::new(100, 0, 10, 100);
        assert_eq!(a.intersection(touching), None);
    }

    #[test]
    fn test_contains_and_center_with_negative_origins() {
        let rect = PhysicalRect::new(-100, -50, 200, 100);
        assert!(rect.contains(-100, -50));
        assert!(rect.contains(0, 0));
        assert!(!rect.contains(100, 0), "le bord droit est exclu");
        assert!(!rect.contains(-101, 0));
        assert_eq!(rect.center(), (0, 0));
    }

    #[test]
    fn test_normalize_keeps_native_work_area_inside_bounds() {
        let state = normalize_layout(&dual_screen(), |bounds| {
            // Barre des tâches de 40 px en bas.
            Some(PhysicalRect::new(
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height - 40,
            ))
        });

        let displays = state.displays();
        assert_eq!(displays.len(), 2);
        assert_eq!(displays[0].work_area, PhysicalRect::new(0, 0, 1920, 1040));
        assert_eq!(displays[0].accuracy, WorkAreaAccuracy::Native);
        assert_eq!(displays[1].scale_factor_milli, 1_500);
    }

    #[test]
    fn test_work_area_outside_bounds_falls_back_and_is_marked() {
        let state = normalize_layout(&dual_screen(), |_| {
            // Zone incohérente, publiée par un gestionnaire de fenêtres fantaisiste.
            Some(PhysicalRect::new(10_000, 10_000, 100, 100))
        });
        for display in state.displays() {
            assert_eq!(display.work_area, display.bounds);
            assert_eq!(display.accuracy, WorkAreaAccuracy::BoundsFallback);
        }
    }

    #[test]
    fn test_absent_work_area_falls_back_and_is_marked() {
        let state = normalize_layout(&dual_screen(), |_| None);
        for display in state.displays() {
            assert_eq!(display.work_area, display.bounds);
            assert_eq!(display.accuracy, WorkAreaAccuracy::BoundsFallback);
        }
    }

    #[test]
    fn test_partial_work_area_is_intersected_not_rejected() {
        // Une zone globale EWMH déborde sur les deux écrans : elle doit être
        // ramenée à la portion qui concerne chaque moniteur.
        let state = normalize_layout(&dual_screen(), |_| {
            Some(PhysicalRect::new(-2560, 0, 4480, 1000))
        });
        let displays = state.displays();
        assert_eq!(displays[0].work_area, PhysicalRect::new(0, 0, 1920, 1000));
        assert_eq!(
            displays[1].work_area,
            PhysicalRect::new(-2560, 0, 2560, 1000)
        );
    }

    #[test]
    fn test_degenerate_monitors_are_dropped() {
        let monitors = vec![
            probe("Nul", 0, 0, 0, 0, false),
            probe("Bon", 0, 0, 1920, 1080, true),
            probe("Débordant", i32::MAX - 5, 0, 1000, 1000, false),
        ];
        let state = normalize_layout(&monitors, Some);
        assert_eq!(state.displays().len(), 1);
        assert_eq!(state.displays()[0].fingerprint.name, "Bon");
    }

    #[test]
    fn test_display_count_is_capped() {
        let monitors: Vec<MonitorProbe> = (0..MAX_DISPLAYS + 8)
            .map(|index| probe("Écran", (index as i32) * 100, 0, 100, 100, index == 0))
            .collect();
        let state = normalize_layout(&monitors, Some);
        assert_eq!(state.displays().len(), MAX_DISPLAYS);
    }

    #[test]
    fn test_an_empty_layout_is_unavailable_not_empty() {
        let state = normalize_layout(&[], Some);
        assert!(!state.is_available());
        assert!(state.displays().is_empty());
        assert!(state.primary().is_none());
    }

    #[test]
    fn test_a_layout_without_primary_promotes_the_first() {
        let monitors = vec![
            probe("A", 0, 0, 800, 600, false),
            probe("B", 800, 0, 800, 600, false),
        ];
        let state = normalize_layout(&monitors, Some);
        let Some(primary) = state.primary() else {
            panic!("un écran principal de repli doit exister");
        };
        assert_eq!(primary.fingerprint.name, "A");
    }

    #[test]
    fn test_scale_factor_is_bounded_and_finite() {
        for (input, expected) in [
            (1.0, 1_000),
            (1.25, 1_250),
            (2.0, 2_000),
            // Une valeur non finie est une donnée corrompue, pas un écran très
            // dense : elle retombe sur 1× plutôt que sur la borne haute.
            (f64::NAN, DEFAULT_SCALE_FACTOR_MILLI),
            (f64::INFINITY, DEFAULT_SCALE_FACTOR_MILLI),
            (f64::NEG_INFINITY, DEFAULT_SCALE_FACTOR_MILLI),
            (-3.0, DEFAULT_SCALE_FACTOR_MILLI),
            (0.0, DEFAULT_SCALE_FACTOR_MILLI),
            // Une valeur finie mais absurde, elle, est simplement bornée.
            (0.01, MIN_SCALE_FACTOR_MILLI),
            (1_000.0, MAX_SCALE_FACTOR_MILLI),
        ] {
            assert_eq!(
                normalize_scale_factor(input),
                expected,
                "facteur mal borné : {input}"
            );
        }
    }

    #[test]
    fn test_display_name_is_truncated_on_character_boundaries() {
        let accented = "é".repeat(MAX_DISPLAY_NAME_CHARS + 20);
        let fingerprint = DisplayFingerprint::new(Some(&accented), 1920, 1080);
        assert_eq!(fingerprint.name.chars().count(), MAX_DISPLAY_NAME_CHARS);

        assert_eq!(DisplayFingerprint::new(None, 1, 1).name, "Écran");
        assert_eq!(DisplayFingerprint::new(Some("   "), 1, 1).name, "Écran");
    }

    #[test]
    fn test_window_is_assigned_to_the_most_overlapping_display() {
        let state = normalize_layout(&dual_screen(), Some);

        // Fenêtre à cheval, majoritairement sur l'écran de gauche.
        let straddling = PhysicalRect::new(-300, 100, 400, 200);
        let Some(display) = state.display_for_window(straddling) else {
            panic!("un écran doit être choisi");
        };
        assert_eq!(display.fingerprint.name, "Gauche");

        let on_primary = PhysicalRect::new(500, 500, 200, 200);
        let Some(display) = state.display_for_window(on_primary) else {
            panic!("un écran doit être choisi");
        };
        assert_eq!(display.fingerprint.name, "Principal");
    }

    #[test]
    fn test_a_fully_offscreen_window_falls_back_to_the_primary() {
        let state = normalize_layout(&dual_screen(), Some);
        let lost = PhysicalRect::new(90_000, 90_000, 100, 100);
        let Some(display) = state.display_for_window(lost) else {
            panic!("un écran de repli doit exister");
        };
        assert!(display.is_primary);
    }

    #[test]
    fn test_fingerprint_matching_survives_a_move() {
        let state = normalize_layout(&dual_screen(), Some);
        let target = DisplayFingerprint::new(Some("Gauche"), 2560, 1440);
        let Some(display) = state.display_matching(&target) else {
            panic!("l'empreinte doit retrouver son écran");
        };
        assert_eq!(display.bounds.x, -2560);

        // Une définition différente ne correspond plus : l'écran a été remplacé.
        let replaced_screen = DisplayFingerprint::new(Some("Gauche"), 1920, 1080);
        assert!(state.display_matching(&replaced_screen).is_none());
    }

    #[test]
    fn test_unavailable_layout_exposes_its_reason() {
        let provider = UnavailableDesktopLayout {
            reason: String::from("Wayland ne publie pas la position des surfaces"),
        };
        let state = provider.resolve(&dual_screen());
        match state {
            DesktopLayoutState::Unavailable { reason } => {
                assert!(reason.contains("Wayland"));
            }
            DesktopLayoutState::Available(_) => panic!("écran synthétique fabriqué à tort"),
        }
    }

    #[test]
    fn test_fixed_layout_returns_its_displays() {
        let state = normalize_layout(&dual_screen(), Some);
        let provider = FixedDesktopLayout {
            displays: state.displays().to_vec(),
        };
        assert_eq!(provider.resolve(&[]).displays().len(), 2);
        assert!(!FixedDesktopLayout::default().resolve(&[]).is_available());
    }
}
