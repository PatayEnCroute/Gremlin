//! Planche de contrôle du panneau de paramètres.
//!
//! Rend le panneau hors écran, à plusieurs densités et dans plusieurs états,
//! puis écrit des images PNG. C'est le pendant de `font_proof_sheet` pour la
//! mise en page : elle se juge en la regardant, pas en relisant les coordonnées.
//!
//! ```bash
//! cargo run -p gremlin-app --example panel_proof_sheet
//! ```
//!
//! Les images sont écrites dans `target/panel_*.png`.

use gremlin_app::config::AppConfig;
use gremlin_app::ui::{
    CommandPalette, ConsumableDragView, PaletteContext, PaletteGroup, PanelInteraction, PanelScene,
    PanelStyle, PromptKind, RaycastRenderer, RepoDisplayInfo, RepoTrackingStatus, TextSize, Theme,
    ThemePreference, UiMetrics,
};
use gremlin_core::{CivilDate, ConsumableKind, PetState};
use gremlin_render::{
    register_default_accessories, AccessoryCatalog, PixelBuffer, SkinManifest, SpriteAtlas,
    WardrobeEquipment,
};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

/// Un cas de figure à rendre.
struct Case {
    name: &'static str,
    /// Skin dont le manifest est monté : il décide de la famille de variantes
    /// servie aux vignettes et à l'aperçu.
    skin: &'static str,
    scale: f64,
    text_size: TextSize,
    /// Palette à employer.
    theme: ThemePreference,
    /// Groupe dans lequel descendre avant le rendu, ou racine si absent.
    group: Option<PaletteGroup>,
    query: &'static str,
    repo_count: usize,
    /// Nombre de descentes dans la liste avant le rendu.
    advance: usize,
    /// Saisie guidée à ouvrir, plutôt qu'un groupe.
    prompt: Option<PromptKind>,
    /// Curseur posé sur le bouton d'action de la ligne survolée.
    hovered_row_action: bool,
    /// Dernier échec de sauvegarde à simuler.
    save_error: Option<&'static str>,
    /// Dernier incident de surveillance à simuler.
    observation_error: Option<&'static str>,
    /// État de productivité à mettre en place avant le rendu.
    fixture: PetFixture,
    /// Le placement natif est exploitable sur la plateforme simulée.
    desktop_available: bool,
    /// Glisser de consommable à dessiner, en fraction de la zone d'aperçu.
    drag: Option<DragCase>,
}

/// État de productivité préparé pour une planche.
///
/// Les fixtures sont montées par les **vraies** méthodes du domaine — commits
/// datés, consommations, démarrage du minuteur — et non par un état bricolé :
/// une planche qui montrerait un état impossible ne prouverait rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PetFixture {
    /// Familier neuf : aucune série, stock de départ.
    Fresh,
    /// Sept jours consécutifs : deux récompenses acquises, une verrouillée.
    Streak7,
    /// Trente jours consécutifs : les trois récompenses acquises.
    Streak30,
    /// Inventaire vidé : chaque objet annonce son refus.
    EmptyInventory,
    /// Bloc de travail en cours.
    PomodoroWork,
    /// Pause proposée en fin de bloc.
    PomodoroBreak,
}

/// Position du fantôme de glisser, relative à la zone d'aperçu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragCase {
    /// Curseur sur l'aperçu : le geste aboutirait.
    OverPreview,
    /// Curseur à côté : le relâchement ne consommera rien.
    BesidePreview,
}

/// Jour civil de référence des planches, fixe pour rester reproductible.
fn proof_today() -> CivilDate {
    CivilDate::new(2024, 5, 30).unwrap_or_else(|_| unreachable!("date de référence invalide"))
}

/// Monte l'état de productivité décrit par la fixture.
fn build_pet(fixture: PetFixture) -> PetState {
    let mut pet = PetState::new("Gizmo");
    let today = proof_today();

    // Jauges partiellement creusées : un familier neuf a les jauges pleines, et
    // tous les objets seraient alors refusés « jauge déjà pleine ». La planche
    // ne montrerait jamais l'état utilisable.
    if fixture != PetFixture::EmptyInventory {
        pet.set_stats(gremlin_core::PetStats::new(58.0, 42.0, 71.0));
    }

    let streak_days = match fixture {
        PetFixture::Streak7 => 7,
        PetFixture::Streak30 => 30,
        _ => 0,
    };
    if streak_days > 0 {
        let days =
            (0..streak_days).filter_map(|offset| today.checked_add_days(offset - streak_days + 1));
        let _ = pet.reconcile_commit_history(days, today);
    }

    if fixture == PetFixture::EmptyInventory {
        // Vidage par la voie normale : jauges creusées d'abord, sinon les
        // objets seraient refusés pour absence d'effet.
        pet.set_stats(gremlin_core::PetStats::new(5.0, 5.0, 5.0));
        for kind in ConsumableKind::ALL {
            while pet.productivity().inventory().quantity(kind) > 0 {
                if pet.use_consumable(kind).is_err() {
                    break;
                }
            }
        }
        pet.set_stats(gremlin_core::PetStats::new(100.0, 100.0, 100.0));
    }

    match fixture {
        PetFixture::PomodoroWork => {
            let _ = pet.start_pomodoro();
            let _ = pet.advance_live_productivity(Duration::from_secs(90));
        }
        PetFixture::PomodoroBreak => {
            let _ = pet.start_pomodoro();
            // Jusqu'à la frontière de phase : la pause est proposée, pas démarrée.
            let remaining = pet
                .productivity()
                .pomodoro()
                .remaining()
                .unwrap_or_default();
            let _ = pet.advance_live_productivity(remaining.min(Duration::from_secs(60)));
            while pet.productivity().pomodoro().is_running() {
                let _ = pet.advance_live_productivity(Duration::from_secs(60));
            }
        }
        _ => {}
    }

    pet
}

/// Construit un cas de phase 8 sur le groupe productivité.
///
/// `advance` fait descendre la sélection : les lignes du minuteur vivent sous le
/// pli, et une planche qui ne les montre pas ne prouve rien à leur sujet.
const fn productivity_case(
    name: &'static str,
    theme: ThemePreference,
    fixture: PetFixture,
    desktop_available: bool,
    drag: Option<DragCase>,
    advance: usize,
    group: PaletteGroup,
) -> Case {
    Case {
        name,
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme,
        group: Some(group),
        query: "",
        repo_count: 2,
        advance,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture,
        desktop_available,
        drag,
    }
}

const CASES: &[Case] = &[
    // --- Phase 8 : série, inventaire, minuteur et placement ---
    productivity_case(
        "panel_productivite_neuve",
        ThemePreference::Dark,
        PetFixture::Fresh,
        true,
        None,
        0,
        PaletteGroup::Productivity,
    ),
    productivity_case(
        "panel_productivite_serie_7",
        ThemePreference::Dark,
        PetFixture::Streak7,
        true,
        None,
        0,
        PaletteGroup::Productivity,
    ),
    productivity_case(
        "panel_productivite_serie_30_clair",
        ThemePreference::Light,
        PetFixture::Streak30,
        true,
        None,
        0,
        PaletteGroup::Productivity,
    ),
    productivity_case(
        "panel_productivite_serie_30_contraste",
        ThemePreference::HighContrast,
        PetFixture::Streak30,
        true,
        None,
        0,
        PaletteGroup::Productivity,
    ),
    productivity_case(
        "panel_inventaire_vide",
        ThemePreference::Dark,
        PetFixture::EmptyInventory,
        true,
        None,
        0,
        PaletteGroup::Productivity,
    ),
    productivity_case(
        "panel_minuteur_travail",
        ThemePreference::Dark,
        PetFixture::PomodoroWork,
        true,
        None,
        9,
        PaletteGroup::Productivity,
    ),
    productivity_case(
        "panel_minuteur_pause",
        ThemePreference::Dark,
        PetFixture::PomodoroBreak,
        true,
        None,
        10,
        PaletteGroup::Productivity,
    ),
    productivity_case(
        "panel_placement_indisponible",
        ThemePreference::Dark,
        PetFixture::Fresh,
        false,
        None,
        // Les bascules de placement vivent en fin de préférences : il faut
        // descendre jusqu'à elles pour les voir.
        18,
        PaletteGroup::Preferences,
    ),
    productivity_case(
        "panel_glisser_sur_apercu",
        ThemePreference::Dark,
        PetFixture::Fresh,
        true,
        Some(DragCase::OverPreview),
        0,
        PaletteGroup::Productivity,
    ),
    productivity_case(
        "panel_glisser_hors_cible",
        ThemePreference::Dark,
        PetFixture::Fresh,
        true,
        Some(DragCase::BesidePreview),
        0,
        PaletteGroup::Productivity,
    ),
    // Racine : les cinq groupes, chacun avec son décompte.
    Case {
        name: "panel_racine",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_racine_150",
        skin: "default",
        scale: 1.5,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Descente dans la garde-robe : fil d'Ariane et libellés de section.
    Case {
        name: "panel_garde_robe",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_garde_robe_compact",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Compact,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Préférences de la phase 7 : bascules d'outillage, focus et rappels.
    Case {
        name: "panel_outillage_focus",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Preferences),
        query: "",
        repo_count: 3,
        advance: 1,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Quarante dépôts, sélection poussée hors de la fenêtre visible.
    Case {
        name: "panel_depots_defiles",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Repos),
        query: "",
        repo_count: 40,
        advance: 14,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Recherche globale sans accent : elle traverse tous les niveaux.
    Case {
        name: "panel_recherche_globale",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "depot",
        repo_count: 6,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Recherche par initiales, depuis l'intérieur d'un groupe.
    Case {
        name: "panel_recherche_initiales",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Wardrobe),
        query: "ez",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Les trois palettes, sur le même contenu, pour les comparer d'un coup d'oeil.
    Case {
        name: "panel_theme_clair",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Light,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_theme_contraste",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::HighContrast,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_texte_grand",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Large,
        theme: ThemePreference::Dark,
        group: None,
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_vide",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "zzz-introuvable",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Le bouton de retrait, au repos puis sous le curseur : les deux teintes se
    // jugent cote a cote, et la corbeille ne doit heurter ni la pastille ni
    // l'ascenseur.
    Case {
        name: "panel_depots_corbeille",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Repos),
        query: "",
        repo_count: 5,
        advance: 3,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_depots_corbeille_survol",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Repos),
        query: "",
        repo_count: 5,
        advance: 3,
        prompt: None,
        hovered_row_action: true,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Le pictogramme doit rester lisible sur les trois palettes et au corps 6x11.
    Case {
        name: "panel_depots_clair",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Light,
        group: Some(PaletteGroup::Repos),
        query: "",
        repo_count: 5,
        advance: 3,
        prompt: None,
        hovered_row_action: true,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_depots_contraste",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::HighContrast,
        group: Some(PaletteGroup::Repos),
        query: "",
        repo_count: 5,
        advance: 3,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_depots_corbeille_200",
        skin: "default",
        scale: 2.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Repos),
        query: "",
        repo_count: 5,
        advance: 3,
        prompt: None,
        hovered_row_action: true,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Etat vide pedagogique : aucun depot declare, le groupe reste accessible.
    Case {
        name: "panel_depots_etat_vide",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Repos),
        query: "",
        repo_count: 0,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Mode saisie : le champ de recherche porte un chemin, la ligne rend compte
    // de sa validite. Le chemin saisi ne designe aucun depot : la pastille doit
    // dire INVALIDE, et le sous-titre en donner la raison exacte -- laquelle
    // depend du systeme, un chemin en `/` n'etant pas absolu sous Windows.
    Case {
        name: "panel_saisie_chemin",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "/chemin/qui/nexiste/pas",
        repo_count: 2,
        advance: 0,
        prompt: Some(PromptKind::AddTrackedRepo),
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Etats d'erreur : ni la pastille ECHEC ni le pictogramme d'alerte des
    // sous-titres n'avaient jamais ete regardes sur une planche.
    Case {
        name: "panel_incidents",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Preferences),
        query: "",
        repo_count: 2,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: Some("disque plein : impossible d'ecrire save.json"),
        observation_error: Some("quota de surveillance du systeme atteint"),
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    Case {
        name: "panel_incidents_clair",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Light,
        group: Some(PaletteGroup::Preferences),
        query: "sauvegarder",
        repo_count: 2,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: Some("disque plein : impossible d'ecrire save.json"),
        observation_error: Some("quota de surveillance du systeme atteint"),
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Garde-robe en grand texte : les vignettes doivent rester lisibles et
    // centrées quand la ligne grandit.
    Case {
        name: "panel_garde_robe_texte_grand",
        skin: "default",
        scale: 1.0,
        text_size: TextSize::Large,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
    // Même garde-robe sur une autre morphologie : les vignettes et l'aperçu
    // doivent basculer sur les variantes dessinées pour ce skin.
    Case {
        name: "panel_garde_robe_evolved",
        skin: "evolved",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
        prompt: None,
        hovered_row_action: false,
        save_error: None,
        observation_error: None,
        fixture: PetFixture::Fresh,
        desktop_available: true,
        drag: None,
    },
];

/// Manifest du skin demandé, ou aucun si le pack intégré est illisible.
///
/// L'aperçu se contente alors des ancres canoniques : la planche reste
/// exploitable même si les assets manquent.
fn load_skin_manifest(skin: &str) -> Option<SkinManifest> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/skins")
        .join(skin)
        .join("manifest.json");
    let json = std::fs::read_to_string(path).ok()?;
    SkinManifest::from_json(&json).ok()
}

/// Chemin de depot factice, absolu sur les trois systemes.
fn proof_repo_path(index: usize) -> std::path::PathBuf {
    if cfg!(windows) {
        std::path::PathBuf::from(format!(r"C:\depots\projet-{index}"))
    } else {
        std::path::PathBuf::from(format!("/depots/projet-{index}"))
    }
}

#[allow(clippy::too_many_lines)]
fn main() -> ExitCode {
    let mut atlas = SpriteAtlas::new();
    atlas.load_default_procedural_sprites();
    let mut catalog = AccessoryCatalog::new();
    register_default_accessories(&mut atlas, &mut catalog);

    let mut wardrobe = WardrobeEquipment::new();
    wardrobe.equip(gremlin_render::AccessoryCategory::Hat, "wizard_hat");

    // Le minuteur est désactivé par défaut : les planches qui le montrent
    // doivent l'activer, comme le ferait l'utilisateur.
    let config = AppConfig {
        pomodoro_enabled: true,
        ..AppConfig::default()
    };

    for case in CASES {
        let pet = build_pet(case.fixture);
        let repos: Vec<RepoDisplayInfo> = (0..case.repo_count)
            .map(|index| RepoDisplayInfo {
                path: proof_repo_path(index),
                name: format!("dépôt-numéro-{index}"),
                branch: Some(if index % 3 == 0 {
                    String::from("main")
                } else {
                    format!("feature/refonte-{index}")
                }),
                last_commit_msg: Some(format!(
                    "fix: gère les caractères « spéciaux » du module {index}"
                )),
                // Un dépôt sur quatre est montré indisponible : c'est l'état
                // qu'il faut pouvoir juger à l'œil, pastille et pictogramme
                // compris.
                status: if index % 4 == 3 {
                    RepoTrackingStatus::Unavailable
                } else {
                    RepoTrackingStatus::Active
                },
                issue: (index % 4 == 3).then(|| String::from("dépôt introuvable sur le disque")),
            })
            .collect();

        let mut palette = CommandPalette::new(&PaletteContext {
            catalog: &catalog,
            wardrobe: &wardrobe,
            pet_state: &pet,
            config: &config,
            autostart_active: true,
            repos: &repos,
            current_dir_repo: None,
            folder_picker_available: true,
            last_save_error: case.save_error,
            last_observation_error: case.observation_error,
            pending_tooling_enabled: None,
            today: Some(proof_today()),
            desktop_placement_available: case.desktop_available,
            desktop_unavailable_reason: (!case.desktop_available)
                .then_some("Wayland ne publie ni la position des surfaces ni la zone de travail"),
        });
        if let Some(group) = case.group {
            palette.enter_group(group);
        }
        if let Some(prompt) = case.prompt {
            palette.enter_prompt(prompt);
        }
        palette.set_query(case.query);
        for _ in 0..case.advance {
            palette.select_next();
        }

        let style = PanelStyle {
            metrics: UiMetrics::for_display(case.scale, case.text_size),
            theme: Theme::resolve(case.theme, None),
        };
        let (width, height) = style.metrics.buffer_size();
        let mut buffer = PixelBuffer::new(width, height);

        let manifest = load_skin_manifest(case.skin);
        let scene = PanelScene {
            wardrobe: &wardrobe,
            atlas: &atlas,
            manifest: manifest.as_ref(),
            catalog: &catalog,
            base_frame_key: "idle_0",
            mood_key: "idle",
        };

        RaycastRenderer::render_panel(
            &mut buffer,
            &style,
            &palette,
            &scene,
            PanelInteraction {
                cursor_visible: !case.query.is_empty(),
                hovered_item: Some(palette.selected_index().saturating_add(2)),
                // Une planche montre la corbeille survolée, l'autre au repos :
                // les deux teintes doivent se juger côte à côte.
                hovered_row_action: case.hovered_row_action,
                consumable_drag: case.drag.map(|drag| {
                    let (x, y, side) = style.metrics.preview_rect();
                    match drag {
                        DragCase::OverPreview => ConsumableDragView {
                            cursor: (x + side / 2, y + side / 2),
                            over_target: true,
                        },
                        DragCase::BesidePreview => ConsumableDragView {
                            cursor: (x - side / 3, y + side / 2),
                            over_target: false,
                        },
                    }
                }),
            },
        );

        let path = format!("target/{}.png", case.name);
        if let Err(e) = write_png(&buffer, &path) {
            eprintln!("Echec d'ecriture de {path} : {e}");
            return ExitCode::FAILURE;
        }
        println!("{path} ({width}x{height})");
    }

    ExitCode::SUCCESS
}

/// Encode un tampon en PNG.
fn write_png(buffer: &PixelBuffer, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let image =
        image::RgbaImage::from_raw(buffer.width(), buffer.height(), buffer.as_bytes().to_vec())
            .ok_or("dimensions du tampon incompatibles avec l'image")?;
    image.save(path)?;
    Ok(())
}
