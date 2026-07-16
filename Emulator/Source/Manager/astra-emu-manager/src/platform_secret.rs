#[cfg(not(target_os = "android"))]
use astra_emu_translation_openai_compatible::PlatformSecretStore;
use astra_emu_translation_openai_compatible::{SecretResolver, TranslationError};

#[derive(Clone)]
pub struct ManagerSecretStore {
    #[cfg(not(target_os = "android"))]
    inner: PlatformSecretStore,
}

impl ManagerSecretStore {
    pub fn open() -> Result<Self, TranslationError> {
        #[cfg(not(target_os = "android"))]
        {
            PlatformSecretStore::new("dev.astraengine.AstraEMU").map(|inner| Self { inner })
        }
        #[cfg(target_os = "android")]
        {
            Ok(Self {})
        }
    }

    pub fn store(&self, reference: &str, secret: &str) -> Result<(), TranslationError> {
        #[cfg(not(target_os = "android"))]
        {
            self.inner.store(reference, secret)
        }
        #[cfg(target_os = "android")]
        {
            crate::android_platform::store_secret(reference, secret)
                .map_err(|_| TranslationError::SecretUnavailable)
        }
    }
}

impl SecretResolver for ManagerSecretStore {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError> {
        #[cfg(not(target_os = "android"))]
        {
            self.inner.resolve(reference)
        }
        #[cfg(target_os = "android")]
        {
            crate::android_platform::resolve_secret(reference)
                .map_err(|_| TranslationError::SecretUnavailable)
        }
    }
}
