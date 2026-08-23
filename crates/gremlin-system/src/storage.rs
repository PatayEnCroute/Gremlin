//! Gestion de la persistance atomique sécurisée sur disque.
//!
//! Le protocole d'écriture est le classique *write-temp-then-rename* :
//! 1. écriture intégrale dans un fichier temporaire adjacent (même système de fichiers) ;
//! 2. `sync_all()` sur ce fichier pour forcer le vidage des tampons ;
//! 3. renommage (remplacement atomique) vers la cible ;
//! 4. `fsync` du répertoire parent (POSIX) pour rendre l'entrée de répertoire durable.
//!
//! Aucune étape ne supprime jamais la cible : un crash à n'importe quel instant
//! laisse soit l'ancienne version intacte, soit la nouvelle version complète.

use crate::error::SystemError;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tracing::{debug, warn};

/// Compteur global garantissant l'unicité des noms de fichiers temporaires
/// entre les threads d'un même processus.
///
/// Le PID seul ne suffit pas : deux threads sauvegardant la même cible
/// partageraient le même nom temporaire et se tronqueraient mutuellement.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Nombre maximal de tentatives de renommage avant abandon.
const RENAME_MAX_ATTEMPTS: u32 = 5;

/// Délai initial entre deux tentatives de renommage (doublé à chaque essai).
const RENAME_INITIAL_BACKOFF: Duration = Duration::from_millis(10);

/// Garde RAII supprimant le fichier temporaire tant que l'écriture n'a pas abouti.
///
/// Évite de laisser traîner des `.save.json.tmp.*` orphelins quand une étape
/// intermédiaire échoue.
struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Désarme la garde : le fichier temporaire a été consommé par le renommage.
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            if let Err(e) = fs::remove_file(&self.path) {
                debug!(
                    path = %self.path.display(),
                    error = %e,
                    "Nettoyage du fichier temporaire impossible"
                );
            }
        }
    }
}

/// Utilitaire de stockage atomique garantissant l'intégrité des fichiers en cas de crash.
pub struct AtomicStorage;

impl AtomicStorage {
    /// Écrit des données de manière atomique dans le fichier cible.
    ///
    /// Sur Unix le fichier est créé en `0600` (lecture/écriture propriétaire uniquement) :
    /// une sauvegarde peut contenir des chemins de dépôts privés.
    ///
    /// # Errors
    /// Renvoie `SystemError::Io` si la création du répertoire parent, l'écriture,
    /// la synchronisation ou le renommage échouent après épuisement des tentatives.
    pub fn write_atomic(path: &Path, content: &[u8]) -> Result<(), SystemError> {
        let parent = Self::parent_dir(path);
        if let Some(parent) = parent {
            fs::create_dir_all(parent)?;
        }

        let guard = TempFileGuard::new(Self::temp_path_for(path));

        // 1. Écriture complète puis synchronisation du fichier temporaire.
        {
            let mut file = Self::create_private_file(guard.path())?;
            file.write_all(content)?;
            file.sync_all()?;
        }

        // 2. Remplacement atomique de la cible. `fs::rename` s'appuie sur
        //    `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` sous Windows et sur
        //    `rename(2)` sous POSIX : les deux écrasent la cible existante.
        //    Un échec traduit presque toujours un verrou temporaire (antivirus,
        //    indexeur, autre processus) : on réessaie plutôt que de supprimer
        //    la cible, ce qui détruirait la garantie d'atomicité.
        Self::rename_with_retry(guard.path(), path)?;
        guard.disarm();

        // 3. Durabilité de l'entrée de répertoire (POSIX).
        if let Some(parent) = parent {
            Self::sync_directory(parent);
        }

        debug!(path = %path.display(), "Écriture atomique terminée avec succès");
        Ok(())
    }

    /// Lit le contenu d'un fichier sous forme de chaîne UTF-8.
    ///
    /// # Errors
    /// Renvoie `SystemError::Io` si le fichier n'existe pas ou ne peut être lu.
    pub fn read_to_string(path: &Path) -> Result<String, SystemError> {
        fs::read_to_string(path).map_err(SystemError::Io)
    }

    /// Lit le contenu binaire d'un fichier.
    ///
    /// # Errors
    /// Renvoie `SystemError::Io` en cas d'erreur de lecture.
    pub fn read_bytes(path: &Path) -> Result<Vec<u8>, SystemError> {
        fs::read(path).map_err(SystemError::Io)
    }

    /// Répertoire parent exploitable de la cible (`None` pour un chemin nu).
    fn parent_dir(path: &Path) -> Option<&Path> {
        path.parent().filter(|p| !p.as_os_str().is_empty())
    }

    /// Construit un nom de fichier temporaire unique pour la cible donnée.
    ///
    /// L'unicité combine le PID (isolation inter-processus) et un compteur
    /// atomique global (isolation inter-threads).
    fn temp_path_for(path: &Path) -> PathBuf {
        let stem = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("storage");
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".{stem}.tmp.{}.{sequence}", std::process::id());

        match Self::parent_dir(path) {
            Some(parent) => parent.join(name),
            None => PathBuf::from(name),
        }
    }

    /// Crée (ou tronque) le fichier temporaire avec des permissions restrictives.
    fn create_private_file(path: &Path) -> Result<File, SystemError> {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let file = options.open(path)?;

        // `mode()` ne s'applique qu'à la création : on resserre aussi les droits
        // d'un éventuel fichier temporaire résiduel.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(file)
    }

    /// Renomme `from` vers `to` avec quelques tentatives espacées.
    ///
    /// Ne supprime jamais `to` : la cible reste valide tant que le renommage n'a
    /// pas réussi.
    fn rename_with_retry(from: &Path, to: &Path) -> Result<(), SystemError> {
        let mut backoff = RENAME_INITIAL_BACKOFF;

        for attempt in 1..=RENAME_MAX_ATTEMPTS {
            match fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e) if attempt < RENAME_MAX_ATTEMPTS => {
                    warn!(
                        attempt,
                        target = %to.display(),
                        error = %e,
                        "Renommage atomique refusé (cible probablement verrouillée), nouvelle tentative"
                    );
                    std::thread::sleep(backoff);
                    backoff *= 2;
                }
                Err(e) => return Err(SystemError::Io(e)),
            }
        }

        // Inatteignable : la boucle renvoie systématiquement au dernier tour.
        Err(SystemError::Io(std::io::Error::other(
            "renommage atomique impossible",
        )))
    }

    /// Force la durabilité de l'entrée de répertoire après le renommage (POSIX).
    ///
    /// L'échec n'est pas fatal : les données sont déjà en place, seule la
    /// résistance à une coupure d'alimentation immédiate est dégradée.
    #[cfg(unix)]
    fn sync_directory(dir: &Path) {
        let result = File::open(dir).and_then(|handle| handle.sync_all());
        if let Err(e) = result {
            debug!(
                path = %dir.display(),
                error = %e,
                "fsync du répertoire parent impossible (durabilité dégradée)"
            );
        }
    }

    /// NTFS ne permet pas d'ouvrir un répertoire comme fichier ; `MoveFileExW`
    /// journalise déjà l'opération de renommage, il n'y a rien à synchroniser.
    #[cfg(not(unix))]
    fn sync_directory(_dir: &Path) {}
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn test_atomic_write_and_read() {
        let dir = TempDir::new("storage_write_read");
        let test_file = dir.join("test_save.json");

        let sample_data = b"{\"name\": \"Gizmo\", \"level\": 5}";
        AtomicStorage::write_atomic(&test_file, sample_data).expect("Écriture initiale réussie");

        let read_str = AtomicStorage::read_to_string(&test_file).expect("Lecture réussie");
        assert_eq!(read_str, "{\"name\": \"Gizmo\", \"level\": 5}");

        // Écrasement atomique d'un fichier déjà présent (branche « rename sur cible existante »).
        let updated_data = b"{\"name\": \"Gizmo\", \"level\": 6}";
        AtomicStorage::write_atomic(&test_file, updated_data).expect("Écrasement réussi");

        let read_updated = AtomicStorage::read_to_string(&test_file).expect("Lecture réussie");
        assert_eq!(read_updated, "{\"name\": \"Gizmo\", \"level\": 6}");
    }

    #[test]
    fn test_write_atomic_creates_missing_parent_directories() {
        let dir = TempDir::new("storage_mkdir");
        let nested = dir.path().join("a").join("b").join("c").join("save.json");

        AtomicStorage::write_atomic(&nested, b"ok").expect("Création récursive du parent");
        assert_eq!(
            AtomicStorage::read_to_string(&nested).expect("Lecture réussie"),
            "ok"
        );
    }

    #[test]
    fn test_binary_roundtrip_preserves_every_byte() {
        let dir = TempDir::new("storage_binary");
        let target = dir.join("blob.bin");

        let payload: Vec<u8> = (0..=u8::MAX).rev().collect();
        AtomicStorage::write_atomic(&target, &payload).expect("Écriture binaire réussie");

        let read_back = AtomicStorage::read_bytes(&target).expect("Lecture binaire réussie");
        assert_eq!(read_back, payload);
        assert_eq!(read_back.len(), 256);
    }

    #[test]
    fn test_write_atomic_fails_when_parent_is_a_regular_file() {
        let dir = TempDir::new("storage_bad_parent");
        let blocker = dir.join("not_a_directory");
        AtomicStorage::write_atomic(&blocker, b"x").expect("Création du bloqueur");

        // `not_a_directory/save.json` ne peut pas exister : `create_dir_all` doit échouer.
        let impossible = blocker.join("save.json");
        let result = AtomicStorage::write_atomic(&impossible, b"y");

        assert!(
            matches!(result, Err(SystemError::Io(_))),
            "une erreur d'I/O réelle est attendue, obtenu : {result:?}"
        );
    }

    #[test]
    fn test_read_missing_file_reports_io_error() {
        let dir = TempDir::new("storage_missing");
        let missing = dir.join("absent.json");

        assert!(matches!(
            AtomicStorage::read_to_string(&missing),
            Err(SystemError::Io(_))
        ));
        assert!(matches!(
            AtomicStorage::read_bytes(&missing),
            Err(SystemError::Io(_))
        ));
    }

    #[test]
    fn test_rename_failure_preserves_target_and_cleans_temporary_file() {
        let dir = TempDir::new("storage_rename_failure");

        // Une cible qui est en réalité un répertoire non vide : le fichier
        // temporaire sera bien écrit, mais aucun renommage ne pourra aboutir.
        let target = dir.join("locked_target");
        fs::create_dir_all(&target).expect("Création du répertoire cible");
        let witness = target.join("witness.txt");
        fs::write(&witness, b"intact").expect("Création du témoin");

        let result = AtomicStorage::write_atomic(&target, b"nouvelle version");
        assert!(
            matches!(result, Err(SystemError::Io(_))),
            "un renommage impossible doit remonter une erreur, obtenu : {result:?}"
        );

        // La « cible » n'a jamais été supprimée : aucune destruction de données.
        assert!(target.is_dir(), "la cible existante doit rester intacte");
        assert_eq!(
            fs::read(&witness).expect("Lecture du témoin"),
            b"intact".to_vec()
        );

        // Et le fichier temporaire a été nettoyé par la garde RAII.
        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .expect("Lecture du répertoire de test")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "aucun fichier temporaire ne doit subsister, trouvés : {leftovers:?}"
        );
    }

    #[test]
    fn test_temp_paths_are_unique_across_calls() {
        let target = Path::new("/tmp/gremlin/save.json");
        let first = AtomicStorage::temp_path_for(target);
        let second = AtomicStorage::temp_path_for(target);

        assert_ne!(
            first, second,
            "deux écritures simultanées ne doivent jamais partager le même fichier temporaire"
        );
        assert_eq!(first.parent(), target.parent());
    }

    #[test]
    fn test_concurrent_writes_never_produce_a_corrupt_file() {
        const THREADS: usize = 8;
        const ITERATIONS: usize = 12;

        let dir = TempDir::new("storage_concurrent");
        let target = dir.join("shared_save.json");

        // Chaque thread écrit une charge utile de taille distincte, ce qui rend
        // toute troncature/entrelacement immédiatement détectable.
        let payloads: Vec<Vec<u8>> = (0..THREADS)
            .map(|id| vec![b'a' + u8::try_from(id).unwrap_or(0); 64 * (id + 1)])
            .collect();

        std::thread::scope(|scope| {
            for payload in &payloads {
                let target = target.clone();
                scope.spawn(move || {
                    for _ in 0..ITERATIONS {
                        AtomicStorage::write_atomic(&target, payload)
                            .expect("Écriture concurrente réussie");
                    }
                });
            }
        });

        let final_content = AtomicStorage::read_bytes(&target).expect("Lecture finale réussie");
        assert!(
            payloads.contains(&final_content),
            "le contenu final doit être exactement l'une des charges utiles écrites (longueur observée : {})",
            final_content.len()
        );

        let leftovers = fs::read_dir(dir.path())
            .expect("Lecture du répertoire de test")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp."))
            .count();
        assert_eq!(leftovers, 0, "aucun fichier temporaire ne doit subsister");
    }
}
