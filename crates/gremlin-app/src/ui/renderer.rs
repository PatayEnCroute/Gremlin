//! Moteur de composition et de rendu graphique de la fenêtre Raycast.

use crate::ui::command_palette::{CommandPalette, PaletteSection};
use crate::ui::font::{draw_text_5x7, text_width_px};
use crate::ui::preview::LivePetPreview;
use crate::ui::text::truncate_with_ellipsis;
use crate::ui::theme::{RaycastLayout, RaycastTheme};
use gremlin_render::{AccessoryCatalog, PixelBuffer, SkinManifest, SpriteAtlas, WardrobeEquipment};

fn fill_rect(buffer: &mut PixelBuffer, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
    for dy in 0..height {
        for dx in 0..width {
            let px = x + dx;
            let py = y + dy;
            if px >= 0 && py >= 0 {
                buffer.blend_pixel(px as u32, py as u32, color);
            }
        }
    }
}

/// Dessine une ligne horizontale ultra-fine (1px).
fn draw_hline(buffer: &mut PixelBuffer, x: i32, y: i32, length: i32, color: [u8; 4]) {
    fill_rect(buffer, x, y, length, 1, color);
}

/// Dessine une ligne verticale ultra-fine (1px).
fn draw_vline(buffer: &mut PixelBuffer, x: i32, y: i32, length: i32, color: [u8; 4]) {
    fill_rect(buffer, x, y, 1, length, color);
}

/// Dessine une barre de progression avec remplissage coloré.
#[allow(clippy::cast_precision_loss)]
fn draw_progress_bar(
    buffer: &mut PixelBuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    ratio: f32,
    color: [u8; 4],
) {
    fill_rect(buffer, x, y, width, height, RaycastTheme::BAR_BG);
    let clamped_ratio = ratio.clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation)]
    let filled_w = ((width as f32) * clamped_ratio).round() as i32;
    if filled_w > 0 {
        fill_rect(buffer, x, y, filled_w, height, color);
    }
}

/// Compositeur logiciel pour l'interface Raycast.
pub struct RaycastRenderer;

impl RaycastRenderer {
    /// Dessine l'interface complète de Raycast dans le tampon de pixels.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    pub fn render_ui(
        buffer: &mut PixelBuffer,
        palette: &CommandPalette,
        wardrobe: &WardrobeEquipment,
        atlas: &SpriteAtlas,
        manifest: Option<&SkinManifest>,
        catalog: &AccessoryCatalog,
        base_frame_key: &str,
        mood_key: &str,
        cursor_blink: bool,
    ) {
        let width = buffer.width() as i32;
        let height = buffer.height() as i32;
        let buf_w = buffer.width() as usize;
        let buf_h = buffer.height() as usize;

        // 1. Fond principal & Panneaux
        fill_rect(buffer, 0, 0, width, height, RaycastTheme::BG_PRIMARY);
        fill_rect(
            buffer,
            RaycastLayout::LEFT_PANE_WIDTH,
            RaycastLayout::SEARCH_BAR_HEIGHT,
            width - RaycastLayout::LEFT_PANE_WIDTH,
            height - RaycastLayout::SEARCH_BAR_HEIGHT - RaycastLayout::FOOTER_HEIGHT,
            RaycastTheme::BG_INSPECTOR,
        );

        // 2. Barre de Recherche Supérieure
        fill_rect(
            buffer,
            0,
            0,
            width,
            RaycastLayout::SEARCH_BAR_HEIGHT,
            RaycastTheme::BG_SEARCH,
        );
        draw_hline(
            buffer,
            0,
            RaycastLayout::SEARCH_BAR_HEIGHT - 1,
            width,
            RaycastTheme::BORDER,
        );

        // Icône de recherche & Texte
        draw_text_5x7(
            buffer.as_bytes_mut(),
            buf_w,
            buf_h,
            ">",
            14,
            14,
            RaycastTheme::ACCENT,
        );

        let query_text = palette.query();
        if query_text.is_empty() {
            draw_text_5x7(
                buffer.as_bytes_mut(),
                buf_w,
                buf_h,
                "Rechercher accessoires, stats, dépôts, paramètres...",
                RaycastLayout::SEARCH_TEXT_X,
                RaycastLayout::SEARCH_TEXT_Y,
                RaycastTheme::TEXT_MUTED,
            );
        } else {
            draw_text_5x7(
                buffer.as_bytes_mut(),
                buf_w,
                buf_h,
                query_text,
                RaycastLayout::SEARCH_TEXT_X,
                RaycastLayout::SEARCH_TEXT_Y,
                RaycastTheme::TEXT_PRIMARY,
            );

            if cursor_blink {
                // Largeur mesurée en caractères : compter les octets décalait
                // le curseur sur toute saisie accentuée.
                let cursor_x = RaycastLayout::SEARCH_TEXT_X + text_width_px(query_text);
                fill_rect(buffer, cursor_x, 13, 2, 9, RaycastTheme::ACCENT);
            }
        }

        // 3. Séparateur vertical entre panneau gauche et droite
        draw_vline(
            buffer,
            RaycastLayout::LEFT_PANE_WIDTH,
            RaycastLayout::SEARCH_BAR_HEIGHT,
            height - RaycastLayout::SEARCH_BAR_HEIGHT - RaycastLayout::FOOTER_HEIGHT,
            RaycastTheme::BORDER,
        );

        // 4. Liste des éléments dans le panneau gauche
        let list_y_start = RaycastLayout::SEARCH_BAR_HEIGHT + RaycastLayout::LIST_TOP_OFFSET;
        let max_visible_items = RaycastLayout::MAX_VISIBLE_ITEMS;
        let selected_idx = palette.selected_index();

        let scroll_offset = if selected_idx >= max_visible_items {
            selected_idx - max_visible_items + 1
        } else {
            0
        };

        for (i, item) in palette
            .filtered_items()
            .skip(scroll_offset)
            .take(max_visible_items)
            .enumerate()
        {
            let actual_idx = scroll_offset + i;
            let row_y = list_y_start + (i as i32) * RaycastLayout::ITEM_ROW_HEIGHT;
            let is_selected = actual_idx == selected_idx;

            if is_selected {
                fill_rect(
                    buffer,
                    6,
                    row_y - 2,
                    RaycastLayout::LEFT_PANE_WIDTH - 12,
                    RaycastLayout::ITEM_ROW_HEIGHT,
                    RaycastTheme::BG_SELECTED,
                );
                // Bordure gauche d'accent
                fill_rect(
                    buffer,
                    6,
                    row_y - 2,
                    3,
                    RaycastLayout::ITEM_ROW_HEIGHT,
                    RaycastTheme::ACCENT,
                );
            }

            // Titre de l'item
            let text_color = if is_selected {
                RaycastTheme::TEXT_PRIMARY
            } else {
                RaycastTheme::TEXT_MUTED
            };

            let title_display =
                truncate_with_ellipsis(&item.title, RaycastLayout::ITEM_TITLE_MAX_CHARS);

            draw_text_5x7(
                buffer.as_bytes_mut(),
                buf_w,
                buf_h,
                &title_display,
                RaycastLayout::LIST_TEXT_X,
                row_y + 4,
                text_color,
            );

            // Badge de droite (ex: [EQUIPE], [MOD], [ON], [85%])
            if let Some(badge) = &item.badge {
                let badge_x = RaycastLayout::LEFT_PANE_WIDTH - RaycastLayout::BADGE_RIGHT_INSET;
                fill_rect(
                    buffer,
                    badge_x - 4,
                    row_y + 2,
                    RaycastLayout::BADGE_WIDTH,
                    RaycastLayout::BADGE_HEIGHT,
                    RaycastTheme::BG_BADGE,
                );

                let badge_color = if item.is_equipped {
                    RaycastTheme::TEXT_BADGE_ACTIVE
                } else {
                    RaycastTheme::TEXT_MUTED
                };

                draw_text_5x7(
                    buffer.as_bytes_mut(),
                    buf_w,
                    buf_h,
                    badge,
                    badge_x,
                    row_y + 5,
                    badge_color,
                );
            }
        }

        // 5. Panneau Droit (Inspection & Live Preview / Stats)
        let inspector_x = RaycastLayout::LEFT_PANE_WIDTH + 12;
        let selected_item = palette.current_selected_item();

        // Boîte d'aperçu Gremlin
        let preview_box_x = inspector_x + 20;
        let preview_box_y = RaycastLayout::SEARCH_BAR_HEIGHT + 10;
        let preview_box_size = RaycastLayout::PREVIEW_BOX_SIZE;

        fill_rect(
            buffer,
            preview_box_x,
            preview_box_y,
            preview_box_size,
            preview_box_size,
            RaycastTheme::BG_PREVIEW_BOX,
        );
        draw_hline(
            buffer,
            preview_box_x,
            preview_box_y,
            preview_box_size,
            RaycastTheme::BORDER_PREVIEW_BOX,
        );
        draw_hline(
            buffer,
            preview_box_x,
            preview_box_y + preview_box_size - 1,
            preview_box_size,
            RaycastTheme::BORDER_PREVIEW_BOX,
        );
        draw_vline(
            buffer,
            preview_box_x,
            preview_box_y,
            preview_box_size,
            RaycastTheme::BORDER_PREVIEW_BOX,
        );
        draw_vline(
            buffer,
            preview_box_x + preview_box_size - 1,
            preview_box_y,
            preview_box_size,
            RaycastTheme::BORDER_PREVIEW_BOX,
        );

        // Rendu du Gremlin animé 1.5x au centre de la boîte
        let (preview_id, preview_cat) =
            selected_item.map_or((None, None), |item| (Some(item.id.as_str()), item.category));

        LivePetPreview::render_preview(
            buffer,
            preview_box_x + (preview_box_size - RaycastLayout::PREVIEW_SPRITE_SIZE) / 2,
            preview_box_y + (preview_box_size - RaycastLayout::PREVIEW_SPRITE_SIZE) / 2,
            1,
            wardrobe,
            preview_id,
            preview_cat,
            atlas,
            manifest,
            catalog,
            base_frame_key,
            mood_key,
        );

        // Panneau sous la boîte d'aperçu selon la section
        if let Some(item) = selected_item {
            let meta_y = preview_box_y + preview_box_size + 8;

            match item.section {
                PaletteSection::PetProfile | PaletteSection::PetCare => {
                    // Affichage des barres vitales
                    let satiety_pct = item
                        .metadata
                        .get("satiety")
                        .and_then(|s| s.trim_end_matches('%').parse::<f32>().ok())
                        .unwrap_or(80.0)
                        / 100.0;
                    let energy_pct = item
                        .metadata
                        .get("energy")
                        .and_then(|s| s.trim_end_matches('%').parse::<f32>().ok())
                        .unwrap_or(80.0)
                        / 100.0;
                    let happy_pct = item
                        .metadata
                        .get("happiness")
                        .and_then(|s| s.trim_end_matches('%').parse::<f32>().ok())
                        .unwrap_or(80.0)
                        / 100.0;

                    let bar_x = inspector_x + 20;
                    let label_w = RaycastLayout::STAT_LABEL_WIDTH;
                    let bar_w = RaycastLayout::STAT_BAR_WIDTH - label_w;
                    let bar_h = RaycastLayout::STAT_BAR_HEIGHT;
                    let line = RaycastLayout::STAT_LINE_SPACING;

                    for (row, (label, ratio, color)) in [
                        ("Faim", satiety_pct, RaycastTheme::BAR_HUNGER),
                        ("Énergie", energy_pct, RaycastTheme::BAR_ENERGY),
                        ("Joie", happy_pct, RaycastTheme::BAR_HAPPINESS),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let row_y = meta_y + (i32::try_from(row).unwrap_or(0) * line);
                        draw_text_5x7(
                            buffer.as_bytes_mut(),
                            buf_w,
                            buf_h,
                            label,
                            bar_x,
                            row_y,
                            RaycastTheme::TEXT_MUTED,
                        );
                        draw_progress_bar(
                            buffer,
                            bar_x + label_w,
                            row_y + 1,
                            bar_w,
                            bar_h,
                            ratio,
                            color,
                        );
                    }

                    if let Some(xp_str) = item.metadata.get("xp") {
                        draw_text_5x7(
                            buffer.as_bytes_mut(),
                            buf_w,
                            buf_h,
                            xp_str,
                            bar_x,
                            meta_y + (3 * line) + 2,
                            RaycastTheme::BAR_XP,
                        );
                    }
                }
                PaletteSection::GitWatcher => {
                    let text_x = inspector_x + 10;
                    draw_text_5x7(
                        buffer.as_bytes_mut(),
                        buf_w,
                        buf_h,
                        &item.title,
                        text_x,
                        meta_y,
                        RaycastTheme::TEXT_PRIMARY,
                    );
                    if let Some(branch) = item.metadata.get("branch") {
                        draw_text_5x7(
                            buffer.as_bytes_mut(),
                            buf_w,
                            buf_h,
                            &format!("Branche: {branch}"),
                            text_x,
                            meta_y + 12,
                            RaycastTheme::ACCENT_GREEN,
                        );
                    }
                    if let Some(commit) = item.metadata.get("last_commit") {
                        // Message de commit arbitraire : troncature par
                        // caractères obligatoire.
                        let commit_short =
                            truncate_with_ellipsis(commit, RaycastLayout::COMMIT_MSG_MAX_CHARS);
                        draw_text_5x7(
                            buffer.as_bytes_mut(),
                            buf_w,
                            buf_h,
                            &commit_short,
                            text_x,
                            meta_y + 24,
                            RaycastTheme::TEXT_MUTED,
                        );
                    }
                }
                _ => {
                    let text_x = inspector_x + 10;
                    draw_text_5x7(
                        buffer.as_bytes_mut(),
                        buf_w,
                        buf_h,
                        &item.title,
                        text_x,
                        meta_y,
                        RaycastTheme::TEXT_PRIMARY,
                    );

                    if let Some(cat) = &item.category {
                        draw_text_5x7(
                            buffer.as_bytes_mut(),
                            buf_w,
                            buf_h,
                            cat.display_name(),
                            text_x,
                            meta_y + 12,
                            RaycastTheme::ACCENT_GREEN,
                        );
                    }

                    let desc = item.metadata.get("description").map_or("", String::as_str);
                    if !desc.is_empty() {
                        let short_desc =
                            truncate_with_ellipsis(desc, RaycastLayout::DESCRIPTION_MAX_CHARS);
                        draw_text_5x7(
                            buffer.as_bytes_mut(),
                            buf_w,
                            buf_h,
                            &short_desc,
                            text_x,
                            meta_y + 24,
                            RaycastTheme::TEXT_MUTED,
                        );
                    }

                    let action_label = if item.is_equipped {
                        "[ ACTIF / RETIRER (Entrée) ]"
                    } else {
                        "[ CHOISIR / ACTIVER (Entrée) ]"
                    };

                    draw_text_5x7(
                        buffer.as_bytes_mut(),
                        buf_w,
                        buf_h,
                        action_label,
                        text_x,
                        meta_y + 40,
                        RaycastTheme::ACCENT,
                    );
                }
            }
        }

        // 6. Pied de page (Barre d'actions clavier)
        let footer_y = height - RaycastLayout::FOOTER_HEIGHT;
        fill_rect(
            buffer,
            0,
            footer_y,
            width,
            RaycastLayout::FOOTER_HEIGHT,
            RaycastTheme::BG_SEARCH,
        );
        draw_hline(buffer, 0, footer_y, width, RaycastTheme::BORDER);

        draw_text_5x7(
            buffer.as_bytes_mut(),
            buf_w,
            buf_h,
            "Entrée Valider   ↑↓ Naviguer   Échap Fermer   Ctrl+S Sauvegarder",
            12,
            footer_y + 9,
            RaycastTheme::TEXT_MUTED,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::ui::command_palette::RepoDisplayInfo;
    use gremlin_core::PetState;
    use gremlin_render::procedural_accessories::register_default_procedural_accessories;

    fn render_with(pet: &PetState, repos: &[RepoDisplayInfo], query: &str) -> PixelBuffer {
        let mut buffer = PixelBuffer::new(RaycastLayout::WIDTH, RaycastLayout::HEIGHT);
        let mut atlas = SpriteAtlas::new();
        atlas.load_default_procedural_sprites();

        let mut catalog = AccessoryCatalog::new();
        register_default_procedural_accessories(&mut atlas, &mut catalog);

        let wardrobe = WardrobeEquipment::new();
        let config = AppConfig::default();
        let mut palette = CommandPalette::new(&crate::ui::PaletteContext {
            catalog: &catalog,
            wardrobe: &wardrobe,
            pet_state: pet,
            config: &config,
            autostart_active: false,
            repos,
            last_save_error: None,
        });
        palette.set_query(query);

        RaycastRenderer::render_ui(
            &mut buffer,
            &palette,
            &wardrobe,
            &atlas,
            None,
            &catalog,
            "idle_0",
            "idle",
            true,
        );

        buffer
    }

    #[test]
    fn test_raycast_renderer_draws_framebuffer() {
        let pet = PetState::new("Gizmo");
        let buffer = render_with(&pet, &[], "");
        assert!(buffer.as_bytes().iter().any(|&b| b > 0));
    }

    #[test]
    fn test_render_survives_accented_and_unicode_content() {
        // Régression : la troncature par octets faisait paniquer le rendu sur
        // un nom de dépôt ou un message de commit accentué, ce qui, avec
        // `panic = "abort"`, tuait l'application.
        let repos = vec![
            RepoDisplayInfo {
                name: "dépôt-très-long-avec-des-accents-éàçù".into(),
                branch: Some("feature/refonte-générale".into()),
                last_commit_msg: Some(
                    "fix: gère les caractères « spéciaux » — et l'unicode 🐉".into(),
                ),
            },
            RepoDisplayInfo {
                name: "漢字".into(),
                branch: None,
                last_commit_msg: Some("🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉🐉".into()),
            },
        ];

        let buffer = render_with(&PetState::new("Gizmo"), &repos, "dépôt");
        assert!(buffer.as_bytes().iter().any(|&b| b > 0));
    }

    #[test]
    fn test_render_survives_dead_pet_with_revive_entry() {
        // « Réanimer Gremlin (Renaissance) » : le libellé accentué de 30
        // caractères qui déclenchait précisément le panic de troncature.
        let mut pet = PetState::new("Gizmo");
        pet.set_stats(gremlin_core::PetStats::new(0.0, 0.0, 0.0));
        assert!(!pet.is_alive());

        let buffer = render_with(&pet, &[], "réanimer");
        assert!(buffer.as_bytes().iter().any(|&b| b > 0));
    }
}
