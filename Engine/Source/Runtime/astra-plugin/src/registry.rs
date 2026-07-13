use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::debug;

use astra_plugin_abi::{
    ExtensionConflict, LoadPhase, PluginDependency, PluginDependencyGraphSnapshot,
    PluginExtensionRegistrySnapshot, ProviderBinding, ProviderExtensionRecord,
};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct EngineModuleSlot(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RegisteredProvider {
    pub slot: EngineModuleSlot,
    pub provider_id: String,
    pub capability: String,
    pub phase: LoadPhase,
    pub packaged: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ServiceRegistry {
    services: BTreeMap<String, String>,
}

impl ServiceRegistry {
    pub fn bind(&mut self, id: impl Into<String>, provider: impl Into<String>) {
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

    pub fn bindings(&self) -> impl Iterator<Item = (&String, &String)> {
        self.services.iter()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionRegistry {
    providers: Vec<RegisteredProvider>,
    conflicts: Vec<ExtensionConflict>,
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
        self.conflicts.retain(|conflict| {
            conflict.slot != slot.0
                || (conflict.selected_provider != provider_id
                    && conflict.conflicting_provider != provider_id)
        });
    }

    pub fn providers(&self) -> &[RegisteredProvider] {
        &self.providers
    }

    pub fn conflicts(&self) -> &[ExtensionConflict] {
        &self.conflicts
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginRegistrar {
    pub services: ServiceRegistry,
    pub extensions: ExtensionRegistry,
    dependency_graph: Vec<PluginDependency>,
}

impl PluginRegistrar {
    pub fn register_provider(&mut self, provider: RegisteredProvider) {
        debug!(
            slot = %provider.slot.0,
            provider_id = %provider.provider_id,
            capability = %provider.capability,
            "plugin.provider.register"
        );
        self.extensions.register(provider);
    }

    pub fn bind_provider(
        &mut self,
        slot: &EngineModuleSlot,
        provider_id: &str,
    ) -> Result<(), String> {
        let exists = self
            .extensions
            .providers()
            .iter()
            .any(|provider| &provider.slot == slot && provider.provider_id == provider_id);
        if !exists {
            return Err(format!(
                "ASTRA_PLUGIN_BINDING_PROVIDER_MISSING: provider {provider_id} is not registered for slot {}",
                slot.0
            ));
        }
        if let Some(selected_provider) = self.services.get(&slot.0) {
            let conflict = ExtensionConflict {
                slot: slot.0.clone(),
                selected_provider: selected_provider.to_string(),
                conflicting_provider: provider_id.to_string(),
                reason: "provider slot already has an explicit binding".to_string(),
            };
            if !self.extensions.conflicts.contains(&conflict) {
                self.extensions.conflicts.push(conflict);
            }
            return Err(format!(
                "ASTRA_PLUGIN_BINDING_CONFLICT: slot {} is already bound to {selected_provider}",
                slot.0
            ));
        }
        self.services.bind(slot.0.clone(), provider_id.to_string());
        Ok(())
    }

    pub fn selected_provider(&self, slot: &EngineModuleSlot) -> Option<&RegisteredProvider> {
        let provider_id = self.services.get(&slot.0)?;
        self.extensions
            .providers()
            .iter()
            .find(|provider| &provider.slot == slot && provider.provider_id == provider_id)
    }

    pub fn runtime_binding(
        &self,
        slot: &EngineModuleSlot,
        package_id: &str,
    ) -> Result<astra_runtime::ValidatedModuleBinding, astra_runtime::RuntimeError> {
        let provider = self.selected_provider(slot).ok_or_else(|| {
            astra_runtime::RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_BINDING_MISSING",
                "runtime module slot has no explicitly selected provider",
            ))
        })?;
        astra_runtime::ValidatedModuleBinding::validate(
            astra_runtime::EngineModuleSlot(slot.0.clone()),
            provider.provider_id.clone(),
            provider.capability.clone(),
            package_id,
            provider.packaged,
            true,
        )
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

    pub fn record_dependency(&mut self, dependency: PluginDependency) {
        self.dependency_graph.push(dependency);
    }

    pub fn dependency_graph(&self) -> &[PluginDependency] {
        &self.dependency_graph
    }

    pub fn extension_registry_snapshot(&self) -> PluginExtensionRegistrySnapshot {
        PluginExtensionRegistrySnapshot {
            schema: "astra.plugin_extension_registry.v1".to_string(),
            providers: self
                .extensions
                .providers()
                .iter()
                .map(|provider| ProviderExtensionRecord {
                    slot: provider.slot.0.clone(),
                    provider_id: provider.provider_id.clone(),
                    capability: provider.capability.clone(),
                    phase: provider.phase,
                    packaged: provider.packaged,
                })
                .collect(),
            bindings: self
                .services
                .bindings()
                .map(|(slot, provider_id)| ProviderBinding {
                    slot: slot.clone(),
                    provider_id: provider_id.clone(),
                })
                .collect(),
            conflicts: self.extensions.conflicts().to_vec(),
        }
    }

    pub fn dependency_graph_snapshot(&self) -> PluginDependencyGraphSnapshot {
        PluginDependencyGraphSnapshot {
            schema: "astra.plugin_dependency_graph.v1".to_string(),
            dependencies: self.dependency_graph.clone(),
        }
    }
}
