//! Planche de contrôle des accessoires sur tous les skins et toutes les poses livrés.
//!
//! ```bash
//! cargo run -p gremlin-render --example accessory_proof_sheet
//! ```
//!
//! Trois images sont écrites sous `target/` : une matrice skin × accessoire, une
//! matrice skin × humeur avec une tenue complète, et le déroulé frame par frame
//! des quatre accessoires animés. Les cellules sont agrandies en
//! nearest-neighbor afin que les défauts d'ancrage restent immédiatement visibles.

use gremlin_render::{
    register_default_accessories, AccessoryCatalog, AccessoryCategory, LayerCompositor,
    PixelBuffer, SkinManifest, SpriteAtlas, SpriteFrame, WardrobeEquipment, CANVAS_SIZE,
};
use image::{imageops::FilterType, Rgba, RgbaImage};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SCALE: u32 = 3;
const CELL_SIZE: u32 = CANVAS_SIZE * SCALE;
const SKINS: [&str; 3] = ["default", "baby", "evolved"];
const ACCESSORIES: [(AccessoryCategory, &str); 13] = [
    (AccessoryCategory::Hat, "wizard_hat"),
    (AccessoryCategory::Hat, "royal_crown"),
    (AccessoryCategory::Hat, "dev_cap"),
    (AccessoryCategory::Glasses, "vr_visor"),
    (AccessoryCategory::Glasses, "cool_shades"),
    (AccessoryCategory::Outfit, "cozy_hoodie"),
    (AccessoryCategory::Held, "coffee_mug"),
    (AccessoryCategory::Held, "dev_keyboard"),
    (AccessoryCategory::Aura, "fire_aura"),
    (AccessoryCategory::Aura, "matrix_aura"),
    // Récompenses de série : la planche les montre au même titre que les
    // autres, le déblocage n'étant pas une affaire de dessin.
    (AccessoryCategory::Hat, "streak_leaf_pin"),
    (AccessoryCategory::Glasses, "focus_headphones"),
    (AccessoryCategory::Aura, "aurora_aura"),
];
/// Les quatre animations discrètes du catalogue, et la longueur de la plus longue.
const ANIMATED: [(AccessoryCategory, &str); 4] = [
    (AccessoryCategory::Hat, "wizard_hat"),
    (AccessoryCategory::Glasses, "vr_visor"),
    (AccessoryCategory::Held, "coffee_mug"),
    (AccessoryCategory::Aura, "matrix_aura"),
];
const MAX_ANIMATION_FRAMES: usize = 4;
const MOODS: [&str; 9] = [
    "idle", "happy", "sleep", "dead", "coding", "hungry", "sick", "angry", "dragged",
];

fn main() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/skins");
    std::fs::create_dir_all("target")?;

    render_accessory_matrix(&root, Path::new("target/accessory_skin_proof.png"))?;
    render_mood_matrix(&root, Path::new("target/accessory_mood_proof.png"))?;
    render_animation_matrix(&root, Path::new("target/accessory_animation_proof.png"))?;

    println!("target/accessory_skin_proof.png");
    println!("target/accessory_mood_proof.png");
    println!("target/accessory_animation_proof.png");
    Ok(())
}

fn render_accessory_matrix(root: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let mut sheet = proof_background(ACCESSORIES.len() as u32, SKINS.len() as u32);

    for (row, skin_id) in SKINS.into_iter().enumerate() {
        let (mut atlas, catalog, manifest, skin_dir) = load_skin(root, skin_id)?;
        load_frame(&mut atlas, &skin_dir, "idle_0", "proof_base")?;

        for (column, (category, accessory_id)) in ACCESSORIES.into_iter().enumerate() {
            let mut equipment = WardrobeEquipment::new();
            equipment.equip(category, accessory_id);
            let pet = compose(
                &equipment,
                &atlas,
                &manifest,
                &catalog,
                "proof_base",
                "idle",
            );
            place_cell(&mut sheet, &pet, column as u32, row as u32)?;
        }
    }

    sheet.save(output)?;
    Ok(())
}

fn render_mood_matrix(root: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let mut sheet = proof_background(MOODS.len() as u32, SKINS.len() as u32);

    for (row, skin_id) in SKINS.into_iter().enumerate() {
        let (mut atlas, catalog, manifest, skin_dir) = load_skin(root, skin_id)?;
        let mut equipment = WardrobeEquipment::new();
        equipment.equip(AccessoryCategory::Hat, "wizard_hat");
        equipment.equip(AccessoryCategory::Glasses, "cool_shades");
        equipment.equip(AccessoryCategory::Outfit, "cozy_hoodie");
        equipment.equip(AccessoryCategory::Held, "coffee_mug");

        for (column, mood) in MOODS.into_iter().enumerate() {
            let disk_key = format!("{mood}_0");
            let atlas_key = format!("proof_{mood}");
            load_frame(&mut atlas, &skin_dir, &disk_key, &atlas_key)?;
            let pet = compose(&equipment, &atlas, &manifest, &catalog, &atlas_key, mood);
            place_cell(&mut sheet, &pet, column as u32, row as u32)?;
        }
    }

    sheet.save(output)?;
    Ok(())
}

/// Déroulé frame par frame des accessoires animés, une ligne par skin.
///
/// Les frames sont échantillonnées au milieu de leur durée : le rendu ne peut
/// donc pas basculer sur la suivante à cause d'un arrondi.
fn render_animation_matrix(root: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let rows = (ANIMATED.len() * SKINS.len()) as u32;
    let mut sheet = proof_background(MAX_ANIMATION_FRAMES as u32, rows);

    let mut row = 0;
    for (category, accessory_id) in ANIMATED {
        for skin_id in SKINS {
            let (mut atlas, catalog, manifest, skin_dir) = load_skin(root, skin_id)?;
            load_frame(&mut atlas, &skin_dir, "idle_0", "proof_base")?;

            let mut equipment = WardrobeEquipment::new();
            equipment.equip(category, accessory_id);

            let item = catalog
                .get(accessory_id)
                .ok_or("accessoire animé absent du catalogue intégré")?;
            let frames = item
                .manifest
                .frames_for_style(&manifest.accessory_style)
                .len()
                .min(MAX_ANIMATION_FRAMES);
            let step = Duration::from_millis(item.manifest.frame_duration_ms);

            for index in 0..frames {
                let elapsed = step * u32::try_from(index).unwrap_or(0) + step / 2;
                let pet = compose_animated(
                    &equipment,
                    &atlas,
                    &manifest,
                    &catalog,
                    "proof_base",
                    "idle",
                    elapsed,
                );
                place_cell(&mut sheet, &pet, index as u32, row)?;
            }
            row += 1;
        }
    }

    sheet.save(output)?;
    Ok(())
}

fn load_skin(
    root: &Path,
    skin_id: &str,
) -> Result<(SpriteAtlas, AccessoryCatalog, SkinManifest, PathBuf), Box<dyn Error>> {
    let skin_dir = root.join(skin_id);
    let json = std::fs::read_to_string(skin_dir.join("manifest.json"))?;
    let manifest = SkinManifest::from_json(&json)?;
    let mut atlas = SpriteAtlas::new();
    let mut catalog = AccessoryCatalog::new();
    register_default_accessories(&mut atlas, &mut catalog);
    Ok((atlas, catalog, manifest, skin_dir))
}

fn load_frame(
    atlas: &mut SpriteAtlas,
    skin_dir: &Path,
    disk_key: &str,
    atlas_key: &str,
) -> Result<(), Box<dyn Error>> {
    let frame = SpriteFrame::from_png_file(skin_dir.join(format!("{disk_key}.png")))?;
    atlas.insert(atlas_key.to_string(), frame);
    Ok(())
}

fn compose(
    equipment: &WardrobeEquipment,
    atlas: &SpriteAtlas,
    manifest: &SkinManifest,
    catalog: &AccessoryCatalog,
    base_frame: &str,
    mood: &str,
) -> PixelBuffer {
    let mut buffer = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    LayerCompositor::compose_layered_pet(
        &mut buffer,
        equipment,
        atlas,
        Some(manifest),
        catalog,
        base_frame,
        mood,
    );
    buffer
}

fn compose_animated(
    equipment: &WardrobeEquipment,
    atlas: &SpriteAtlas,
    manifest: &SkinManifest,
    catalog: &AccessoryCatalog,
    base_frame: &str,
    mood: &str,
    elapsed: Duration,
) -> PixelBuffer {
    let mut buffer = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    LayerCompositor::compose_layered_pet_animated(
        &mut buffer,
        equipment,
        atlas,
        Some(manifest),
        catalog,
        base_frame,
        mood,
        elapsed,
    );
    buffer
}

fn proof_background(columns: u32, rows: u32) -> RgbaImage {
    let mut image = RgbaImage::new(columns * CELL_SIZE, rows * CELL_SIZE);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let checker = ((x / (SCALE * 4)) + (y / (SCALE * 4))) % 2;
        *pixel = if checker == 0 {
            Rgba([19, 21, 27, 255])
        } else {
            Rgba([24, 27, 34, 255])
        };
    }
    image
}

fn place_cell(
    sheet: &mut RgbaImage,
    pet: &PixelBuffer,
    column: u32,
    row: u32,
) -> Result<(), image::ImageError> {
    let native = RgbaImage::from_raw(CANVAS_SIZE, CANVAS_SIZE, pet.as_bytes().to_vec())
        .ok_or_else(|| {
            image::ImageError::Limits(image::error::LimitError::from_kind(
                image::error::LimitErrorKind::DimensionError,
            ))
        })?;
    let scaled = image::imageops::resize(&native, CELL_SIZE, CELL_SIZE, FilterType::Nearest);
    image::imageops::overlay(
        sheet,
        &scaled,
        i64::from(column * CELL_SIZE),
        i64::from(row * CELL_SIZE),
    );
    Ok(())
}
