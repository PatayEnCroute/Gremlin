//! Orchestrateur principal de l'application Gremlin.
//!
//! ## Cadencement
//!
//! La boucle n'interroge jamais activement le système : elle programme un
//! réveil via `ControlFlow::WaitUntil` et se rendort. Le délai dépend du
//! contexte — voir les constantes `*_FRAME_INTERVAL` — et la fréquence
//! effective est donc dictée par l'animation en cours, pas par une cadence
//! fixe. Les signaux Git et assets réveillent la boucle immédiatement grâce au
//! pont de réveil ([`CustomAppEvent::Wake`]) : ils ne subissent pas la latence
//! du prochain réveil programmé.
//!
//! ## Horloges
//!
//! Les horloges de [`LoopClocks`] sont volontairement distinctes : `frame`
//! cadence l'animation, `simulation` cadence le moteur métier, `auto_save`
//! cadence la persistance. Les confondre exposait au risque d'appliquer deux
//! fois la même décroissance.

use crate::config::AppConfig;
use crate::desktop;
use crate::error::AppError;
use crate::persistence::PersistenceManager;
use crate::renderer::AppRenderer;
use crate::ui::{
    CommandPalette, PaletteContext, PaletteExecutionResult, RaycastLayout, RaycastRenderer,
    RepoDisplayInfo,
};
use crossbeam_channel::{Receiver, Sender};
use gremlin_core::{CoreEvent, PetMood, PetState};
use gremlin_render::{
    register_default_procedural_accessories, AccessoryCatalog, AccessoryItem, AccessoryManifest,
    AnimationController, LayerCompositor, PixelBuffer, PlayMode, SkinManifest, SpriteAnimation,
    SpriteAtlas,
};
use gremlin_system::{
    AppPaths, AutostartManager, PlatformImpl, PlatformWindowExt, SystemTrayManager, TrayMenuAction,
    WindowConfig,
};
use gremlin_watcher::{AssetSignal, AssetWatcher, DevSignal, RepoWatcher, WatcherStatus};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, Modifiers, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

/// Largeur native du framebuffer interne, en pixels.
pub const NATIVE_WIDTH: u32 = 64;
/// Hauteur native du framebuffer interne, en pixels.
pub const NATIVE_HEIGHT: u32 = 64;

/// Cadence pendant un glisser-déposer (~30 images par seconde).
const DRAG_FRAME_INTERVAL: Duration = Duration::from_millis(33);
/// Cadence lorsque la palette est ouverte (~10 images par seconde).
const PALETTE_FRAME_INTERVAL: Duration = Duration::from_millis(100);
/// Intervalle minimal entre deux réveils, soit un plafond à 60 images par seconde.
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// Intervalle de réveil lorsqu'aucune animation n'attend d'image suivante.
const IDLE_WAKE_INTERVAL: Duration = Duration::from_secs(1);
/// Période de clignotement du curseur de la barre de recherche.
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);
/// Pas minimal entre deux avancées de la simulation métier.
const SIMULATION_TICK_INTERVAL: Duration = Duration::from_secs(1);
/// Capacité des canaux de signaux entrants.
///
/// Bornée volontairement : sous un flot d'événements de système de fichiers,
/// un canal non borné laisserait la mémoire croître sans limite.
const SIGNAL_CHANNEL_CAPACITY: usize = 1024;

/// Événements personnalisés injectés dans la boucle d'événements winit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomAppEvent {
    /// Réveille la boucle : des signaux sont disponibles dans les canaux.
    Wake,
}

/// Animation correspondant à une humeur.
///
/// Source de vérité unique : les correspondances humeur → animation étaient
/// auparavant dupliquées à trois endroits, avec des divergences.
const fn animation_key_for_mood(mood: PetMood) -> &'static str {
    match mood {
        PetMood::Dead => "dead",
        PetMood::Sick => "sick",
        PetMood::Sleeping => "sleep",
        PetMood::Hungry => "hungry",
        PetMood::Coding => "coding",
        PetMood::Angry => "angry",
        PetMood::Happy => "happy",
        PetMood::Tired => "idle",
    }
}

/// Clé d'humeur utilisée pour décaler les accessoires lors de la composition.
///
/// Toutes les humeurs n'ont pas de silhouette dédiée : celles qui n'en ont pas
/// retombent explicitement sur la plus proche.
const fn accessory_mood_key(mood: PetMood) -> &'static str {
    match mood {
        PetMood::Dead => "dead",
        PetMood::Sick => "sick",
        PetMood::Sleeping => "sleep",
        PetMood::Hungry => "hungry",
        PetMood::Happy | PetMood::Coding => "happy",
        PetMood::Tired | PetMood::Angry => "idle",
    }
}

/// Options de construction de l'application.
///
/// Elles fournissent le point d'injection qui manquait pour tester
/// l'orchestrateur : sans elles, instancier `GremlinApp` déclenchait un scan du
/// répertoire personnel réel et créait une icône dans la zone de notification.
pub struct AppOptions {
    /// Active la surveillance des dépôts Git et des assets.
    pub enable_watchers: bool,
    /// Active l'icône de la zone de notification et le gestionnaire d'autostart.
    pub enable_system_integration: bool,
    /// Chemins applicatifs à utiliser ; résolus automatiquement si absents.
    pub paths: Option<AppPaths>,
    /// Proxy de réveil de la boucle d'événements.
    pub wake_proxy: Option<EventLoopProxy<CustomAppEvent>>,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            enable_watchers: true,
            enable_system_integration: true,
            paths: None,
            wake_proxy: None,
        }
    }
}

impl AppOptions {
    /// Configuration sans effet de bord : ni surveillance disque, ni intégration système.
    ///
    /// Utilisée par la suite de tests ; le binaire, lui, passe toujours par
    /// [`AppOptions::default`].
    #[must_use]
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn headless(paths: AppPaths) -> Self {
        Self {
            enable_watchers: false,
            enable_system_integration: false,
            paths: Some(paths),
            wake_proxy: None,
        }
    }
}

/// Ressources graphiques et d'animation.
struct Visuals {
    pixel_buffer: PixelBuffer,
    sprite_atlas: SpriteAtlas,
    accessory_catalog: AccessoryCatalog,
    animation_controller: AnimationController,
    active_manifest: Option<SkinManifest>,
}

/// État transitoire de l'interface.
#[derive(Debug)]
struct UiState {
    is_palette_open: bool,
    suspended_click_through: bool,
    cursor_blink_state: bool,
    is_dragging: bool,
    needs_redraw: bool,
    exit_requested: bool,
    modifiers: ModifiersState,
    last_save_error: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            is_palette_open: false,
            suspended_click_through: false,
            cursor_blink_state: true,
            is_dragging: false,
            needs_redraw: true,
            exit_requested: false,
            modifiers: ModifiersState::empty(),
            last_save_error: None,
        }
    }
}

/// Horloges indépendantes de la boucle principale.
#[derive(Debug)]
struct LoopClocks {
    /// Dernière image composée, base du delta d'animation.
    frame: Instant,
    /// Dernier pas de simulation métier appliqué.
    simulation: Instant,
    /// Dernière sauvegarde automatique.
    auto_save: Instant,
    /// Dernière bascule du curseur clignotant.
    cursor_blink: Instant,
}

impl LoopClocks {
    const fn new(now: Instant) -> Self {
        Self {
            frame: now,
            simulation: now,
            auto_save: now,
            cursor_blink: now,
        }
    }
}

/// Surveillants d'arrière-plan et canaux entrants.
struct WatcherBridge {
    repo_watcher: Option<RepoWatcher>,
    asset_watcher: Option<AssetWatcher>,
    dev_receiver: Receiver<DevSignal>,
    asset_receiver: Receiver<AssetSignal>,
    status_receiver: Receiver<WatcherStatus>,
    /// Conservés pour permettre l'injection de signaux depuis un test.
    #[cfg_attr(not(test), allow(dead_code))]
    dev_sender: Sender<DevSignal>,
    #[cfg_attr(not(test), allow(dead_code))]
    asset_sender: Sender<AssetSignal>,
}

impl WatcherBridge {
    /// Arrête les surveillants et libère les émetteurs qu'ils détiennent.
    fn shutdown(&mut self) {
        self.repo_watcher = None;
        self.asset_watcher = None;
    }
}

/// État global de l'application orchestrant la logique, les graphismes, l'UI et l'OS.
pub struct GremlinApp {
    config: AppConfig,
    paths: AppPaths,
    pet_state: PetState,
    visuals: Visuals,
    ui: UiState,
    clocks: LoopClocks,
    watchers: WatcherBridge,
    wake_bridge: Option<JoinHandle<()>>,
    window: Option<Arc<Window>>,
    renderer: Option<AppRenderer>,
    platform: PlatformImpl,
    tray_manager: Option<SystemTrayManager>,
    autostart_manager: Option<AutostartManager>,
    monitored_repos: Vec<RepoDisplayInfo>,
    command_palette: CommandPalette,
}

impl GremlinApp {
    /// Crée une nouvelle instance de l'application avec la configuration par défaut.
    ///
    /// # Errors
    /// Renvoie `AppError` si l'initialisation des chemins standards échoue.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(config: AppConfig) -> Result<Self, AppError> {
        Self::with_state_and_config(PetState::new("Gremlin"), config)
    }

    /// Crée une instance avec un état de jeu et une configuration pré-chargés.
    ///
    /// # Errors
    /// Renvoie `AppError` si l'initialisation des chemins ou du système échoue.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_state_and_config(pet_state: PetState, config: AppConfig) -> Result<Self, AppError> {
        Self::with_options(pet_state, config, AppOptions::default())
    }

    /// Crée une instance en contrôlant finement les effets de bord système.
    ///
    /// # Errors
    /// Renvoie `AppError` si l'initialisation des chemins échoue.
    pub fn with_options(
        pet_state: PetState,
        mut config: AppConfig,
        options: AppOptions,
    ) -> Result<Self, AppError> {
        if config.normalize() {
            warn!("Configuration hors bornes détectée au démarrage : valeurs corrigées");
        }

        let paths = match options.paths {
            Some(paths) => paths,
            None => AppPaths::new()?,
        };
        if let Err(e) = paths.ensure_directories_exist() {
            warn!("Impossible de préparer les répertoires applicatifs : {e}");
        }

        let (dev_sender, dev_receiver) = crossbeam_channel::bounded(SIGNAL_CHANNEL_CAPACITY);
        let (asset_sender, asset_receiver) = crossbeam_channel::bounded(SIGNAL_CHANNEL_CAPACITY);

        // Le pont de réveil relaie les signaux vers les canaux consommés par la
        // boucle, puis réveille winit. Sans proxy (tests, mode headless), les
        // surveillants écrivent directement dans les canaux.
        let (watcher_dev_sender, watcher_asset_sender, wake_bridge) =
            options.wake_proxy.map_or_else(
                || (dev_sender.clone(), asset_sender.clone(), None),
                |proxy| {
                    let (raw_dev_tx, raw_dev_rx) =
                        crossbeam_channel::bounded(SIGNAL_CHANNEL_CAPACITY);
                    let (raw_asset_tx, raw_asset_rx) =
                        crossbeam_channel::bounded(SIGNAL_CHANNEL_CAPACITY);
                    let handle = spawn_wake_bridge(
                        proxy,
                        raw_dev_rx,
                        dev_sender.clone(),
                        raw_asset_rx,
                        asset_sender.clone(),
                    );
                    (raw_dev_tx, raw_asset_tx, handle)
                },
            );

        // Canal de fiabilité : signale les enregistrements de surveillance
        // ratés et les pertes d'événements, jusqu'ici invisibles hors journaux.
        let (status_sender, status_receiver) = crossbeam_channel::bounded(SIGNAL_CHANNEL_CAPACITY);

        let (repo_watcher, asset_watcher) = if options.enable_watchers {
            Self::spawn_watchers(
                &config,
                &paths,
                watcher_dev_sender,
                watcher_asset_sender,
                status_sender,
            )
        } else {
            (None, None)
        };

        let mut sprite_atlas = SpriteAtlas::new();
        let mut accessory_catalog = AccessoryCatalog::new();
        register_default_procedural_accessories(&mut sprite_atlas, &mut accessory_catalog);

        let (autostart_manager, tray_manager) = if options.enable_system_integration {
            Self::spawn_system_integration(&config, &pet_state)
        } else {
            (None, None)
        };

        let autostart_active = autostart_manager
            .as_ref()
            .is_some_and(AutostartManager::is_enabled);

        let monitored_repos = Vec::new();
        let command_palette = CommandPalette::new(&PaletteContext {
            catalog: &accessory_catalog,
            wardrobe: &config.wardrobe,
            pet_state: &pet_state,
            config: &config,
            autostart_active,
            repos: &monitored_repos,
            last_save_error: None,
        });

        let now = Instant::now();
        let mut app = Self {
            config,
            paths,
            pet_state,
            visuals: Visuals {
                pixel_buffer: PixelBuffer::new(NATIVE_WIDTH, NATIVE_HEIGHT),
                sprite_atlas,
                accessory_catalog,
                animation_controller: AnimationController::new(),
                active_manifest: None,
            },
            ui: UiState::default(),
            clocks: LoopClocks::new(now),
            watchers: WatcherBridge {
                repo_watcher,
                asset_watcher,
                dev_receiver,
                asset_receiver,
                status_receiver,
                dev_sender,
                asset_sender,
            },
            wake_bridge,
            window: None,
            renderer: None,
            platform: PlatformImpl,
            tray_manager,
            autostart_manager,
            monitored_repos,
            command_palette,
        };

        let active_skin = app.config.active_skin.clone();
        app.load_skin(&active_skin);
        app.scan_custom_accessories_and_skins();
        app.sync_animation_with_mood();

        Ok(app)
    }

    /// Démarre les surveillants Git et d'assets, en tolérant leur indisponibilité.
    fn spawn_watchers(
        config: &AppConfig,
        paths: &AppPaths,
        dev_sender: Sender<DevSignal>,
        asset_sender: Sender<AssetSignal>,
        status_sender: Sender<WatcherStatus>,
    ) -> (Option<RepoWatcher>, Option<AssetWatcher>) {
        let mut repo_watcher = match RepoWatcher::new_with_config(dev_sender, &config.watcher) {
            Ok(w) => Some(w),
            Err(e) => {
                warn!("Échec d'initialisation du watcher Git : {e}");
                None
            }
        };

        if let Some(ref mut watcher) = repo_watcher {
            if let Err(e) = watcher.set_status_sender(status_sender) {
                warn!("Rapports de fiabilité de la surveillance indisponibles : {e}");
            }

            // La découverte consomme directement `auto_discovery`, `custom_roots`
            // et `max_scan_depth` de la configuration : l'orchestrateur n'a plus
            // à réimplémenter cette logique.
            if let Err(e) = watcher.start_auto_discovery() {
                warn!("Découverte automatique des dépôts indisponible : {e}");
            }
        }

        let mut asset_watcher = match AssetWatcher::new_with_config(asset_sender, &config.watcher) {
            Ok(w) => Some(w),
            Err(e) => {
                warn!("Échec d'initialisation du watcher d'assets : {e}");
                None
            }
        };

        if let Some(ref mut watcher) = asset_watcher {
            for dir in [paths.skins_dir(), paths.config_dir().join("accessories")] {
                if let Err(e) = watcher.watch_directory(dir) {
                    warn!("Échec de surveillance d'un répertoire d'assets : {e}");
                }
            }
        }

        (repo_watcher, asset_watcher)
    }

    /// Initialise l'autostart et l'icône de la zone de notification.
    fn spawn_system_integration(
        config: &AppConfig,
        pet_state: &PetState,
    ) -> (Option<AutostartManager>, Option<SystemTrayManager>) {
        let autostart_manager = match AutostartManager::new("Gremlin") {
            Ok(m) => Some(m),
            Err(e) => {
                warn!("Échec d'initialisation de l'AutostartManager : {e}");
                None
            }
        };

        let autostart_active = autostart_manager
            .as_ref()
            .is_some_and(AutostartManager::is_enabled);

        let tray_manager = match SystemTrayManager::new(
            config.click_through_enabled,
            autostart_active,
            pet_state.is_sleeping(),
        ) {
            Ok(t) => Some(t),
            Err(e) => {
                warn!("Échec d'initialisation du Systray : {e}");
                None
            }
        };

        (autostart_manager, tray_manager)
    }

    /// Charge un pack de skin depuis le disque ou bascule sur les graphismes de secours.
    pub fn load_skin(&mut self, skin_name: &str) {
        let skin_paths = [
            self.paths.skins_dir().join(skin_name),
            PathBuf::from("assets/skins").join(skin_name),
            PathBuf::from("../../assets/skins").join(skin_name),
        ];

        for skin_dir in &skin_paths {
            if skin_dir.exists() && self.try_load_skin_from_dir(skin_dir) {
                info!(skin = %skin_name, path = %skin_dir.display(), "Skin chargé depuis le disque");
                return;
            }
        }

        info!("Utilisation des graphismes procéduraux par défaut");
        self.load_procedural_fallback_skin();
    }

    fn try_load_skin_from_dir(&mut self, skin_dir: &Path) -> bool {
        let manifest_path = skin_dir.join("manifest.json");
        let Ok(manifest_content) = fs::read_to_string(&manifest_path) else {
            return false;
        };

        let manifest = match SkinManifest::from_json(&manifest_content) {
            Ok(manifest) => manifest,
            Err(e) => {
                warn!(path = %manifest_path.display(), "Manifest de skin invalide : {e}");
                return false;
            }
        };

        let mut loaded_any = false;
        if let Ok(entries) = fs::read_dir(skin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("png") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    match self.visuals.sprite_atlas.load_from_png_file(stem, &path) {
                        Ok(()) => loaded_any = true,
                        Err(e) => {
                            warn!(path = %path.display(), "Sprite ignoré : {e}");
                        }
                    }
                }
            }
        }

        if loaded_any {
            self.visuals.animation_controller = manifest.build_animation_controller();
            self.visuals.active_manifest = Some(manifest);
            true
        } else {
            false
        }
    }

    fn load_procedural_fallback_skin(&mut self) {
        self.visuals.sprite_atlas.load_default_procedural_sprites();

        // Le jeu de sprites procéduraux ne fournit pas d'images dédiées pour
        // « coding » et « angry » : on réutilise les silhouettes les plus
        // proches avec un rythme distinct, plutôt que de laisser `play()`
        // échouer silencieusement sur une animation inexistante.
        let animations: [(&str, &[&str], u64, PlayMode); 9] = [
            ("idle", &["idle_0", "idle_1"], 300, PlayMode::Loop),
            ("happy", &["happy_0", "happy_1"], 180, PlayMode::Loop),
            ("coding", &["happy_0", "happy_1"], 140, PlayMode::Loop),
            ("hungry", &["hungry_0", "hungry_1"], 400, PlayMode::Loop),
            ("sleep", &["sleep_0", "sleep_1"], 600, PlayMode::Loop),
            ("sick", &["sick_0", "sick_1"], 500, PlayMode::Loop),
            ("angry", &["sick_0", "sick_1"], 220, PlayMode::Loop),
            ("dead", &["dead"], 1000, PlayMode::Once),
            ("dragged", &["dragged_0", "dragged_1"], 120, PlayMode::Loop),
        ];

        let mut controller = AnimationController::new();
        for (name, frames, millis, mode) in animations {
            controller.register(SpriteAnimation::uniform(
                name,
                frames,
                Duration::from_millis(millis),
                mode,
            ));
        }

        self.visuals.animation_controller = controller;
        self.visuals.active_manifest = None;
    }

    /// Scanne les répertoires utilisateur pour charger les accessoires et skins mods.
    pub fn scan_custom_accessories_and_skins(&mut self) {
        self.visuals.accessory_catalog.clear_mods();

        let acc_dir = self.paths.config_dir().join("accessories");
        if let Ok(entries) = fs::read_dir(&acc_dir) {
            for entry in entries.flatten() {
                let sub_path = entry.path();
                if sub_path.is_dir() {
                    self.load_accessory_mod(&sub_path);
                }
            }
        }

        self.rebuild_palette_items();
        self.ui.needs_redraw = true;
    }

    /// Charge un accessoire modé depuis son répertoire.
    fn load_accessory_mod(&mut self, dir: &Path) {
        let Ok(content) = fs::read_to_string(dir.join("manifest.json")) else {
            return;
        };
        let manifest = match AccessoryManifest::from_json(&content) {
            Ok(manifest) => manifest,
            Err(e) => {
                warn!(path = %dir.display(), "Manifest d'accessoire invalide : {e}");
                return;
            }
        };

        if let Ok(png_entries) = fs::read_dir(dir) {
            for png_entry in png_entries.flatten() {
                let p = png_entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("png") {
                    continue;
                }
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    if let Err(e) = self.visuals.sprite_atlas.load_from_png_file(stem, &p) {
                        warn!(path = %p.display(), "Sprite d'accessoire ignoré : {e}");
                    }
                }
            }
        }

        info!(id = %manifest.id, "Accessoire mod chargé avec succès");
        self.visuals
            .accessory_catalog
            .register(AccessoryItem::from_mod(manifest, dir));
    }

    /// Reconstruit la liste des éléments de la palette de commande.
    pub fn rebuild_palette_items(&mut self) {
        let autostart_active = self
            .autostart_manager
            .as_ref()
            .is_some_and(AutostartManager::is_enabled);

        self.command_palette.rebuild_items(&PaletteContext {
            catalog: &self.visuals.accessory_catalog,
            wardrobe: &self.config.wardrobe,
            pet_state: &self.pet_state,
            config: &self.config,
            autostart_active,
            repos: &self.monitored_repos,
            last_save_error: self.ui.last_save_error.as_deref(),
        });
    }

    /// Aligne l'animation courante sur l'état émotionnel ou physique du familier.
    pub fn sync_animation_with_mood(&mut self) {
        let target = if self.ui.is_dragging {
            "dragged"
        } else {
            animation_key_for_mood(self.pet_state.mood())
        };

        self.visuals.animation_controller.play(target, false);
    }

    /// Bascule l'affichage de la fenêtre de paramètres façon Raycast.
    pub fn toggle_palette(&mut self) {
        self.ui.is_palette_open = !self.ui.is_palette_open;

        if self.ui.is_palette_open {
            // Suspension du click-through pour permettre la saisie.
            if self.config.click_through_enabled {
                self.ui.suspended_click_through = true;
                self.apply_click_through(false);
            }
            self.resize_to(RaycastLayout::WIDTH, RaycastLayout::HEIGHT, true);
            self.rebuild_palette_items();
        } else {
            if self.ui.suspended_click_through {
                self.ui.suspended_click_through = false;
                self.apply_click_through(true);
            }
            self.resize_to(NATIVE_WIDTH, NATIVE_HEIGHT, false);
        }

        self.request_redraw();
    }

    /// Redimensionne le tampon logiciel, la surface GPU et la fenêtre.
    fn resize_to(&mut self, buffer_w: u32, buffer_h: u32, native_surface: bool) {
        self.visuals.pixel_buffer = PixelBuffer::new(buffer_w, buffer_h);

        let (surface_w, surface_h) = if native_surface {
            (buffer_w, buffer_h)
        } else {
            (
                buffer_w.saturating_mul(self.config.scale_factor),
                buffer_h.saturating_mul(self.config.scale_factor),
            )
        };

        if let Some(renderer) = &mut self.renderer {
            if let Err(e) = renderer.resize_buffer(buffer_w, buffer_h) {
                warn!("Échec de redimensionnement du tampon : {e}");
            }
            if let Err(e) = renderer.resize_surface(surface_w, surface_h) {
                warn!("Échec de redimensionnement de la surface : {e}");
            }
        }

        if let Some(window) = &self.window {
            let _ = window.request_inner_size(LogicalSize::new(surface_w, surface_h));
        }
    }

    /// Applique l'état de click-through à la fenêtre, en remontant les échecs.
    fn apply_click_through(&self, enabled: bool) {
        let Some(window) = &self.window else {
            return;
        };
        if let Err(e) = self.platform.set_click_through(window, enabled) {
            warn!("Le mode click-through n'a pas pu être appliqué : {e}");
        }
    }

    /// Marque la frame comme à redessiner et réveille la fenêtre.
    fn request_redraw(&mut self) {
        self.ui.needs_redraw = true;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Sauvegarde l'état et mémorise l'échec éventuel pour l'afficher.
    ///
    /// Les échecs de sauvegarde étaient auparavant ignorés : l'utilisateur
    /// pouvait déclencher « Sauvegarder » et croire l'opération réussie.
    fn persist_state(&mut self, reason: &str) {
        match PersistenceManager::save(&self.paths, &self.pet_state, &self.config) {
            Ok(()) => {
                if self.ui.last_save_error.take().is_some() {
                    self.rebuild_palette_items();
                    self.ui.needs_redraw = true;
                }
            }
            Err(e) => {
                error!(reason, "Échec de la sauvegarde : {e}");
                self.ui.last_save_error = Some(e.to_string());
                self.rebuild_palette_items();
                self.ui.needs_redraw = true;
            }
        }
    }

    /// Traite les événements clavier dans la palette de commande.
    pub fn handle_palette_key(&mut self, key_event: &KeyEvent) {
        if key_event.state != ElementState::Pressed {
            return;
        }

        // Ctrl+S : raccourci annoncé dans le pied de page.
        if self.ui.modifiers.control_key() {
            if let Key::Character(text) = &key_event.logical_key {
                if text.eq_ignore_ascii_case("s") {
                    self.persist_state("raccourci Ctrl+S");
                    self.ui.needs_redraw = true;
                    return;
                }
            }
        }

        match &key_event.logical_key {
            Key::Named(NamedKey::ArrowDown) => {
                self.command_palette.select_next();
                self.ui.needs_redraw = true;
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.command_palette.select_prev();
                self.ui.needs_redraw = true;
            }
            Key::Named(NamedKey::Escape) => {
                self.toggle_palette();
            }
            Key::Named(NamedKey::Backspace) => {
                self.command_palette.pop_char();
                self.ui.needs_redraw = true;
            }
            Key::Named(NamedKey::Enter) => {
                let res = self.command_palette.execute_selected(&self.config.wardrobe);
                self.handle_execution_result(res);
                self.rebuild_palette_items();
                self.ui.needs_redraw = true;
            }
            Key::Character(text) => {
                for ch in text.chars().filter(|c| !c.is_control()) {
                    self.command_palette.push_char(ch);
                    self.ui.needs_redraw = true;
                }
            }
            _ => {}
        }
    }

    /// Exécute le résultat d'une commande sélectionnée dans la palette ou le systray.
    pub fn handle_execution_result(&mut self, result: PaletteExecutionResult) {
        match result {
            PaletteExecutionResult::EquipAccessory { category, id } => {
                info!(category = ?category, id = %id, "Accessoire équipé");
                self.config.wardrobe.equip(category, id);
            }
            PaletteExecutionResult::UnequipAccessory { category } => {
                info!(category = ?category, "Accessoire retiré");
                self.config.wardrobe.unequip(category);
            }
            PaletteExecutionResult::FeedPet => self.apply_care("nourrir", PetState::feed),
            PaletteExecutionResult::PetGremlin => self.apply_care("caresser", PetState::pet),
            PaletteExecutionResult::HealPet => self.apply_care("soigner", PetState::heal),
            PaletteExecutionResult::RevivePet => {
                match self.pet_state.revive() {
                    Ok(_) => info!("Gremlin a été réanimé"),
                    Err(e) => warn!("Réanimation impossible : {e}"),
                }
                self.sync_animation_with_mood();
            }
            PaletteExecutionResult::ToggleClickThrough => {
                self.config.click_through_enabled = !self.config.click_through_enabled;
                if let Some(tray) = &self.tray_manager {
                    tray.set_click_through_checked(self.config.click_through_enabled);
                }
                if !self.ui.is_palette_open {
                    self.apply_click_through(self.config.click_through_enabled);
                }
            }
            PaletteExecutionResult::ToggleAutostart => self.toggle_autostart(),
            PaletteExecutionResult::SetScaleFactor(factor) => {
                self.config.scale_factor =
                    factor.clamp(AppConfig::MIN_SCALE_FACTOR, AppConfig::MAX_SCALE_FACTOR);
                if !self.ui.is_palette_open {
                    self.resize_to(NATIVE_WIDTH, NATIVE_HEIGHT, false);
                }
            }
            PaletteExecutionResult::ToggleSleep => {
                if let Err(e) = self.pet_state.toggle_sleep() {
                    warn!("Bascule du sommeil impossible : {e}");
                }
                if let Some(tray) = &self.tray_manager {
                    tray.set_sleep_state(self.pet_state.is_sleeping());
                }
                self.sync_animation_with_mood();
            }
            PaletteExecutionResult::ReloadAssets => {
                info!("Rechargement complet des assets demandé");
                let active_skin = self.config.active_skin.clone();
                self.load_skin(&active_skin);
                self.scan_custom_accessories_and_skins();
            }
            PaletteExecutionResult::OpenModsFolder => {
                Self::open_folder(&self.paths.skins_dir());
            }
            PaletteExecutionResult::OpenDataFolder => {
                let save_file = self.paths.save_file();
                let target = save_file.parent().unwrap_or(&save_file).to_path_buf();
                Self::open_folder(&target);
            }
            PaletteExecutionResult::SaveNow => self.persist_state("action utilisateur"),
            PaletteExecutionResult::None => {}
        }
    }

    /// Applique une action de soin et journalise son éventuel refus.
    fn apply_care(
        &mut self,
        label: &str,
        action: fn(&mut PetState, Option<f32>) -> Result<Vec<CoreEvent>, gremlin_core::CoreError>,
    ) {
        match action(&mut self.pet_state, None) {
            Ok(_) => info!("Action « {label} » appliquée au familier"),
            Err(e) => warn!("Action « {label} » refusée : {e}"),
        }
        self.sync_animation_with_mood();
    }

    /// Bascule le lancement automatique au démarrage de session.
    fn toggle_autostart(&mut self) {
        let Some(autostart) = &self.autostart_manager else {
            warn!("Autostart indisponible sur cette plateforme");
            return;
        };

        let target = !autostart.is_enabled();
        let outcome = if target {
            autostart.enable()
        } else {
            autostart.disable()
        };

        match outcome {
            Ok(()) => {
                self.config.autostart_enabled = target;
                if let Some(tray) = &self.tray_manager {
                    tray.set_autostart_checked(target);
                }
                info!(target, "Statut de l'autostart basculé");
            }
            Err(e) => warn!("Échec de modification de l'autostart : {e}"),
        }
    }

    /// Ouvre un répertoire dans le gestionnaire de fichiers du système.
    fn open_folder(path: &Path) {
        if let Err(e) = desktop::open_directory(path) {
            warn!(path = %path.display(), "Impossible d'ouvrir le répertoire : {e}");
        }
    }

    /// Draine les signaux entrants (systray, assets, Git) et applique leurs effets.
    ///
    /// Ne fait **pas** avancer la simulation : celle-ci est cadencée séparément
    /// par [`GremlinApp::advance_simulation`].
    pub fn pump_events(&mut self) -> Vec<CoreEvent> {
        self.drain_tray_actions();
        self.drain_watcher_status();
        self.drain_asset_signals();
        self.drain_dev_signals()
    }

    /// Remonte les incidents de fiabilité de la surveillance Git.
    ///
    /// Un enregistrement raté ou une perte d'événements signifie que des
    /// commits ne seront pas comptabilisés : le silence n'est pas acceptable.
    fn drain_watcher_status(&self) {
        while let Ok(status) = self.watchers.status_receiver.try_recv() {
            match status {
                WatcherStatus::WatchFailed { path, reason } => {
                    warn!(
                        path = %path.display(),
                        "Surveillance non enregistrée, les commits de ce chemin seront ignorés : {reason}"
                    );
                }
                WatcherStatus::EventsLost { dropped, reason } => {
                    warn!(
                        dropped,
                        "Événements de système de fichiers perdus, resynchronisation : {reason}"
                    );
                }
            }
        }
    }

    fn drain_tray_actions(&mut self) {
        let Some(tray) = &self.tray_manager else {
            return;
        };

        let actions: Vec<TrayMenuAction> = tray.poll_events();
        for action in actions {
            match action {
                TrayMenuAction::OpenRaycastSettings => {
                    if !self.ui.is_palette_open {
                        self.toggle_palette();
                    }
                }
                TrayMenuAction::ToggleSleep => {
                    self.handle_execution_result(PaletteExecutionResult::ToggleSleep);
                }
                TrayMenuAction::ToggleClickThrough => {
                    self.handle_execution_result(PaletteExecutionResult::ToggleClickThrough);
                }
                TrayMenuAction::ToggleAutostart => {
                    self.handle_execution_result(PaletteExecutionResult::ToggleAutostart);
                }
                TrayMenuAction::ReloadAssets => {
                    self.handle_execution_result(PaletteExecutionResult::ReloadAssets);
                }
                TrayMenuAction::OpenDataFolder => {
                    self.handle_execution_result(PaletteExecutionResult::OpenDataFolder);
                }
                TrayMenuAction::Quit => {
                    // La sortie passe par la boucle d'événements : un
                    // `process::exit` ici court-circuiterait les destructeurs
                    // des threads de surveillance et de la surface GPU.
                    info!("Fermeture demandée depuis la zone de notification");
                    self.ui.exit_requested = true;
                }
            }
        }
    }

    fn drain_asset_signals(&mut self) {
        while let Ok(asset_signal) = self.watchers.asset_receiver.try_recv() {
            match asset_signal {
                AssetSignal::SkinChanged { skin_name, .. } => {
                    info!(skin = %skin_name, "Hot-reload : modification du skin détectée");
                    if self.config.active_skin == skin_name {
                        self.load_skin(&skin_name);
                        self.ui.needs_redraw = true;
                    }
                }
                AssetSignal::AccessoryChanged { accessory_id, .. } => {
                    info!(id = %accessory_id, "Hot-reload : modification d'accessoire détectée");
                    self.scan_custom_accessories_and_skins();
                }
                AssetSignal::AssetsReloadRequested => {
                    info!("Hot-reload : rechargement général des assets");
                    let active_skin = self.config.active_skin.clone();
                    self.load_skin(&active_skin);
                    self.scan_custom_accessories_and_skins();
                }
            }
        }
    }

    fn drain_dev_signals(&mut self) -> Vec<CoreEvent> {
        let mut core_events = Vec::new();

        while let Ok(signal) = self.watchers.dev_receiver.try_recv() {
            match signal {
                DevSignal::CommitCreated {
                    repo_name,
                    branch,
                    commit_sha,
                    message,
                    ..
                } => {
                    info!(
                        repo = %repo_name,
                        branch = %branch,
                        sha = ?commit_sha,
                        msg = ?message,
                        "Commit Git assimilé"
                    );
                    match self.pet_state.handle_commit(&repo_name, &branch) {
                        Ok(events) => core_events.extend(events),
                        Err(e) => warn!("Commit ignoré : {e}"),
                    }

                    if let Some(r) = self
                        .monitored_repos
                        .iter_mut()
                        .find(|r| r.name == repo_name)
                    {
                        r.branch = Some(branch);
                        r.last_commit_msg = message;
                    }

                    self.sync_animation_with_mood();
                    self.ui.needs_redraw = true;
                }
                DevSignal::BranchChanged {
                    repo_name,
                    old_branch,
                    new_branch,
                    ..
                } => {
                    info!(repo = %repo_name, from = %old_branch, to = %new_branch, "Bascule de branche");
                    if let Err(e) = self.pet_state.pet(Some(2.0)) {
                        warn!("Récompense de bascule de branche ignorée : {e}");
                    }

                    if let Some(r) = self
                        .monitored_repos
                        .iter_mut()
                        .find(|r| r.name == repo_name)
                    {
                        r.branch = Some(new_branch);
                    }

                    self.sync_animation_with_mood();
                    self.ui.needs_redraw = true;
                }
                DevSignal::RepoDiscovered { repo_name, .. } => {
                    if !self.monitored_repos.iter().any(|r| r.name == repo_name) {
                        // La branche reste inconnue jusqu'au premier signal qui
                        // la renseigne : aucune valeur n'est inventée ici.
                        self.monitored_repos.push(RepoDisplayInfo {
                            name: repo_name,
                            branch: None,
                            last_commit_msg: None,
                        });
                        self.rebuild_palette_items();
                        self.ui.needs_redraw = true;
                    }
                }
                DevSignal::RepoRemoved { repo_name, .. } => {
                    self.monitored_repos.retain(|r| r.name != repo_name);
                    self.rebuild_palette_items();
                    self.ui.needs_redraw = true;
                }
            }
        }

        core_events
    }

    /// Fait avancer la simulation métier si le pas minimal est écoulé.
    fn advance_simulation(&mut self, now: Instant) -> Vec<CoreEvent> {
        let elapsed = now.duration_since(self.clocks.simulation);
        if elapsed < SIMULATION_TICK_INTERVAL {
            return Vec::new();
        }

        self.clocks.simulation = now;
        let previous_mood = self.pet_state.mood();
        let events = self.pet_state.tick(elapsed);

        if self.pet_state.mood() != previous_mood {
            self.sync_animation_with_mood();
            self.ui.needs_redraw = true;
        }

        events
    }

    /// Recompose le framebuffer logiciel avec la frame active ou l'UI Raycast.
    fn compose_frame(&mut self) {
        let mood_key = accessory_mood_key(self.pet_state.mood());

        if self.ui.is_palette_open {
            let base_key = self
                .visuals
                .animation_controller
                .current_frame_key()
                .unwrap_or("idle_0");

            RaycastRenderer::render_ui(
                &mut self.visuals.pixel_buffer,
                &self.command_palette,
                &self.config.wardrobe,
                &self.visuals.sprite_atlas,
                self.visuals.active_manifest.as_ref(),
                &self.visuals.accessory_catalog,
                base_key,
                mood_key,
                self.ui.cursor_blink_state,
            );
            return;
        }

        self.visuals.pixel_buffer.clear(0, 0, 0, 0);
        if let Some(frame_key) = self.visuals.animation_controller.current_frame_key() {
            LayerCompositor::compose_layered_pet(
                &mut self.visuals.pixel_buffer,
                &self.config.wardrobe,
                &self.visuals.sprite_atlas,
                self.visuals.active_manifest.as_ref(),
                &self.visuals.accessory_catalog,
                frame_key,
                mood_key,
            );
        }
    }

    /// Émetteur de signaux de développement, pour injection depuis un test ou un pont.
    #[must_use]
    pub fn dev_sender(&self) -> Sender<DevSignal> {
        self.watchers.dev_sender.clone()
    }

    /// Émetteur de signaux d'assets, pour injection depuis un test ou un pont.
    #[must_use]
    pub fn asset_sender(&self) -> Sender<AssetSignal> {
        self.watchers.asset_sender.clone()
    }

    /// Chemins applicatifs résolus.
    #[must_use]
    pub const fn paths(&self) -> &AppPaths {
        &self.paths
    }

    /// Configuration applicative courante.
    #[must_use]
    pub const fn config(&self) -> &AppConfig {
        &self.config
    }

    /// État courant du familier.
    #[must_use]
    pub const fn pet_state(&self) -> &PetState {
        &self.pet_state
    }

    /// Catalogue d'accessoires chargé.
    #[must_use]
    pub const fn accessory_catalog(&self) -> &AccessoryCatalog {
        &self.visuals.accessory_catalog
    }

    /// Indique si la palette est actuellement ouverte.
    #[must_use]
    pub const fn is_palette_open(&self) -> bool {
        self.ui.is_palette_open
    }

    /// Dernière erreur de sauvegarde rencontrée, le cas échéant.
    #[must_use]
    pub fn last_save_error(&self) -> Option<&str> {
        self.ui.last_save_error.as_deref()
    }

    /// Dépôts Git actuellement suivis pour l'affichage.
    #[must_use]
    pub fn monitored_repos(&self) -> &[RepoDisplayInfo] {
        &self.monitored_repos
    }

    /// Délai avant le prochain réveil de la boucle.
    fn next_wake_delay(&self) -> Duration {
        if self.ui.is_dragging {
            return DRAG_FRAME_INTERVAL;
        }
        if self.ui.is_palette_open {
            return PALETTE_FRAME_INTERVAL;
        }
        self.visuals
            .animation_controller
            .time_until_next_frame()
            .map_or(IDLE_WAKE_INTERVAL, |wait| wait.max(MIN_FRAME_INTERVAL))
    }
}

impl Drop for GremlinApp {
    fn drop(&mut self) {
        // Les surveillants détiennent les émetteurs du pont de réveil : les
        // libérer d'abord permet au thread de pont de sortir de sa boucle.
        self.watchers.shutdown();
        if let Some(handle) = self.wake_bridge.take() {
            let _ = handle.join();
        }
    }
}

/// Démarre le pont qui relaie les signaux vers la boucle et la réveille.
fn spawn_wake_bridge(
    proxy: EventLoopProxy<CustomAppEvent>,
    raw_dev: Receiver<DevSignal>,
    dev_out: Sender<DevSignal>,
    raw_asset: Receiver<AssetSignal>,
    asset_out: Sender<AssetSignal>,
) -> Option<JoinHandle<()>> {
    let spawned = std::thread::Builder::new()
        .name(String::from("gremlin-wake-bridge"))
        .spawn(move || {
            let mut dev_open = true;
            let mut asset_open = true;

            while dev_open || asset_open {
                crossbeam_channel::select! {
                    recv(raw_dev) -> msg => match msg {
                        Ok(signal) => {
                            if dev_out.send(signal).is_err() {
                                break;
                            }
                        }
                        Err(_) => dev_open = false,
                    },
                    recv(raw_asset) -> msg => match msg {
                        Ok(signal) => {
                            if asset_out.send(signal).is_err() {
                                break;
                            }
                        }
                        Err(_) => asset_open = false,
                    },
                }

                if proxy.send_event(CustomAppEvent::Wake).is_err() {
                    break;
                }
            }
        });

    match spawned {
        Ok(handle) => Some(handle),
        Err(e) => {
            warn!("Pont de réveil indisponible, repli sur le réveil périodique : {e}");
            None
        }
    }
}

impl ApplicationHandler<CustomAppEvent> for GremlinApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let win_cfg = WindowConfig {
            title: String::from("Gremlin"),
            width: NATIVE_WIDTH.saturating_mul(self.config.scale_factor),
            height: NATIVE_HEIGHT.saturating_mul(self.config.scale_factor),
            transparent: true,
            decorations: false,
            always_on_top: true,
            resizable: false,
            icon: gremlin_system::load_app_icon(),
        };

        match event_loop.create_window(win_cfg.to_window_attributes()) {
            Ok(window) => {
                let arc_window = Arc::new(window);
                if let Some(icon) = gremlin_system::load_app_icon() {
                    arc_window.set_window_icon(Some(icon));
                }

                self.window = Some(arc_window.clone());
                if self.config.click_through_enabled {
                    self.apply_click_through(true);
                }

                match AppRenderer::new(arc_window.clone(), NATIVE_WIDTH, NATIVE_HEIGHT) {
                    Ok(renderer) => {
                        self.renderer = Some(renderer);
                        info!("Surface GPU Pixels initialisée avec succès");
                    }
                    Err(e) => warn!("Échec d'initialisation du renderer Pixels GPU : {e}"),
                }

                self.ui.needs_redraw = true;
                arc_window.request_redraw();
                info!("Fenêtre Gremlin initialisée avec succès");
            }
            Err(e) => warn!("Impossible de créer la fenêtre winit : {e}"),
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: CustomAppEvent) {
        match event {
            CustomAppEvent::Wake => {
                // Le drainage effectif a lieu dans `about_to_wait`, qui suit
                // immédiatement : il suffit ici d'avoir interrompu l'attente.
                self.ui.needs_redraw = true;
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Demande de fermeture reçue : sauvegarde puis arrêt");
                self.persist_state("fermeture de la fenêtre");
                event_loop.exit();
            }
            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.ui.modifiers = Modifiers::state(&new_modifiers);
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    if let Err(e) = renderer.resize_surface(size.width, size.height) {
                        warn!("Erreur lors du redimensionnement de la surface Pixels : {e}");
                    }
                }
                self.request_redraw();
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if self.ui.is_palette_open {
                    self.handle_palette_key(&key_event);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                } else if key_event.state == ElementState::Pressed
                    && key_event.logical_key == Key::Named(NamedKey::Space)
                {
                    self.toggle_palette();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Right, ElementState::Pressed) => self.toggle_palette(),
                (MouseButton::Left, ElementState::Pressed) if !self.ui.is_palette_open => {
                    self.ui.is_dragging = true;
                    self.visuals.animation_controller.play("dragged", false);
                    self.request_redraw();
                    if let Some(window) = &self.window {
                        let _ = window.drag_window();
                    }
                }
                (MouseButton::Left, ElementState::Released) if !self.ui.is_palette_open => {
                    if self.ui.is_dragging {
                        self.ui.is_dragging = false;
                        self.sync_animation_with_mood();
                        self.request_redraw();
                    }
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                self.compose_frame();
                if let Some(renderer) = &mut self.renderer {
                    if let Err(e) = renderer.render_buffer(&self.visuals.pixel_buffer) {
                        warn!("Erreur lors du rendu GPU Pixels : {e}");
                    }
                }
                self.ui.needs_redraw = false;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let frame_delta = now.duration_since(self.clocks.frame);
        self.clocks.frame = now;

        self.pump_events();

        if self.ui.exit_requested {
            self.persist_state("sortie via la zone de notification");
            event_loop.exit();
            return;
        }

        self.advance_simulation(now);

        if now.duration_since(self.clocks.auto_save)
            >= Duration::from_secs(self.config.auto_save_interval_secs)
        {
            self.clocks.auto_save = now;
            self.persist_state("sauvegarde automatique");
        }

        if self.ui.is_palette_open
            && now.duration_since(self.clocks.cursor_blink) >= CURSOR_BLINK_INTERVAL
        {
            self.clocks.cursor_blink = now;
            self.ui.cursor_blink_state = !self.ui.cursor_blink_state;
            self.ui.needs_redraw = true;
        }

        if self.visuals.animation_controller.update(frame_delta) {
            self.ui.needs_redraw = true;
        }

        if self.ui.needs_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(now + self.next_wake_delay()));
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use gremlin_render::AccessoryCategory;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Répertoire applicatif temporaire, nettoyé à la destruction.
    struct TempEnv {
        root: PathBuf,
        paths: AppPaths,
    }

    impl TempEnv {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "gremlin-app-{}-{label}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let config = root.join("config");
            let data = root.join("data");
            let cache = root.join("cache");
            for dir in [&config, &data, &cache] {
                std::fs::create_dir_all(dir).expect("répertoire de test");
            }
            Self {
                paths: AppPaths::from_dirs(config, data, cache),
                root,
            }
        }
    }

    impl Drop for TempEnv {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Construit une application sans aucun effet de bord système.
    fn headless_app(env: &TempEnv, config: AppConfig) -> GremlinApp {
        GremlinApp::with_options(
            PetState::new("Gizmo"),
            config,
            AppOptions::headless(env.paths.clone()),
        )
        .expect("construction headless")
    }

    #[test]
    fn test_app_initialization_and_animation_sync() {
        let env = TempEnv::new("init");
        let app = headless_app(&env, AppConfig::default());

        assert_eq!(app.visuals.pixel_buffer.width(), NATIVE_WIDTH);
        assert_eq!(app.visuals.pixel_buffer.height(), NATIVE_HEIGHT);
        assert!(app
            .visuals
            .animation_controller
            .current_frame_key()
            .is_some());
        assert!(!app.accessory_catalog().is_empty());
    }

    #[test]
    fn test_headless_app_starts_no_watcher_and_no_tray() {
        let env = TempEnv::new("headless");
        let app = headless_app(&env, AppConfig::default());

        assert!(app.watchers.repo_watcher.is_none());
        assert!(app.watchers.asset_watcher.is_none());
        assert!(app.tray_manager.is_none());
        assert!(app.autostart_manager.is_none());
    }

    #[test]
    fn test_every_mood_maps_to_a_registered_animation() {
        // Régression : « coding » et « angry » n'étaient enregistrées nulle
        // part, `play()` était donc un no-op silencieux pour ces humeurs.
        let env = TempEnv::new("moods");
        let mut app = headless_app(&env, AppConfig::default());
        app.load_procedural_fallback_skin();

        for mood in [
            PetMood::Happy,
            PetMood::Coding,
            PetMood::Hungry,
            PetMood::Tired,
            PetMood::Sick,
            PetMood::Angry,
            PetMood::Sleeping,
            PetMood::Dead,
        ] {
            let key = animation_key_for_mood(mood);
            app.visuals.animation_controller.play(key, true);
            assert_eq!(
                app.visuals.animation_controller.current_animation_name(),
                Some(key),
                "aucune animation enregistrée pour l'humeur {mood:?}"
            );
        }
    }

    #[test]
    fn test_drag_animation_transition() {
        let env = TempEnv::new("drag");
        let mut app = headless_app(&env, AppConfig::default());

        assert_eq!(
            app.visuals.animation_controller.current_animation_name(),
            Some("happy")
        );

        app.ui.is_dragging = true;
        app.sync_animation_with_mood();
        assert_eq!(
            app.visuals.animation_controller.current_animation_name(),
            Some("dragged")
        );

        app.ui.is_dragging = false;
        app.sync_animation_with_mood();
        assert_eq!(
            app.visuals.animation_controller.current_animation_name(),
            Some("happy")
        );
    }

    #[test]
    fn test_toggle_palette_and_click_through_suspension() {
        let env = TempEnv::new("palette");
        let mut app = headless_app(
            &env,
            AppConfig {
                click_through_enabled: true,
                ..AppConfig::default()
            },
        );

        assert!(!app.is_palette_open());
        assert_eq!(app.visuals.pixel_buffer.width(), NATIVE_WIDTH);

        app.toggle_palette();
        assert!(app.is_palette_open());
        assert!(app.ui.suspended_click_through);
        assert_eq!(app.visuals.pixel_buffer.width(), RaycastLayout::WIDTH);
        assert_eq!(app.visuals.pixel_buffer.height(), RaycastLayout::HEIGHT);

        app.compose_frame();
        assert!(app.visuals.pixel_buffer.as_bytes().iter().any(|&b| b > 0));

        app.config
            .wardrobe
            .equip(AccessoryCategory::Hat, "wizard_hat");
        assert!(app.config.wardrobe.is_equipped("wizard_hat"));

        app.toggle_palette();
        assert!(!app.is_palette_open());
        assert!(!app.ui.suspended_click_through);
        assert_eq!(app.visuals.pixel_buffer.width(), NATIVE_WIDTH);
    }

    #[test]
    fn test_simulation_advances_only_after_the_tick_interval() {
        let env = TempEnv::new("tick");
        let mut app = headless_app(&env, AppConfig::default());

        // Immédiatement après la construction, aucun pas ne doit être appliqué.
        let before = app.pet_state.stats();
        assert!(app.advance_simulation(Instant::now()).is_empty());
        assert_eq!(before, app.pet_state.stats());

        // Une heure plus tard, la décroissance s'applique une seule fois.
        let later = app.clocks.simulation + Duration::from_secs(3600);
        let events = app.advance_simulation(later);
        assert!(!events.is_empty());
        let after_first = app.pet_state.stats();
        assert!(after_first.satiety() < before.satiety());

        // Rappeler la fonction sans temps écoulé ne doit rien décroître de plus.
        assert!(app.advance_simulation(later).is_empty());
        assert_eq!(after_first, app.pet_state.stats());
    }

    #[test]
    fn test_commit_signal_is_consumed_and_awards_xp() {
        let env = TempEnv::new("commit");
        let mut app = headless_app(&env, AppConfig::default());
        let xp_before = app.pet_state.progression().total_xp();

        app.dev_sender()
            .send(DevSignal::CommitCreated {
                repo_path: env.root.clone(),
                repo_name: String::from("gremlin"),
                branch: String::from("main"),
                commit_sha: Some("a".repeat(40)),
                message: Some(String::from("feat: première pierre")),
            })
            .expect("envoi du signal");

        let events = app.pump_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, CoreEvent::CommitReceived { .. })));
        assert!(app.pet_state.progression().total_xp() > xp_before);
        assert_eq!(app.pet_state.progression().total_commits(), 1);
    }

    #[test]
    fn test_repo_discovery_and_removal_update_the_palette() {
        let env = TempEnv::new("repos");
        let mut app = headless_app(&env, AppConfig::default());
        let sender = app.dev_sender();

        sender
            .send(DevSignal::RepoDiscovered {
                path: env.root.clone(),
                repo_name: String::from("alpha"),
            })
            .expect("envoi");
        app.pump_events();

        assert_eq!(app.monitored_repos().len(), 1);
        // Aucune branche n'est inventée à la découverte.
        assert_eq!(app.monitored_repos()[0].branch, None);
        assert_eq!(app.monitored_repos()[0].branch_label(), "inconnue");

        // Un commit renseigne la branche réelle.
        sender
            .send(DevSignal::CommitCreated {
                repo_path: env.root.clone(),
                repo_name: String::from("alpha"),
                branch: String::from("develop"),
                commit_sha: None,
                message: None,
            })
            .expect("envoi");
        app.pump_events();
        assert_eq!(app.monitored_repos()[0].branch.as_deref(), Some("develop"));

        sender
            .send(DevSignal::RepoRemoved {
                path: env.root.clone(),
                repo_name: String::from("alpha"),
            })
            .expect("envoi");
        app.pump_events();

        assert!(app.monitored_repos().is_empty());
    }

    #[test]
    fn test_scale_factor_is_clamped_when_applied() {
        let env = TempEnv::new("scale");
        let mut app = headless_app(&env, AppConfig::default());

        app.handle_execution_result(PaletteExecutionResult::SetScaleFactor(0));
        assert_eq!(app.config().scale_factor, AppConfig::MIN_SCALE_FACTOR);

        app.handle_execution_result(PaletteExecutionResult::SetScaleFactor(9_999));
        assert_eq!(app.config().scale_factor, AppConfig::MAX_SCALE_FACTOR);
    }

    #[test]
    fn test_save_roundtrip_through_the_application() {
        let env = TempEnv::new("save");
        let mut app = headless_app(&env, AppConfig::default());
        app.handle_execution_result(PaletteExecutionResult::FeedPet);
        app.handle_execution_result(PaletteExecutionResult::SaveNow);

        assert!(
            app.last_save_error().is_none(),
            "la sauvegarde ne devait pas échouer : {:?}",
            app.last_save_error()
        );
        assert!(env.paths.save_file().exists());
    }

    #[test]
    fn test_actions_on_a_dead_pet_do_not_panic() {
        let env = TempEnv::new("dead");
        let mut app = headless_app(&env, AppConfig::default());
        app.pet_state
            .set_stats(gremlin_core::PetStats::new(0.0, 0.0, 0.0));

        for action in [
            PaletteExecutionResult::FeedPet,
            PaletteExecutionResult::PetGremlin,
            PaletteExecutionResult::HealPet,
            PaletteExecutionResult::ToggleSleep,
        ] {
            app.handle_execution_result(action);
        }
        assert!(!app.pet_state.is_alive());

        app.handle_execution_result(PaletteExecutionResult::RevivePet);
        assert!(app.pet_state.is_alive());
    }

    #[test]
    fn test_wake_delay_matches_the_documented_pacing() {
        let env = TempEnv::new("pacing");
        let mut app = headless_app(&env, AppConfig::default());

        app.ui.is_dragging = true;
        assert_eq!(app.next_wake_delay(), DRAG_FRAME_INTERVAL);

        app.ui.is_dragging = false;
        app.ui.is_palette_open = true;
        assert_eq!(app.next_wake_delay(), PALETTE_FRAME_INTERVAL);

        app.ui.is_palette_open = false;
        assert!(app.next_wake_delay() >= MIN_FRAME_INTERVAL);
        assert!(app.next_wake_delay() <= IDLE_WAKE_INTERVAL);
    }
}
