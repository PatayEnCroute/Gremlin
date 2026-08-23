//! Gestion des menus et interactions avec la zone de notification (systray).

use crate::error::SystemError;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use std::collections::HashMap;
use tracing::{debug, info};
use tray_icon::{
    Icon as TrayIconData, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

/// Libellé de l'action de sommeil lorsque le familier dort.
const LABEL_WAKE: &str = "⏰ Réveiller Gremlin";

/// Libellé de l'action de sommeil lorsque le familier est éveillé.
const LABEL_SLEEP: &str = "😴 Endormir Gremlin (Pause)";

/// Actions utilisateur déclenchables depuis le menu ou l'icône de la barre des tâches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrayMenuAction {
    /// Ouvre le centre de commandes et de paramètres Raycast.
    OpenRaycastSettings,
    /// Bascule l'état de sommeil / veille du familier.
    ToggleSleep,
    /// Bascule le mode click-through (souris traverse la fenêtre).
    ToggleClickThrough,
    /// Bascule le lancement automatique au démarrage de l'OS.
    ToggleAutostart,
    /// Recharge à chaud tous les skins et accessoires du disque.
    ReloadAssets,
    /// Ouvre le dossier local des données et mods.
    OpenDataFolder,
    /// Quitter proprement l'application.
    Quit,
}

/// Table de correspondance entre identifiants d'éléments de menu et actions.
///
/// Logique purement fonctionnelle, extraite du gestionnaire systray afin
/// d'être testable sans boucle d'événements ni environnement graphique.
#[derive(Debug, Clone, Default)]
pub struct TrayActionMap {
    entries: HashMap<MenuId, TrayMenuAction>,
}

impl TrayActionMap {
    /// Crée une table vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Associe un identifiant d'élément de menu à une action.
    pub fn register(&mut self, id: MenuId, action: TrayMenuAction) {
        self.entries.insert(id, action);
    }

    /// Traduit un identifiant d'élément en action, si elle est connue.
    #[must_use]
    pub fn resolve(&self, id: &MenuId) -> Option<TrayMenuAction> {
        self.entries.get(id).copied()
    }

    /// Nombre d'associations enregistrées.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Indique si aucune association n'est enregistrée.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Traduit un clic direct sur l'icône du systray en action applicative.
///
/// Seul le relâchement du bouton gauche est significatif : le bouton droit est
/// réservé à l'ouverture du menu contextuel par l'OS.
#[must_use]
pub const fn action_for_tray_click(
    button: MouseButton,
    state: MouseButtonState,
) -> Option<TrayMenuAction> {
    match (button, state) {
        (MouseButton::Left, MouseButtonState::Up) => Some(TrayMenuAction::OpenRaycastSettings),
        _ => None,
    }
}

/// Gestionnaire natif de la zone de notification système (Systray / `MenuBar`).
pub struct SystemTrayManager {
    _tray_icon: TrayIcon,
    _menu: Menu,
    _action_open_raycast: MenuItem,
    action_toggle_sleep: MenuItem,
    action_click_through: CheckMenuItem,
    action_autostart: CheckMenuItem,
    _action_reload_assets: MenuItem,
    _action_open_folder: MenuItem,
    _action_quit: MenuItem,
    actions: TrayActionMap,
}

impl SystemTrayManager {
    /// Initialise une nouvelle icône dans la zone de notification avec son menu contextuel.
    ///
    /// # Errors
    /// * [`SystemError::MenuBuildFailed`] si un élément ne peut pas être ajouté
    ///   au menu contextuel (un menu vide serait pire qu'une erreur explicite) ;
    /// * [`SystemError::TrayCreationFailed`] si l'icône système ne peut pas être créée.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        click_through: bool,
        autostart: bool,
        is_sleeping: bool,
    ) -> Result<Self, SystemError> {
        let menu = Menu::new();

        let action_open_raycast = MenuItem::new("🎛️ Paramètres & Garde-robe (Raycast)", true, None);
        let action_toggle_sleep = MenuItem::new(sleep_label(is_sleeping), true, None);
        let action_click_through = CheckMenuItem::new(
            "🖱️ Mode Traversant (Click-Through)",
            true,
            click_through,
            None,
        );
        let action_autostart =
            CheckMenuItem::new("🚀 Lancer au démarrage de l'OS", true, autostart, None);
        let action_reload_assets = MenuItem::new("🔄 Recharger les Mods & Skins", true, None);
        let action_open_folder = MenuItem::new("📁 Ouvrir le dossier des données", true, None);
        let action_quit = MenuItem::new("🚪 Quitter Gremlin", true, None);

        let mut actions = TrayActionMap::new();
        actions.register(
            action_open_raycast.id().clone(),
            TrayMenuAction::OpenRaycastSettings,
        );
        actions.register(
            action_toggle_sleep.id().clone(),
            TrayMenuAction::ToggleSleep,
        );
        actions.register(
            action_click_through.id().clone(),
            TrayMenuAction::ToggleClickThrough,
        );
        actions.register(
            action_autostart.id().clone(),
            TrayMenuAction::ToggleAutostart,
        );
        actions.register(
            action_reload_assets.id().clone(),
            TrayMenuAction::ReloadAssets,
        );
        actions.register(
            action_open_folder.id().clone(),
            TrayMenuAction::OpenDataFolder,
        );
        actions.register(action_quit.id().clone(), TrayMenuAction::Quit);

        menu.append_items(&[
            &action_open_raycast,
            &action_toggle_sleep,
            &PredefinedMenuItem::separator(),
            &action_click_through,
            &action_autostart,
            &PredefinedMenuItem::separator(),
            &action_reload_assets,
            &action_open_folder,
            &PredefinedMenuItem::separator(),
            &action_quit,
        ])
        .map_err(|e| SystemError::MenuBuildFailed(e.to_string()))?;

        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip("Gremlin — Compagnon de bureau pour développeurs");

        if let Some(icon) = Self::load_tray_icon() {
            builder = builder.with_icon(icon);
        }

        let tray_icon = builder
            .build()
            .map_err(|e| SystemError::TrayCreationFailed(e.to_string()))?;

        info!("Zone de notification système (Systray) initialisée avec succès");

        Ok(Self {
            _tray_icon: tray_icon,
            _menu: menu,
            _action_open_raycast: action_open_raycast,
            action_toggle_sleep,
            action_click_through,
            action_autostart,
            _action_reload_assets: action_reload_assets,
            _action_open_folder: action_open_folder,
            _action_quit: action_quit,
            actions,
        })
    }

    /// Charge l'icône RGBA pour la barre d'état.
    fn load_tray_icon() -> Option<TrayIconData> {
        let raw_bytes = crate::window::EMBEDDED_APP_ICON_PNG;
        image::load_from_memory(raw_bytes).ok().and_then(|img| {
            let resized = img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3);
            let rgba = resized.to_rgba8();
            let (w, h) = rgba.dimensions();
            TrayIconData::from_rgba(rgba.into_raw(), w, h).ok()
        })
    }

    /// Met à jour l'état de la coche du mode click-through dans le menu systray.
    pub fn set_click_through_checked(&self, checked: bool) {
        self.action_click_through.set_checked(checked);
    }

    /// Met à jour l'état de la coche de l'autostart dans le menu systray.
    pub fn set_autostart_checked(&self, checked: bool) {
        self.action_autostart.set_checked(checked);
    }

    /// Met à jour le libellé de l'action de sommeil selon l'état actuel.
    pub fn set_sleep_state(&self, is_sleeping: bool) {
        self.action_toggle_sleep.set_text(sleep_label(is_sleeping));
    }

    /// Consomme les événements de menu ou de clic sur l'icône systray sans bloquer.
    #[must_use]
    pub fn poll_events(&self) -> Vec<TrayMenuAction> {
        let mut actions = Vec::new();

        // 1. Événements des éléments du menu contextuel
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = self.actions.resolve(&event.id) {
                debug!(action = ?action, "Action systray reçue via menu contextuel");
                actions.push(action);
            }
        }

        // 2. Événements directs sur l'icône du systray (ex: Clic gauche = Ouvrir Raycast)
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if let Some(action) = action_for_tray_click(button, button_state) {
                    debug!(action = ?action, "Clic sur l'icône systray traduit en action");
                    actions.push(action);
                }
            }
        }

        actions
    }
}

/// Libellé de l'entrée de menu basculant le sommeil du familier.
const fn sleep_label(is_sleeping: bool) -> &'static str {
    if is_sleeping {
        LABEL_WAKE
    } else {
        LABEL_SLEEP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_map_resolves_registered_identifiers() {
        let mut map = TrayActionMap::new();
        assert!(map.is_empty());

        map.register(MenuId::new("1001"), TrayMenuAction::ToggleSleep);
        map.register(MenuId::new("1002"), TrayMenuAction::Quit);

        assert_eq!(map.len(), 2);
        assert_eq!(
            map.resolve(&MenuId::new("1001")),
            Some(TrayMenuAction::ToggleSleep)
        );
        assert_eq!(
            map.resolve(&MenuId::new("1002")),
            Some(TrayMenuAction::Quit)
        );
    }

    #[test]
    fn test_action_map_ignores_unknown_identifiers() {
        let mut map = TrayActionMap::new();
        map.register(MenuId::new("1001"), TrayMenuAction::ToggleSleep);

        // Un événement provenant d'un autre menu de l'application ne doit
        // déclencher aucune action fantôme.
        assert_eq!(map.resolve(&MenuId::new("9999")), None);
        assert_eq!(map.resolve(&MenuId::new("")), None);
    }

    #[test]
    fn test_action_map_last_registration_wins() {
        let mut map = TrayActionMap::new();
        map.register(MenuId::new("1001"), TrayMenuAction::ToggleSleep);
        map.register(MenuId::new("1001"), TrayMenuAction::ReloadAssets);

        assert_eq!(map.len(), 1);
        assert_eq!(
            map.resolve(&MenuId::new("1001")),
            Some(TrayMenuAction::ReloadAssets)
        );
    }

    #[test]
    fn test_tray_click_mapping_only_reacts_to_left_button_release() {
        assert_eq!(
            action_for_tray_click(MouseButton::Left, MouseButtonState::Up),
            Some(TrayMenuAction::OpenRaycastSettings)
        );
        assert_eq!(
            action_for_tray_click(MouseButton::Left, MouseButtonState::Down),
            None
        );
        assert_eq!(
            action_for_tray_click(MouseButton::Right, MouseButtonState::Up),
            None
        );
        assert_eq!(
            action_for_tray_click(MouseButton::Middle, MouseButtonState::Up),
            None
        );
    }

    #[test]
    fn test_sleep_label_switches_with_state() {
        assert_eq!(sleep_label(true), LABEL_WAKE);
        assert_eq!(sleep_label(false), LABEL_SLEEP);
        assert_ne!(sleep_label(true), sleep_label(false));
    }
}
