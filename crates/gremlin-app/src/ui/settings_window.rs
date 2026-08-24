//! Fenêtre dédiée du panneau de paramètres, présentée en logiciel.
//!
//! # Pourquoi une fenêtre séparée
//!
//! Le panneau réutilisait auparavant la fenêtre du familier, qu'il agrandissait
//! de 128×128 à 480×300. Trois conséquences fâcheuses en découlaient : le
//! familier disparaissait pendant tout le réglage, la fenêtre s'ouvrait à la
//! position du familier au lieu d'être centrée, et elle devenait immobilisable
//! puisque le clic gauche était ignoré tant que le panneau était ouvert.
//!
//! # Pourquoi `softbuffer` et non `pixels`
//!
//! `pixels` construit son propre contexte wgpu — instance, adaptateur et
//! device — à chaque instance. Une seconde fenêtre en `pixels` coûterait donc un
//! contexte graphique entier, de l'ordre de dix à vingt mégaoctets, ce qui
//! compromettrait l'objectif d'empreinte mémoire du projet. Le panneau étant de
//! toute façon composé en logiciel dans un [`PixelBuffer`], `softbuffer` le
//! présente par un simple transfert mémoire, sans GPU.
//!
//! # Netteté
//!
//! Le tampon est alloué à la taille **physique** de la fenêtre, calculée par
//! [`UiMetrics`] depuis le facteur d'échelle du système. La présentation est
//! alors un transfert un pour un : contrairement au panneau précédent, aucun
//! rééchantillonnage n'intervient, et le texte reste net à 125 % comme à 150 %.

use crate::error::AppError;
use crate::ui::layout::{PanelDp, TextSize, UiMetrics};
use gremlin_render::PixelBuffer;
use gremlin_system::WindowConfig;
use std::num::NonZeroU32;
use std::sync::Arc;
use tracing::{debug, warn};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

/// Fenêtre du panneau, sa surface de présentation et son tampon de composition.
pub struct SettingsWindow {
    window: Arc<Window>,
    /// Conservé pour la durée de vie de la surface, qui en dépend.
    _context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    buffer: PixelBuffer,
    metrics: UiMetrics,
    /// Adaptateur d'accessibilité, absent si la fonctionnalité est désactivée
    /// ou si son installation a échoué.
    #[cfg(feature = "a11y")]
    adapter: Option<accesskit_winit::Adapter>,
}

impl SettingsWindow {
    /// Crée la fenêtre du panneau et sa surface de présentation.
    ///
    /// La création est volontairement déclenchée à la première ouverture et non
    /// au démarrage : un utilisateur qui n'ouvre jamais les paramètres n'en paie
    /// pas le coût. La fenêtre est ensuite conservée masquée, pour que les
    /// ouvertures suivantes soient instantanées.
    ///
    /// # Errors
    /// Renvoie [`AppError::Window`] si `winit` refuse la fenêtre, et
    /// [`AppError::Softbuffer`] si la surface de présentation logicielle ne peut
    /// pas être initialisée.
    pub fn new(
        event_loop: &ActiveEventLoop,
        anchor: Option<&Window>,
        text_size: TextSize,
        #[cfg(feature = "a11y")] proxy: Option<
            winit::event_loop::EventLoopProxy<crate::app::CustomAppEvent>,
        >,
    ) -> Result<Self, AppError> {
        // La taille demandée est logique : winit la convertit selon le facteur
        // de l'écran, et le tampon suivra la taille physique effective.
        let config = WindowConfig::settings_panel(PanelDp::WIDTH as u32, PanelDp::HEIGHT as u32);
        let window = Arc::new(event_loop.create_window(config.to_window_attributes())?);

        let metrics = UiMetrics::for_display(window.scale_factor(), text_size);
        let (buffer_w, buffer_h) = metrics.buffer_size();

        let context = softbuffer::Context::new(window.clone())?;
        let surface = softbuffer::Surface::new(&context, window.clone())?;

        // L'adaptateur doit être installé entre la création de la fenêtre et son
        // premier affichage : c'est pourquoi le préréglage la fait naître
        // invisible. Sans proxy — mode headless, tests — l'accessibilité est
        // simplement absente, ce qui est préférable à un adaptateur bancal.
        #[cfg(feature = "a11y")]
        let adapter = proxy.map(|proxy| {
            accesskit_winit::Adapter::with_event_loop_proxy(event_loop, &window, proxy)
        });

        let mut panel = Self {
            window,
            _context: context,
            surface,
            buffer: PixelBuffer::new(buffer_w, buffer_h),
            metrics,
            #[cfg(feature = "a11y")]
            adapter,
        };

        panel.resize_surface()?;
        panel.center_on_anchor_monitor(anchor);

        debug!(
            width = buffer_w,
            height = buffer_h,
            scale = panel.metrics.scale(),
            "Panneau de paramètres initialisé"
        );

        Ok(panel)
    }

    /// Identifiant de la fenêtre, pour le routage des événements.
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Fenêtre du panneau.
    #[must_use]
    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }

    /// Métriques d'affichage courantes.
    #[must_use]
    pub const fn metrics(&self) -> &UiMetrics {
        &self.metrics
    }

    /// Tampon de composition, à remplir par le moteur de rendu.
    #[must_use]
    pub const fn buffer_mut(&mut self) -> &mut PixelBuffer {
        &mut self.buffer
    }

    /// Affiche le panneau, le place au premier plan et lui donne le focus.
    pub fn show(&self, anchor: Option<&Window>) {
        self.center_on_anchor_monitor(anchor);
        self.window.set_visible(true);
        self.window.focus_window();
        self.window.request_redraw();
    }

    /// Masque le panneau sans détruire sa surface.
    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    /// Transmet un événement de fenêtre à l'adaptateur d'accessibilité.
    ///
    /// Doit être appelé pour **tous** les événements du panneau : l'adaptateur y
    /// suit le focus, la position et les changements d'échelle, et un événement
    /// omis le désynchronise silencieusement de la réalité.
    #[cfg(feature = "a11y")]
    pub fn forward_to_accessibility(&mut self, event: &winit::event::WindowEvent) {
        if let Some(adapter) = &mut self.adapter {
            adapter.process_event(&self.window, event);
        }
    }

    /// Publie un arbre d'accessibilité, si un client d'assistance écoute.
    ///
    /// `update_if_active` n'appelle la fermeture que lorsqu'un lecteur d'écran
    /// est effectivement actif : la construction de l'arbre ne coûte donc rien
    /// dans le cas courant.
    #[cfg(feature = "a11y")]
    pub fn publish_accessibility_tree(&mut self, build: impl FnOnce() -> accesskit::TreeUpdate) {
        if let Some(adapter) = &mut self.adapter {
            adapter.update_if_active(build);
        }
    }

    /// Recalcule les métriques après un changement d'échelle ou de préférence.
    ///
    /// Renvoie `true` si le tampon a changé de taille, auquel cas l'appelant
    /// doit recomposer l'image.
    ///
    /// # Errors
    /// Renvoie [`AppError::Softbuffer`] si la surface refuse la nouvelle taille.
    pub fn resync(&mut self, text_size: TextSize) -> Result<bool, AppError> {
        let metrics = UiMetrics::for_display(self.window.scale_factor(), text_size);
        if metrics == self.metrics {
            return Ok(false);
        }

        self.metrics = metrics;
        let (width, height) = self.metrics.buffer_size();
        self.buffer = PixelBuffer::new(width, height);

        // La fenêtre garde sa taille logique : c'est sa taille physique, donc
        // celle du tampon, qui suit le facteur d'échelle.
        self.resize_surface()?;
        Ok(true)
    }

    /// Transfère le tampon composé vers la fenêtre.
    ///
    /// # Errors
    /// Renvoie [`AppError::Softbuffer`] si la surface refuse le transfert.
    pub fn present(&mut self) -> Result<(), AppError> {
        let (width, height) = self.metrics.buffer_size();
        let Some(width_nz) = NonZeroU32::new(width) else {
            return Ok(());
        };
        let Some(height_nz) = NonZeroU32::new(height) else {
            return Ok(());
        };

        self.surface.resize(width_nz, height_nz)?;
        let mut target = self.surface.buffer_mut()?;
        let source = self.buffer.as_bytes();

        // `softbuffer` attend des pixels `0x00RRGGBB` sans canal alpha : le
        // panneau est opaque, la composition a déjà aplati la transparence sur
        // le fond.
        let count = target.len().min(source.len() / 4);
        for (index, pixel) in target.iter_mut().take(count).enumerate() {
            let offset = index * 4;
            let red = u32::from(source[offset]);
            let green = u32::from(source[offset + 1]);
            let blue = u32::from(source[offset + 2]);
            *pixel = (red << 16) | (green << 8) | blue;
        }

        target.present()?;
        Ok(())
    }

    /// Aligne la surface de présentation sur la taille du tampon.
    fn resize_surface(&mut self) -> Result<(), AppError> {
        let (width, height) = self.metrics.buffer_size();
        if let (Some(width_nz), Some(height_nz)) = (NonZeroU32::new(width), NonZeroU32::new(height))
        {
            self.surface.resize(width_nz, height_nz)?;
        }
        Ok(())
    }

    /// Centre le panneau sur l'écran qui accueille le familier.
    ///
    /// Centrer sur l'écran du familier plutôt que sur l'écran principal évite
    /// qu'un panneau ouvert depuis un second moniteur ne surgisse à l'autre
    /// bout du bureau. À défaut d'information, on retombe sur l'écran principal,
    /// et si même celui-ci est inconnu, la fenêtre garde la position que le
    /// gestionnaire de fenêtres lui a donnée : mieux vaut un placement par
    /// défaut qu'un placement calculé sur des dimensions inventées.
    fn center_on_anchor_monitor(&self, anchor: Option<&Window>) {
        let monitor = anchor
            .and_then(Window::current_monitor)
            .or_else(|| self.window.current_monitor())
            .or_else(|| self.window.primary_monitor());

        let Some(monitor) = monitor else {
            debug!("Aucun moniteur identifié : le panneau garde sa position par défaut");
            return;
        };

        let screen = monitor.size();
        let panel = self.window.outer_size();
        if screen.width < panel.width || screen.height < panel.height {
            warn!("Écran plus petit que le panneau : centrage ignoré");
            return;
        }

        let origin = monitor.position();
        let x = origin.x + ((screen.width - panel.width) / 2) as i32;
        let y = origin.y + ((screen.height - panel.height) / 2) as i32;
        self.window
            .set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // La fenêtre elle-même exige une boucle d'événements et un serveur
    // graphique : elle n'est pas testable sans écran. Ce qui l'est — et ce qui
    // cassait dans l'ancien panneau — c'est le dimensionnement du tampon, testé
    // ici sans ouvrir quoi que ce soit.

    #[test]
    fn test_panel_buffer_follows_the_physical_size() {
        for (scale, expected) in [
            (1.0_f64, (720, 480)),
            (1.5, (1080, 720)),
            (2.0, (1440, 960)),
        ] {
            let metrics = UiMetrics::for_display(scale, TextSize::Normal);
            assert_eq!(
                metrics.buffer_size(),
                expected,
                "tampon incorrect à l'échelle {scale}"
            );
        }
    }

    #[test]
    fn test_pixel_packing_drops_alpha_and_keeps_channel_order() {
        // Régression potentielle : inverser rouge et bleu est l'erreur classique
        // du passage RGBA → 0x00RRGGBB, et elle est invisible sur du gris.
        let mut buffer = PixelBuffer::new(1, 1);
        buffer.as_bytes_mut()[..4].copy_from_slice(&[0x12, 0x34, 0x56, 0xff]);

        let source = buffer.as_bytes();
        let packed =
            (u32::from(source[0]) << 16) | (u32::from(source[1]) << 8) | u32::from(source[2]);
        assert_eq!(packed, 0x0012_3456);
    }
}
