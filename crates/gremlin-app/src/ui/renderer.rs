//! Composition du panneau de paramètres.
//!
//! # Ce que cette réécriture corrige
//!
//! * **Netteté.** Toute coordonnée passe par [`UiMetrics::px`] : la mise en page
//!   suit le facteur d'échelle du système au pixel près, au lieu de laisser la
//!   présentation réétirer une image de taille fixe.
//! * **En-têtes de section.** `PaletteSection::header_title` existait mais
//!   n'était appelé nulle part : les dix sections n'étaient jamais dessinées et
//!   la liste apparaissait comme un tas plat. Le nom de section est désormais
//!   posé une fois par groupe, aligné à droite, avec un filet de séparation.
//! * **Sous-titres.** Chaque `PaletteItem` calculait un `subtitle` que le rendu
//!   ignorait. Il est affiché sous le titre.
//! * **Badges à largeur mesurée.** Le badge avait un fond de 52 pixels fixes,
//!   alors que « RESURRECTION » en occupe davantage : le texte débordait dans le
//!   panneau d'inspection. La pastille est maintenant dimensionnée par la mesure
//!   réelle du texte.
//! * **Hiérarchie rétablie.** Les titres non sélectionnés étaient dans la même
//!   teinte que les libellés secondaires.
//! * **Repères de liste.** Ascenseur, compteur de résultats et état vide
//!   explicite : la liste ne défilait auparavant sans aucun repère.
//! * **Descriptions entières.** Le panneau d'inspection coupait à vingt-huit
//!   caractères ; il passe à la ligne par mots.

use crate::ui::command_palette::{CommandPalette, PaletteItem, PaletteSection};
use crate::ui::font;
use crate::ui::layout::{PanelDp, UiMetrics};
use crate::ui::preview::LivePetPreview;
use crate::ui::theme::Theme;
use gremlin_render::{AccessoryCatalog, PixelBuffer, SkinManifest, SpriteAtlas, WardrobeEquipment};

/// Ressources graphiques nécessaires à l'aperçu vivant du familier.
///
/// Regroupées pour éviter une signature à neuf paramètres positionnels, comme
/// l'exigent les conventions du projet.
#[derive(Clone, Copy)]
pub struct PanelScene<'a> {
    /// Équipement porté par le familier.
    pub wardrobe: &'a WardrobeEquipment,
    /// Atlas des sprites chargés.
    pub atlas: &'a SpriteAtlas,
    /// Manifest du skin actif, s'il y en a un.
    pub manifest: Option<&'a SkinManifest>,
    /// Catalogue des accessoires disponibles.
    pub catalog: &'a AccessoryCatalog,
    /// Clé de l'image d'animation courante.
    pub base_frame_key: &'a str,
    /// Clé de l'humeur courante, pour les décalages d'accessoires.
    pub mood_key: &'a str,
}

/// État d'interaction du panneau qui influence le dessin.
#[derive(Debug, Clone, Copy, Default)]
pub struct PanelInteraction {
    /// Curseur de saisie visible sur cette image.
    pub cursor_visible: bool,
    /// Ligne survolée par la souris, exprimée en indice d'item filtré.
    pub hovered_item: Option<usize>,
}

/// Style résolu du panneau : métriques d'affichage et palette de couleurs.
///
/// Les deux voyagent ensemble dans tout le rendu. Les séparer en deux paramètres
/// aurait ajouté un argument à une quinzaine de fonctions, sans qu'aucune n'ait
/// besoin de l'un sans l'autre.
#[derive(Debug, Clone, Copy)]
pub struct PanelStyle {
    /// Métriques dérivées de l'écran et de la préférence de taille.
    pub metrics: UiMetrics,
    /// Palette de couleurs résolue.
    pub theme: Theme,
}

/// État de dessin d'une ligne de la liste.
#[derive(Debug, Clone, Copy)]
struct RowState {
    /// Ordonnée du haut de la ligne, en pixels.
    y: i32,
    /// La ligne porte la sélection courante.
    is_selected: bool,
    /// La souris survole la ligne.
    is_hovered: bool,
    /// La ligne ouvre un nouveau groupe de section.
    starts_group: bool,
}

/// Compositeur logiciel du panneau de paramètres.
pub struct RaycastRenderer;

impl RaycastRenderer {
    /// Dessine le panneau complet dans `buffer`.
    pub fn render_panel(
        buffer: &mut PixelBuffer,
        style: &PanelStyle,
        palette: &CommandPalette,
        scene: &PanelScene<'_>,
        interaction: PanelInteraction,
    ) {
        let width = style.metrics.px(PanelDp::WIDTH);
        let height = style.metrics.px(PanelDp::HEIGHT);
        let left_pane = style.metrics.px(PanelDp::LEFT_PANE_WIDTH);
        let search_h = style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT);
        let footer_y = height - style.metrics.px(PanelDp::FOOTER_HEIGHT);

        // Fond opaque : la présentation logicielle n'a pas de canal alpha, la
        // composition doit donc déjà être aplatie.
        fill(buffer, 0, 0, width, height, style.theme.bg_primary);
        fill(
            buffer,
            left_pane,
            search_h,
            width - left_pane,
            footer_y - search_h,
            style.theme.bg_inspector,
        );

        Self::draw_search_bar(buffer, style, palette, interaction.cursor_visible);
        fill(
            buffer,
            left_pane,
            search_h,
            1,
            footer_y - search_h,
            style.theme.border,
        );

        Self::draw_list(buffer, style, palette, interaction);
        Self::draw_inspector(buffer, style, palette, scene);
        Self::draw_footer(buffer, style);
    }

    /// Barre de recherche : chevron, saisie ou invite, curseur, compteur.
    fn draw_search_bar(
        buffer: &mut PixelBuffer,
        style: &PanelStyle,
        palette: &CommandPalette,
        cursor_visible: bool,
    ) {
        let width = style.metrics.px(PanelDp::WIDTH);
        let height = style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT);
        let body = style.metrics.body_glyph();
        let caption = style.metrics.caption_glyph();

        fill(buffer, 0, 0, width, height, style.theme.bg_search);
        fill(buffer, 0, height - 1, width, 1, style.theme.border);

        let text_y = (height - body.height_px()) / 2;
        font::draw(
            buffer,
            "›",
            style.metrics.px(PanelDp::PANE_PADDING),
            text_y,
            style.theme.accent,
            body,
        );

        let mut text_x = style.metrics.px(PanelDp::SEARCH_TEXT_X);

        // Fil d'Ariane : il dit à l'utilisateur dans quel groupe il se trouve,
        // sans quoi une sous-liste est indiscernable de la racine.
        if let Some(crumb) = palette.view().breadcrumb() {
            font::draw(
                buffer,
                crumb,
                text_x,
                text_y,
                style.theme.accent_green,
                body,
            );
            text_x += font::measure(crumb, body) + style.metrics.px(4);

            font::draw(buffer, "›", text_x, text_y, style.theme.text_section, body);
            text_x += font::measure("›", body) + style.metrics.px(6);
        }

        let query = palette.query();

        if query.is_empty() {
            let placeholder = if palette.view().breadcrumb().is_some() {
                "Filtrer, ou rechercher partout…"
            } else {
                "Rechercher un accessoire, un dépôt, un réglage…"
            };
            font::draw(
                buffer,
                placeholder,
                text_x,
                text_y,
                style.theme.text_muted,
                body,
            );
        } else {
            font::draw(
                buffer,
                query,
                text_x,
                text_y,
                style.theme.text_primary,
                body,
            );
        }

        if cursor_visible {
            // Le curseur est posé à la position réelle de saisie, mesurée sur le
            // texte qui le précède : il restait auparavant collé en fin de
            // chaîne, et la largeur moyenne par caractère le décalait dès qu'un
            // accent était tapé.
            let before_caret = query.get(..palette.caret()).unwrap_or(query);
            let caret_x = text_x + font::measure(before_caret, body);
            fill(
                buffer,
                caret_x,
                text_y,
                style.metrics.px(2),
                body.height_px(),
                style.theme.accent,
            );
        }

        // Compteur de résultats, aligné sur le bord droit du panneau gauche.
        let total = palette.filtered_len();
        let counter = if total == 1 {
            String::from("1 résultat")
        } else {
            format!("{total} résultats")
        };
        let counter_x = style.metrics.px(PanelDp::LEFT_PANE_WIDTH)
            - style.metrics.px(PanelDp::PANE_PADDING)
            - font::measure(&counter, caption);
        font::draw(
            buffer,
            &counter,
            counter_x,
            (height - caption.height_px()) / 2,
            style.theme.text_section,
            caption,
        );
    }

    /// Liste des items, en-têtes de groupe, ascenseur et état vide.
    fn draw_list(
        buffer: &mut PixelBuffer,
        style: &PanelStyle,
        palette: &CommandPalette,
        interaction: PanelInteraction,
    ) {
        let visible = style.metrics.visible_rows();
        let total = palette.filtered_len();

        if total == 0 {
            Self::draw_empty_state(buffer, style);
            return;
        }

        // Même source de vérité que le test de survol : voir
        // `CommandPalette::scroll_offset`.
        let scroll = palette.scroll_offset(visible);

        // La section du dernier item *précédant* la fenêtre visible : sans elle,
        // un groupe commencé hors écran reprendrait un en-tête à tort après
        // défilement.
        let mut previous_section = scroll
            .checked_sub(1)
            .and_then(|index| palette.filtered_item(index))
            .map(|item| item.section);

        for row in 0..visible {
            let index = scroll + row;
            let Some(item) = palette.filtered_item(index) else {
                break;
            };

            let state = RowState {
                y: style.metrics.row_top(row),
                is_selected: index == palette.selected_index(),
                is_hovered: interaction.hovered_item == Some(index),
                starts_group: previous_section != Some(item.section),
            };
            previous_section = Some(item.section);

            Self::draw_row(buffer, style, item, state);
        }

        Self::draw_scrollbar(buffer, style, total, visible, scroll);
    }

    /// Dessine une ligne : fond, liseré, titre, sous-titre, badge, section.
    fn draw_row(buffer: &mut PixelBuffer, style: &PanelStyle, item: &PaletteItem, state: RowState) {
        let body = style.metrics.body_glyph();
        let caption = style.metrics.caption_glyph();
        let left_pane = style.metrics.px(PanelDp::LEFT_PANE_WIDTH);
        let padding = style.metrics.px(PanelDp::PANE_PADDING);
        let row_height = style.metrics.px(PanelDp::ROW_HEIGHT);
        let row_width = left_pane - style.metrics.px(PanelDp::SCROLLBAR_WIDTH + 4);

        if state.is_selected {
            fill(
                buffer,
                0,
                state.y,
                row_width,
                row_height,
                style.theme.bg_selected,
            );
            fill(
                buffer,
                0,
                state.y,
                style.metrics.px(PanelDp::SELECTION_MARKER_WIDTH),
                row_height,
                style.theme.accent,
            );
        } else if state.is_hovered {
            fill(
                buffer,
                0,
                state.y,
                row_width,
                row_height,
                style.theme.bg_row_hover,
            );
            // Le survol porte son propre liseré, de demi-largeur. Le fond seul
            // était à 1,13:1 du fond du panneau, donc quasi invisible ; le
            // renforcer l'aurait en revanche rendu indiscernable du fond de la
            // ligne sélectionnée. Un liseré fin lève l'ambiguïté : fin pour le
            // survol, plein pour la sélection.
            fill(
                buffer,
                0,
                state.y,
                (style.metrics.px(PanelDp::SELECTION_MARKER_WIDTH) / 2).max(1),
                row_height,
                style.theme.accent,
            );
        }

        if state.starts_group && !state.is_selected {
            fill(
                buffer,
                padding,
                state.y,
                left_pane - 2 * padding,
                1,
                style.theme.border,
            );
        }

        let text_x = padding + style.metrics.px(PanelDp::SELECTION_MARKER_WIDTH + 5);

        // Le badge occupe la bande basse de la ligne, le libellé de section la
        // bande haute. Chaque texte est donc borné par le voisin de *sa* bande :
        // le titre par la section, le sous-titre par le badge. Les croiser
        // laissait le sous-titre passer sous la pastille.
        let badge_left = Self::draw_badge(buffer, style, item, state.y, row_height);
        let section_left = if state.starts_group && !item.is_group_entry() {
            // Nom de section posé une fois par groupe : c'est ce qui rend enfin
            // `header_title` vivant.
            Self::draw_section_label(buffer, style, item.section, state.y)
        } else {
            style.metrics.px(PanelDp::LEFT_PANE_WIDTH)
                - style
                    .metrics
                    .px(PanelDp::PANE_PADDING + PanelDp::SCROLLBAR_WIDTH + 4)
        };

        let title_room = (section_left - text_x - padding).max(style.metrics.px(40));
        let title_color = if state.is_selected {
            style.theme.text_primary
        } else {
            style.theme.text_title_idle
        };

        // Coupure signalée par des points de suspension : garder la première
        // ligne d'un retour à la ligne amputait le libellé en silence.
        let title = font::fit(&item.title, title_room, body);
        font::draw(
            buffer,
            &title,
            text_x,
            state.y + style.metrics.px(6),
            title_color,
            body,
        );

        let subtitle_room = (badge_left - text_x - padding).max(style.metrics.px(40));
        let subtitle = font::fit(&item.subtitle, subtitle_room, caption);
        font::draw(
            buffer,
            &subtitle,
            text_x,
            state.y + style.metrics.px(6) + body.height_px() + style.metrics.px(3),
            style.theme.text_muted,
            caption,
        );
    }

    /// Message affiché lorsque la recherche ne retient aucun item.
    ///
    /// La liste vide ne dessinait auparavant rien du tout : l'utilisateur ne
    /// pouvait pas distinguer une recherche infructueuse d'une application figée.
    fn draw_empty_state(buffer: &mut PixelBuffer, style: &PanelStyle) {
        let body = style.metrics.body_glyph();
        let caption = style.metrics.caption_glyph();
        let padding = style.metrics.px(PanelDp::PANE_PADDING);
        let left_pane = style.metrics.px(PanelDp::LEFT_PANE_WIDTH);
        let text_x = padding + style.metrics.px(8);

        let list_top = style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT);
        let list_bottom =
            style.metrics.px(PanelDp::HEIGHT) - style.metrics.px(PanelDp::FOOTER_HEIGHT);
        let center_y = list_top + (list_bottom - list_top) / 2;

        font::draw(
            buffer,
            "Aucun résultat",
            text_x,
            center_y - body.height_px(),
            style.theme.text_primary,
            body,
        );

        let hint = "Effacez la recherche pour retrouver la liste complète.";
        for (line_index, line) in font::wrap(hint, left_pane - 2 * padding, caption)
            .iter()
            .enumerate()
        {
            font::draw(
                buffer,
                line,
                text_x,
                center_y + (line_index as i32) * style.metrics.px(PanelDp::CAPTION_TEXT_HEIGHT + 3),
                style.theme.text_muted,
                caption,
            );
        }
    }

    /// Libellé de section, en capitales atténuées alignées à droite.
    ///
    /// Renvoie l'abscisse de son bord gauche, qui borne la place du titre : les
    /// deux occupent la même bande haute de la ligne.
    fn draw_section_label(
        buffer: &mut PixelBuffer,
        style: &PanelStyle,
        section: PaletteSection,
        row_y: i32,
    ) -> i32 {
        let caption = style.metrics.caption_glyph();
        let label = section.header_title();
        let x = style.metrics.px(PanelDp::LEFT_PANE_WIDTH)
            - style
                .metrics
                .px(PanelDp::PANE_PADDING + PanelDp::SCROLLBAR_WIDTH + 4)
            - font::measure(label, caption);

        font::draw(
            buffer,
            label,
            x,
            row_y + style.metrics.px(5),
            style.theme.text_section,
            caption,
        );

        x
    }

    /// Pastille de statut, dimensionnée par la mesure du texte.
    ///
    /// Renvoie l'abscisse de son bord gauche, qui borne la place du titre.
    fn draw_badge(
        buffer: &mut PixelBuffer,
        style: &PanelStyle,
        item: &PaletteItem,
        row_y: i32,
        row_height: i32,
    ) -> i32 {
        let right_edge = style.metrics.px(PanelDp::LEFT_PANE_WIDTH)
            - style
                .metrics
                .px(PanelDp::PANE_PADDING + PanelDp::SCROLLBAR_WIDTH + 4);

        let Some(badge) = item.badge.as_deref() else {
            return right_edge;
        };

        let caption = style.metrics.caption_glyph();
        let text_width = font::measure(badge, caption);
        let pad_x = style.metrics.px(PanelDp::BADGE_PADDING_X);
        let pad_y = style.metrics.px(PanelDp::BADGE_PADDING_Y);
        let box_width = text_width + 2 * pad_x;
        let box_height = caption.height_px() + 2 * pad_y;
        let box_x = right_edge - box_width;

        // La pastille est posée en bas de ligne : le libellé de section occupe
        // le haut, les deux ne peuvent donc pas se recouvrir.
        let box_y = row_y + row_height - box_height - style.metrics.px(5);

        fill(
            buffer,
            box_x,
            box_y,
            box_width,
            box_height,
            style.theme.bg_badge,
        );

        let color = if item.is_equipped {
            style.theme.text_badge_active
        } else {
            style.theme.text_muted
        };
        font::draw(buffer, badge, box_x + pad_x, box_y + pad_y, color, caption);

        box_x
    }

    /// Ascenseur de la liste, masqué quand tout tient à l'écran.
    fn draw_scrollbar(
        buffer: &mut PixelBuffer,
        style: &PanelStyle,
        total: usize,
        visible: usize,
        scroll: usize,
    ) {
        if total <= visible {
            return;
        }

        let track_x = style.metrics.px(PanelDp::LEFT_PANE_WIDTH)
            - style.metrics.px(PanelDp::SCROLLBAR_WIDTH + 4);
        let track_y = style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT);
        let track_h = style.metrics.px(PanelDp::HEIGHT)
            - style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT)
            - style.metrics.px(PanelDp::FOOTER_HEIGHT);
        let bar_w = style.metrics.px(PanelDp::SCROLLBAR_WIDTH);

        fill(
            buffer,
            track_x,
            track_y,
            bar_w,
            track_h,
            style.theme.scrollbar_track,
        );

        // Proportions calculées en entiers : un curseur d'au moins huit pixels
        // reste saisissable même sur une liste de deux cents dépôts.
        let visible_i = visible.min(total) as i32;
        let total_i = total.max(1) as i32;
        let thumb_h = ((track_h * visible_i) / total_i)
            .max(style.metrics.px(8))
            .min(track_h);
        let travel = track_h - thumb_h;
        let max_scroll = (total_i - visible_i).max(1);
        let thumb_y = track_y + (travel * (scroll as i32).min(max_scroll)) / max_scroll;

        fill(
            buffer,
            track_x,
            thumb_y,
            bar_w,
            thumb_h,
            style.theme.scrollbar_thumb,
        );
    }

    /// Panneau d'inspection : aperçu vivant, description, jauges vitales.
    fn draw_inspector(
        buffer: &mut PixelBuffer,
        style: &PanelStyle,
        palette: &CommandPalette,
        scene: &PanelScene<'_>,
    ) {
        let body = style.metrics.body_glyph();
        let caption = style.metrics.caption_glyph();
        let pane_x =
            style.metrics.px(PanelDp::LEFT_PANE_WIDTH) + style.metrics.px(PanelDp::PANE_PADDING);
        let pane_width = style.metrics.px(PanelDp::WIDTH)
            - style.metrics.px(PanelDp::LEFT_PANE_WIDTH)
            - 2 * style.metrics.px(PanelDp::PANE_PADDING);
        let selected = palette.current_selected_item();

        // Zone d'aperçu, centrée dans le panneau. Elle n'est pas peinte : le
        // familier repose directement sur le fond de l'inspecteur.
        //
        // Elle recevait auparavant un fond propre, plus sombre que l'inspecteur
        // — presque noir en thème sombre, noir franc en contraste renforcé — et
        // un contour. Cela plaquait le familier dans un caisson dont il n'avait
        // pas besoin : ses bords adoucis se mélangent désormais au panneau, et
        // sa silhouette se lit sans encadrement.
        let box_size = style.metrics.px(PanelDp::PREVIEW_AREA);
        let box_x = pane_x + (pane_width - box_size) / 2;
        let box_y = style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT + 14);

        // L'aperçu est agrandi par un facteur entier : c'est le seul moyen de
        // grossir un sprite pixel-art sans le rendre flou.
        let sprite_scale = (box_size / PanelDp::PREVIEW_SPRITE).max(1);
        let sprite_side = PanelDp::PREVIEW_SPRITE * sprite_scale;
        let (preview_id, preview_category) =
            selected.map_or((None, None), |item| (Some(item.id.as_str()), item.category));

        LivePetPreview::render_preview(
            buffer,
            box_x + (box_size - sprite_side) / 2,
            box_y + (box_size - sprite_side) / 2,
            sprite_scale as u32,
            scene.wardrobe,
            preview_id,
            preview_category,
            scene.atlas,
            scene.manifest,
            scene.catalog,
            scene.base_frame_key,
            scene.mood_key,
        );

        let Some(item) = selected else {
            return;
        };

        let mut y = box_y + box_size + style.metrics.px(14);
        let line = style.metrics.px(PanelDp::INSPECTOR_LINE_SPACING);

        // Titre, sur plusieurs lignes au besoin.
        for text_line in font::wrap(&item.title, pane_width, body) {
            font::draw(buffer, text_line, pane_x, y, style.theme.text_primary, body);
            y += line;
        }

        if let Some(category) = item.category {
            font::draw(
                buffer,
                category.display_name(),
                pane_x,
                y,
                style.theme.accent_green,
                caption,
            );
            y += line;
        }

        // Description entière, coupée entre les mots.
        if let Some(description) = item.metadata.get("description") {
            y += style.metrics.px(4);
            for text_line in font::wrap(description, pane_width, caption) {
                font::draw(
                    buffer,
                    text_line,
                    pane_x,
                    y,
                    style.theme.text_muted,
                    caption,
                );
                y += style.metrics.px(PanelDp::CAPTION_TEXT_HEIGHT + 3);
            }
        }

        if matches!(
            item.section,
            PaletteSection::PetProfile | PaletteSection::PetCare
        ) {
            Self::draw_vitals(buffer, style, item, pane_x, y + style.metrics.px(6));
        }
    }

    /// Jauges vitales du familier.
    fn draw_vitals(
        buffer: &mut PixelBuffer,
        style: &PanelStyle,
        item: &PaletteItem,
        x: i32,
        y: i32,
    ) {
        let caption = style.metrics.caption_glyph();
        let label_width = style.metrics.px(PanelDp::STAT_LABEL_WIDTH);
        let bar_width = style.metrics.px(PanelDp::STAT_BAR_WIDTH);
        let bar_height = style.metrics.px(PanelDp::STAT_BAR_HEIGHT);
        let spacing = style.metrics.px(PanelDp::STAT_LINE_SPACING);

        let gauges = [
            ("Faim", "satiety", style.theme.bar_hunger),
            ("Énergie", "energy", style.theme.bar_energy),
            ("Joie", "happiness", style.theme.bar_happiness),
        ];

        for (row, (label, key, color)) in gauges.into_iter().enumerate() {
            // Une jauge sans donnée n'est pas dessinée à zéro : un rectangle vide
            // ressemble à une jauge au plus bas, ce qui alarmerait à tort. C'est
            // le cas des entrées de racine, qui portent la section du groupe sans
            // porter ses métriques.
            let Some(ratio) = item
                .metadata
                .get(key)
                .and_then(|value| value.trim_end_matches('%').parse::<f32>().ok())
                .filter(|value| value.is_finite())
                .map(|value| value / 100.0)
            else {
                continue;
            };

            let row_y = y + (row as i32) * spacing;
            font::draw(buffer, label, x, row_y, style.theme.text_muted, caption);

            progress_bar(
                buffer,
                BarRect {
                    x: x + label_width,
                    y: row_y + (caption.height_px() - bar_height) / 2,
                    width: bar_width,
                    height: bar_height,
                },
                ratio,
                color,
                style.theme.bar_bg,
            );
        }

        if let Some(xp) = item.metadata.get("xp") {
            font::draw(
                buffer,
                xp,
                x,
                y + 3 * spacing + style.metrics.px(2),
                style.theme.bar_xp,
                caption,
            );
        }
    }

    /// Barre de raccourcis clavier en pied de panneau.
    fn draw_footer(buffer: &mut PixelBuffer, style: &PanelStyle) {
        let caption = style.metrics.caption_glyph();
        let height = style.metrics.px(PanelDp::HEIGHT);
        let footer_h = style.metrics.px(PanelDp::FOOTER_HEIGHT);
        let footer_y = height - footer_h;

        fill(
            buffer,
            0,
            footer_y,
            style.metrics.px(PanelDp::WIDTH),
            footer_h,
            style.theme.bg_search,
        );
        fill(
            buffer,
            0,
            footer_y,
            style.metrics.px(PanelDp::WIDTH),
            1,
            style.theme.border,
        );

        font::draw(
            buffer,
            "Entrée valider   ↑↓ naviguer   Échap fermer   Ctrl+S sauvegarder",
            style.metrics.px(PanelDp::PANE_PADDING),
            footer_y + (footer_h - caption.height_px()) / 2,
            style.theme.text_muted,
            caption,
        );
    }
}

/// Remplit un rectangle.
fn fill(buffer: &mut PixelBuffer, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
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

/// Rectangle d'une jauge : position et dimensions.
#[derive(Debug, Clone, Copy)]
struct BarRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

/// Jauge horizontale remplie proportionnellement.
///
/// Les couleurs sont passées en paramètres plutôt que lues dans une palette
/// globale : cette fonction est libre, elle ne connaît pas le thème courant.
#[allow(clippy::cast_precision_loss)]
fn progress_bar(
    buffer: &mut PixelBuffer,
    rect: BarRect,
    ratio: f32,
    color: [u8; 4],
    track: [u8; 4],
) {
    let BarRect {
        x,
        y,
        width,
        height,
    } = rect;
    fill(buffer, x, y, width, height, track);

    // `f32::clamp` propagerait un `NaN` : la valeur vient d'une métadonnée
    // analysée depuis une chaîne, elle est donc traitée comme non fiable.
    let safe_ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.0
    };

    let filled = ((width as f32) * safe_ratio).round() as i32;
    if filled > 0 {
        fill(buffer, x, y, filled.min(width), height, color);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::ui::command_palette::RepoDisplayInfo;
    use crate::ui::layout::TextSize;
    use gremlin_core::PetState;
    use gremlin_render::procedural_accessories::register_default_procedural_accessories;

    struct Harness {
        atlas: SpriteAtlas,
        catalog: AccessoryCatalog,
        wardrobe: WardrobeEquipment,
    }

    impl Harness {
        fn new() -> Self {
            let mut atlas = SpriteAtlas::new();
            atlas.load_default_procedural_sprites();
            let mut catalog = AccessoryCatalog::new();
            register_default_procedural_accessories(&mut atlas, &mut catalog);

            Self {
                atlas,
                catalog,
                wardrobe: WardrobeEquipment::new(),
            }
        }

        fn scene(&self) -> PanelScene<'_> {
            PanelScene {
                wardrobe: &self.wardrobe,
                atlas: &self.atlas,
                manifest: None,
                catalog: &self.catalog,
                base_frame_key: "idle_0",
                mood_key: "idle",
            }
        }
    }

    fn render(
        pet: &PetState,
        repos: &[RepoDisplayInfo],
        query: &str,
        scale: f64,
    ) -> (PixelBuffer, PanelStyle) {
        let harness = Harness::new();
        let config = AppConfig::default();
        let style = PanelStyle {
            metrics: UiMetrics::for_display(scale, TextSize::Normal),
            theme: Theme::DARK,
        };
        let (width, height) = style.metrics.buffer_size();
        let mut buffer = PixelBuffer::new(width, height);

        let mut palette = CommandPalette::new(&crate::ui::PaletteContext {
            catalog: &harness.catalog,
            wardrobe: &harness.wardrobe,
            pet_state: pet,
            config: &config,
            autostart_active: false,
            repos,
            last_save_error: None,
        });
        palette.set_query(query);

        RaycastRenderer::render_panel(
            &mut buffer,
            &style,
            &palette,
            &harness.scene(),
            PanelInteraction {
                cursor_visible: true,
                hovered_item: Some(1),
            },
        );

        (buffer, style)
    }

    fn is_opaque_everywhere(buffer: &PixelBuffer) -> bool {
        buffer.as_bytes().chunks_exact(4).all(|px| px[3] == 255)
    }

    #[test]
    fn test_panel_is_fully_opaque() {
        // La présentation logicielle ignore le canal alpha : un pixel resté
        // transparent afficherait une couleur imprévisible à l'écran.
        let (buffer, _) = render(&PetState::new("Gizmo"), &[], "", 1.0);
        assert!(
            is_opaque_everywhere(&buffer),
            "le panneau laisse des pixels non opaques"
        );
    }

    #[test]
    fn test_panel_renders_at_every_density() {
        for scale in [1.0_f64, 1.25, 1.5, 2.0] {
            let (buffer, style) = render(&PetState::new("Gizmo"), &[], "", scale);
            assert_eq!(
                (buffer.width(), buffer.height()),
                style.metrics.buffer_size(),
                "tampon désaligné à l'échelle {scale}"
            );
            assert!(is_opaque_everywhere(&buffer));
        }
    }

    #[test]
    fn test_empty_result_state_is_drawn() {
        // La liste vide n'affichait auparavant rien du tout : l'utilisateur ne
        // savait pas si sa recherche avait échoué ou si l'application avait figé.
        let (buffer, style) = render(&PetState::new("Gizmo"), &[], "zzz-introuvable", 1.0);

        // Du texte doit apparaître quelque part dans la zone de liste. On
        // balaie la zone entière : sonder une ligne devinée rendrait le test
        // solidaire d'une position exacte, qu'un ajustement de maquette
        // invaliderait sans qu'aucune régression n'ait eu lieu.
        let list_top = style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT) as usize;
        let list_bottom =
            (style.metrics.px(PanelDp::HEIGHT) - style.metrics.px(PanelDp::FOOTER_HEIGHT)) as usize;
        let pane_width = style.metrics.px(PanelDp::LEFT_PANE_WIDTH) as usize;
        let stride = buffer.width() as usize;

        let inked = (list_top..list_bottom)
            .flat_map(|y| (0..pane_width).map(move |x| (y * stride + x) * 4))
            .filter(|&idx| {
                buffer
                    .as_bytes()
                    .get(idx)
                    .is_some_and(|&r| r > style.theme.bg_primary[0] + 40)
            })
            .count();

        assert!(
            inked > 50,
            "aucun message d'état vide dessiné ({inked} pixels de texte)"
        );
    }

    #[test]
    fn test_long_badge_no_longer_overflows_the_pane() {
        // Régression : « RESURRECTION » dépassait le fond de badge de 52 pixels
        // fixes et débordait dans le panneau d'inspection.
        let mut dead = PetState::new("Gizmo");
        dead.set_stats(gremlin_core::PetStats::new(0.0, 0.0, 0.0));
        let (buffer, style) = render(&dead, &[], "réanimer", 1.0);

        // La colonne du séparateur vertical doit rester la couleur de bordure :
        // aucun pixel de badge ne l'a franchie.
        let separator_x = style.metrics.px(PanelDp::LEFT_PANE_WIDTH) as usize;
        let top = style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT) as usize;
        let bottom =
            (style.metrics.px(PanelDp::HEIGHT) - style.metrics.px(PanelDp::FOOTER_HEIGHT)) as usize;

        for y in top..bottom {
            let idx = (y * buffer.width() as usize + separator_x) * 4;
            let pixel = &buffer.as_bytes()[idx..idx + 3];
            assert_eq!(
                pixel,
                &style.theme.border[..3],
                "un badge a franchi le séparateur à la ligne {y}"
            );
        }
    }

    #[test]
    fn test_render_survives_hostile_repository_metadata() {
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
                last_commit_msg: Some("🐉".repeat(40)),
            },
        ];

        let (buffer, _) = render(&PetState::new("Gizmo"), &repos, "dépôt", 1.0);
        assert!(is_opaque_everywhere(&buffer));
    }

    #[test]
    fn test_scrollbar_appears_only_when_the_list_overflows() {
        let harness = Harness::new();
        let config = AppConfig::default();
        let style = PanelStyle {
            metrics: UiMetrics::for_display(1.0, TextSize::Normal),
            theme: Theme::DARK,
        };
        let (width, height) = style.metrics.buffer_size();

        let repos: Vec<RepoDisplayInfo> = (0..200)
            .map(|index| RepoDisplayInfo {
                name: format!("dépôt-{index}"),
                branch: Some(String::from("main")),
                last_commit_msg: None,
            })
            .collect();

        let mut palette = CommandPalette::new(&crate::ui::PaletteContext {
            catalog: &harness.catalog,
            wardrobe: &harness.wardrobe,
            pet_state: &PetState::new("Gizmo"),
            config: &config,
            autostart_active: false,
            repos: &repos,
            last_save_error: None,
        });

        // Les deux cents dépôts vivent dans leur groupe : la racine, elle, tient
        // en cinq lignes et n'a donc aucune raison d'afficher un ascenseur.
        palette.enter_group(crate::ui::PaletteGroup::Repos);

        let mut buffer = PixelBuffer::new(width, height);
        RaycastRenderer::render_panel(
            &mut buffer,
            &style,
            &palette,
            &harness.scene(),
            PanelInteraction::default(),
        );

        let track_x = (style.metrics.px(PanelDp::LEFT_PANE_WIDTH)
            - style.metrics.px(PanelDp::SCROLLBAR_WIDTH + 4)) as usize;
        let track_y = style.metrics.px(PanelDp::SEARCH_BAR_HEIGHT) as usize + 4;
        let idx = (track_y * buffer.width() as usize + track_x) * 4;
        let pixel = &buffer.as_bytes()[idx..idx + 3];
        assert!(
            pixel == &style.theme.scrollbar_thumb[..3]
                || pixel == &style.theme.scrollbar_track[..3],
            "aucun ascenseur dessiné pour 200 dépôts : {pixel:?}"
        );

        // Une liste vide ne doit au contraire rien afficher.
        palette.set_query("zzz-aucune-correspondance-possible");
        let mut short = PixelBuffer::new(width, height);
        RaycastRenderer::render_panel(
            &mut short,
            &style,
            &palette,
            &harness.scene(),
            PanelInteraction::default(),
        );
        let idx = (track_y * short.width() as usize + track_x) * 4;
        assert_ne!(
            &short.as_bytes()[idx..idx + 3],
            &style.theme.scrollbar_thumb[..3],
            "ascenseur dessiné alors que la liste est vide"
        );
    }

    #[test]
    fn test_selection_scrolls_into_view() {
        let harness = Harness::new();
        let config = AppConfig::default();
        let style = PanelStyle {
            metrics: UiMetrics::for_display(1.0, TextSize::Normal),
            theme: Theme::DARK,
        };
        let (width, height) = style.metrics.buffer_size();

        let mut palette = CommandPalette::new(&crate::ui::PaletteContext {
            catalog: &harness.catalog,
            wardrobe: &harness.wardrobe,
            pet_state: &PetState::new("Gizmo"),
            config: &config,
            autostart_active: false,
            repos: &[],
            last_save_error: None,
        });

        // On descend au-delà de la fenêtre visible : le rendu doit suivre sans
        // sortir du tampon ni paniquer.
        for _ in 0..palette.filtered_len() {
            palette.select_next();
            let mut buffer = PixelBuffer::new(width, height);
            RaycastRenderer::render_panel(
                &mut buffer,
                &style,
                &palette,
                &harness.scene(),
                PanelInteraction::default(),
            );
            assert!(is_opaque_everywhere(&buffer));
        }
    }
}
