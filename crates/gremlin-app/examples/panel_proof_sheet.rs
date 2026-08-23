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
    CommandPalette, PaletteContext, PaletteGroup, PanelInteraction, PanelScene, PanelStyle,
    RaycastRenderer, RepoDisplayInfo, TextSize, Theme, ThemePreference, UiMetrics,
};
use gremlin_core::PetState;
use gremlin_render::{
    register_default_procedural_accessories, AccessoryCatalog, PixelBuffer, SpriteAtlas,
    WardrobeEquipment,
};
use std::process::ExitCode;

/// Un cas de figure à rendre.
struct Case {
    name: &'static str,
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
}

const CASES: &[Case] = &[
    // Racine : les cinq groupes, chacun avec son décompte.
    Case {
        name: "panel_racine",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "",
        repo_count: 3,
        advance: 0,
    },
    Case {
        name: "panel_racine_150",
        scale: 1.5,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "",
        repo_count: 3,
        advance: 0,
    },
    // Descente dans la garde-robe : fil d'Ariane et libellés de section.
    Case {
        name: "panel_garde_robe",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
    },
    Case {
        name: "panel_garde_robe_compact",
        scale: 1.0,
        text_size: TextSize::Compact,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
    },
    // Quarante dépôts, sélection poussée hors de la fenêtre visible.
    Case {
        name: "panel_depots_defiles",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Repos),
        query: "",
        repo_count: 40,
        advance: 14,
    },
    // Recherche globale sans accent : elle traverse tous les niveaux.
    Case {
        name: "panel_recherche_globale",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "depot",
        repo_count: 6,
        advance: 0,
    },
    // Recherche par initiales, depuis l'intérieur d'un groupe.
    Case {
        name: "panel_recherche_initiales",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: Some(PaletteGroup::Wardrobe),
        query: "ez",
        repo_count: 3,
        advance: 0,
    },
    // Les trois palettes, sur le même contenu, pour les comparer d'un coup d'oeil.
    Case {
        name: "panel_theme_clair",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Light,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
    },
    Case {
        name: "panel_theme_contraste",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::HighContrast,
        group: Some(PaletteGroup::Wardrobe),
        query: "",
        repo_count: 3,
        advance: 0,
    },
    Case {
        name: "panel_texte_grand",
        scale: 1.0,
        text_size: TextSize::Large,
        theme: ThemePreference::Dark,
        group: None,
        query: "",
        repo_count: 3,
        advance: 0,
    },
    Case {
        name: "panel_vide",
        scale: 1.0,
        text_size: TextSize::Normal,
        theme: ThemePreference::Dark,
        group: None,
        query: "zzz-introuvable",
        repo_count: 3,
        advance: 0,
    },
];

fn main() -> ExitCode {
    let mut atlas = SpriteAtlas::new();
    atlas.load_default_procedural_sprites();
    let mut catalog = AccessoryCatalog::new();
    register_default_procedural_accessories(&mut atlas, &mut catalog);

    let mut wardrobe = WardrobeEquipment::new();
    wardrobe.equip(gremlin_render::AccessoryCategory::Hat, "wizard_hat");

    let pet = PetState::new("Gizmo");
    let config = AppConfig::default();

    for case in CASES {
        let repos: Vec<RepoDisplayInfo> = (0..case.repo_count)
            .map(|index| RepoDisplayInfo {
                name: format!("dépôt-numéro-{index}"),
                branch: Some(if index % 3 == 0 {
                    String::from("main")
                } else {
                    format!("feature/refonte-{index}")
                }),
                last_commit_msg: Some(format!(
                    "fix: gère les caractères « spéciaux » du module {index}"
                )),
            })
            .collect();

        let mut palette = CommandPalette::new(&PaletteContext {
            catalog: &catalog,
            wardrobe: &wardrobe,
            pet_state: &pet,
            config: &config,
            autostart_active: true,
            repos: &repos,
            last_save_error: None,
        });
        if let Some(group) = case.group {
            palette.enter_group(group);
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

        let scene = PanelScene {
            wardrobe: &wardrobe,
            atlas: &atlas,
            manifest: None,
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
