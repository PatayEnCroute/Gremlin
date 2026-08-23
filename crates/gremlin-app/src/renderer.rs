//! Gestionnaire de surface et de rendu GPU avec `pixels` / `wgpu`.

use crate::error::AppError;
use gremlin_render::PixelBuffer;
use pixels::{wgpu, Pixels, PixelsBuilder, SurfaceTexture};
use std::sync::Arc;
use tracing::info;
use winit::window::Window;

/// Orchestrateur de rendu GPU encapsulant la surface `pixels`.
pub struct AppRenderer {
    pixels: Pixels<'static>,
    buffer_width: u32,
    buffer_height: u32,
}

impl AppRenderer {
    /// Initialise le renderer GPU avec une surface transparente.
    ///
    /// # Errors
    /// Renvoie `AppError::Pixels` si l'initialisation du contexte GPU wgpu échoue.
    pub fn new(
        window: Arc<Window>,
        buffer_width: u32,
        buffer_height: u32,
    ) -> Result<Self, AppError> {
        let size = window.inner_size();
        let surface_texture = SurfaceTexture::new(size.width, size.height, window);
        let pixels = PixelsBuilder::new(buffer_width, buffer_height, surface_texture)
            .clear_color(wgpu::Color::TRANSPARENT)
            .build()?;

        info!(
            width = buffer_width,
            height = buffer_height,
            window_w = size.width,
            window_h = size.height,
            "Renderer GPU pixels initialisé avec succès"
        );

        Ok(Self {
            pixels,
            buffer_width,
            buffer_height,
        })
    }

    /// Redimensionne la surface de présentation GPU à la taille de la fenêtre hôte.
    ///
    /// # Errors
    /// Renvoie `AppError::Pixels` en cas d'échec du redimensionnement de la swapchain.
    pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), AppError> {
        if width > 0 && height > 0 {
            self.pixels.resize_surface(width, height)?;
        }
        Ok(())
    }

    /// Redimensionne le framebuffer interne.
    ///
    /// # Errors
    /// Renvoie `AppError::Pixels` en cas d'erreur de redimensionnement de texture.
    #[allow(dead_code)]
    pub fn resize_buffer(&mut self, width: u32, height: u32) -> Result<(), AppError> {
        if width > 0 && height > 0 {
            self.buffer_width = width;
            self.buffer_height = height;
            self.pixels.resize_buffer(width, height)?;
        }
        Ok(())
    }

    /// Copie le tampon de pixels logiciel vers la texture GPU et déclenche le rendu.
    ///
    /// # Errors
    /// Renvoie `AppError::Pixels` si l'envoi de la commande de rendu échoue.
    pub fn render_buffer(&mut self, buffer: &PixelBuffer) -> Result<(), AppError> {
        let frame = self.pixels.frame_mut();
        let src_bytes = buffer.as_bytes();
        let copy_len = frame.len().min(src_bytes.len());
        frame[..copy_len].copy_from_slice(&src_bytes[..copy_len]);

        self.pixels.render()?;
        Ok(())
    }

    /// Largeur du framebuffer interne.
    #[must_use]
    #[allow(dead_code)]
    pub const fn buffer_width(&self) -> u32 {
        self.buffer_width
    }

    /// Hauteur du framebuffer interne.
    #[must_use]
    #[allow(dead_code)]
    pub const fn buffer_height(&self) -> u32 {
        self.buffer_height
    }
}
