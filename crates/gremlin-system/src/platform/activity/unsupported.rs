//! Repli explicite pour les plateformes sans compteur d'inactivité pris en charge.

use super::IdleBackend;
use crate::error::SystemError;

pub fn default_backend() -> Result<Box<dyn IdleBackend>, SystemError> {
    Err(SystemError::ActivityUnavailable(
        "plateforme non prise en charge".to_owned(),
    ))
}
