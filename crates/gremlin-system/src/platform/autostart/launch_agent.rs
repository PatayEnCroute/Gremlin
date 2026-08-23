//! Backend macOS : `LaunchAgent` déposé dans `~/Library/LaunchAgents`.
//!
//! Compilé sur toutes les plateformes (il ne s'agit que d'écriture de fichiers)
//! afin de rester testable en dehors de macOS ; seule la sélection par défaut
//! est conditionnée à `target_os = "macos"`.

use super::{AutostartBackend, AutostartTarget};
use crate::error::SystemError;
use crate::storage::AtomicStorage;
use std::path::{Path, PathBuf};
use tracing::info;

/// Backend écrivant un `LaunchAgent` plist dans un répertoire donné.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchAgentBackend {
    directory: PathBuf,
}

impl LaunchAgentBackend {
    /// Construit le backend à partir du répertoire des `LaunchAgents`.
    #[must_use]
    pub fn new(launch_agents_dir: PathBuf) -> Self {
        Self {
            directory: launch_agents_dir,
        }
    }

    /// Construit le backend à partir du répertoire personnel de l'utilisateur.
    #[must_use]
    pub fn from_home(home: &Path) -> Self {
        Self::new(home.join("Library").join("LaunchAgents"))
    }

    /// Chemin du plist correspondant à la cible.
    #[must_use]
    pub fn plist_path(&self, target: &AutostartTarget) -> PathBuf {
        self.directory
            .join(format!("{}.plist", target.reverse_dns_label()))
    }

    /// Sérialise le `LaunchAgent` en XML valide.
    ///
    /// Toutes les valeurs interpolées passent par `escape_xml` : les chemins
    /// macOS acceptent `&`, `<` et `'`, qui casseraient sinon le document (et
    /// permettraient d'injecter des clés `launchd` arbitraires).
    #[must_use]
    pub fn render_plist(target: &AutostartTarget) -> String {
        let label = escape_xml(&target.reverse_dns_label());
        let program = escape_xml(&target.executable_string());

        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{program}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#
        )
    }
}

impl AutostartBackend for LaunchAgentBackend {
    fn is_enabled(&self, target: &AutostartTarget) -> bool {
        self.plist_path(target).is_file()
    }

    fn enable(&self, target: &AutostartTarget) -> Result<(), SystemError> {
        let plist_path = self.plist_path(target);
        let content = Self::render_plist(target);

        // Écriture atomique : un plist tronqué est rejeté par `launchd` et
        // désactive silencieusement le démarrage automatique.
        AtomicStorage::write_atomic(&plist_path, content.as_bytes())?;

        info!(path = %plist_path.display(), "Autostart macOS LaunchAgent créé avec succès");
        Ok(())
    }

    fn disable(&self, target: &AutostartTarget) -> Result<(), SystemError> {
        let plist_path = self.plist_path(target);

        match std::fs::remove_file(&plist_path) {
            Ok(()) => {
                info!(path = %plist_path.display(), "Autostart macOS LaunchAgent supprimé");
                Ok(())
            }
            // Déjà absent : l'opération est idempotente.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(SystemError::Io(e)),
        }
    }
}

/// Échappe les cinq entités prédéfinies XML.
///
/// Indispensable pour les chemins macOS, qui autorisent `&`, `<`, `>`, `"` et `'`.
fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn target(exe: &str) -> AutostartTarget {
        AutostartTarget::new("Gremlin", PathBuf::from(exe))
    }

    #[test]
    fn test_escape_xml_handles_every_predefined_entity() {
        assert_eq!(
            escape_xml(r#"a&b<c>d"e'f"#),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
        assert_eq!(escape_xml("chemin/normal-42"), "chemin/normal-42");
    }

    #[test]
    fn test_render_plist_escapes_hostile_paths() {
        let hostile = target("/Users/x/Dev & Tools/<gremlin>'s app");
        let plist = LaunchAgentBackend::render_plist(&hostile);

        assert!(plist.contains("Dev &amp; Tools"), "plist rendu : {plist}");
        assert!(plist.contains("&lt;gremlin&gt;&apos;s app"));
        // Aucune séquence brute ne subsiste en dehors du balisage attendu.
        assert!(!plist.contains("& "));
        assert!(!plist.contains("<gremlin>"));
    }

    #[test]
    fn test_render_plist_rejects_key_injection() {
        // Un composant de chemin conçu pour refermer la balise et injecter des clés.
        let injected = target("/tmp/x</string><key>RunAtLoad</key><false/><string>");
        let plist = LaunchAgentBackend::render_plist(&injected);

        assert!(!plist.contains("<false/>"), "plist rendu : {plist}");
        assert_eq!(
            plist.matches("<key>RunAtLoad</key>").count(),
            1,
            "une seule clé RunAtLoad doit exister"
        );
        assert!(plist.contains("&lt;/string&gt;&lt;key&gt;"));
    }

    #[test]
    fn test_enable_is_enabled_disable_roundtrip() {
        let dir = TempDir::new("launch_agent");
        let backend = LaunchAgentBackend::new(dir.path().join("LaunchAgents"));
        let target = target("/Applications/Gremlin.app/Contents/MacOS/gremlin");

        assert!(!backend.is_enabled(&target));

        backend.enable(&target).expect("Activation réussie");
        assert!(backend.is_enabled(&target));

        let path = backend.plist_path(&target);
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("com.gremlin.desktop.plist")
        );
        let written = std::fs::read_to_string(&path).expect("Lecture du plist");
        assert!(written.contains("<string>com.gremlin.desktop</string>"));
        assert!(written.contains("/Applications/Gremlin.app/Contents/MacOS/gremlin"));

        backend.disable(&target).expect("Désactivation réussie");
        assert!(!backend.is_enabled(&target));

        // Idempotence : désactiver deux fois reste un succès.
        backend.disable(&target).expect("Double désactivation");
    }

    #[test]
    fn test_enable_creates_missing_directory() {
        let dir = TempDir::new("launch_agent_mkdir");
        let backend = LaunchAgentBackend::from_home(dir.path());
        let target = target("/opt/gremlin");

        backend.enable(&target).expect("Activation réussie");
        assert!(dir.path().join("Library").join("LaunchAgents").is_dir());
    }
}
