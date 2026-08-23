//! Composant d'aperçu animé interactif en direct du Gremlin habillé.

use gremlin_render::{
    AccessoryCatalog, AccessoryCategory, LayerCompositor, PixelBuffer, SkinManifest, SpriteAtlas,
    WardrobeEquipment,
};

/// Générateur de rendu d'aperçu pour le panneau d'inspection Raycast.
pub struct LivePetPreview;

impl LivePetPreview {
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
        let mut pet_buf = PixelBuffer::new(64, 64);
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
            target_buffer.blit(pet_buf.as_bytes(), 64, 64, dest_x, dest_y);
        } else {
            let s = scale as i32;
            let src_bytes = pet_buf.as_bytes();

            for py in 0..64i32 {
                for px in 0..64i32 {
                    let src_idx = ((py as usize) * 64 + (px as usize)) * 4;
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
