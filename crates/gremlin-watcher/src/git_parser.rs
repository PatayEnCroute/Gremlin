//! Parsing natif et résilient des métadonnées de dépôts Git sans dépendance externe.
//!
//! Les fichiers lus proviennent de dépôts trouvés sur le disque : ils sont donc
//! traités comme des **entrées non fiables**. Toutes les lectures sont plafonnées en
//! taille et toute référence extraite de `HEAD` est validée avant d'être jointe au
//! répertoire `.git` (protection contre les traversées de chemin).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

/// Taille maximale lue pour les petits fichiers de métadonnées (`HEAD`, refs loose).
const MAX_SMALL_FILE_BYTES: u64 = 4 * 1024;

/// Taille maximale lue en fin de journal de références (`logs/HEAD`).
const MAX_REFLOG_TAIL_BYTES: u64 = 8 * 1024;

/// Taille maximale lue dans `packed-refs`.
const MAX_PACKED_REFS_BYTES: u64 = 4 * 1024 * 1024;

/// Taille maximale lue dans `COMMIT_EDITMSG`.
const MAX_COMMIT_MSG_BYTES: u64 = 64 * 1024;

/// Longueur d'un SHA-1 Git complet.
const SHA1_LEN: usize = 40;

/// Entrée analysée du journal de références Git (`.git/logs/HEAD`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    /// Ancien commit SHA.
    pub old_sha: String,
    /// Nouveau commit SHA.
    pub new_sha: String,
    /// Action Git (ex: "commit", "commit (amend)", "checkout").
    pub action: String,
    /// Message ou détail de l'action.
    pub message: Option<String>,
}

impl ReflogEntry {
    /// Indique si l'action journalisée correspond à la **création effective d'un commit**.
    ///
    /// Permet de distinguer un `git commit` d'un simple `git checkout` ou `git clone`,
    /// qui font eux aussi bouger le SHA pointé par `HEAD` sans qu'aucun commit
    /// n'ait été créé par l'utilisateur.
    #[must_use]
    pub fn is_commit_action(&self) -> bool {
        let action = self.action.trim();
        let verb = action.split_once(' ').map_or(action, |(head, _)| head);

        match verb {
            // "commit", "commit (amend)", "commit (initial)", "merge feature-x"...
            "commit" | "merge" | "cherry-pick" | "revert" | "am" | "applypatch" => true,
            // Seuls les rejeux de rebase produisent réellement des commits.
            "rebase" => ["(pick)", "(squash)", "(fixup)", "(amend)", "(continue)"]
                .iter()
                .any(|marker| action.contains(marker)),
            _ => false,
        }
    }
}

/// Instantané cohérent des métadonnées d'un dépôt, obtenu en une seule passe d'I/O.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoSnapshot {
    /// Branche active (ou `detached@<sha court>`), `None` si `HEAD` est illisible.
    pub branch: Option<String>,
    /// SHA complet du commit pointé par `HEAD`.
    pub commit_sha: Option<String>,
    /// Dernier message de commit connu.
    pub message: Option<String>,
    /// Dernière entrée du journal `logs/HEAD`.
    pub last_reflog: Option<ReflogEntry>,
}

/// Analyseur de fichiers internes Git (`HEAD`, `refs`, `logs/HEAD`, `COMMIT_EDITMSG`).
pub struct GitRefParser;

impl GitRefParser {
    /// Lit en une seule passe l'ensemble des métadonnées utiles d'un dépôt.
    ///
    /// Évite les lectures redondantes du reflog (auparavant parcouru jusqu'à deux
    /// fois par cycle de debounce).
    #[must_use]
    pub fn read_snapshot(git_dir: &Path) -> RepoSnapshot {
        let head = read_capped(&git_dir.join("HEAD"), MAX_SMALL_FILE_BYTES);
        let last_reflog = Self::read_last_reflog_entry(git_dir);

        let branch = head.as_deref().and_then(parse_branch_name);
        let commit_sha = head
            .as_deref()
            .and_then(|content| Self::resolve_head_sha(git_dir, content));
        let message = last_reflog
            .as_ref()
            .and_then(|entry| entry.message.clone())
            .filter(|msg| !msg.is_empty())
            .or_else(|| read_commit_editmsg(git_dir));

        RepoSnapshot {
            branch,
            commit_sha,
            message,
            last_reflog,
        }
    }

    /// Extrait le nom de la branche active depuis le fichier `.git/HEAD`.
    #[must_use]
    pub fn read_current_branch(git_dir: &Path) -> Option<String> {
        let content = read_capped(&git_dir.join("HEAD"), MAX_SMALL_FILE_BYTES)?;
        parse_branch_name(&content)
    }

    /// Récupère le SHA complet du commit pointé par `HEAD`.
    #[must_use]
    pub fn read_head_commit_sha(git_dir: &Path) -> Option<String> {
        let content = read_capped(&git_dir.join("HEAD"), MAX_SMALL_FILE_BYTES)?;
        Self::resolve_head_sha(git_dir, &content)
    }

    /// Résout le SHA de `HEAD` à partir du contenu déjà lu du fichier `HEAD`.
    fn resolve_head_sha(git_dir: &Path, head_content: &str) -> Option<String> {
        let trimmed = head_content.trim();

        let Some(ref_path) = trimmed.strip_prefix("ref: ") else {
            return is_full_sha(trimmed).then(|| trimmed.to_string());
        };
        let ref_path = ref_path.trim();

        // Référence "loose" (ex: .git/refs/heads/main).
        if let Some(ref_file) = safe_ref_join(git_dir, ref_path) {
            if let Some(content) = read_capped(&ref_file, MAX_SMALL_FILE_BYTES) {
                let sha = content.trim();
                if is_full_sha(sha) {
                    return Some(sha.to_string());
                }
            }
        }

        // Repli : recherche dans le fichier `packed-refs`.
        if let Some(sha) = Self::find_in_packed_refs(git_dir, ref_path) {
            return Some(sha);
        }

        // Repli : journal **spécifique à cette référence** (`logs/refs/heads/main`).
        // `logs/HEAD` ne convient pas : sa dernière entrée peut concerner une tout
        // autre référence et renverrait alors un SHA erroné.
        Self::read_last_reflog_entry_for_ref(git_dir, ref_path)
            .map(|entry| entry.new_sha)
            .filter(|sha| is_full_sha(sha))
    }

    /// Analyse la dernière ligne du reflog `.git/logs/HEAD`.
    #[must_use]
    pub fn read_last_reflog_entry(git_dir: &Path) -> Option<ReflogEntry> {
        let log_path = git_dir.join("logs").join("HEAD");
        parse_last_reflog_line(&read_tail(&log_path, MAX_REFLOG_TAIL_BYTES)?)
    }

    /// Analyse la dernière ligne du reflog propre à une référence (`logs/refs/heads/main`).
    #[must_use]
    pub fn read_last_reflog_entry_for_ref(git_dir: &Path, ref_path: &str) -> Option<ReflogEntry> {
        let log_path = safe_ref_join(&git_dir.join("logs"), ref_path)?;
        parse_last_reflog_line(&read_tail(&log_path, MAX_REFLOG_TAIL_BYTES)?)
    }

    /// Tente de lire le dernier message de commit (via reflog ou `COMMIT_EDITMSG`).
    #[must_use]
    pub fn read_last_commit_message(git_dir: &Path) -> Option<String> {
        Self::read_last_reflog_entry(git_dir)
            .and_then(|entry| entry.message)
            .filter(|msg| !msg.is_empty())
            .or_else(|| read_commit_editmsg(git_dir))
    }

    /// Extrait le nom canonique du dépôt à partir de son chemin racine.
    #[must_use]
    pub fn extract_repo_name(repo_root: &Path) -> String {
        repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed_repo")
            .to_string()
    }

    /// Recherche un ref dans `.git/packed-refs`.
    fn find_in_packed_refs(git_dir: &Path, ref_path: &str) -> Option<String> {
        let content = read_capped(&git_dir.join("packed-refs"), MAX_PACKED_REFS_BYTES)?;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
                continue;
            }
            if let Some((sha, name)) = line.split_once(' ') {
                if name.trim() == ref_path && is_full_sha(sha) {
                    return Some(sha.to_string());
                }
            }
        }

        None
    }
}

/// Vérifie qu'une chaîne est un SHA-1 Git complet.
fn is_full_sha(value: &str) -> bool {
    value.len() == SHA1_LEN && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Déduit le nom de branche affichable à partir du contenu brut de `HEAD`.
fn parse_branch_name(head_content: &str) -> Option<String> {
    let trimmed = head_content.trim();

    if let Some(rest) = trimmed.strip_prefix("ref: refs/heads/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("ref: ") {
        return Some(rest.to_string());
    }
    if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        // Tête détachée (Detached HEAD)
        let short_sha = trimmed.get(..7)?;
        return Some(format!("detached@{short_sha}"));
    }
    None
}

/// Valide une référence issue de `HEAD` puis la joint au répertoire de base.
///
/// Un dépôt malveillant peut écrire `ref: ../../../../etc/passwd` dans `HEAD` ;
/// la référence doit donc être relative, sans `..`, et rester sous `refs/`.
fn safe_ref_join(base_dir: &Path, ref_path: &str) -> Option<PathBuf> {
    if ref_path.is_empty() {
        return None;
    }
    let relative = Path::new(ref_path);
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }
    // Une référence symbolique Git valide vit toujours sous `refs/`.
    if relative.components().next() != Some(Component::Normal("refs".as_ref())) {
        return None;
    }
    Some(base_dir.join(relative))
}

/// Analyse la dernière ligne non vide d'un extrait de reflog.
fn parse_last_reflog_line(content: &str) -> Option<ReflogEntry> {
    let last_line = content.lines().rev().find(|l| !l.trim().is_empty())?;

    // Format Git reflog :
    // <old-sha> <new-sha> <name> <<email>> <timestamp> <tz>\t<action>: <message>
    let (header, action_part) = last_line
        .split_once('\t')
        .map_or((last_line, None), |(h, a)| (h, Some(a)));

    let mut parts = header.split_whitespace();
    let old_sha = parts.next()?.to_string();
    let new_sha = parts.next()?.to_string();

    let (action, message) = action_part.map_or_else(
        || (String::from("unknown"), None),
        |act| {
            act.split_once(": ").map_or_else(
                || (act.trim().to_string(), None),
                |(a, m)| (a.trim().to_string(), Some(m.trim().to_string())),
            )
        },
    );

    Some(ReflogEntry {
        old_sha,
        new_sha,
        action,
        message,
    })
}

/// Lit la première ligne utile de `COMMIT_EDITMSG`.
fn read_commit_editmsg(git_dir: &Path) -> Option<String> {
    let content = read_capped(&git_dir.join("COMMIT_EDITMSG"), MAX_COMMIT_MSG_BYTES)?;
    content
        .lines()
        .find(|l| {
            let trimmed = l.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(|line| line.trim().to_string())
}

/// Lit au plus `max_bytes` octets depuis le début d'un fichier.
fn read_capped(path: &Path, max_bytes: u64) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut buffer = Vec::new();
    let _ = file.take(max_bytes).read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// Lit au plus `max_bytes` octets à la **fin** d'un fichier.
///
/// Le journal `logs/HEAD` d'un dépôt actif peut peser plusieurs mégaoctets alors
/// que seule sa dernière ligne nous intéresse : la lecture intégrale est proscrite.
fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);

    if start > 0 {
        let _ = file.seek(SeekFrom::Start(start)).ok()?;
    }

    let mut buffer = Vec::new();
    let _ = file.take(max_bytes).read_to_end(&mut buffer).ok()?;
    let text = String::from_utf8_lossy(&buffer).into_owned();

    if start == 0 {
        return Some(text);
    }

    // La première ligne lue est potentiellement tronquée : elle est écartée.
    Some(
        text.split_once('\n')
            .map_or(String::new(), |(_, rest)| rest.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::{GitRefParser, ReflogEntry, RepoSnapshot};
    use crate::test_support::{write_file, TempDirGuard};

    fn reflog(action: &str) -> ReflogEntry {
        ReflogEntry {
            old_sha: "1".repeat(40),
            new_sha: "2".repeat(40),
            action: action.to_string(),
            message: None,
        }
    }

    #[test]
    fn test_read_symbolic_branch() {
        let guard = TempDirGuard::new("git_branch");
        let git_dir = guard.child(".git");
        write_file(
            &git_dir.join("HEAD"),
            "ref: refs/heads/feature/tamagotchi-xp\n",
        );

        let branch = GitRefParser::read_current_branch(&git_dir);
        assert_eq!(branch, Some("feature/tamagotchi-xp".to_string()));
    }

    #[test]
    fn test_read_detached_head() {
        let guard = TempDirGuard::new("git_detached");
        let git_dir = guard.child(".git");
        write_file(
            &git_dir.join("HEAD"),
            "e4d3c2b1a09876543210fedcba9876543210abcd\n",
        );

        assert_eq!(
            GitRefParser::read_current_branch(&git_dir),
            Some("detached@e4d3c2b".to_string())
        );
        assert_eq!(
            GitRefParser::read_head_commit_sha(&git_dir),
            Some("e4d3c2b1a09876543210fedcba9876543210abcd".to_string())
        );
    }

    #[test]
    fn test_read_reflog_entry_and_message() {
        let guard = TempDirGuard::new("git_reflog");
        let git_dir = guard.child(".git");
        let log_content = "0000000000000000000000000000000000000000 1111111111111111111111111111111111111111 Dev <dev@gremlin.rs> 1700000000 +0100\tclone: from git@github.com\n1111111111111111111111111111111111111111 2222222222222222222222222222222222222222 Dev <dev@gremlin.rs> 1700000100 +0100\tcommit: feat: add snack animations\n";
        write_file(&git_dir.join("logs").join("HEAD"), log_content);

        let Some(entry) = GitRefParser::read_last_reflog_entry(&git_dir) else {
            panic!("le reflog doit être analysable");
        };
        assert_eq!(entry.old_sha, "1".repeat(40));
        assert_eq!(entry.new_sha, "2".repeat(40));
        assert_eq!(entry.action, "commit");
        assert_eq!(
            entry.message,
            Some("feat: add snack animations".to_string())
        );
        assert!(entry.is_commit_action());

        assert_eq!(
            GitRefParser::read_last_commit_message(&git_dir),
            Some("feat: add snack animations".to_string())
        );
    }

    #[test]
    fn test_reflog_tail_read_on_large_journal() {
        let guard = TempDirGuard::new("git_reflog_tail");
        let git_dir = guard.child(".git");

        // Journal volumineux : seule la fin doit être lue.
        let mut content = String::new();
        for i in 0..20_000 {
            let line = format!(
                "{old} {new} Dev <dev@gremlin.rs> 17000000{i:02} +0100\tcommit: entrée {i}\n",
                old = "a".repeat(40),
                new = "b".repeat(40),
            );
            content.push_str(&line);
        }
        let last_line = format!(
            "{old} {new} Dev <dev@gremlin.rs> 1700009999 +0100\tcommit: dernière entrée\n",
            old = "b".repeat(40),
            new = "c".repeat(40),
        );
        content.push_str(&last_line);
        write_file(&git_dir.join("logs").join("HEAD"), &content);

        let Some(entry) = GitRefParser::read_last_reflog_entry(&git_dir) else {
            panic!("la fin du reflog doit être analysable");
        };
        assert_eq!(entry.new_sha, "c".repeat(40));
        assert_eq!(entry.message, Some("dernière entrée".to_string()));
    }

    #[test]
    fn test_path_traversal_in_head_is_rejected() {
        let guard = TempDirGuard::new("git_traversal");
        let git_dir = guard.child(".git");

        // Cible hors du dépôt contenant un faux SHA de 40 caractères hexadécimaux.
        let outside = guard.child("outside");
        write_file(&outside.join("secret"), &format!("{}\n", "f".repeat(40)));

        write_file(&git_dir.join("HEAD"), "ref: ../outside/secret\n");
        assert_eq!(
            GitRefParser::read_head_commit_sha(&git_dir),
            None,
            "une référence remontant hors du dépôt ne doit jamais être lue"
        );

        write_file(&git_dir.join("HEAD"), "ref: /etc/passwd\n");
        assert_eq!(GitRefParser::read_head_commit_sha(&git_dir), None);
    }

    #[test]
    fn test_packed_refs_resolution() {
        let guard = TempDirGuard::new("git_packed");
        let git_dir = guard.child(".git");
        write_file(&git_dir.join("HEAD"), "ref: refs/heads/main\n");
        write_file(
            &git_dir.join("packed-refs"),
            &format!(
                "# pack-refs with: peeled fully-peeled sorted\n{sha} refs/heads/main\n",
                sha = "d".repeat(40)
            ),
        );

        assert_eq!(
            GitRefParser::read_head_commit_sha(&git_dir),
            Some("d".repeat(40))
        );
    }

    #[test]
    fn test_reflog_fallback_uses_ref_specific_journal() {
        let guard = TempDirGuard::new("git_reflog_ref");
        let git_dir = guard.child(".git");
        write_file(&git_dir.join("HEAD"), "ref: refs/heads/main\n");

        // `logs/HEAD` concerne une autre branche : il ne doit pas servir de repli.
        write_file(
            &git_dir.join("logs").join("HEAD"),
            &format!(
                "{a} {b} Dev <d@rs.org> 1700000000 +0000\tcheckout: moving from main to other\n",
                a = "1".repeat(40),
                b = "9".repeat(40)
            ),
        );
        assert_eq!(GitRefParser::read_head_commit_sha(&git_dir), None);

        // Le journal propre à la référence, lui, fait autorité.
        write_file(
            &git_dir.join("logs").join("refs").join("heads").join("main"),
            &format!(
                "{a} {b} Dev <d@rs.org> 1700000000 +0000\tcommit: ok\n",
                a = "1".repeat(40),
                b = "7".repeat(40)
            ),
        );
        assert_eq!(
            GitRefParser::read_head_commit_sha(&git_dir),
            Some("7".repeat(40))
        );
    }

    #[test]
    fn test_snapshot_reads_everything_at_once() {
        let guard = TempDirGuard::new("git_snapshot");
        let git_dir = guard.child(".git");
        write_file(&git_dir.join("HEAD"), "ref: refs/heads/main\n");
        write_file(
            &git_dir.join("refs").join("heads").join("main"),
            &format!("{}\n", "5".repeat(40)),
        );
        write_file(
            &git_dir.join("logs").join("HEAD"),
            &format!(
                "{a} {b} Dev <d@rs.org> 1700000000 +0000\tcommit: snapshot\n",
                a = "4".repeat(40),
                b = "5".repeat(40)
            ),
        );

        let snapshot = GitRefParser::read_snapshot(&git_dir);
        assert_eq!(snapshot.branch, Some("main".to_string()));
        assert_eq!(snapshot.commit_sha, Some("5".repeat(40)));
        assert_eq!(snapshot.message, Some("snapshot".to_string()));
        assert!(snapshot.last_reflog.is_some_and(|e| e.is_commit_action()));
    }

    #[test]
    fn test_commit_action_discrimination() {
        assert!(reflog("commit").is_commit_action());
        assert!(reflog("commit (amend)").is_commit_action());
        assert!(reflog("commit (initial)").is_commit_action());
        assert!(reflog("merge feature-skin").is_commit_action());
        assert!(reflog("cherry-pick").is_commit_action());
        assert!(reflog("rebase (pick)").is_commit_action());

        assert!(!reflog("checkout").is_commit_action());
        assert!(!reflog("clone").is_commit_action());
        assert!(!reflog("reset").is_commit_action());
        assert!(!reflog("pull").is_commit_action());
        assert!(!reflog("branch").is_commit_action());
        assert!(!reflog("rebase (finish)").is_commit_action());
        assert!(!reflog("unknown").is_commit_action());
    }

    #[test]
    fn test_missing_files_are_tolerated() {
        let guard = TempDirGuard::new("git_missing");
        let git_dir = guard.child(".git");

        assert_eq!(GitRefParser::read_current_branch(&git_dir), None);
        assert_eq!(GitRefParser::read_head_commit_sha(&git_dir), None);
        assert_eq!(GitRefParser::read_last_reflog_entry(&git_dir), None);
        assert_eq!(
            GitRefParser::read_snapshot(&git_dir),
            RepoSnapshot::default()
        );
    }

    #[test]
    fn test_extract_repo_name() {
        let guard = TempDirGuard::new("git_name");
        let name = GitRefParser::extract_repo_name(guard.path());
        assert!(name.starts_with("gremlin_test_git_name"));
    }
}
