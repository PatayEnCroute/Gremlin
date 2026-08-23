//! Gestionnaire de la sauvegarde résiliente et du rattrapage hors-ligne.
//!
//! Règle de sûreté centrale : **une sauvegarde existante n'est jamais écrasée
//! sans avoir été lue ou mise de côté**. Une erreur d'entrée/sortie et une
//! absence de sauvegarde sont donc deux résultats distincts — les confondre
//! faisait démarrer un nouveau familier dont la première sauvegarde
//! automatique détruisait la progression réelle du joueur.

use crate::config::AppConfig;
use crate::error::AppError;
use gremlin_core::{CoreEvent, PetState};
use gremlin_system::{AppPaths, AtomicStorage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};

/// Version courante du schéma de l'enveloppe de sauvegarde.
pub const SAVE_ENVELOPE_VERSION: u32 = 1;

/// Horodatage UNIX courant en secondes, ou 0 si l'horloge est antérieure à l'époque.
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Données complètes de sauvegarde de Gremlin et de ses préférences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetSaveData {
    /// Version du schéma de sauvegarde pour compatibilité ascendante.
    pub version: u32,
    /// Horodatage UNIX (en secondes) de la dernière sauvegarde.
    pub last_saved_at: u64,
    /// État complet du familier (stats, humeur, XP, progression).
    pub pet_state: PetState,
    /// Configuration persistante de l'application.
    pub config: AppConfig,
}

impl Default for PetSaveData {
    fn default() -> Self {
        Self {
            version: SAVE_ENVELOPE_VERSION,
            last_saved_at: 0,
            pet_state: PetState::default(),
            config: AppConfig::default(),
        }
    }
}

impl PetSaveData {
    /// Construit une nouvelle structure de sauvegarde à l'instant présent.
    #[must_use]
    pub fn new(pet_state: PetState, config: AppConfig) -> Self {
        Self {
            version: SAVE_ENVELOPE_VERSION,
            last_saved_at: now_unix_secs(),
            pet_state,
            config,
        }
    }

    /// Restaure les invariants de l'état et de la configuration chargés.
    ///
    /// Renvoie `true` si une correction a été nécessaire.
    pub fn normalize(&mut self) -> bool {
        self.version = SAVE_ENVELOPE_VERSION;
        self.pet_state.normalize();
        let config_repaired = self.config.normalize();

        // Un horodatage postérieur à l'heure courante (horloge reculée, fichier
        // bricolé) doit être ramené à « maintenant » : sinon le rattrapage
        // hors-ligne travaille sur un delta négatif ou aberrant.
        let now = now_unix_secs();
        let timestamp_repaired = self.last_saved_at > now;
        if timestamp_repaired {
            self.last_saved_at = now;
        }

        config_repaired || timestamp_repaired
    }
}

/// Résultat de la tentative de chargement d'une sauvegarde.
#[derive(Debug)]
pub enum LoadOutcome {
    /// Une sauvegarde valide a été chargée.
    Loaded(Box<PetSaveData>),
    /// Aucune sauvegarde n'existe : premier lancement.
    Fresh,
    /// La sauvegarde était illisible mais a été mise de côté sous `backup`.
    ///
    /// Démarrer un nouveau familier est sûr : l'original est préservé.
    Recovered {
        /// Chemin du fichier de sauvegarde mis de côté.
        backup: PathBuf,
    },
}

/// Gestionnaire de persistance et de cycle de vie sur disque.
pub struct PersistenceManager;

impl PersistenceManager {
    /// Sauvegarde l'état du familier et la configuration de manière atomique sur le disque.
    ///
    /// # Errors
    /// Renvoie `AppError` en cas d'échec de sérialisation JSON ou d'écriture disque.
    pub fn save(
        paths: &AppPaths,
        pet_state: &PetState,
        config: &AppConfig,
    ) -> Result<(), AppError> {
        let save_data = PetSaveData::new(pet_state.clone(), config.clone());
        let json_str = serde_json::to_string_pretty(&save_data)?;

        let save_path = paths.save_file();
        AtomicStorage::write_atomic(&save_path, json_str.as_bytes()).map_err(AppError::System)?;

        info!(path = %save_path.display(), "Sauvegarde atomique effectuée avec succès");
        Ok(())
    }

    /// Charge l'état sauvegardé depuis le disque.
    ///
    /// Une sauvegarde présente mais illisible est déplacée vers un fichier
    /// horodaté avant que l'appelant ne reparte d'un état neuf.
    ///
    /// # Errors
    /// Renvoie `AppError` uniquement si le fichier existe et ne peut pas être
    /// **lu** (verrou antivirus, permissions, disque défaillant). L'appelant ne
    /// doit alors surtout pas écraser la sauvegarde.
    pub fn load(paths: &AppPaths) -> Result<LoadOutcome, AppError> {
        let save_path = paths.save_file();
        if !save_path.exists() {
            return Ok(LoadOutcome::Fresh);
        }

        let content = match AtomicStorage::read_to_string(&save_path) {
            Ok(content) => content,
            Err(e) => {
                error!(
                    path = %save_path.display(),
                    "Sauvegarde illisible : l'application refuse de l'écraser ({e})"
                );
                return Err(AppError::System(e));
            }
        };

        match serde_json::from_str::<PetSaveData>(&content) {
            Ok(mut data) => {
                if data.normalize() {
                    warn!("Valeurs hors bornes détectées dans la sauvegarde : corrigées");
                }
                info!(
                    path = %save_path.display(),
                    level = data.pet_state.progression().level(),
                    xp = data.pet_state.progression().total_xp(),
                    "Sauvegarde Gremlin chargée avec succès"
                );
                Ok(LoadOutcome::Loaded(Box::new(data)))
            }
            Err(e) => {
                let backup = Self::quarantine_corrupt_save(&save_path)?;
                warn!(
                    path = %save_path.display(),
                    backup = %backup.display(),
                    "Sauvegarde corrompue mise de côté, démarrage d'un nouveau familier : {e}"
                );
                Ok(LoadOutcome::Recovered { backup })
            }
        }
    }

    /// Déplace une sauvegarde corrompue vers un fichier horodaté.
    ///
    /// # Errors
    /// Renvoie `AppError` si la mise de côté échoue : sans elle, le prochain
    /// enregistrement automatique détruirait définitivement le fichier.
    fn quarantine_corrupt_save(save_path: &Path) -> Result<PathBuf, AppError> {
        let stem = save_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("save.json");
        let backup = save_path.with_file_name(format!("{stem}.corrupt-{}", now_unix_secs()));

        std::fs::rename(save_path, &backup)?;
        Ok(backup)
    }

    /// Applique la simulation temporelle hors-ligne sur l'état chargé.
    pub fn apply_offline_catchup(save_data: &mut PetSaveData) -> Vec<CoreEvent> {
        if !save_data.config.offline_catchup_enabled {
            return Vec::new();
        }

        let now_secs = now_unix_secs();
        if now_secs <= save_data.last_saved_at {
            return Vec::new();
        }

        let elapsed_secs = now_secs.saturating_sub(save_data.last_saved_at);
        let max_secs = u64::from(save_data.config.max_offline_catchup_hours).saturating_mul(3600);
        let clamped_secs = elapsed_secs.min(max_secs);

        info!(
            elapsed_hours = elapsed_secs / 3600,
            simulated_hours = clamped_secs / 3600,
            "Rattrapage hors-ligne en cours..."
        );

        let events = save_data.pet_state.tick(Duration::from_secs(clamped_secs));

        info!(
            mood = ?save_data.pet_state.mood(),
            alive = save_data.pet_state.is_alive(),
            "Simulation hors-ligne terminée"
        );

        events
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Répertoire temporaire unique nettoyé à la destruction.
    struct TempPaths {
        root: PathBuf,
        paths: AppPaths,
    }

    impl TempPaths {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let unique = format!(
                "gremlin-persist-{}-{label}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let root = std::env::temp_dir().join(unique);
            let config = root.join("config");
            let data = root.join("data");
            let cache = root.join("cache");
            for dir in [&config, &data, &cache] {
                std::fs::create_dir_all(dir).expect("création du répertoire de test");
            }

            Self {
                paths: AppPaths::from_dirs(config, data, cache),
                root,
            }
        }
    }

    impl Drop for TempPaths {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn test_save_data_serialization_roundtrip() {
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();
        let save_data = PetSaveData::new(pet, config);

        let json = serde_json::to_string_pretty(&save_data).expect("Sérialisation OK");
        let deserialized: PetSaveData = serde_json::from_str(&json).expect("Désérialisation OK");

        assert_eq!(save_data, deserialized);
    }

    #[test]
    fn test_disk_roundtrip_preserves_state() {
        let tmp = TempPaths::new("roundtrip");
        let mut pet = PetState::new("Gizmo");
        pet.handle_commit("repo", "main").expect("commit accepté");
        let config = AppConfig {
            scale_factor: 4,
            active_skin: String::from("neon"),
            ..AppConfig::default()
        };

        PersistenceManager::save(&tmp.paths, &pet, &config).expect("écriture disque");

        let outcome = PersistenceManager::load(&tmp.paths).expect("lecture disque");
        let LoadOutcome::Loaded(data) = outcome else {
            panic!("la sauvegarde écrite doit être rechargée");
        };

        assert_eq!(data.pet_state, pet);
        assert_eq!(data.config.scale_factor, 4);
        assert_eq!(data.config.active_skin, "neon");
        assert_eq!(data.version, SAVE_ENVELOPE_VERSION);
    }

    #[test]
    fn test_missing_save_reports_fresh_start() {
        let tmp = TempPaths::new("fresh");
        assert!(matches!(
            PersistenceManager::load(&tmp.paths).expect("pas d'erreur"),
            LoadOutcome::Fresh
        ));
    }

    #[test]
    fn test_corrupt_save_is_quarantined_not_destroyed() {
        let tmp = TempPaths::new("corrupt");
        let save_path = tmp.paths.save_file();
        if let Some(parent) = save_path.parent() {
            std::fs::create_dir_all(parent).expect("répertoire parent");
        }
        std::fs::write(&save_path, b"{ ceci n'est pas du JSON").expect("écriture du fichier abîmé");

        let outcome = PersistenceManager::load(&tmp.paths).expect("corruption gérée sans erreur");
        let LoadOutcome::Recovered { backup } = outcome else {
            panic!("une sauvegarde corrompue doit être mise en quarantaine");
        };

        assert!(backup.exists(), "la copie de secours doit exister");
        assert!(
            !save_path.exists(),
            "le fichier corrompu doit avoir été déplacé"
        );
        let preserved = std::fs::read_to_string(&backup).expect("relecture de la copie");
        assert!(preserved.contains("ceci n'est pas du JSON"));
    }

    #[test]
    fn test_hostile_values_are_normalized_on_load() {
        let tmp = TempPaths::new("hostile");
        let save_path = tmp.paths.save_file();
        if let Some(parent) = save_path.parent() {
            std::fs::create_dir_all(parent).expect("répertoire parent");
        }
        std::fs::write(
            &save_path,
            br#"{"version": 1, "last_saved_at": 0,
                 "config": { "scale_factor": 0, "auto_save_interval_secs": 0,
                             "max_offline_catchup_hours": 0, "active_skin": "../../etc" }}"#,
        )
        .expect("écriture");

        let LoadOutcome::Loaded(data) = PersistenceManager::load(&tmp.paths).expect("chargement")
        else {
            panic!("la sauvegarde doit être chargée après réparation");
        };

        assert_eq!(data.config.scale_factor, AppConfig::MIN_SCALE_FACTOR);
        assert!(data.config.auto_save_interval_secs >= AppConfig::MIN_AUTO_SAVE_INTERVAL_SECS);
        assert!(data.config.max_offline_catchup_hours >= 1);
        assert!(!data.config.active_skin.contains(".."));
    }

    #[test]
    fn test_future_timestamp_is_clamped() {
        let mut data = PetSaveData::new(PetState::new("Gizmo"), AppConfig::default());
        data.last_saved_at = u64::MAX;
        assert!(data.normalize());
        assert!(data.last_saved_at <= now_unix_secs());

        // Aucun rattrapage aberrant ne doit en découler.
        let events = PersistenceManager::apply_offline_catchup(&mut data);
        assert!(events.is_empty());
    }

    #[test]
    fn test_offline_catchup_simulation() {
        let mut pet = PetState::new("Gizmo");
        pet.sleep().expect("mise en sommeil");
        let config = AppConfig {
            offline_catchup_enabled: true,
            max_offline_catchup_hours: 24,
            ..AppConfig::default()
        };

        let past_time = now_unix_secs().saturating_sub(3600 * 4);
        let mut save_data = PetSaveData {
            version: SAVE_ENVELOPE_VERSION,
            last_saved_at: past_time,
            pet_state: pet,
            config,
        };

        let _events = PersistenceManager::apply_offline_catchup(&mut save_data);
        // Endormi pendant 4 heures : un peu d'énergie perdue, toujours vivant.
        assert!(save_data.pet_state.is_alive());
        assert!(save_data.pet_state.stats().satiety() < 100.0);
    }

    #[test]
    fn test_offline_catchup_respects_the_configured_ceiling() {
        let build = |hours: u32| {
            let mut data = PetSaveData::new(
                PetState::new("Gizmo"),
                AppConfig {
                    offline_catchup_enabled: true,
                    max_offline_catchup_hours: hours,
                    ..AppConfig::default()
                },
            );
            // Absence prolongée : dix ans.
            data.last_saved_at = now_unix_secs().saturating_sub(10 * 365 * 24 * 3600);
            data
        };

        let mut one_hour = build(1);
        PersistenceManager::apply_offline_catchup(&mut one_hour);
        assert!(
            one_hour.pet_state.is_alive(),
            "un plafond d'une heure ne peut pas tuer le familier"
        );

        let mut long = build(720);
        PersistenceManager::apply_offline_catchup(&mut long);
        assert!(!long.pet_state.is_alive());
    }

    #[test]
    fn test_catchup_disabled_leaves_state_untouched() {
        let mut data = PetSaveData::new(
            PetState::new("Gizmo"),
            AppConfig {
                offline_catchup_enabled: false,
                ..AppConfig::default()
            },
        );
        data.last_saved_at = now_unix_secs().saturating_sub(48 * 3600);
        let before = data.pet_state.clone();

        assert!(PersistenceManager::apply_offline_catchup(&mut data).is_empty());
        assert_eq!(before, data.pet_state);
    }
}
