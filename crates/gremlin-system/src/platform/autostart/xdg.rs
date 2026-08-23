//! Backend Linux : entrée `.desktop` déposée dans `$XDG_CONFIG_HOME/autostart`.
//!
//! Compilé sur toutes les plateformes (il ne s'agit que d'écriture de fichiers)
//! afin de rester testable en dehors de Linux ; seule la sélection par défaut
//! est conditionnée à `target_os = "linux"`.

use super::{AutostartBackend, AutostartTarget};
use crate::error::SystemError;
use crate::storage::AtomicStorage;
use std::path::{Path, PathBuf};
use tracing::info;

/// Backend écrivant une entrée `.desktop` d'autostart dans un répertoire donné.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdgAutostartBackend {
    directory: PathBuf,
}

impl XdgAutostartBackend {
    /// Construit le backend à partir du répertoire `autostart` complet.
    #[must_use]
    pub fn new(autostart_dir: PathBuf) -> Self {
        Self {
            directory: autostart_dir,
        }
    }

    /// Construit le backend à partir du répertoire de configuration XDG.
    #[must_use]
    pub fn from_config_dir(config_dir: &Path) -> Self {
        Self::new(config_dir.join("autostart"))
    }

    /// Chemin de l'entrée `.desktop` correspondant à la cible.
    #[must_use]
    pub fn desktop_entry_path(&self, target: &AutostartTarget) -> PathBuf {
        self.directory
            .join(format!("{}.desktop", target.file_slug()))
    }

    /// Sérialise l'entrée `.desktop` conformément à la *Desktop Entry
    /// Specification* freedesktop.org.
    #[must_use]
    pub fn render_desktop_entry(target: &AutostartTarget) -> String {
        let name = escape_desktop_string(target.app_name());
        let exec = quote_exec_argument(&target.executable_string());

        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.0\n\
             Name={name}\n\
             Exec={exec}\n\
             Terminal=false\n\
             Hidden=false\n\
             NoDisplay=false\n\
             X-GNOME-Autostart-enabled=true\n"
        )
    }
}

impl AutostartBackend for XdgAutostartBackend {
    fn is_enabled(&self, target: &AutostartTarget) -> bool {
        self.desktop_entry_path(target).is_file()
    }

    fn enable(&self, target: &AutostartTarget) -> Result<(), SystemError> {
        let desktop_path = self.desktop_entry_path(target);
        let content = Self::render_desktop_entry(target);

        // Écriture atomique : une entrée `.desktop` tronquée est ignorée (au
        // mieux) ou signalée comme invalide par l'environnement de bureau.
        AtomicStorage::write_atomic(&desktop_path, content.as_bytes())?;

        info!(path = %desktop_path.display(), "Autostart Linux .desktop créé avec succès");
        Ok(())
    }

    fn disable(&self, target: &AutostartTarget) -> Result<(), SystemError> {
        let desktop_path = self.desktop_entry_path(target);

        match std::fs::remove_file(&desktop_path) {
            Ok(()) => {
                info!(path = %desktop_path.display(), "Autostart Linux .desktop supprimé");
                Ok(())
            }
            // Déjà absent : l'opération est idempotente.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(SystemError::Io(e)),
        }
    }
}

/// Échappe une valeur de type `string` d'un fichier `.desktop`.
///
/// Les séquences reconnues par la spécification sont `\s`, `\n`, `\t`, `\r` et
/// `\\`. Sans cet échappement, un nom d'application contenant un saut de ligne
/// injecterait des clés arbitraires dans le groupe `[Desktop Entry]`.
fn escape_desktop_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// Construit un argument `Exec=` entre guillemets, doublement échappé.
///
/// Deux règles se superposent lors de la lecture d'un fichier `.desktop` :
/// 1. la règle générale des valeurs `string` déséchappe d'abord `\\` en `\` ;
/// 2. la règle de mise entre guillemets déséchappe ensuite `\"`, ``\` ``,
///    `\$` et `\\`.
///
/// Une contre-oblique littérale s'écrit donc avec **quatre** contre-obliques,
/// et les caractères réservés `"`, `` ` `` et `$` sont précédés de `\\`. Les
/// codes de champ (`%f`, `%U`, …) imposent en outre de doubler les `%`
/// littéraux.
fn quote_exec_argument(argument: &str) -> String {
    let mut out = String::with_capacity(argument.len() + 2);
    out.push('"');

    for ch in argument.chars() {
        match ch {
            '\\' => out.push_str(r"\\\\"),
            '"' | '`' | '$' => {
                out.push_str(r"\\");
                out.push(ch);
            }
            '%' => out.push_str("%%"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            _ => out.push(ch),
        }
    }

    out.push('"');
    out
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn target(name: &str, exe: &str) -> AutostartTarget {
        AutostartTarget::new(name, PathBuf::from(exe))
    }

    #[test]
    fn test_quote_exec_argument_escapes_reserved_characters() {
        assert_eq!(
            quote_exec_argument("/usr/bin/gremlin"),
            r#""/usr/bin/gremlin""#
        );
        assert_eq!(
            quote_exec_argument(r"C:\Apps\gremlin.exe"),
            r#""C:\\\\Apps\\\\gremlin.exe""#
        );
        assert_eq!(quote_exec_argument("/opt/a$b"), r#""/opt/a\\$b""#);
        assert_eq!(quote_exec_argument("/opt/a`b"), r#""/opt/a\\`b""#);
        assert_eq!(quote_exec_argument(r#"/opt/a"b"#), r#""/opt/a\\"b""#);
    }

    #[test]
    fn test_quote_exec_argument_doubles_field_codes() {
        assert_eq!(
            quote_exec_argument("/opt/100%/gremlin"),
            r#""/opt/100%%/gremlin""#
        );
        // Un chemin contenant `%U` ne doit pas être interprété comme un code de champ.
        assert!(!quote_exec_argument("/opt/%U/gremlin").contains("/%U/"));
    }

    #[test]
    fn test_escape_desktop_string_prevents_key_injection() {
        let escaped = escape_desktop_string("Gremlin\nExec=/bin/sh -c evil");
        assert!(!escaped.contains('\n'), "obtenu : {escaped}");
        assert_eq!(escaped, r"Gremlin\nExec=/bin/sh -c evil");
        assert_eq!(escape_desktop_string(r"Back\slash"), r"Back\\slash");
    }

    #[test]
    fn test_render_desktop_entry_is_single_group_and_escaped() {
        let hostile = target("Gremlin\nX-Evil=1", r#"/opt/Dev & Tools/"gremlin" $HOME"#);
        let entry = XdgAutostartBackend::render_desktop_entry(&hostile);

        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert_eq!(entry.matches("[Desktop Entry]").count(), 1);
        // Le saut de ligne est neutralisé : `X-Evil` reste dans la valeur de
        // `Name` et ne devient jamais une clé à part entière.
        assert!(
            !entry.lines().any(|line| line.starts_with("X-Evil")),
            "entrée rendue : {entry}"
        );
        assert!(entry.contains(r"Name=Gremlin\nX-Evil=1"));
        assert!(entry.contains(r#"Exec="/opt/Dev & Tools/\\"gremlin\\" \\$HOME""#));

        // Le groupe contient exactement les clés attendues, une par ligne.
        let keys: Vec<&str> = entry
            .lines()
            .skip(1)
            .filter(|l| !l.is_empty())
            .filter_map(|l| l.split('=').next())
            .collect();
        assert_eq!(
            keys,
            [
                "Type",
                "Version",
                "Name",
                "Exec",
                "Terminal",
                "Hidden",
                "NoDisplay",
                "X-GNOME-Autostart-enabled",
            ]
        );
    }

    #[test]
    fn test_enable_is_enabled_disable_roundtrip() {
        let dir = TempDir::new("xdg_autostart");
        let backend = XdgAutostartBackend::from_config_dir(dir.path());
        let target = target("Gremlin", "/usr/local/bin/gremlin");

        assert!(!backend.is_enabled(&target));

        backend.enable(&target).expect("Activation réussie");
        assert!(backend.is_enabled(&target));

        let path = backend.desktop_entry_path(&target);
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("gremlin.desktop")
        );
        assert!(path.starts_with(dir.path().join("autostart")));

        let written = std::fs::read_to_string(&path).expect("Lecture de l'entrée");
        assert!(written.contains("Name=Gremlin\n"));
        assert!(written.contains(r#"Exec="/usr/local/bin/gremlin""#));

        backend.disable(&target).expect("Désactivation réussie");
        assert!(!backend.is_enabled(&target));

        // Idempotence : désactiver deux fois reste un succès.
        backend.disable(&target).expect("Double désactivation");
    }
}
