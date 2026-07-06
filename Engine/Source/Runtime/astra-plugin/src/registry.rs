use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::debug;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct EngineModuleSlot(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RegisteredProvider {
    pub slot: EngineModuleSlot,
    pub provider_id: String,
    pub capability: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ServiceRegistry {
    services: BTreeMap<String, String>,
}

impl ServiceRegistry {
    pub fn register(&mut self, id: impl Into<String>, provider: impl Into<String>) {
        self.services.insert(id.into(), provider.into());
    }

    pub fn unregister(&mut self, id: &str, provider: &str) {
        if self
            .services
            .get(id)
            .is_some_and(|current| current == provider)
        {
            self.services.remove(id);
        }
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.services.get(id).map(String::as_str)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionRegistry {
    providers: Vec<RegisteredProvider>,
}

impl ExtensionRegistry {
    pub fn register(&mut self, provider: RegisteredProvider) {
        self.providers.push(provider);
        self.providers.sort_by(|a, b| {
            (a.slot.0.as_str(), a.provider_id.as_str())
                .cmp(&(b.slot.0.as_str(), b.provider_id.as_str()))
        });
    }

    pub fn unregister(&mut self, slot: &EngineModuleSlot, provider_id: &str) {
        self.providers
            .retain(|provider| &provider.slot != slot || provider.provider_id != provider_id);
    }

    pub fn select(&self, slot: &EngineModuleSlot) -> Option<&RegisteredProvider> {
        self.providers
            .iter()
            .find(|provider| &provider.slot == slot)
    }

    pub fn providers(&self) -> &[RegisteredProvider] {
        &self.providers
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginRegistrar {
    pub services: ServiceRegistry,
    pub extensions: ExtensionRegistry,
}

impl PluginRegistrar {
    pub fn register_provider(&mut self, provider: RegisteredProvider) {
        debug!(
            slot = %provider.slot.0,
            provider_id = %provider.provider_id,
            capability = %provider.capability,
            "plugin.provider.register"
        );
        self.services
            .register(provider.slot.0.clone(), provider.provider_id.clone());
        self.extensions.register(provider);
    }

    pub fn selected_provider(&self, slot: &EngineModuleSlot) -> Option<&RegisteredProvider> {
        let provider_id = self.services.get(&slot.0)?;
        self.extensions
            .providers()
            .iter()
            .find(|provider| &provider.slot == slot && provider.provider_id == provider_id)
    }

    pub fn unregister_provider(&mut self, provider: &RegisteredProvider) {
        debug!(
            slot = %provider.slot.0,
            provider_id = %provider.provider_id,
            "plugin.provider.unregister"
        );
        self.services
            .unregister(&provider.slot.0, &provider.provider_id);
        self.extensions
            .unregister(&provider.slot, &provider.provider_id);
    }
}
