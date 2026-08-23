//! Génère les icônes multi-tailles de l'application Gremlin à partir de `Icon/Logo.png`.
//!
//! L'encodeur ICO est écrit à la main : le workspace épingle `image` avec
//! `default-features = false` et seulement les features `png` et `jpeg`, donc
//! `image::codecs::ico` n'est pas disponible.

use image::{DynamicImage, RgbaImage};
use std::fs;
use std::path::Path;

/// Taille à partir de laquelle une entrée ICO est stockée en PNG compressé.
///
/// Recommandation Microsoft : seules les entrées 256x256 sont encodées en PNG ; les
/// tailles inférieures doivent rester des DIB BMP pour rester lisibles par les
/// anciennes versions du shell Windows (Windows XP et antérieur ignorent purement et
/// simplement une entrée PNG).
const PNG_ENTRY_MIN_SIZE: u32 = 256;

/// Taille en octets de l'en-tête `BITMAPINFOHEADER`.
const BITMAPINFOHEADER_SIZE: u32 = 40;

/// Encode une image en DIB BMP 32 bits (BGRA, lignes de bas en haut) tel qu'attendu
/// dans une entrée ICO, masque AND compris.
fn encode_bmp_dib(img: &RgbaImage) -> Vec<u8> {
    let width = img.width();
    let height = img.height();

    // Le masque AND est padé à 4 octets par ligne, même s'il est intégralement nul :
    // avec du 32 bpp c'est le canal alpha qui fait foi, mais l'en-tête l'exige.
    let and_row_bytes = width.div_ceil(32) * 4;
    let and_mask_len = (and_row_bytes as usize) * (height as usize);
    let xor_len = (width as usize) * (height as usize) * 4;

    let mut dib = Vec::with_capacity(BITMAPINFOHEADER_SIZE as usize + xor_len + and_mask_len);

    // BITMAPINFOHEADER
    dib.extend_from_slice(&BITMAPINFOHEADER_SIZE.to_le_bytes());
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    // Hauteur doublée : le DIB embarque le masque XOR puis le masque AND.
    dib.extend_from_slice(&((height as i32) * 2).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&((xor_len + and_mask_len) as u32).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // Masque XOR : BGRA, lignes de bas en haut.
    for y in (0..height).rev() {
        for x in 0..width {
            let px = img.get_pixel(x, y).0;
            dib.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }

    // Masque AND entièrement nul (transparence pilotée par le canal alpha).
    dib.resize(dib.len() + and_mask_len, 0);

    dib
}

/// Encode une image en PNG, format attendu pour les grandes entrées ICO.
fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, image::ImageError> {
    let mut png_bytes = Vec::new();
    let dyn_img = DynamicImage::ImageRgba8(img.clone());
    dyn_img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )?;
    Ok(png_bytes)
}

fn create_ico(
    images: &[(u32, u32, RgbaImage)],
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Format de fichier ICO standard Windows
    let mut ico_data = Vec::new();
    let num_images = u16::try_from(images.len())?;

    // ICONDIR Header (6 bytes)
    ico_data.extend_from_slice(&0u16.to_le_bytes()); // Reserved (0)
    ico_data.extend_from_slice(&1u16.to_le_bytes()); // Type 1 = ICO
    ico_data.extend_from_slice(&num_images.to_le_bytes()); // Nombre d'images

    // Calculer le décalage initial des données d'image (après le répertoire d'entrées)
    let header_size = 6 + (images.len() * 16);
    let mut current_offset = u32::try_from(header_size)?;

    let mut payloads = Vec::with_capacity(images.len());
    for (w, h, img) in images {
        // PNG pour les grandes tailles, DIB BMP pour les petites (compatibilité shell).
        let payload = if *w >= PNG_ENTRY_MIN_SIZE || *h >= PNG_ENTRY_MIN_SIZE {
            encode_png(img)?
        } else {
            encode_bmp_dib(img)
        };

        let b_width = if *w >= 256 { 0u8 } else { *w as u8 };
        let b_height = if *h >= 256 { 0u8 } else { *h as u8 };
        let bytes_len = u32::try_from(payload.len())?;

        // ICONDIRENTRY (16 bytes)
        ico_data.push(b_width);
        ico_data.push(b_height);
        ico_data.push(0); // Color palette
        ico_data.push(0); // Reserved
        ico_data.extend_from_slice(&1u16.to_le_bytes()); // Color planes (1)
        ico_data.extend_from_slice(&32u16.to_le_bytes()); // Bits per pixel (32)
        ico_data.extend_from_slice(&bytes_len.to_le_bytes()); // Taille des données
        ico_data.extend_from_slice(&current_offset.to_le_bytes()); // Offset

        current_offset = current_offset
            .checked_add(bytes_len)
            .ok_or("fichier ICO trop volumineux")?;
        payloads.push(payload);
    }

    // Écriture des charges utiles consécutives
    for payload in payloads {
        ico_data.extend_from_slice(&payload);
    }

    fs::write(output_path, ico_data)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Génération des Icônes d'Application Gremlin ===");

    let logo_path = Path::new("Icon/Logo.png");
    if !logo_path.exists() {
        eprintln!("Erreur : Icon/Logo.png introuvable !");
        return Ok(());
    }

    let assets_dir = Path::new("assets");
    fs::create_dir_all(assets_dir)?;

    let src_img = image::open(logo_path)?;
    println!("Logo source : {}x{}", src_img.width(), src_img.height());

    // 1. Sauvegarder la copie directe en assets/icon.png
    fs::copy(logo_path, assets_dir.join("icon.png"))?;
    println!("  -> Copié vers assets/icon.png");

    let sizes = [16, 24, 32, 48, 64, 128, 256];
    let mut ico_images = Vec::new();

    for size in sizes {
        let resized = src_img
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .to_rgba8();

        let png_name = format!("icon_{size}.png");
        let out_file = assets_dir.join(&png_name);
        resized.save(&out_file)?;
        println!("  -> Généré : assets/{png_name}");

        ico_images.push((size, size, resized));
    }

    // 2. Générer assets/icon.ico pour Windows
    let ico_path = assets_dir.join("icon.ico");
    create_ico(&ico_images, &ico_path)?;
    println!("  -> Généré : assets/icon.ico (256 en PNG, tailles inférieures en DIB BMP)");

    println!("\nToutes les icônes d'application ont été générées avec succès !");
    Ok(())
}
