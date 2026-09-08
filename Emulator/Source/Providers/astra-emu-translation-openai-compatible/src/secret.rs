//! Platform keyring integration.  Only opaque references are persisted by
//! the Manager; credential bytes stay in the platform keyring and never enter
//! logs, reports, or the translation cache.

use crate::model::{validate_secret_reference, SecretResolver, TranslationError};

const MAX_SECRET_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
#[cfg(not(target_os = "android"))]
pub struct PlatformSecretStore {
    service: String,
}

#[cfg(not(target_os = "android"))]
impl PlatformSecretStore {
    pub fn new(service: impl Into<String>) -> Result<Self, TranslationError> {
        let service = service.into();
        if service.is_empty() || service.len() > 128 || !service.is_ascii() {
            return Err(TranslationError::InvalidProfile("invalid keyring service"));
        }
        Ok(Self { service })
    }

    pub fn store(&self, reference: &str, secret: &str) -> Result<(), TranslationError> {
        validate_secret_reference(reference)?;
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(TranslationError::SecretUnavailable);
        }
        self.entry(reference)?
            .set_password(secret)
            .map_err(|_| TranslationError::SecretUnavailable)
    }

    pub fn delete(&self, reference: &str) -> Result<(), TranslationError> {
        validate_secret_reference(reference)?;
        self.entry(reference)?
            .delete_credential()
            .map_err(|_| TranslationError::SecretUnavailable)
    }

    fn entry(&self, reference: &str) -> Result<keyring::Entry, TranslationError> {
        keyring::Entry::new(&self.service, reference)
            .map_err(|_| TranslationError::SecretUnavailable)
    }
}

#[cfg(not(target_os = "android"))]
impl SecretResolver for PlatformSecretStore {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError> {
        validate_secret_reference(reference)?;
        let secret = self
            .entry(reference)?
            .get_password()
            .map_err(|_| TranslationError::SecretUnavailable)?;
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(TranslationError::SecretUnavailable);
        }
        Ok(secret)
    }
}
