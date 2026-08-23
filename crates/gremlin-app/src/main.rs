//! # Gremlin — Le compagnon de bureau pour développeurs
//!
//! Point d'entrée de l'exécutable natif. Volontairement mince : toute la
//! logique réside dans la bibliothèque [`gremlin_app`], ce qui la rend
//! testable.

use gremlin_app::app::{AppOptions, CustomAppEvent, GremlinApp};
use gremlin_app::persistence::{LoadOutcome, PersistenceManager, PetSaveData};
use gremlin_app::AppConfig;
use gremlin_core::PetState;
use gremlin_system::AppPaths;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use winit::event_loop::EventLoop;

/// Code de sortie utilisé lorsqu'une sauvegarde existante est illisible.
const EXIT_UNREADABLE_SAVE: i32 = 2;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Démarrage de Gremlin v{}", env!("CARGO_PKG_VERSION"));

    let paths = AppPaths::new()?;
    if let Err(e) = paths.ensure_directories_exist() {
        warn!("Impossible de préparer les répertoires applicatifs : {e}");
    }

    // Une sauvegarde illisible n'est jamais confondue avec une absence de
    // sauvegarde : démarrer un nouveau familier déclencherait un enregistrement
    // automatique qui détruirait la progression du joueur.
    let save_data = match PersistenceManager::load(&paths) {
        Ok(LoadOutcome::Loaded(data)) => Some(*data),
        Ok(LoadOutcome::Fresh) => {
            info!("Aucune sauvegarde antérieure : création d'un nouveau Gremlin");
            None
        }
        Ok(LoadOutcome::Recovered { backup }) => {
            warn!(
                backup = %backup.display(),
                "Sauvegarde illisible mise de côté : un nouveau Gremlin démarre"
            );
            None
        }
        Err(e) => {
            error!(
                path = %paths.save_file().display(),
                "Sauvegarde existante illisible : arrêt pour ne pas l'écraser ({e})"
            );
            error!("Vérifiez les droits d'accès au fichier, ou déplacez-le pour repartir à neuf.");
            std::process::exit(EXIT_UNREADABLE_SAVE);
        }
    };

    let (pet_state, config) = save_data.map_or_else(
        || (PetState::new("Gremlin"), AppConfig::default()),
        |mut data| {
            let events = PersistenceManager::apply_offline_catchup(&mut data);
            if !events.is_empty() {
                info!(
                    events_count = events.len(),
                    "Événements de simulation hors-ligne appliqués"
                );
            }
            let PetSaveData {
                pet_state, config, ..
            } = data;
            (pet_state, config)
        },
    );

    // Le proxy est créé avant l'application : il permet aux surveillants
    // d'arrière-plan de réveiller la boucle dès qu'un signal arrive, au lieu
    // d'attendre le prochain réveil programmé.
    let event_loop = EventLoop::<CustomAppEvent>::with_user_event().build()?;
    let options = AppOptions {
        paths: Some(paths),
        wake_proxy: Some(event_loop.create_proxy()),
        ..AppOptions::default()
    };

    let mut app = GremlinApp::with_options(pet_state, config, options)?;
    event_loop.run_app(&mut app)?;

    // Sauvegarde finale à la fermeture.
    if let Err(e) = PersistenceManager::save(app.paths(), app.pet_state(), app.config()) {
        error!("Échec de la sauvegarde finale à l'extinction : {e}");
    }

    info!("Arrêt propre de Gremlin.");
    Ok(())
}
