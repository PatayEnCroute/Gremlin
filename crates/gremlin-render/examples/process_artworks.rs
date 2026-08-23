//! Traitement universel et agnostique des fichiers de `PictureBank` vers des artworks transparents et des skins Gremlin.
//!
//! Les manifests produits utilisent directement les types du crate
//! ([`SkinManifest`], [`AnimationDef`], [`AnchorPoint`], [`PlayMode`]) : le format écrit
//! ici ne peut donc pas diverger de celui que le parser sait relire.

use gremlin_render::{AnchorPoint, AnimationDef, PlayMode, SkinManifest, CANVAS_SIZE};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

/// Extensions traitées lors du scan de `PictureBank`.
///
/// Volontairement limité aux formats réellement décodables : le workspace épingle
/// `image` avec `default-features = false` et seulement les features `png` et `jpeg`.
/// Accepter `webp`/`bmp` ici ferait échouer tout le lot sur la première image de ce type.
const SUPPORTED_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// Détecte avec une haute précision un pixel de fond de damier (extérieur ou intérieur).
/// Préserve strictement tous les blancs (dents, reflets, cornes, métal), les noirs de contour et les teintes saturées.
fn is_checkerboard_tile_pixel(pixel: Rgba<u8>) -> bool {
    let r = i32::from(pixel[0]);
    let g = i32::from(pixel[1]);
    let b = i32::from(pixel[2]);

    let max_val = r.max(g).max(b);
    let min_val = r.min(g).min(b);
    let diff = max_val - min_val;
    let brightness = (r + g + b) / 3;

    // Le damier JPEG a une saturation quasi nulle et une luminosité médiane
    // Jamais de blanc (>160), jamais de noir de contour (<30), jamais de couleur vive (diff > 22).
    diff <= 22 && (30..=155).contains(&brightness)
}

/// Supprime le fond de damier extérieur par propagation (flood-fill)
/// puis supprime les cavités intérieures fermées sans endommager les détails du personnage.
#[must_use]
pub fn extract_perfect_transparency(img: &DynamicImage) -> RgbaImage {
    let (width, height) = img.dimensions();
    let mut rgba = img.to_rgba8();

    // Une image de dimension nulle n'a ni contour ni intérieur : `width - 1`
    // déborderait et l'algorithme n'aurait rien à traiter.
    if width == 0 || height == 0 {
        return rgba;
    }

    // Produit calculé en `usize` : `width * height` déborderait en `u32` sur une
    // image démesurée avant même la conversion.
    let mut visited = vec![false; (width as usize) * (height as usize)];
    let mut queue = VecDeque::new();

    // 1. Flood-fill STRICT depuis le contour extérieur de l'image
    for x in 0..width {
        queue.push_back((x, 0));
        queue.push_back((x, height - 1));
    }
    for y in 0..height {
        queue.push_back((0, y));
        queue.push_back((width - 1, y));
    }

    while let Some((x, y)) = queue.pop_front() {
        if x >= width || y >= height {
            continue;
        }

        let idx = (y as usize) * (width as usize) + (x as usize);
        if visited[idx] {
            continue;
        }
        visited[idx] = true;

        let pixel = rgba.get_pixel(x, y);

        if is_checkerboard_tile_pixel(*pixel) {
            rgba.put_pixel(x, y, Rgba([0, 0, 0, 0]));

            if x > 0 {
                queue.push_back((x - 1, y));
            }
            if x + 1 < width {
                queue.push_back((x + 1, y));
            }
            if y > 0 {
                queue.push_back((x, y - 1));
            }
            if y + 1 < height {
                queue.push_back((x, y + 1));
            }
        }
    }

    // 2. Traitement des cavités intérieures fermées (ex: entre les lignes du réseau cyber ou entre les membres)
    let mut interior_visited = visited.clone();

    for start_y in 0..height {
        for start_x in 0..width {
            let start_idx = (start_y as usize) * (width as usize) + (start_x as usize);
            if interior_visited[start_idx] || rgba.get_pixel(start_x, start_y)[3] == 0 {
                continue;
            }

            if is_checkerboard_tile_pixel(*rgba.get_pixel(start_x, start_y)) {
                let mut region_pixels = Vec::new();
                let mut region_queue = VecDeque::new();
                let mut has_character_feature = false;

                region_queue.push_back((start_x, start_y));
                interior_visited[start_idx] = true;

                while let Some((cx, cy)) = region_queue.pop_front() {
                    region_pixels.push((cx, cy));
                    let p = rgba.get_pixel(cx, cy);

                    let r = i32::from(p[0]);
                    let g = i32::from(p[1]);
                    let b = i32::from(p[2]);
                    let diff = (r - g).abs().max((g - b).abs()).max((b - r).abs());
                    let brightness = (r + g + b) / 3;

                    // Si la région contient des couleurs vives ou de la haute luminosité (blanc/reflet), c'est le personnage
                    if diff > 26 || brightness > 165 {
                        has_character_feature = true;
                    }

                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let nx = cx as i32 + dx;
                        let ny = cy as i32 + dy;
                        if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                            let unx = nx as u32;
                            let uny = ny as u32;
                            let nidx = (uny as usize) * (width as usize) + (unx as usize);
                            if !interior_visited[nidx] && rgba.get_pixel(unx, uny)[3] > 0 {
                                let np = rgba.get_pixel(unx, uny);
                                if is_checkerboard_tile_pixel(*np) {
                                    interior_visited[nidx] = true;
                                    region_queue.push_back((unx, uny));
                                }
                            }
                        }
                    }
                }

                if !has_character_feature && !region_pixels.is_empty() {
                    for (cx, cy) in region_pixels {
                        rgba.put_pixel(cx, cy, Rgba([0, 0, 0, 0]));
                    }
                }
            }
        }
    }

    rgba
}

/// Redimensionnement haute fidélité pour les sprites natifs en jeu.
#[must_use]
pub fn downscale_sprite(img: &RgbaImage, target_width: u32, target_height: u32) -> RgbaImage {
    let dynamic = DynamicImage::ImageRgba8(img.clone());
    let scaled = dynamic.resize_exact(
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3,
    );
    scaled.to_rgba8()
}

/// Déduit automatiquement le pack de skin cible et le nom d'animation à partir du chemin du fichier.
fn infer_skin_and_animation(path: &Path, bank_root: &Path) -> (String, String) {
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_ascii_lowercase();

    // Vérifier si le fichier est dans un sous-dossier de PictureBank
    let relative = path.strip_prefix(bank_root).unwrap_or(path);
    let parent_folder = relative.parent().and_then(|p| p.to_str()).unwrap_or("");

    let skin_name = if !parent_folder.is_empty() && parent_folder != "." {
        parent_folder.replace(['\\', '/'], "_").to_ascii_lowercase()
    } else if file_stem.contains("baby") || file_stem.contains("larva") || file_stem.contains("egg")
    {
        String::from("baby")
    } else if file_stem.contains("evolve")
        || file_stem.contains("cyber")
        || file_stem.contains("mega")
        || file_stem.contains("boss")
    {
        String::from("evolved")
    } else {
        String::from("default")
    };

    let anim_name = if file_stem.contains("adult")
        || file_stem.contains("default")
        || file_stem.contains("idle")
        || file_stem.contains("baby")
        || file_stem.contains("evolved")
    {
        String::from("idle")
    } else if file_stem.contains("happy")
        || file_stem.contains("joy")
        || file_stem.contains("smile")
    {
        String::from("happy")
    } else if file_stem.contains("code")
        || file_stem.contains("coding")
        || file_stem.contains("dev")
        || file_stem.contains("work")
    {
        String::from("coding")
    } else if file_stem.contains("angry") || file_stem.contains("rage") || file_stem.contains("mad")
    {
        String::from("angry")
    } else if file_stem.contains("hungry")
        || file_stem.contains("food")
        || file_stem.contains("eat")
        || file_stem.contains("starv")
    {
        String::from("hungry")
    } else if file_stem.contains("sick")
        || file_stem.contains("ill")
        || file_stem.contains("dizzy")
        || file_stem.contains("poison")
    {
        String::from("sick")
    } else if file_stem.contains("dead")
        || file_stem.contains("die")
        || file_stem.contains("rip")
        || file_stem.contains("ko")
        || file_stem.contains("ghost")
    {
        String::from("dead")
    } else if file_stem.contains("sleep") || file_stem.contains("rest") || file_stem.contains("bed")
    {
        String::from("sleep")
    } else {
        file_stem
    };

    (skin_name, anim_name)
}

/// Cadence et mode de lecture associés à un nom d'animation.
fn animation_timing(anim_name: &str) -> (u64, PlayMode) {
    match anim_name {
        "dead" => (1000, PlayMode::Once),
        "happy" => (180, PlayMode::Loop),
        "hungry" => (400, PlayMode::Loop),
        "sick" => (500, PlayMode::Loop),
        "coding" | "angry" => (200, PlayMode::Loop),
        _ => (300, PlayMode::Loop),
    }
}

/// Points d'attache de référence documentés pour un pack de skin.
///
/// Ces ancrages sont une **métadonnée descriptive** destinée aux auteurs de skins et à
/// l'outillage : le compositeur ne les ajoute pas comme translation, puisque tous les
/// calques sont peints sur un canevas pleine taille déjà positionné (voir
/// `gremlin_render::layer`).
fn reference_anchors(skin_name: &str) -> BTreeMap<String, AnchorPoint> {
    let (hat_y, glasses_y, held_y) = match skin_name {
        "baby" => (6, 22, 24),
        "evolved" => (2, 18, 30),
        _ => (4, 20, 28),
    };

    [
        ("aura", AnchorPoint { x: 0, y: 0 }),
        ("base", AnchorPoint { x: 0, y: 0 }),
        ("outfit", AnchorPoint { x: 0, y: 0 }),
        (
            "glasses",
            AnchorPoint {
                x: 16,
                y: glasses_y,
            },
        ),
        ("hat", AnchorPoint { x: 16, y: hat_y }),
        ("held", AnchorPoint { x: 32, y: held_y }),
    ]
    .into_iter()
    .map(|(name, point)| (name.to_string(), point))
    .collect()
}

/// Association `nom d'animation -> clés de frames` pour un pack de skin.
type AnimationFrames = BTreeMap<String, Vec<String>>;

/// Traite une image source : artwork HD transparent + sprite(s) de skin.
///
/// Renvoie le nom du pack de skin et le nom de l'animation alimentés par cette image.
fn process_single_artwork(
    path: &Path,
    bank_dir: &Path,
    artworks_out: &Path,
    skins_out: &Path,
    skin_animations: &mut HashMap<String, AnimationFrames>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (skin_name, anim_name) = infer_skin_and_animation(path, bank_dir);
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("artwork");

    println!(
        "Traitement de {} -> Skin: [{}], Animation: [{}]",
        path.display(),
        skin_name,
        anim_name
    );

    let img = image::open(path)?;
    let transparent_img = extract_perfect_transparency(&img);

    // 1. Sauvegarde de l'Artwork HD transparent
    let hd_path = artworks_out.join(format!("{skin_name}_{file_stem}_transparent.png"));
    transparent_img.save(&hd_path)?;
    println!("  -> Artwork HD : {}", hd_path.display());

    // 2. Sauvegarde du sprite pleine taille dans le dossier de skin approprié
    let target_skin_dir = skins_out.join(&skin_name);
    fs::create_dir_all(&target_skin_dir)?;

    let sprite = downscale_sprite(&transparent_img, CANVAS_SIZE, CANVAS_SIZE);
    let sprite_frame_name = format!("{anim_name}_0");
    let sprite_file = target_skin_dir.join(format!("{sprite_frame_name}.png"));
    sprite.save(&sprite_file)?;
    println!("  -> Sprite Frame : {}", sprite_file.display());

    let anim_map = skin_animations.entry(skin_name).or_default();

    // Création automatique de la frame 1 (micro-boucle) pour idle/happy/hungry/sick
    if matches!(anim_name.as_str(), "idle" | "happy" | "hungry" | "sick") {
        let frame_1_name = format!("{anim_name}_1");
        sprite.save(target_skin_dir.join(format!("{frame_1_name}.png")))?;
        anim_map.insert(anim_name, vec![sprite_frame_name, frame_1_name]);
    } else {
        anim_map.insert(anim_name, vec![sprite_frame_name]);
    }

    Ok(())
}

/// Construit le `SkinManifest` d'un pack à partir des animations collectées.
fn build_skin_manifest(skin_name: &str, anims: &AnimationFrames) -> SkinManifest {
    let mut animations: BTreeMap<String, AnimationDef> = anims
        .iter()
        .map(|(anim_name, frames)| {
            let (frame_duration_ms, mode) = animation_timing(anim_name);
            (
                anim_name.clone(),
                AnimationDef {
                    frames: frames.clone(),
                    frame_duration_ms,
                    mode,
                },
            )
        })
        .collect();

    // Si l'animation 'dragged' n'est pas spécifiquement fournie, créer un fallback gracieux
    if !animations.contains_key("dragged") {
        let fallback_frame: Vec<String> = if animations.contains_key("angry") {
            vec![String::from("angry_0")]
        } else if animations.contains_key("idle") {
            vec![String::from("idle_0")]
        } else {
            Vec::new()
        };

        if !fallback_frame.is_empty() {
            animations.insert(
                String::from("dragged"),
                AnimationDef {
                    frames: fallback_frame,
                    frame_duration_ms: 200,
                    mode: PlayMode::Loop,
                },
            );
        }
    }

    let display_name = match skin_name {
        "default" => "Classic Gremlin",
        "baby" => "Baby Gremlin",
        "evolved" => "Evolved Cyber Gremlin",
        other => other,
    };

    SkinManifest {
        name: String::from(display_name),
        author: String::from("Gremlin Studio"),
        version: String::from("1.0.0"),
        frame_width: CANVAS_SIZE,
        frame_height: CANVAS_SIZE,
        anchors: reference_anchors(skin_name),
        animations,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Traitement Agnostique & Haute Fidélité de PictureBank ===");

    let bank_dir = Path::new("PictureBank");
    if !bank_dir.exists() {
        eprintln!("Erreur : dossier PictureBank introuvable !");
        return Ok(());
    }

    let artworks_out = Path::new("assets/artworks");
    let skins_out = Path::new("assets/skins");

    fs::create_dir_all(artworks_out)?;
    fs::create_dir_all(skins_out)?;

    // Découverte dynamique agnostique de toutes les images supportées
    let mut image_files = Vec::new();
    collect_image_files(bank_dir, &mut image_files)?;
    image_files.sort();

    println!(
        "{} image(s) découverte(s) dans PictureBank.",
        image_files.len()
    );

    let mut skin_animations: HashMap<String, AnimationFrames> = HashMap::new();

    for path in &image_files {
        process_single_artwork(
            path,
            bank_dir,
            artworks_out,
            skins_out,
            &mut skin_animations,
        )?;
    }

    // Génération dynamique et agnostique des manifest.json pour chaque pack de skin
    for (skin_name, anims) in &skin_animations {
        let manifest = build_skin_manifest(skin_name, anims);

        // Contrôle de non-régression : ce que l'on écrit doit être relisible par le parser.
        manifest.validate()?;

        let json = serde_json::to_string_pretty(&manifest)?;
        fs::write(skins_out.join(skin_name).join("manifest.json"), json)?;
        println!(
            "  -> Manifest généré pour [{skin_name}] avec {} animation(s)",
            anims.len()
        );
    }

    println!("\nTraitement agnostique terminé avec succès !");
    Ok(())
}

fn collect_image_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_image_files(&path, files)?;
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
            {
                files.push(path);
            }
        }
    }

    Ok(())
}
