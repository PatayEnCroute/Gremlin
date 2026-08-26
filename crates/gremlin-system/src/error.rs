//! Types d'erreurs pour les intégrations et appels système OS.

use thiserror::Error;

/// Erreurs de gestion des fenêtres et ressources du système hôte.
#[derive(Debug, Error)]
pub enum SystemError {
    /// Impossible de résoudre les répertoires standards de l'OS.
    #[error("impossible de résoudre les répertoires standards de l'utilisateur")]
    PathResolutionFailed,

    /// Échec de création ou configuration de la fenêtre native.
    #[error("erreur de gestion de fenêtre : {0}")]
    WindowError(String),

    /// La plateforme (ou le backend de fenêtrage courant) ne sait pas rendre une
    /// fenêtre traversante par la souris.
    ///
    /// Concerne notamment iOS, Android, Web, Orbital et Wayland selon le backend
    /// `winit` utilisé. On remonte une vraie erreur plutôt qu'un faux succès afin
    /// que l'appelant n'affiche pas la fonctionnalité comme active.
    #[error("le mode click-through n'est pas supporté par cette plateforme")]
    ClickThroughUnsupported,

    /// Échec d'une opération sur le registre Windows (clé `Run`).
    #[error("échec de l'opération registre « {operation} » (code Win32 {code})")]
    Registry {
        /// Nom de l'appel Win32 fautif (`RegOpenKeyExW`, `RegSetValueExW`, …).
        operation: &'static str,
        /// Code d'erreur Win32 renvoyé par l'appel.
        code: u32,
    },

    /// Échec de construction du menu contextuel de la zone de notification.
    #[error("échec de construction du menu systray : {0}")]
    MenuBuildFailed(String),

    /// Échec de création de l'icône de la zone de notification.
    #[error("échec de création de l'icône systray : {0}")]
    TrayCreationFailed(String),

    /// Aucun mécanisme de démarrage automatique n'est connu pour cette plateforme.
    #[error("le démarrage automatique n'est pas supporté par cette plateforme")]
    AutostartUnsupported,

    /// La session graphique courante n'expose pas de compteur d'inactivité.
    #[error("mesure d'inactivité indisponible : {0}")]
    ActivityUnavailable(String),

    /// Le compteur d'inactivité existe, mais sa lecture a échoué.
    #[error("échec de lecture de l'inactivité : {0}")]
    ActivityReadFailed(String),

    /// La date civile locale n'a pas pu être obtenue.
    ///
    /// Remontée telle quelle plutôt que repliée sur UTC : une date fausse
    /// décalerait silencieusement les séries de productivité d'une journée.
    #[error("date locale indisponible : {0}")]
    CalendarUnavailable(String),

    /// La topologie des écrans n'est pas interrogeable sur cette plateforme.
    ///
    /// C'est le cas de Wayland, dont le protocole ne donne à un client ordinaire
    /// ni sa position globale ni celle des autres surfaces. L'interface doit le
    /// dire au lieu de proposer un magnétisme qui ne s'appliquerait pas.
    #[error("topologie des écrans indisponible : {0}")]
    DesktopLayoutUnavailable(String),

    /// La topologie existe, mais sa lecture a échoué.
    #[error("échec de lecture de la topologie des écrans : {0}")]
    DesktopLayoutReadFailed(String),

    /// Erreur d'I/O système.
    #[error("erreur I/O système : {0}")]
    Io(#[from] std::io::Error),
}
