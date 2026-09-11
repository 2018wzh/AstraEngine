use astra_emu_translation_openai_compatible::PlatformSecretStore;
use astra_emu_translation_openai_compatible::{SecretResolver, TranslationError};

#[derive(Clone)]
pub struct ManagerSecretStore {
    inner: PlatformSecretStore,
}

impl ManagerSecretStore {
    pub fn open() -> Result<Self, TranslationError> {
        PlatformSecretStore::new("dev.astraengine.AstraEMU").map(|inner| Self { inner })
    }

    pub fn store(&self, reference: &str, secret: &str) -> Result<(), TranslationError> {
        self.inner.store(reference, secret)
    }

    pub fn delete(&self, reference: &str) -> Result<(), TranslationError> {
        self.inner.delete(reference)
    }
}

impl SecretResolver for ManagerSecretStore {
    fn resolve(&self, reference: &str) -> Result<String, TranslationError> {
        self.inner.resolve(reference)
    }
}
