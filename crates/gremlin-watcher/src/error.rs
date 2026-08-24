//! Types d'erreurs pour la surveillance de fichiers et dépôts Git.

use thiserror::Error;

/// Erreurs de surveillance de fichiers et de découverte de dépôts.
#[derive(Debug, Error)]
pub enum WatcherError {
    /// Erreur liée au moteur `notify`.
    #[error("erreur notify : {0}")]
    Notify(#[from] notify::Error),

    /// Erreur d'entrée/sortie système.
    #[error("erreur I/O : {0}")]
    Io(#[from] std::io::Error),

    /// Canal de communication interrompu ou fermé.
    #[error("échec de transmission sur le canal de signaux")]
    ChannelClosed,

    /// Le canal de contrôle borné est resté saturé.
    #[error("le canal de contrôle de la surveillance est saturé")]
    ChannelFull,

    /// Le worker de surveillance n'a pas confirmé l'opération dans le délai imparti.
    #[error("le worker de surveillance n'a pas répondu dans le délai imparti")]
    Timeout,
}
