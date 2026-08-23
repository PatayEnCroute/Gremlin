//! Backend Windows : valeur sous `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

use super::{AutostartBackend, AutostartTarget};
use crate::error::SystemError;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use tracing::{info, warn};
use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, ERROR_SUCCESS,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};

/// Sous-clé standard des programmes lancés à l'ouverture de session.
const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Backend s'appuyant sur la clé `Run` de l'utilisateur courant.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RegistryRunBackend;

/// Poignée de clé de registre refermée automatiquement.
struct RegistryKey(HKEY);

impl RegistryKey {
    /// Ouvre la sous-clé `Run` avec les droits demandés.
    ///
    /// Renvoie `Ok(None)` si la clé n'existe pas, `Err` pour toute autre erreur.
    #[allow(unsafe_code)]
    fn open_run(access: REG_SAM_FLAGS) -> Result<Option<Self>, SystemError> {
        let subkey = to_wide(RUN_SUBKEY);
        let mut handle: HKEY = std::ptr::null_mut();

        // SAFETY: `subkey` est une chaîne UTF-16 terminée par NUL maintenue en
        // vie pendant tout l'appel, et `handle` est un emplacement valide et
        // aligné dans lequel l'API écrit la poignée résultante.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                access,
                &raw mut handle,
            )
        };

        match status {
            ERROR_SUCCESS => Ok(Some(Self(handle))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            code => Err(SystemError::Registry {
                operation: "RegOpenKeyExW",
                code,
            }),
        }
    }

    fn raw(&self) -> HKEY {
        self.0
    }
}

impl Drop for RegistryKey {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: `self.0` provient d'un `RegOpenKeyExW` réussi et n'est refermée
        // qu'une seule fois, à la destruction de cette garde.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

impl AutostartBackend for RegistryRunBackend {
    #[allow(unsafe_code)]
    fn is_enabled(&self, target: &AutostartTarget) -> bool {
        // La signature publique renvoie un `bool` : une clé illisible ne peut
        // qu'être rapportée comme « non activée », mais elle est journalisée
        // plutôt que passée sous silence.
        let key = match RegistryKey::open_run(KEY_READ) {
            Ok(Some(key)) => key,
            Ok(None) => return false,
            Err(e) => {
                warn!(error = %e, "Lecture de la clé Run impossible, autostart supposé inactif");
                return false;
            }
        };

        let value_name = to_wide(target.app_name());
        let mut data_type: REG_VALUE_TYPE = 0;
        let mut data_len: u32 = 0;

        // SAFETY: la poignée est valide (garde vivante), `value_name` est
        // terminée par NUL, et les deux pointeurs de sortie désignent des
        // variables locales valides. Les pointeurs nuls signalent à l'API que
        // l'on ne veut que le type et la taille de la donnée, pas son contenu.
        let status = unsafe {
            RegQueryValueExW(
                key.raw(),
                value_name.as_ptr(),
                std::ptr::null(),
                &raw mut data_type,
                std::ptr::null_mut(),
                &raw mut data_len,
            )
        };

        status == ERROR_SUCCESS
    }

    #[allow(unsafe_code)]
    fn enable(&self, target: &AutostartTarget) -> Result<(), SystemError> {
        let key = RegistryKey::open_run(KEY_WRITE)?.ok_or(SystemError::Registry {
            operation: "RegOpenKeyExW",
            code: ERROR_FILE_NOT_FOUND,
        })?;

        let value_name = to_wide(target.app_name());
        // Les guillemets protègent les chemins contenant des espaces.
        let value_data = to_wide(&format!("\"{}\"", target.executable_string()));
        // `cbdata` s'exprime en octets, pas en unités UTF-16 : d'où `size_of::<u16>()`.
        let byte_len = u32::try_from(value_data.len() * size_of::<u16>()).map_err(|_| {
            SystemError::Registry {
                operation: "RegSetValueExW",
                code: ERROR_INVALID_PARAMETER,
            }
        })?;

        // SAFETY: la poignée est valide, les deux chaînes UTF-16 sont terminées
        // par NUL et restent vivantes pendant l'appel, et `byte_len` décrit
        // exactement la taille en octets du tampon `value_data`.
        let status = unsafe {
            RegSetValueExW(
                key.raw(),
                value_name.as_ptr(),
                0,
                REG_SZ,
                value_data.as_ptr().cast(),
                byte_len,
            )
        };

        if status == ERROR_SUCCESS {
            info!("Autostart Windows activé avec succès dans le registre");
            Ok(())
        } else {
            Err(SystemError::Registry {
                operation: "RegSetValueExW",
                code: status,
            })
        }
    }

    #[allow(unsafe_code)]
    fn disable(&self, target: &AutostartTarget) -> Result<(), SystemError> {
        // Clé absente : rien n'a jamais été enregistré.
        let Some(key) = RegistryKey::open_run(KEY_WRITE)? else {
            return Ok(());
        };

        let value_name = to_wide(target.app_name());

        // SAFETY: la poignée est valide (garde vivante) et `value_name` est une
        // chaîne UTF-16 terminée par NUL vivante pendant tout l'appel.
        let status = unsafe { RegDeleteValueW(key.raw(), value_name.as_ptr()) };

        match status {
            ERROR_SUCCESS => {
                info!("Autostart Windows désactivé du registre");
                Ok(())
            }
            // Valeur déjà absente : l'opération est idempotente.
            ERROR_FILE_NOT_FOUND => Ok(()),
            code => Err(SystemError::Registry {
                operation: "RegDeleteValueW",
                code,
            }),
        }
    }
}

/// Convertit une chaîne Rust en tampon UTF-16 terminé par NUL.
fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_wide_is_nul_terminated() {
        let wide = to_wide("ab");
        assert_eq!(wide, vec![u16::from(b'a'), u16::from(b'b'), 0]);
    }

    #[test]
    fn test_byte_length_uses_u16_width() {
        let wide = to_wide("Gremlin");
        assert_eq!(wide.len() * size_of::<u16>(), 16);
    }

    /// Le backend interroge le registre réel : la seule invariance testable
    /// sans modifier la machine est que la lecture ne panique pas et reste
    /// cohérente entre deux appels.
    #[test]
    fn test_is_enabled_is_side_effect_free() {
        let target = AutostartTarget::new(
            "GremlinTestValeurInexistante",
            std::path::PathBuf::from("C:\\gremlin.exe"),
        );
        let backend = RegistryRunBackend;
        assert_eq!(backend.is_enabled(&target), backend.is_enabled(&target));
        assert!(
            !backend.is_enabled(&target),
            "aucune valeur de test ne doit exister dans la clé Run"
        );
    }
}
