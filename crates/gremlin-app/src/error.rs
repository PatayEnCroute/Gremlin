//! Types d'erreurs pour l'orchestrateur de l'application Gremlin.

use thiserror::Error;

/// Erreurs au niveau application.
#[derive(Debug, Error)]
pub enum AppError {
    /// Erreur provenant du cœur de jeu.
    #[error("erreur de moteur : {0}")]
    Core(#[from] gremlin_core::CoreError),

    /// Erreur provenant de la surveillance Git.
    #[error("erreur de surveillance : {0}")]
    Watcher(#[from] gremlin_watcher::WatcherError),

    /// Erreur de rendu graphique.
    #[error("erreur de rendu : {0}")]
    Render(#[from] gremlin_render::RenderError),

    /// Erreur de gestion GPU / Pixels.
    #[error("erreur de surface GPU pixels : {0}")]
    Pixels(#[from] pixels::Error),

    /// Erreur de texture GPU / Pixels.
    #[error("erreur de texture pixels : {0}")]
    Texture(#[from] pixels::TextureError),

    /// Erreur de présentation logicielle du panneau de paramètres.
    ///
    /// Le panneau est présenté par `softbuffer` — un transfert mémoire sans
    /// GPU — et non par `pixels` : une seconde surface wgpu coûterait un
    /// contexte graphique entier, contre l'objectif d'empreinte mémoire.
    #[error("erreur de surface logicielle : {0}")]
    Softbuffer(#[from] softbuffer::SoftBufferError),

    /// Échec de création d'une fenêtre.
    #[error("erreur de création de fenêtre : {0}")]
    Window(#[from] winit::error::OsError),

    /// Erreur système / OS.
    #[error("erreur système : {0}")]
    System(#[from] gremlin_system::SystemError),

    /// Erreur de configuration ou sérialisation.
    #[error("erreur de configuration : {0}")]
    Config(#[from] serde_json::Error),

    /// Erreur d'I/O.
    #[error("erreur I/O : {0}")]
    Io(#[from] std::io::Error),
}
