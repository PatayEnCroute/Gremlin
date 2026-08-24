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
//! Les horloges de `LoopClocks` sont volontairement distinctes : `frame`
//! cadence l'animation, `simulation` cadence le moteur métier, `auto_save`
//! cadence la persistance. Les confondre exposait au risque d'appliquer deux
//! fois la même décroissance.

use crate::config::AppConfig;
use crate::desktop;
use crate::error::AppError;
use crate::persistence::PersistenceManager;
use crate::renderer::AppRenderer;
use crate::ui::{
    CommandPalette, PaletteContext, PaletteExecutionResult, PanelInteraction, PanelScene,
    PanelStyle, RaycastRenderer, RepoDisplayInfo, SettingsWindow, SystemTheme, TextSize, Theme,
};
use crate::visual_feedback::{VisualAnchors, VisualCue, VisualFeedback};
use crossbeam_channel::{Receiver, Sender};
use gremlin_core::{
    ActivityState, BuildSummary, BuildTool, CoreEvent, PetMood, PetState, RepositoryId,
    TestFramework, TestSummary,
};
use gremlin_render::{
    register_default_procedural_accessories, AccessoryCatalog, AccessoryItem, AccessoryManifest,
    AnimationController, LayerCompositor, PixelBuffer, PlayMode, SkinManifest,
    SpeechBubbleRenderer, SpriteAnimation, SpriteAtlas, SpriteFrame, TransitionRenderer,
};
use gremlin_system::{
    ActivityEvent, ActivityMonitor, AppPaths, AutostartManager, LayeredSurface, PlatformImpl,
    PlatformWindowExt, SystemTrayManager, TrayMenuAction, WindowConfig,
};
use gremlin_watcher::{
    AssetSignal, AssetWatcher, DevSignal, ParsedBuildReport, ParsedTestReport, RepoWatcher,
    ReportBuildTool, ReportFramework, ToolingStateAck, WatcherStatus,
};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, Modifiers, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

/// Agrandissement maximal de la scène du familier à la présentation.
///
/// Reprend la borne du module de présentation en couches, afin que les deux
/// chemins ne puissent pas diverger.
const MAX_PRESENTATION_SCALE: u32 = 8;

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
/// Nombre maximal de chemins associés à des identifiants de session.
const REPOSITORY_ID_CAPACITY: usize = 64;

/// Vérifie qu'une clé de sprite désigne uniquement un fichier voisin du manifest.
///
/// Les manifests de mods sont des entrées non fiables : une clé contenant un
/// séparateur ne doit jamais permettre de sortir du répertoire de l'accessoire.
fn is_safe_sprite_key(key: &str) -> bool {
    if key.is_empty() || key.contains('/') || key.contains('\\') {
        return false;
    }

    let path = Path::new(key);
    path.file_name().and_then(std::ffi::OsStr::to_str) == Some(key) && path.extension().is_none()
}

/// Événements personnalisés injectés dans la boucle d'événements winit.
///
/// Le variant d'accessibilité transporte un `TreeUpdate` et une requête
/// d'action : il n'est donc ni `Copy` ni `Eq`, ce qui a fait retirer ces dérives
/// de l'énumération entière —  comprise,  ne
/// l'implémentant pas non plus.
#[derive(Debug)]
pub enum CustomAppEvent {
    /// Réveille la boucle : des signaux sont disponibles dans les canaux.
    Wake,
    /// Événement émis par l'adaptateur d'accessibilité.
    #[cfg(feature = "a11y")]
    Accessibility(accesskit_winit::Event),
}

#[cfg(feature = "a11y")]
impl From<accesskit_winit::Event> for CustomAppEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
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

/// Tampons fixes utilisés uniquement par la scène 64×64 du compagnon.
struct SceneBuffers {
    current: PixelBuffer,
    outgoing: PixelBuffer,
    presented: PixelBuffer,
    has_presented: bool,
}

impl SceneBuffers {
    fn new() -> Self {
        Self {
            current: PixelBuffer::new(NATIVE_WIDTH, NATIVE_HEIGHT),
            outgoing: PixelBuffer::new(NATIVE_WIDTH, NATIVE_HEIGHT),
            presented: PixelBuffer::new(NATIVE_WIDTH, NATIVE_HEIGHT),
            has_presented: false,
        }
    }

    fn capture_presented(&mut self) -> bool {
        if !self.has_presented {
            return false;
        }
        self.outgoing
            .as_bytes_mut()
            .copy_from_slice(self.presented.as_bytes());
        true
    }

    fn invalidate(&mut self) {
        self.has_presented = false;
        self.current.clear(0, 0, 0, 0);
        self.outgoing.clear(0, 0, 0, 0);
        self.presented.clear(0, 0, 0, 0);
    }
}

/// Ressources graphiques et d'animation.
struct Visuals {
    pixel_buffer: PixelBuffer,
    scene_buffers: SceneBuffers,
    sprite_atlas: SpriteAtlas,
    accessory_catalog: AccessoryCatalog,
    animation_controller: AnimationController,
    active_manifest: Option<SkinManifest>,
    feedback: VisualFeedback,
    scene_elapsed: Duration,
}

impl Visuals {
    fn new(sprite_atlas: SpriteAtlas, accessory_catalog: AccessoryCatalog) -> Self {
        Self {
            pixel_buffer: PixelBuffer::new(NATIVE_WIDTH, NATIVE_HEIGHT),
            scene_buffers: SceneBuffers::new(),
            sprite_atlas,
            accessory_catalog,
            animation_controller: AnimationController::new(),
            active_manifest: None,
            feedback: VisualFeedback::new(),
            scene_elapsed: Duration::ZERO,
        }
    }
}

/// État transitoire de l'interface.
#[derive(Debug)]
struct UiState {
    is_palette_open: bool,
    cursor_blink_state: bool,
    is_dragging: bool,
    needs_redraw: bool,
    /// Le panneau doit être recomposé et re-présenté.
    ///
    /// Distinct de `needs_redraw`, qui concerne la fenêtre du familier : les
    /// deux fenêtres ont désormais des cycles de rafraîchissement séparés, et
    /// les confondre ferait recomposer la scène 64×64 à chaque frappe au clavier.
    panel_needs_redraw: bool,
    /// Ligne du panneau survolée par la souris, en indice d'item filtré.
    hovered_item: Option<usize>,
    /// Le panneau a déjà reçu le focus depuis sa dernière ouverture.
    ///
    /// Garde-fou : sans lui, un gestionnaire de fenêtres qui refuse le focus à
    /// l'affichage déclencherait la fermeture sur perte de focus dans l'instant
    /// qui suit l'ouverture.
    panel_had_focus: bool,
    exit_requested: bool,
    modifiers: ModifiersState,
    last_save_error: Option<String>,
    /// Dernier incident de surveillance présenté dans le panneau.
    last_observation_error: Option<String>,
    /// État demandé au worker, en attente de confirmation asynchrone.
    pending_tooling_enabled: Option<bool>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            is_palette_open: false,
            cursor_blink_state: true,
            is_dragging: false,
            needs_redraw: true,
            panel_needs_redraw: false,
            hovered_item: None,
            panel_had_focus: false,
            exit_requested: false,
            modifiers: ModifiersState::empty(),
            last_save_error: None,
            last_observation_error: None,
            pending_tooling_enabled: None,
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
    repository_ids: HashMap<PathBuf, RepositoryId>,
    next_repository_id: u64,
    tooling_ack: Option<Receiver<ToolingStateAck>>,
}

fn convert_test_summary(report: ParsedTestReport) -> TestSummary {
    let framework = match report.framework {
        ReportFramework::Rust => TestFramework::CargoTest,
        ReportFramework::JavaScript => TestFramework::JavaScript,
        ReportFramework::Python => TestFramework::Pytest,
        ReportFramework::Go => TestFramework::GoTest,
        ReportFramework::Dotnet => TestFramework::DotnetTest,
        ReportFramework::Generic => TestFramework::GenericJunit,
    };
    TestSummary::new(
        framework,
        report.passed,
        report.failed,
        report.skipped,
        report.duration,
    )
}

fn convert_build_summary(report: ParsedBuildReport) -> BuildSummary {
    let tool = match report.tool {
        ReportBuildTool::Cargo => BuildTool::Cargo,
        ReportBuildTool::Npm => BuildTool::Npm,
        ReportBuildTool::WebpackOrVite => BuildTool::WebpackOrVite,
        ReportBuildTool::Python => BuildTool::Python,
        ReportBuildTool::Go => BuildTool::Go,
        ReportBuildTool::Dotnet => BuildTool::Dotnet,
        ReportBuildTool::Generic => BuildTool::Generic,
    };
    BuildSummary::new(tool, report.success, report.duration)
}

impl WatcherBridge {
    /// Arrête les surveillants et libère les émetteurs qu'ils détiennent.
    fn shutdown(&mut self) {
        self.repo_watcher = None;
        self.asset_watcher = None;
    }

    fn repository_id(&mut self, path: &Path) -> RepositoryId {
        if let Some(id) = self.repository_ids.get(path) {
            return *id;
        }
        if self.repository_ids.len() >= REPOSITORY_ID_CAPACITY {
            if let Some(oldest) = self.repository_ids.keys().next().cloned() {
                let _ = self.repository_ids.remove(&oldest);
            }
        }
        let id = RepositoryId::new(self.next_repository_id);
        self.next_repository_id = self.next_repository_id.saturating_add(1);
        let _ = self.repository_ids.insert(path.to_path_buf(), id);
        id
    }

    fn remove_repository(&mut self, path: &Path) {
        let _ = self.repository_ids.remove(path);
    }
}

/// Mesure d'activité et état de session associés, séparés des watchers disque.
struct ActivityBridge {
    monitor: Option<ActivityMonitor>,
    latest: ActivityState,
    development_seen: bool,
    system_integration_available: bool,
}

impl ActivityBridge {
    fn new(enabled: bool, system_integration_available: bool) -> (Self, Option<String>) {
        let mut bridge = Self {
            monitor: None,
            latest: ActivityState::Unavailable,
            development_seen: false,
            system_integration_available,
        };
        let error = if enabled {
            bridge.start().err().map(|error| error.to_string())
        } else {
            None
        };
        (bridge, error)
    }

    fn start(&mut self) -> Result<(), gremlin_system::SystemError> {
        if !self.system_integration_available {
            return Err(gremlin_system::SystemError::ActivityUnavailable(
                "intégration système désactivée".to_owned(),
            ));
        }
        if self.monitor.is_some() {
            return Ok(());
        }
        self.monitor = Some(ActivityMonitor::start()?);
        Ok(())
    }

    fn stop(&mut self) {
        self.monitor = None;
        self.latest = ActivityState::Unavailable;
        self.development_seen = false;
    }

    fn drain(&mut self, idle_threshold: Duration) -> Option<String> {
        let Some(monitor) = &self.monitor else {
            self.latest = ActivityState::Unavailable;
            return None;
        };
        let mut latest_event = None;
        while let Ok(event) = monitor.events().try_recv() {
            latest_event = Some(event);
        }
        match latest_event {
            Some(ActivityEvent::Sample(sample)) => {
                self.latest = if sample.idle_for() >= idle_threshold {
                    ActivityState::Idle(sample.idle_for())
                } else {
                    ActivityState::Active
                };
                None
            }
            Some(ActivityEvent::Unavailable(reason) | ActivityEvent::ReadFailed(reason)) => {
                self.latest = ActivityState::Unavailable;
                Some(reason)
            }
            None => None,
        }
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
    activity: ActivityBridge,
    wake_bridge: Option<JoinHandle<()>>,
    window: Option<Arc<Window>>,
    renderer: Option<AppRenderer>,
    /// Fenêtre dédiée du panneau de paramètres.
    ///
    /// Créée à la première ouverture puis conservée masquée : un utilisateur qui
    /// n'ouvre jamais les réglages n'en paie pas le coût, et les ouvertures
    /// suivantes sont immédiates.
    settings: Option<SettingsWindow>,
    /// Présentation à alpha par pixel de la fenêtre du familier.
    ///
    /// Renseignée uniquement là où elle est nécessaire et disponible — sous
    /// Windows. Une surface graphique attachée à un HWND n'y propose aucun mode
    /// de composition honorant l'alpha : la fenêtre transparente s'y afficherait
    /// dans un carré noir. Ailleurs, le chemin GPU habituel suffit et ce champ
    /// reste vide.
    layered: Option<LayeredSurface>,
    /// Proxy de la boucle d'événements, requis par l'adaptateur d'accessibilité.
    ///
    /// Conservé même sans la feature `a11y`, pour que la construction de
    /// l'application reste identique dans les deux configurations.
    #[cfg_attr(not(feature = "a11y"), allow(dead_code))]
    proxy: Option<EventLoopProxy<CustomAppEvent>>,
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
    // Le constructeur assemble les cinq caisses et leurs points d'injection ;
    // les états runtime restent regroupés dans leurs sous-structures dédiées.
    #[allow(clippy::too_many_lines)]
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
        // Le proxy est conservé en plus d'être confié au pont de réveil :
        // l'adaptateur d'accessibilité en a besoin pour remonter ses requêtes
        // dans la boucle d'événements.
        let retained_proxy = options.wake_proxy.clone();
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

        let (activity, activity_error) = ActivityBridge::new(
            config.focus_tracking_enabled,
            options.enable_system_integration,
        );

        let monitored_repos = Vec::new();
        let command_palette = CommandPalette::new(&PaletteContext {
            catalog: &accessory_catalog,
            wardrobe: &config.wardrobe,
            pet_state: &pet_state,
            config: &config,
            autostart_active,
            repos: &monitored_repos,
            last_save_error: None,
            last_observation_error: activity_error.as_deref(),
            pending_tooling_enabled: None,
        });

        let now = Instant::now();
        let visuals = Visuals::new(sprite_atlas, accessory_catalog);
        let mut app = Self {
            config,
            paths,
            pet_state,
            visuals,
            ui: UiState {
                last_observation_error: activity_error,
                ..UiState::default()
            },
            clocks: LoopClocks::new(now),
            watchers: WatcherBridge {
                repo_watcher,
                asset_watcher,
                dev_receiver,
                asset_receiver,
                status_receiver,
                dev_sender,
                asset_sender,
                repository_ids: HashMap::new(),
                next_repository_id: 1,
                tooling_ack: None,
            },
            activity,
            wake_bridge,
            window: None,
            renderer: None,
            settings: None,
            layered: None,
            proxy: retained_proxy,
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
                self.finish_skin_reload();
                return;
            }
        }

        info!("Utilisation des graphismes procéduraux par défaut");
        self.load_procedural_fallback_skin();
        self.finish_skin_reload();
    }

    fn finish_skin_reload(&mut self) {
        self.sync_visual_anchors();
        self.sync_animation_with_mood();
        self.visuals.feedback.cancel_transition();
        self.visuals.scene_buffers.invalidate();
        self.ui.needs_redraw = true;
    }

    /// Alimente les effets avec les points sémantiques optionnels du skin.
    fn sync_visual_anchors(&mut self) {
        let mut anchors = VisualAnchors::default();
        if let Some(manifest) = &self.visuals.active_manifest {
            if let Some(head) = manifest.anchors.get("head") {
                anchors.head = (head.x, head.y);
            }
            if let Some(effect) = manifest.anchors.get("effect_origin") {
                anchors.effect = (effect.x, effect.y);
            }
        }
        self.visuals.feedback.set_anchors(anchors);
    }

    fn try_load_skin_from_dir(&mut self, skin_dir: &Path) -> bool {
        let manifest_path = skin_dir.join("manifest.json");
        let Ok(manifest_content) = fs::read_to_string(&manifest_path) else {
            return false;
        };

        let mut manifest = match SkinManifest::from_json(&manifest_content) {
            Ok(manifest) => manifest,
            Err(e) => {
                warn!(path = %manifest_path.display(), "Manifest de skin invalide : {e}");
                return false;
            }
        };

        let mut loaded_keys = BTreeSet::new();
        if let Ok(entries) = fs::read_dir(skin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("png") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    match self
                        .visuals
                        .sprite_atlas
                        .load_from_png_file_checked(stem, &path, &manifest)
                    {
                        Ok(()) => {
                            loaded_keys.insert(stem.to_string());
                        }
                        Err(e) => {
                            warn!(path = %path.display(), "Sprite ignoré : {e}");
                        }
                    }
                }
            }
        }

        for (animation_name, definition) in &mut manifest.animations {
            definition.frames.retain(|key| {
                let loaded = loaded_keys.contains(key);
                if !loaded {
                    warn!(
                        animation = %animation_name,
                        frame = %key,
                        "Frame de skin absente ou invalide : référence retirée"
                    );
                }
                loaded
            });
        }

        if loaded_keys.is_empty() {
            false
        } else {
            self.visuals.animation_controller = manifest.build_animation_controller();
            self.visuals.active_manifest = Some(manifest);
            true
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
        let mut manifest = match AccessoryManifest::from_json(&content) {
            Ok(manifest) => manifest,
            Err(e) => {
                warn!(path = %dir.display(), "Manifest d'accessoire invalide : {e}");
                return;
            }
        };

        let expected_width = manifest.frame_width;
        let expected_height = manifest.frame_height;
        let accessory_id = manifest.id.clone();
        manifest.frames.retain(|key| {
            if !is_safe_sprite_key(key) {
                warn!(
                    accessory = %accessory_id,
                    frame = %key,
                    "Clé de frame d'accessoire non sûre : entrée ignorée"
                );
                return false;
            }
            let path = dir.join(format!("{key}.png"));
            let frame = match SpriteFrame::from_png_file(&path) {
                Ok(frame) => frame,
                Err(e) => {
                    warn!(
                        accessory = %accessory_id,
                        frame = %key,
                        path = %path.display(),
                        "Frame d'accessoire absente ou corrompue : {e}"
                    );
                    return false;
                }
            };
            if frame.width != expected_width || frame.height != expected_height {
                warn!(
                    accessory = %accessory_id,
                    frame = %key,
                    width = frame.width,
                    height = frame.height,
                    expected_width,
                    expected_height,
                    "Dimensions de frame d'accessoire incompatibles"
                );
                return false;
            }
            self.visuals.sprite_atlas.insert(key.clone(), frame);
            true
        });

        if manifest.frames.is_empty() {
            warn!(id = %manifest.id, "Accessoire ignoré : aucune frame valide");
            return;
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
            last_observation_error: self.ui.last_observation_error.as_deref(),
            pending_tooling_enabled: self.ui.pending_tooling_enabled,
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

    /// Route un lot complet d'événements métier vers les retours visuels.
    fn apply_core_events(&mut self, events: &[CoreEvent]) {
        if events.is_empty() {
            return;
        }

        let outcome = self.visuals.feedback.handle_core_events(events);
        if outcome.mood_changed {
            let captured =
                !self.ui.is_palette_open && self.visuals.scene_buffers.capture_presented();
            self.sync_animation_with_mood();
            if captured {
                self.visuals.feedback.start_transition();
            } else {
                self.visuals.feedback.cancel_transition();
            }
        }

        if outcome.dirty {
            self.request_redraw();
        }
    }

    /// Bascule l'affichage du panneau de paramètres.
    ///
    /// Cette méthode ne touche plus qu'à l'état : la création et l'affichage de
    /// la fenêtre sont réconciliés par `reconcile_settings_window`, qui
    /// dispose de la boucle d'événements. Séparer les deux rend la bascule
    /// testable sans écran.
    ///
    /// Le panneau occupant désormais sa propre fenêtre, la scène du familier
    /// n'est plus détruite : il continue de s'animer à côté des réglages. Le
    /// mode click-through n'a plus à être suspendu non plus, puisque la saisie
    /// arrive sur une fenêtre distincte qui, elle, n'est pas traversante.
    pub fn toggle_palette(&mut self) {
        self.ui.is_palette_open = !self.ui.is_palette_open;
        self.ui.hovered_item = None;

        if self.ui.is_palette_open {
            self.rebuild_palette_items();
        }

        self.ui.panel_needs_redraw = true;
        self.request_redraw();
    }

    /// Aligne l'existence et la visibilité de la fenêtre de paramètres sur l'état.
    ///
    /// Appelée depuis la boucle d'événements, seul endroit où l'on dispose de
    /// l'`ActiveEventLoop` nécessaire à la création d'une fenêtre.
    fn reconcile_settings_window(&mut self, event_loop: &ActiveEventLoop) {
        if !self.ui.is_palette_open {
            if let Some(panel) = &self.settings {
                panel.hide();
            }
            return;
        }

        if self.settings.is_none() {
            #[cfg(feature = "a11y")]
            let outcome = SettingsWindow::new(
                event_loop,
                self.window.as_deref(),
                self.text_size(),
                self.proxy.clone(),
            );
            #[cfg(not(feature = "a11y"))]
            let outcome = SettingsWindow::new(event_loop, self.window.as_deref(), self.text_size());

            match outcome {
                Ok(panel) => {
                    info!("Fenêtre de paramètres créée");
                    self.settings = Some(panel);
                }
                Err(e) => {
                    // Échec explicite plutôt que faux succès : sans panneau,
                    // l'état « ouvert » serait mensonger.
                    error!("Impossible d'ouvrir le panneau de paramètres : {e}");
                    self.ui.is_palette_open = false;
                    return;
                }
            }
        }

        let anchor = self.window.clone();
        let text_size = self.text_size();
        if let Some(panel) = &mut self.settings {
            match panel.resync(text_size) {
                Ok(changed) => self.ui.panel_needs_redraw |= changed,
                Err(e) => warn!("Le panneau n'a pas pu suivre l'échelle de l'écran : {e}"),
            }
            panel.show(anchor.as_deref());
        }
    }

    /// Préférence de taille de texte du panneau.
    const fn text_size(&self) -> TextSize {
        self.config.ui.text_size
    }

    /// Réaligne les métriques du panneau après un changement de préférence.
    fn resync_panel_metrics(&mut self) {
        let text_size = self.text_size();
        if let Some(panel) = &mut self.settings {
            match panel.resync(text_size) {
                Ok(_) => self.ui.panel_needs_redraw = true,
                Err(e) => warn!("Le panneau n'a pas pu suivre la nouvelle taille de texte : {e}"),
            }
        }
    }

    /// Palette de couleurs à employer pour le panneau.
    ///
    /// Le thème du système est lu sur la fenêtre du panneau, seul endroit où le
    /// gestionnaire de fenêtres le rapporte. Il vaut `None` sur les
    /// environnements qui ne l'exposent pas, et la résolution retombe alors sur
    /// la palette sombre.
    fn resolve_theme(&self) -> Theme {
        let system = self
            .settings
            .as_ref()
            .and_then(|panel| panel.window().theme())
            .map(|theme| match theme {
                winit::window::Theme::Light => SystemTheme::Light,
                winit::window::Theme::Dark => SystemTheme::Dark,
            });

        Theme::resolve(self.config.ui.theme, system)
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

    /// Traite un événement destiné à la fenêtre du familier.
    fn handle_pet_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
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
                if key_event.state == ElementState::Pressed
                    && key_event.logical_key == Key::Named(NamedKey::Space)
                {
                    self.toggle_palette();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Right, ElementState::Pressed) => self.toggle_palette(),
                (MouseButton::Left, ElementState::Pressed) => {
                    self.ui.is_dragging = true;
                    self.visuals.animation_controller.play("dragged", false);
                    self.visuals.feedback.handle_cue(VisualCue::DragStarted);
                    self.request_redraw();
                    if let Some(window) = &self.window {
                        let _ = window.drag_window();
                    }
                }
                (MouseButton::Left, ElementState::Released) => {
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
                self.present_companion();
                self.ui.needs_redraw = false;
            }
            _ => {}
        }
    }

    /// Traite un événement destiné à la fenêtre de paramètres.
    ///
    /// La fermeture du panneau n'arrête jamais l'application : elle referme le
    /// panneau et rend la main au familier.
    fn handle_panel_event(&mut self, event: WindowEvent) {
        // L'adaptateur voit passer tous les événements : il y suit le focus, la
        // position et l'échelle. En omettre un le désynchronise en silence.
        #[cfg(feature = "a11y")]
        if let Some(panel) = &mut self.settings {
            panel.forward_to_accessibility(&event);
        }

        match event {
            WindowEvent::CloseRequested => {
                if self.ui.is_palette_open {
                    self.toggle_palette();
                }
            }
            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.ui.modifiers = Modifiers::state(&new_modifiers);
            }
            WindowEvent::Focused(has_focus) => self.handle_panel_focus(has_focus),
            WindowEvent::ScaleFactorChanged { .. } => {
                // L'écran a changé de densité : les métriques et le tampon
                // doivent suivre, sans quoi le texte redeviendrait flou.
                self.ui.panel_needs_redraw = true;
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                self.handle_palette_key(&key_event);
                self.ui.panel_needs_redraw = true;
            }
            WindowEvent::CursorMoved { position, .. } => self.handle_panel_hover(position),
            WindowEvent::CursorLeft { .. } => {
                if self.ui.hovered_item.take().is_some() {
                    self.ui.panel_needs_redraw = true;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => self.handle_panel_scroll(delta),
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left && state == ElementState::Pressed {
                    self.handle_panel_click();
                }
            }
            WindowEvent::RedrawRequested => {
                self.compose_panel();
                if let Some(panel) = &mut self.settings {
                    if let Err(e) = panel.present() {
                        warn!("Échec de présentation du panneau : {e}");
                    }
                }
                // L'arbre est republié en même temps que l'image : les deux
                // décrivent le même état, et les faire diverger reviendrait à
                // mentir au lecteur d'écran.
                self.publish_accessibility_tree();
                self.ui.panel_needs_redraw = false;
            }
            _ => {}
        }
    }

    /// Republie l'arbre d'accessibilité décrivant l'état courant du panneau.
    ///
    /// Sans client d'assistance actif, l'appel ne construit rien : le coût est
    /// nul pour qui n'utilise pas de lecteur d'écran.
    #[cfg(feature = "a11y")]
    fn publish_accessibility_tree(&mut self) {
        let palette = &self.command_palette;
        if let Some(panel) = &mut self.settings {
            panel.publish_accessibility_tree(|| crate::ui::a11y::tree_update(palette));
        }
    }

    /// Variante sans accessibilité : rien à publier.
    ///
    /// Conserve la signature de la variante active pour que les appelants
    /// n'aient pas à être eux-mêmes conditionnés.
    #[cfg(not(feature = "a11y"))]
    #[allow(clippy::unused_self, clippy::needless_pass_by_ref_mut)]
    const fn publish_accessibility_tree(&mut self) {}

    /// Applique une action demandée par un lecteur d'écran.
    #[cfg(feature = "a11y")]
    fn handle_accessibility_event(&mut self, event: accesskit_winit::Event) {
        use accesskit::Action;
        use accesskit_winit::WindowEvent as A11yEvent;

        // Une requête destinée à une fenêtre qui n'est plus la nôtre est ignorée
        // plutôt que appliquée à tort.
        if self
            .settings
            .as_ref()
            .is_none_or(|panel| panel.id() != event.window_id)
        {
            return;
        }

        match event.window_event {
            A11yEvent::InitialTreeRequested => {
                self.publish_accessibility_tree();
            }
            A11yEvent::AccessibilityDeactivated => {}
            A11yEvent::ActionRequested(request) => {
                let Some(index) = crate::ui::a11y::row_index(request.target_node) else {
                    return;
                };

                match request.action {
                    Action::Focus | Action::ScrollIntoView => {
                        self.command_palette.select_index(index);
                    }
                    Action::Click => {
                        self.command_palette.select_index(index);
                        let result = self.command_palette.execute_selected(&self.config.wardrobe);
                        self.handle_execution_result(result);
                        self.rebuild_palette_items();
                    }
                    _ => return,
                }

                self.ui.panel_needs_redraw = true;
            }
        }
    }

    /// Referme le panneau lorsqu'il perd le focus, façon Raycast.
    ///
    /// Le repli n'a lieu qu'après une prise de focus effective : sans ce garde,
    /// un gestionnaire de fenêtres refusant le focus à l'ouverture refermerait
    /// le panneau immédiatement après l'avoir affiché.
    fn handle_panel_focus(&mut self, has_focus: bool) {
        if has_focus {
            self.ui.panel_had_focus = true;
            return;
        }

        if self.ui.panel_had_focus && self.ui.is_palette_open && self.config.ui.close_on_focus_loss
        {
            self.ui.panel_had_focus = false;
            self.toggle_palette();
        }
    }

    /// Met à jour la ligne survolée depuis la position du curseur.
    fn handle_panel_hover(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        let Some(panel) = &self.settings else {
            return;
        };

        let metrics = panel.metrics();
        let visible = metrics.visible_rows();
        // La position arrive en pixels physiques, exactement l'unité du tampon :
        // aucune conversion d'échelle n'est nécessaire.
        let hovered = metrics
            .row_at(position.x as i32, position.y as i32, visible)
            .and_then(|row| self.command_palette.item_at_visible_row(row, visible));

        if hovered != self.ui.hovered_item {
            self.ui.hovered_item = hovered;
            self.ui.panel_needs_redraw = true;
        }
    }

    /// Active la ligne cliquée, ou déplace la fenêtre depuis son en-tête.
    fn handle_panel_click(&mut self) {
        if let Some(index) = self.ui.hovered_item {
            self.command_palette.select_index(index);
            let result = self.command_palette.execute_selected(&self.config.wardrobe);
            self.handle_execution_result(result);
            self.rebuild_palette_items();
            self.ui.panel_needs_redraw = true;
            return;
        }

        // Hors de la liste, le clic sert à déplacer le panneau : sans bordure de
        // fenêtre, c'est la seule prise disponible.
        if let Some(panel) = &self.settings {
            let _ = panel.window().drag_window();
        }
    }

    /// Fait défiler la liste à la molette.
    ///
    /// La molette déplace la sélection plutôt qu'un décalage indépendant : la
    /// liste reste ainsi pilotée par un seul état, et le clavier et la souris ne
    /// peuvent pas se contredire.
    fn handle_panel_scroll(&mut self, delta: winit::event::MouseScrollDelta) {
        use winit::event::MouseScrollDelta;

        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                if y.is_finite() {
                    -y.signum() as i32
                } else {
                    0
                }
            }
            MouseScrollDelta::PixelDelta(position) => {
                if position.y.abs() < 1.0 {
                    0
                } else {
                    -position.y.signum() as i32
                }
            }
        };

        match steps.cmp(&0) {
            std::cmp::Ordering::Greater => self.command_palette.select_next(),
            std::cmp::Ordering::Less => self.command_palette.select_prev(),
            std::cmp::Ordering::Equal => return,
        }

        self.ui.hovered_item = None;
        self.ui.panel_needs_redraw = true;
    }

    /// Traite les événements clavier du panneau de paramètres.
    ///
    /// # Répartition des touches
    ///
    /// Les flèches horizontales déplacent le **curseur de saisie**, jamais la
    /// navigation : c'est la convention de tout champ de texte, et la violer
    /// rendrait la correction d'une frappe impossible. La navigation passe donc
    /// par `Tab` et `Entrée` pour descendre, `Échap` et le retour arrière sur
    /// saisie vide pour remonter — la répartition de Raycast.
    ///
    /// `Ctrl+A` n'est volontairement pas lié : sans modèle de sélection de
    /// texte, il ne pourrait que mentir sur son effet. `Ctrl+U` efface la ligne
    /// et `Ctrl+W` le mot précédent, deux conventions sans ambiguïté.
    pub fn handle_palette_key(&mut self, key_event: &KeyEvent) {
        if key_event.state != ElementState::Pressed {
            return;
        }

        if self.ui.modifiers.control_key()
            && self.handle_palette_control_key(&key_event.logical_key)
        {
            return;
        }

        let page = self
            .settings
            .as_ref()
            .map_or(1, |panel| panel.metrics().visible_rows());

        match &key_event.logical_key {
            Key::Named(NamedKey::ArrowDown) => self.command_palette.select_next(),
            Key::Named(NamedKey::ArrowUp) => self.command_palette.select_prev(),
            Key::Named(NamedKey::PageDown) => self.command_palette.select_page_down(page),
            Key::Named(NamedKey::PageUp) => self.command_palette.select_page_up(page),
            Key::Named(NamedKey::ArrowLeft) => self.command_palette.move_caret_left(),
            Key::Named(NamedKey::ArrowRight) => self.command_palette.move_caret_right(),
            Key::Named(NamedKey::Home) => self.command_palette.move_caret_to_start(),
            Key::Named(NamedKey::End) => self.command_palette.move_caret_to_end(),
            Key::Named(NamedKey::Escape) => self.handle_palette_escape(),
            Key::Named(NamedKey::Backspace) => {
                // Retour arrière sur saisie vide : on remonte d'un niveau plutôt
                // que de ne rien faire, ce qui donne un geste de sortie continu.
                if self.command_palette.query().is_empty() {
                    if !self.command_palette.ascend() {
                        return;
                    }
                } else {
                    self.command_palette.delete_before_caret();
                }
            }
            Key::Named(NamedKey::Tab | NamedKey::Enter) => {
                let result = self.command_palette.execute_selected(&self.config.wardrobe);
                self.handle_execution_result(result);
                self.rebuild_palette_items();
            }
            Key::Character(text) => {
                for ch in text.chars().filter(|c| !c.is_control()) {
                    self.command_palette.insert_char(ch);
                }
            }
            _ => return,
        }

        self.ui.hovered_item = None;
        self.ui.panel_needs_redraw = true;
    }

    /// Traite les raccourcis à modificateur du panneau.
    ///
    /// Renvoie `true` lorsque la touche a été consommée.
    fn handle_palette_control_key(&mut self, key: &Key) -> bool {
        let Key::Character(text) = key else {
            return false;
        };

        if text.eq_ignore_ascii_case("s") {
            // Raccourci annoncé dans le pied de page.
            self.persist_state("raccourci Ctrl+S");
            self.ui.panel_needs_redraw = true;
            return true;
        }
        if text.eq_ignore_ascii_case("u") {
            self.command_palette.clear_query();
            self.ui.panel_needs_redraw = true;
            return true;
        }
        if text.eq_ignore_ascii_case("w") {
            self.command_palette.delete_word_before_caret();
            self.ui.panel_needs_redraw = true;
            return true;
        }

        false
    }

    /// Applique `Échap` selon le contexte : effacer, remonter, puis fermer.
    ///
    /// Cette gradation évite qu'une recherche en cours ne soit perdue en même
    /// temps que le panneau : une première pression rend la liste, une seconde
    /// remonte, une troisième ferme.
    fn handle_palette_escape(&mut self) {
        if !self.command_palette.query().is_empty() {
            self.command_palette.clear_query();
            return;
        }
        if self.command_palette.ascend() {
            return;
        }
        self.toggle_palette();
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
            PaletteExecutionResult::RevivePet => match self.pet_state.revive() {
                Ok(events) => {
                    info!("Gremlin a été réanimé");
                    self.apply_core_events(&events);
                }
                Err(e) => warn!("Réanimation impossible : {e}"),
            },
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
            PaletteExecutionResult::ToggleToolingWatcher => self.toggle_tooling_watcher(),
            PaletteExecutionResult::ToggleFocusTracking => self.toggle_focus_tracking(),
            PaletteExecutionResult::ToggleBreakReminders => self.toggle_break_reminders(),
            PaletteExecutionResult::SetScaleFactor(factor) => {
                self.config.scale_factor =
                    factor.clamp(AppConfig::MIN_SCALE_FACTOR, AppConfig::MAX_SCALE_FACTOR);
                if !self.ui.is_palette_open {
                    self.resize_to(NATIVE_WIDTH, NATIVE_HEIGHT, false);
                }
            }
            PaletteExecutionResult::ToggleSleep => {
                match self.pet_state.toggle_sleep() {
                    Ok(events) => self.apply_core_events(&events),
                    Err(e) => warn!("Bascule du sommeil impossible : {e}"),
                }
                if let Some(tray) = &self.tray_manager {
                    tray.set_sleep_state(self.pet_state.is_sleeping());
                }
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
            PaletteExecutionResult::CycleTextSize => {
                self.config.ui.text_size = self.config.ui.text_size.next();
                self.resync_panel_metrics();
                self.persist_state("changement de taille de texte");
            }
            PaletteExecutionResult::CycleTheme => {
                self.config.ui.theme = self.config.ui.theme.next();
                self.ui.panel_needs_redraw = true;
                self.persist_state("changement de thème");
            }
            PaletteExecutionResult::ToggleReducedMotion => {
                self.config.ui.reduced_motion = !self.config.ui.reduced_motion;
                // Curseur figé en position visible : le laisser dans son dernier
                // état aléatoire pourrait l'éteindre définitivement.
                self.ui.cursor_blink_state = true;
                self.ui.panel_needs_redraw = true;
                self.persist_state("changement du réglage d'animation");
            }
            PaletteExecutionResult::ToggleCloseOnFocusLoss => {
                self.config.ui.close_on_focus_loss = !self.config.ui.close_on_focus_loss;
                self.ui.panel_needs_redraw = true;
                self.persist_state("changement de fermeture automatique");
            }
            PaletteExecutionResult::None => {}
        }
    }

    fn toggle_tooling_watcher(&mut self) {
        let target = !self.config.watcher.tooling_enabled;
        if let Some(watcher) = &self.watchers.repo_watcher {
            match watcher.request_tooling_enabled(target) {
                Ok(receiver) => {
                    self.watchers.tooling_ack = Some(receiver);
                    self.ui.pending_tooling_enabled = Some(target);
                }
                Err(error) => {
                    warn!("Bascule de la surveillance d'outillage refusée : {error}");
                    self.ui.last_observation_error = Some(error.to_string());
                }
            }
        } else {
            self.config.watcher.tooling_enabled = target;
            self.ui.last_observation_error = Some(
                "Watcher indisponible ; préférence enregistrée pour le prochain démarrage"
                    .to_owned(),
            );
        }
        self.rebuild_palette_items();
        self.ui.panel_needs_redraw = true;
    }

    fn toggle_focus_tracking(&mut self) {
        if self.config.focus_tracking_enabled {
            self.activity.stop();
            self.pet_state.reset_focus_session();
            self.config.focus_tracking_enabled = false;
        } else {
            match self.activity.start() {
                Ok(()) => self.config.focus_tracking_enabled = true,
                Err(error) => {
                    warn!("Activation du suivi de focus impossible : {error}");
                    self.ui.last_observation_error = Some(error.to_string());
                }
            }
        }
        self.rebuild_palette_items();
        self.ui.panel_needs_redraw = true;
    }

    fn toggle_break_reminders(&mut self) {
        self.config.break_reminders_enabled = !self.config.break_reminders_enabled;
        self.rebuild_palette_items();
        self.ui.panel_needs_redraw = true;
    }

    /// Applique une action de soin et journalise son éventuel refus.
    fn apply_care(
        &mut self,
        label: &str,
        action: fn(&mut PetState, Option<f32>) -> Result<Vec<CoreEvent>, gremlin_core::CoreError>,
    ) {
        match action(&mut self.pet_state, None) {
            Ok(events) => {
                info!("Action « {label} » appliquée au familier");
                self.apply_core_events(&events);
            }
            Err(e) => warn!("Action « {label} » refusée : {e}"),
        }
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
    /// par `GremlinApp::advance_simulation`.
    pub fn pump_events(&mut self) -> Vec<CoreEvent> {
        self.drain_tray_actions();
        self.drain_tooling_ack();
        self.drain_watcher_status();
        self.drain_asset_signals();
        self.drain_dev_signals()
    }

    fn drain_tooling_ack(&mut self) {
        let received = self.watchers.tooling_ack.as_ref().map(|receiver| {
            receiver.try_recv().map_err(|error| match error {
                crossbeam_channel::TryRecvError::Empty => None,
                crossbeam_channel::TryRecvError::Disconnected => Some(
                    "Le worker s'est arrêté avant de confirmer la bascule d'outillage".to_owned(),
                ),
            })
        });
        let ack = match received {
            Some(Ok(ack)) => ack,
            Some(Err(Some(error))) => {
                self.watchers.tooling_ack = None;
                self.ui.pending_tooling_enabled = None;
                self.ui.last_observation_error = Some(error);
                self.rebuild_palette_items();
                self.ui.panel_needs_redraw = true;
                return;
            }
            Some(Err(None)) | None => return,
        };
        self.watchers.tooling_ack = None;
        self.ui.pending_tooling_enabled = None;
        self.config.watcher.tooling_enabled = ack.enabled;
        if let Some(error) = ack.error {
            self.ui.last_observation_error = Some(error);
        }
        self.rebuild_palette_items();
        self.ui.panel_needs_redraw = true;
    }

    /// Remonte les incidents de fiabilité de la surveillance Git.
    ///
    /// Un enregistrement raté ou une perte d'événements signifie que des
    /// commits ne seront pas comptabilisés : le silence n'est pas acceptable.
    fn drain_watcher_status(&mut self) {
        let mut palette_changed = false;
        while let Ok(status) = self.watchers.status_receiver.try_recv() {
            match status {
                WatcherStatus::WatchFailed { path, reason } => {
                    warn!(
                        path = %path.display(),
                        "Surveillance non enregistrée, les commits de ce chemin seront ignorés : {reason}"
                    );
                    self.ui.last_observation_error = Some(format!(
                        "Surveillance impossible pour {} : {reason}",
                        path.display()
                    ));
                    palette_changed = true;
                }
                WatcherStatus::EventsLost { dropped, reason } => {
                    warn!(
                        dropped,
                        "Événements de système de fichiers perdus, resynchronisation : {reason}"
                    );
                    self.ui.last_observation_error =
                        Some(format!("{dropped} événement(s) perdu(s) : {reason}"));
                    palette_changed = true;
                }
                WatcherStatus::ReportRejected { path, reason } => {
                    warn!(path = %path.display(), "Rapport d'outillage refusé : {reason}");
                    self.ui.last_observation_error =
                        Some(format!("Rapport {} refusé : {reason}", path.display()));
                    palette_changed = true;
                }
                WatcherStatus::ToolingStateChanged { enabled, error } => {
                    self.config.watcher.tooling_enabled = enabled;
                    self.ui.pending_tooling_enabled = None;
                    if let Some(reason) = error {
                        self.ui.last_observation_error = Some(reason);
                    }
                    palette_changed = true;
                }
            }
        }
        if palette_changed {
            self.rebuild_palette_items();
            self.ui.panel_needs_redraw = true;
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
                    repo_path,
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
                    let _repository_id = self.watchers.repository_id(&repo_path);
                    self.activity.development_seen = true;

                    if let Some(r) = self
                        .monitored_repos
                        .iter_mut()
                        .find(|r| r.name == repo_name)
                    {
                        r.branch = Some(branch);
                        r.last_commit_msg = message;
                    }

                    self.ui.needs_redraw = true;
                }
                DevSignal::BranchChanged {
                    repo_name,
                    old_branch,
                    new_branch,
                    ..
                } => {
                    info!(repo = %repo_name, from = %old_branch, to = %new_branch, "Bascule de branche");
                    match self.pet_state.pet(Some(2.0)) {
                        Ok(events) => core_events.extend(events),
                        Err(e) => warn!("Récompense de bascule de branche ignorée : {e}"),
                    }

                    if let Some(r) = self
                        .monitored_repos
                        .iter_mut()
                        .find(|r| r.name == repo_name)
                    {
                        r.branch = Some(new_branch);
                    }

                    self.ui.needs_redraw = true;
                }
                DevSignal::RepoDiscovered { repo_name, path } => {
                    let _repository_id = self.watchers.repository_id(&path);
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
                DevSignal::RepoRemoved { repo_name, path } => {
                    self.watchers.remove_repository(&path);
                    self.monitored_repos.retain(|r| r.name != repo_name);
                    self.rebuild_palette_items();
                    self.ui.needs_redraw = true;
                }
                signal @ (DevSignal::TestCompleted { .. } | DevSignal::BuildCompleted { .. }) => {
                    self.handle_tooling_signal(signal, &mut core_events);
                }
            }
        }

        self.apply_core_events(&core_events);
        core_events
    }

    fn handle_tooling_signal(&mut self, signal: DevSignal, core_events: &mut Vec<CoreEvent>) {
        let result = match signal {
            DevSignal::TestCompleted {
                repo_name,
                repo_path,
                report_path,
                run_id,
                summary,
            } => {
                info!(repo = %repo_name, report = %report_path.display(), run_id = ?run_id, "Rapport de tests assimilé");
                let repository_id = self.watchers.repository_id(&repo_path);
                self.pet_state.handle_test_run(
                    repository_id,
                    &repo_name,
                    convert_test_summary(summary),
                )
            }
            DevSignal::BuildCompleted {
                repo_name,
                repo_path,
                report_path,
                run_id,
                summary,
            } => {
                info!(repo = %repo_name, report = %report_path.display(), run_id = %run_id, "Rapport de build assimilé");
                let repository_id = self.watchers.repository_id(&repo_path);
                self.pet_state.handle_build_result(
                    repository_id,
                    &repo_name,
                    convert_build_summary(summary),
                )
            }
            DevSignal::CommitCreated { .. }
            | DevSignal::BranchChanged { .. }
            | DevSignal::RepoDiscovered { .. }
            | DevSignal::RepoRemoved { .. } => return,
        };
        self.activity.development_seen = true;
        match result {
            Ok(events) => core_events.extend(events),
            Err(error) => warn!("Rapport d'outillage ignoré : {error}"),
        }
        self.ui.needs_redraw = true;
        self.ui.panel_needs_redraw = true;
    }

    /// Fait avancer la simulation métier si le pas minimal est écoulé.
    fn advance_simulation(&mut self, now: Instant) -> Vec<CoreEvent> {
        let elapsed = now.duration_since(self.clocks.simulation);
        if elapsed < SIMULATION_TICK_INTERVAL {
            return Vec::new();
        }

        self.clocks.simulation = now;
        if let Some(error) = self
            .activity
            .drain(self.pet_state.config().focus.idle_reset_threshold())
        {
            warn!("Mesure de focus indisponible : {error}");
            self.ui.last_observation_error = Some(error);
            self.rebuild_palette_items();
            self.ui.panel_needs_redraw = true;
        }

        let mut events = self.pet_state.tick(elapsed);
        if self.config.focus_tracking_enabled {
            let development_seen = std::mem::take(&mut self.activity.development_seen);
            let mut focus_events =
                self.pet_state
                    .track_focus(elapsed, self.activity.latest, development_seen);
            if !self.config.break_reminders_enabled {
                focus_events.retain(|event| !matches!(event, CoreEvent::BreakRecommended { .. }));
            }
            events.extend(focus_events);
        }
        self.apply_core_events(&events);
        events
    }

    /// Présente la scène du familier par le chemin disponible.
    ///
    /// La présentation en couches a la priorité : elle seule honore le canal
    /// alpha. Le chemin GPU ne sert que là où elle n'existe pas.
    fn present_companion(&mut self) {
        let scale = self.presentation_scale();

        if let Some(surface) = &mut self.layered {
            if let Err(e) = surface.present(
                self.visuals.pixel_buffer.as_bytes(),
                NATIVE_WIDTH,
                NATIVE_HEIGHT,
                scale,
            ) {
                warn!("Échec de présentation en couches : {e}");
            }
            return;
        }

        if let Some(renderer) = &mut self.renderer {
            if let Err(e) = renderer.render_buffer(&self.visuals.pixel_buffer) {
                warn!("Erreur lors du rendu GPU Pixels : {e}");
            }
        }
    }

    /// Facteur d'agrandissement entier appliqué à la scène du familier.
    ///
    /// Combine la préférence d'échelle de l'utilisateur et la densité de l'écran.
    /// Il doit rester **entier** : c'est la seule mise à l'échelle qui préserve le
    /// pixel-art. Le résultat est borné, la densité venant du système et l'échelle
    /// d'un fichier de configuration éditable à la main.
    fn presentation_scale(&self) -> u32 {
        let density = self
            .window
            .as_ref()
            .map_or(1.0, |window| window.scale_factor());
        let density = if density.is_finite() && density > 0.0 {
            density
        } else {
            1.0
        };

        let combined = f64::from(self.config.scale_factor) * density;
        let rounded = if combined.is_finite() {
            combined.round()
        } else {
            1.0
        };

        (rounded as u32).clamp(1, MAX_PRESENTATION_SCALE)
    }

    /// Compose le panneau de paramètres dans son propre tampon.
    ///
    /// Les emprunts restent disjoints par champ : le tampon vient de
    /// `self.settings`, la scène de `self.visuals` et `self.config`, la liste de
    /// `self.command_palette`.
    fn compose_panel(&mut self) {
        let mood_key = accessory_mood_key(self.pet_state.mood());
        let base_frame_key = self
            .visuals
            .animation_controller
            .current_frame_key()
            .unwrap_or("idle_0");

        let interaction = PanelInteraction {
            cursor_visible: self.ui.cursor_blink_state,
            hovered_item: self.ui.hovered_item,
        };
        let scene = PanelScene {
            wardrobe: &self.config.wardrobe,
            atlas: &self.visuals.sprite_atlas,
            manifest: self.visuals.active_manifest.as_ref(),
            catalog: &self.visuals.accessory_catalog,
            base_frame_key,
            mood_key,
        };

        let theme = self.resolve_theme();
        let Some(panel) = self.settings.as_mut() else {
            return;
        };
        let style = PanelStyle {
            metrics: *panel.metrics(),
            theme,
        };
        RaycastRenderer::render_panel(
            panel.buffer_mut(),
            &style,
            &self.command_palette,
            &scene,
            interaction,
        );
    }

    /// Recompose la scène du familier.
    ///
    /// Ne dépend plus de l'état du panneau : le familier vit sa vie dans sa
    /// fenêtre pendant que les réglages sont ouverts.
    fn compose_frame(&mut self) {
        let mood_key = accessory_mood_key(self.pet_state.mood());

        self.visuals.scene_buffers.current.clear(0, 0, 0, 0);
        if let Some(frame_key) = self.visuals.animation_controller.current_frame_key() {
            LayerCompositor::compose_layered_pet_animated(
                &mut self.visuals.scene_buffers.current,
                &self.config.wardrobe,
                &self.visuals.sprite_atlas,
                self.visuals.active_manifest.as_ref(),
                &self.visuals.accessory_catalog,
                frame_key,
                mood_key,
                self.visuals.scene_elapsed,
            );
        }

        self.visuals.scene_buffers.presented.clear(0, 0, 0, 0);
        let transition = self.visuals.feedback.transition();
        if transition.is_active() {
            TransitionRenderer::blend(
                &self.visuals.scene_buffers.outgoing,
                &self.visuals.scene_buffers.current,
                &mut self.visuals.scene_buffers.presented,
                transition.progress(),
                transition.incoming_offset_y(),
            );
        } else {
            self.visuals
                .scene_buffers
                .presented
                .as_bytes_mut()
                .copy_from_slice(self.visuals.scene_buffers.current.as_bytes());
        }
        self.visuals.scene_buffers.has_presented = true;

        self.visuals.pixel_buffer.clear(0, 0, 0, 0);
        self.visuals.pixel_buffer.blit(
            self.visuals.scene_buffers.presented.as_bytes(),
            NATIVE_WIDTH,
            NATIVE_HEIGHT,
            0,
            0,
        );
        self.visuals
            .feedback
            .render_particles(&mut self.visuals.pixel_buffer);
        if let Some(view) = self.visuals.feedback.dialogue_view() {
            SpeechBubbleRenderer::render(&mut self.visuals.pixel_buffer, view);
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
    ///
    /// # Le panneau ne confisque plus la cadence
    ///
    /// Le panneau imposait auparavant son intervalle de 100 ms et court-circuitait
    /// tout le reste, ce qui se justifiait tant qu'il *remplaçait* la scène du
    /// familier. Maintenant qu'il occupe sa propre fenêtre, ce plancher plafonnait
    /// l'animation du familier à dix images par seconde pendant tout le réglage.
    /// La cadence du familier est donc toujours calculée, et celle du panneau ne
    /// vient que la resserrer.
    ///
    /// Ce resserrement n'a qu'une raison d'être : faire clignoter le curseur de
    /// saisie. Le mode mouvement réduit l'éteint, et le plancher disparaît avec
    /// lui — la boucle n'a plus alors à se réveiller que sur événement.
    fn next_wake_delay(&self) -> Duration {
        let mut delay = if self.ui.is_dragging {
            DRAG_FRAME_INTERVAL
        } else {
            self.visuals
                .animation_controller
                .time_until_next_frame()
                .map_or(IDLE_WAKE_INTERVAL, |wait| wait.max(MIN_FRAME_INTERVAL))
        };
        if let Some(accessory_delay) = self.next_accessory_frame_delay() {
            delay = delay.min(accessory_delay.max(MIN_FRAME_INTERVAL));
        }
        if let Some(feedback_delay) = self.visuals.feedback.next_wake_delay() {
            delay = delay.min(feedback_delay.max(MIN_FRAME_INTERVAL));
        }
        if self.ui.is_palette_open && !self.config.ui.reduced_motion {
            delay = delay.min(PALETTE_FRAME_INTERVAL);
        }
        delay
    }

    fn next_accessory_frame_delay(&self) -> Option<Duration> {
        self.config
            .wardrobe
            .equipped_slots()
            .filter_map(|(_, accessory_id)| {
                self.visuals
                    .accessory_catalog
                    .get(accessory_id)?
                    .manifest
                    .time_until_next_frame(self.visuals.scene_elapsed)
            })
            .min()
    }

    fn accessory_frame_changed(&self, before: Duration, after: Duration) -> bool {
        self.config
            .wardrobe
            .equipped_slots()
            .filter_map(|(_, accessory_id)| self.visuals.accessory_catalog.get(accessory_id))
            .any(|item| item.manifest.frame_key_at(before) != item.manifest.frame_key_at(after))
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
            let never_dev = crossbeam_channel::never();
            let never_asset = crossbeam_channel::never();

            while dev_open || asset_open {
                let dev_receiver = if dev_open { &raw_dev } else { &never_dev };
                let asset_receiver = if asset_open { &raw_asset } else { &never_asset };
                crossbeam_channel::select! {
                    recv(dev_receiver) -> msg => match msg {
                        Ok(signal) => {
                            match dev_out.try_send(signal) {
                                Ok(()) | Err(crossbeam_channel::TrySendError::Full(_)) => {}
                                Err(crossbeam_channel::TrySendError::Disconnected(_)) => break,
                            }
                        }
                        Err(_) => dev_open = false,
                    },
                    recv(asset_receiver) -> msg => match msg {
                        Ok(signal) => {
                            match asset_out.try_send(signal) {
                                Ok(()) | Err(crossbeam_channel::TrySendError::Full(_)) => {}
                                Err(crossbeam_channel::TrySendError::Disconnected(_)) => break,
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
            visible: true,
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

                // La présentation en couches est tentée d'abord : là où elle
                // fonctionne, elle est la seule à honorer le canal alpha, et elle
                // évite au passage tout contexte graphique pour le familier.
                match LayeredSurface::new(&arc_window) {
                    Ok(surface) => {
                        self.layered = Some(surface);
                        info!("Présentation en couches active : transparence par pixel");
                    }
                    Err(e) => {
                        // Attendu hors Windows : le chemin GPU y suffit.
                        debug!("Présentation en couches indisponible, repli GPU : {e}");
                        match AppRenderer::new(arc_window.clone(), NATIVE_WIDTH, NATIVE_HEIGHT) {
                            Ok(renderer) => {
                                self.renderer = Some(renderer);
                                info!("Surface GPU Pixels initialisée avec succès");
                            }
                            Err(e) => {
                                warn!("Échec d'initialisation du renderer Pixels GPU : {e}");
                            }
                        }
                    }
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
            #[cfg(feature = "a11y")]
            CustomAppEvent::Accessibility(event) => self.handle_accessibility_event(event),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Deux fenêtres coexistent désormais : l'identifiant, jusqu'ici ignoré,
        // décide de la destination. Sans ce routage, une frappe destinée au
        // panneau déplacerait le familier, et une fermeture du panneau
        // arrêterait l'application.
        if self
            .settings
            .as_ref()
            .is_some_and(|panel| panel.id() == window_id)
        {
            self.handle_panel_event(event);
            return;
        }

        self.handle_pet_event(event_loop, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let frame_delta = now.duration_since(self.clocks.frame);
        self.clocks.frame = now;

        let previous_scene_elapsed = self.visuals.scene_elapsed;
        self.visuals.scene_elapsed = self.visuals.scene_elapsed.saturating_add(frame_delta);
        if !self.ui.is_palette_open
            && self.accessory_frame_changed(previous_scene_elapsed, self.visuals.scene_elapsed)
        {
            self.ui.needs_redraw = true;
        }

        if !self.ui.is_palette_open && self.visuals.animation_controller.update(frame_delta) {
            self.ui.needs_redraw = true;
        }

        if self.visuals.feedback.update(frame_delta) && !self.ui.is_palette_open {
            self.ui.needs_redraw = true;
        }

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

        // Mouvement réduit : le curseur reste allumé plutôt que de clignoter, ce
        // qui supprime la seule animation permanente du panneau.
        if self.ui.is_palette_open
            && !self.config.ui.reduced_motion
            && now.duration_since(self.clocks.cursor_blink) >= CURSOR_BLINK_INTERVAL
        {
            self.clocks.cursor_blink = now;
            self.ui.cursor_blink_state = !self.ui.cursor_blink_state;
            self.ui.panel_needs_redraw = true;
        }

        // Seul endroit disposant de la boucle d'événements : la fenêtre de
        // paramètres y est créée, affichée ou masquée selon l'état.
        self.reconcile_settings_window(event_loop);

        if self.ui.needs_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }

        if self.ui.panel_needs_redraw {
            if let Some(panel) = &self.settings {
                panel.window().request_redraw();
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
        assert!(app.activity.monitor.is_none());
        assert!(app
            .ui
            .last_observation_error
            .as_deref()
            .is_some_and(|message| message.contains("intégration système désactivée")));
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
    fn test_opening_the_panel_leaves_the_companion_window_intact() {
        // Le panneau occupe désormais sa propre fenêtre. La fenêtre du familier
        // ne doit donc plus être métamorphosée : elle gardait auparavant les
        // dimensions du panneau, ce qui faisait disparaître le familier pendant
        // tout le réglage et rendait la fenêtre immobilisable.
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
        assert_eq!(
            app.visuals.pixel_buffer.width(),
            NATIVE_WIDTH,
            "la fenêtre du familier a été redimensionnée par l'ouverture du panneau"
        );
        assert_eq!(app.visuals.pixel_buffer.height(), NATIVE_HEIGHT);
        assert!(
            app.ui.panel_needs_redraw,
            "l'ouverture doit demander une composition du panneau"
        );

        // Le click-through n'a plus à être suspendu : la saisie arrive sur une
        // fenêtre distincte, qui n'est pas traversante.
        assert!(app.config.click_through_enabled);

        // Le familier continue de se composer pendant que le panneau est ouvert.
        app.compose_frame();
        assert!(app.visuals.pixel_buffer.as_bytes().iter().any(|&b| b > 0));

        app.config
            .wardrobe
            .equip(AccessoryCategory::Hat, "wizard_hat");
        assert!(app.config.wardrobe.is_equipped("wizard_hat"));

        app.toggle_palette();
        assert!(!app.is_palette_open());
        assert_eq!(app.visuals.pixel_buffer.width(), NATIVE_WIDTH);
    }

    #[test]
    fn test_mouse_row_targeting_agrees_with_the_rendered_scroll() {
        // Le clic doit activer exactement la ligne dessinée : le rendu et le
        // pointage partagent `scroll_offset`, et ce test le verrouille.
        let env = TempEnv::new("survol");
        let mut app = headless_app(&env, AppConfig::default());
        app.toggle_palette();

        // On descend dans la garde-robe : la racine n'énumère que cinq groupes,
        // trop peu pour déborder de la fenêtre visible.
        app.command_palette
            .enter_group(crate::ui::PaletteGroup::Wardrobe);

        let metrics = crate::ui::UiMetrics::for_display(1.0, TextSize::Normal);
        let visible = metrics.visible_rows();
        let total = app.command_palette.filtered_len();
        assert!(
            total > visible,
            "liste trop courte pour exercer le défilement"
        );

        // Sélection poussée au-delà de la fenêtre visible.
        for _ in 0..visible + 2 {
            app.command_palette.select_next();
        }

        let scroll = app.command_palette.scroll_offset(visible);
        for row in 0..visible {
            assert_eq!(
                app.command_palette.item_at_visible_row(row, visible),
                Some(scroll + row),
                "ligne visible {row} désalignée avec le défilement"
            );
        }

        // Hors liste, aucun item ciblé.
        assert_eq!(
            app.command_palette
                .item_at_visible_row(visible + 50, visible),
            None
        );
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
        assert_eq!(
            app.visuals.feedback.active_dialogue(),
            Some(crate::dialogue::DialogueId::Commit)
        );
        assert!(app.visuals.feedback.active_particle_count() > 0);
    }

    #[test]
    fn test_test_report_reaches_core_and_visual_feedback() {
        let env = TempEnv::new("test-report");
        let mut app = headless_app(&env, AppConfig::default());
        app.dev_sender()
            .send(DevSignal::TestCompleted {
                repo_name: "gremlin".into(),
                repo_path: env.root.clone(),
                report_path: env.root.join("junit.xml"),
                run_id: Some("run-1".into()),
                summary: ParsedTestReport {
                    framework: ReportFramework::Rust,
                    passed: 12,
                    failed: 0,
                    skipped: 1,
                    duration: Duration::from_secs(2),
                },
            })
            .expect("envoi du rapport");

        let events = app.pump_events();
        assert!(events.iter().any(
            |event| matches!(event, CoreEvent::TestRunReceived { xp_gained, .. } if *xp_gained > 0)
        ));
        assert_eq!(app.pet_state.progression().total_tests_passed(), 12);
        assert_eq!(
            app.visuals.feedback.active_dialogue(),
            Some(crate::dialogue::DialogueId::TestsPassed)
        );
    }

    #[test]
    fn test_build_report_reaches_core_and_visual_feedback() {
        let env = TempEnv::new("build-report");
        let mut app = headless_app(&env, AppConfig::default());
        app.dev_sender()
            .send(DevSignal::BuildCompleted {
                repo_name: "gremlin".into(),
                repo_path: env.root.clone(),
                report_path: env.root.join(".gremlin/results/build.json"),
                run_id: "build-1".into(),
                summary: ParsedBuildReport {
                    tool: ReportBuildTool::Cargo,
                    success: false,
                    duration: Duration::from_secs(3),
                },
            })
            .expect("envoi du rapport");

        let events = app.pump_events();
        assert!(events.iter().any(|event| matches!(
            event,
            CoreEvent::BuildCompleted { summary, .. } if !summary.success()
        )));
        assert_eq!(
            app.visuals.feedback.active_dialogue(),
            Some(crate::dialogue::DialogueId::BuildFailed)
        );
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
    fn test_composed_pet_frame_keeps_its_transparency() {
        // La fenêtre du familier est transparente : tout pixel que le sprite ne
        // couvre pas doit rester à alpha zéro dans le tampon. Un fond opaque ici
        // se traduirait par un carré visible sur le bureau.
        let env = TempEnv::new("alpha");
        let mut app = headless_app(&env, AppConfig::default());
        app.load_skin("default");
        app.compose_frame();

        let bytes = app.visuals.pixel_buffer.as_bytes();
        let width = app.visuals.pixel_buffer.width() as usize;
        let height = app.visuals.pixel_buffer.height() as usize;

        // Les quatre coins sont hors silhouette dans tous les sprites livrés.
        for (x, y) in [
            (0, 0),
            (width - 1, 0),
            (0, height - 1),
            (width - 1, height - 1),
        ] {
            let alpha = bytes[(y * width + x) * 4 + 3];
            assert_eq!(
                alpha, 0,
                "le coin ({x}, {y}) du tampon est opaque : la fenêtre montrera un fond"
            );
        }

        // Et le sprite ne doit pas couvrir toute la toile : sans pixels
        // transparents du tout, le test précédent ne prouverait rien.
        let transparent = bytes.chunks_exact(4).filter(|px| px[3] == 0).count();
        let opaque = bytes.chunks_exact(4).filter(|px| px[3] > 0).count();
        assert!(
            transparent > 0 && opaque > 0,
            "toile entièrement uniforme : {transparent} transparents, {opaque} opaques"
        );
    }

    #[test]
    fn test_wake_delay_matches_the_documented_pacing() {
        let env = TempEnv::new("pacing");
        let mut app = headless_app(&env, AppConfig::default());

        app.ui.is_dragging = true;
        assert_eq!(app.next_wake_delay(), DRAG_FRAME_INTERVAL);

        // Panneau ouvert : sa cadence resserre celle du familier sans la
        // remplacer. Au repos, le familier demanderait une seconde entière ; le
        // curseur de saisie exige davantage.
        app.ui.is_dragging = false;
        app.ui.is_palette_open = true;
        assert_eq!(app.next_wake_delay(), PALETTE_FRAME_INTERVAL);

        // Mouvement réduit : plus de curseur clignotant, donc plus de plancher.
        // La boucle retombe sur le seul besoin du familier.
        app.config.ui.reduced_motion = true;
        assert!(
            app.next_wake_delay() > PALETTE_FRAME_INTERVAL,
            "le mouvement réduit doit relâcher la cadence du panneau"
        );
        app.config.ui.reduced_motion = false;

        // Le familier garde sa cadence d'animation même panneau ouvert : sa
        // scène n'est plus remplacée, elle continue de vivre à côté.
        app.visuals.scene_elapsed = Duration::from_millis(225);
        app.config
            .wardrobe
            .equip(AccessoryCategory::Hat, "wizard_hat");
        assert_eq!(
            app.next_wake_delay(),
            Duration::from_millis(25),
            "le panneau ne doit pas plafonner l'animation du familier"
        );
        app.config.wardrobe.unequip(AccessoryCategory::Hat);
        app.visuals.scene_elapsed = Duration::ZERO;

        app.ui.is_palette_open = false;
        assert!(app.next_wake_delay() >= MIN_FRAME_INTERVAL);
        assert!(app.next_wake_delay() <= IDLE_WAKE_INTERVAL);
    }

    #[test]
    fn test_animated_accessory_sets_an_exact_wake_deadline() {
        let env = TempEnv::new("animated-accessory");
        let mut app = headless_app(&env, AppConfig::default());
        app.visuals.animation_controller = AnimationController::new();
        app.visuals.scene_elapsed = Duration::from_millis(225);
        app.config
            .wardrobe
            .equip(AccessoryCategory::Hat, "wizard_hat");

        assert_eq!(app.next_wake_delay(), Duration::from_millis(25));
    }

    #[test]
    fn test_skin_effect_anchors_are_consumed_by_the_feedback_adapter() {
        let env = TempEnv::new("skin-anchors");
        let mut app = headless_app(&env, AppConfig::default());

        app.load_skin("baby");

        assert_eq!(
            app.visuals.feedback.anchors(),
            VisualAnchors {
                head: (32, 18),
                effect: (32, 31),
            }
        );
    }

    #[test]
    fn test_sprite_keys_cannot_escape_their_mod_directory() {
        assert!(is_safe_sprite_key("wizard_hat_0"));
        assert!(!is_safe_sprite_key(""));
        assert!(!is_safe_sprite_key(".."));
        assert!(!is_safe_sprite_key("../outside"));
        assert!(!is_safe_sprite_key("..\\outside"));
        assert!(!is_safe_sprite_key("frame.png"));
    }
}
