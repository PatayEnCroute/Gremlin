//! Backend inerte pour les plateformes sans mécanisme d'autostart connu.
//!
//! Il ne prétend jamais avoir réussi : `enable` remonte
//! [`SystemError::AutostartUnsupported`] pour que l'interface ne coche pas une
//! case correspondant à une fonctionnalité inexistante.

use super::{AutostartBackend, AutostartTarget};
use crate::error::SystemError;

/// Backend qui déclare honnêtement l'absence de support.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedBackend;

impl AutostartBackend for UnsupportedBackend {
    fn is_enabled(&self, _target: &AutostartTarget) -> bool {
        false
    }

    fn enable(&self, _target: &AutostartTarget) -> Result<(), SystemError> {
        Err(SystemError::AutostartUnsupported)
    }

    fn disable(&self, _target: &AutostartTarget) -> Result<(), SystemError> {
        // Rien n'a jamais pu être enregistré : la désactivation est déjà acquise.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_unsupported_backend_never_fakes_success() {
        let target = AutostartTarget::new("Gremlin", PathBuf::from("/opt/gremlin"));
        let backend = UnsupportedBackend;

        assert!(!backend.is_enabled(&target));
        assert!(matches!(
            backend.enable(&target),
            Err(SystemError::AutostartUnsupported)
        ));
        assert!(backend.disable(&target).is_ok());
    }
}
