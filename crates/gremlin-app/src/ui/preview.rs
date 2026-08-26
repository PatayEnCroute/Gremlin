//! Composant d'aperçu animé interactif en direct du Gremlin habillé.

use gremlin_render::{
    AccessoryCatalog, AccessoryCategory, LayerCompositor, PixelBuffer, SkinManifest, SpriteAtlas,
    SpriteFrame, WardrobeEquipment, CANVAS_SIZE,
};

/// Côté de la toile de composition, en pixels natifs.
const CANVAS_SIDE: i32 = CANVAS_SIZE as i32;

/// Générateur de rendu d'aperçu pour le panneau d'inspection Raycast.
pub struct LivePetPreview;

impl LivePetPreview {
    /// Coin supérieur gauche auquel poser la toile pour que le corps — et non le
    /// coin vide de la toile — soit centré dans la zone d'aperçu.
    ///
    /// Le calage se fait sur les limites alpha de la frame de base, jamais sur
    /// l'image composée : un chapeau haut ou une aura large ferait sinon sauter
    /// le familier d'un item survolé au suivant. Le résultat est ensuite borné à
    /// la zone, afin que l'aperçu ne déborde jamais sur le texte qui l'entoure.
    #[must_use]
    pub fn canvas_origin(
        atlas: &SpriteAtlas,
        base_frame_key: &str,
        area_x: i32,
        area_y: i32,
        area_size: i32,
        scale: u32,
    ) -> (i32, i32) {
        let scale = i32::try_from(scale.max(1)).unwrap_or(1);
        let canvas_side = CANVAS_SIDE.saturating_mul(scale);
        let centered = (
            area_x + (area_size - canvas_side) / 2,
            area_y + (area_size - canvas_side) / 2,
        );

        // Sans marge à distribuer, le centrage de la toile reprend la main.
        if canvas_side >= area_size {
            return centered;
        }

        let Some(bounds) = atlas
            .get(base_frame_key)
            .and_then(SpriteFrame::opaque_bounds)
        else {
            return centered;
        };
        let center_of = |start: u32, span: u32| {
            let start = i32::try_from(start).unwrap_or(0);
            let span = i32::try_from(span).unwrap_or(0);
            (start * scale) + (span * scale) / 2
        };

        (
            (area_x + area_size / 2 - center_of(bounds.x, bounds.width))
                .clamp(area_x, area_x + area_size - canvas_side),
            (area_y + area_size / 2 - center_of(bounds.y, bounds.height))
                .clamp(area_y, area_y + area_size - canvas_side),
        )
    }

    /// Rend le Gremlin avec son équipement actif ou l'item actuellement survolé/sélectionné
    /// ("Live Try-On" en temps réel) dans un tampon de pixels aux dimensions données.
    #[allow(clippy::too_many_arguments)]
    pub fn render_preview(
        target_buffer: &mut PixelBuffer,
        dest_x: i32,
        dest_y: i32,
        scale: u32,
        equipment: &WardrobeEquipment,
        preview_item_id: Option<&str>,
        preview_item_category: Option<AccessoryCategory>,
        atlas: &SpriteAtlas,
        manifest: Option<&SkinManifest>,
        catalog: &AccessoryCatalog,
        base_frame_key: &str,
        mood_key: &str,
    ) {
        // Préparer un équipement virtuel incluant temporairement l'item sélectionné pour l'aperçu
        let mut preview_equipment = equipment.clone();
        if let (Some(id), Some(cat)) = (preview_item_id, preview_item_category) {
            preview_equipment.equip(cat, id);
        }

        // Rendu 64x64 natif
        let mut pet_buf = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        LayerCompositor::compose_layered_pet(
            &mut pet_buf,
            &preview_equipment,
            atlas,
            manifest,
            catalog,
            base_frame_key,
            mood_key,
        );

        // Blit avec mise à l'échelle (scale: 1x, 2x) vers le tampon de destination
        if scale <= 1 {
            target_buffer.blit(pet_buf.as_bytes(), CANVAS_SIZE, CANVAS_SIZE, dest_x, dest_y);
        } else {
            let s = scale as i32;
            let src_bytes = pet_buf.as_bytes();

            for py in 0..CANVAS_SIDE {
                for px in 0..CANVAS_SIDE {
                    let src_idx = ((py as usize) * (CANVAS_SIZE as usize) + (px as usize)) * 4;
                    if src_idx + 3 < src_bytes.len() {
                        let color = [
                            src_bytes[src_idx],
                            src_bytes[src_idx + 1],
                            src_bytes[src_idx + 2],
                            src_bytes[src_idx + 3],
                        ];
                        if color[3] > 0 {
                            for dy in 0..s {
                                for dx in 0..s {
                                    target_buffer.blend_pixel(
                                        (dest_x + px * s + dx) as u32,
                                        (dest_y + py * s + dy) as u32,
                                        color,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
