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
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderBindingContext {
    pub package_id: String,
    pub target: String,
    pub profile: String,
    pub required_capability: String,
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
}

impl ProviderBindingContext {
    pub fn from_runtime_package(
        package: &astra_runtime::PackageHandle,
        required_capability: impl Into<String>,
    ) -> Self {
        Self {
            package_id: package.package_id.clone(),
            target: package.target.clone(),
            profile: package.profile.clone(),
            required_capability: required_capability.into(),
            engine_version: package.engine_version.clone(),
            rustc_fingerprint: package.rustc_fingerprint.clone(),
            feature_fingerprint: package.feature_fingerprint.clone(),
            abi_fingerprint: package.abi_fingerprint.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct SelectedProviderBinding {
    provider_id: String,
    context: ProviderBindingContext,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ServiceRegistry {
    services: BTreeMap<String, SelectedProviderBinding>,
}

impl ServiceRegistry {
    fn bind(
        &mut self,
        id: impl Into<String>,
        provider_id: impl Into<String>,
        context: ProviderBindingContext,
    ) {
        self.services.insert(
            id.into(),
            SelectedProviderBinding {
                provider_id: provider_id.into(),
                context,
            },
        );
    }

    pub fn unregister(&mut self, id: &str, provider: &str) {
        if self
            .services
            .get(id)
            .is_some_and(|current| current.provider_id == provider)
        {
            self.services.remove(id);
        }
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.services
            .get(id)
            .map(|binding| binding.provider_id.as_str())
    }

    fn bindings(&self) -> impl Iterator<Item = (&String, &SelectedProviderBinding)> {
        self.services.iter()
    }

    fn binding(&self, id: &str) -> Option<&SelectedProviderBinding> {
        self.services.get(id)
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
        context: ProviderBindingContext,
    ) -> Result<(), String> {
        let provider = self
            .extensions
            .providers()
            .iter()
            .find(|provider| &provider.slot == slot && provider.provider_id == provider_id)
            .ok_or_else(|| format!(
                "ASTRA_PLUGIN_BINDING_PROVIDER_MISSING: provider {provider_id} is not registered for slot {}",
                slot.0
            ))?;
        if provider.capability != context.required_capability {
            return Err("ASTRA_PLUGIN_BINDING_CAPABILITY_MISMATCH".to_string());
        }
        if provider.engine_version != context.engine_version
            || provider.rustc_fingerprint != context.rustc_fingerprint
            || provider.feature_fingerprint != context.feature_fingerprint
            || provider.abi_fingerprint != context.abi_fingerprint
        {
            return Err("ASTRA_PLUGIN_BINDING_FINGERPRINT_MISMATCH".to_string());
        }
        astra_runtime::ValidatedModuleBinding::validate(
            astra_runtime::EngineModuleSlot(slot.0.clone()),
            provider.provider_id.clone(),
            provider.capability.clone(),
            astra_runtime::ModuleBindingContext {
                package_id: context.package_id.clone(),
                target: context.target.clone(),
                profile: context.profile.clone(),
                engine_version: context.engine_version.clone(),
                rustc_fingerprint: context.rustc_fingerprint.clone(),
                feature_fingerprint: context.feature_fingerprint.clone(),
                abi_fingerprint: context.abi_fingerprint.clone(),
            },
            provider.packaged,
            true,
        )
        .map_err(|error| error.to_string())?;
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
        self.services
            .bind(slot.0.clone(), provider_id.to_string(), context);
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
    ) -> Result<astra_runtime::ValidatedModuleBinding, astra_runtime::RuntimeError> {
        let provider = self.selected_provider(slot).ok_or_else(|| {
            astra_runtime::RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_BINDING_MISSING",
                "runtime module slot has no explicitly selected provider",
            ))
        })?;
        let context = &self
            .services
            .binding(&slot.0)
            .ok_or_else(|| {
                astra_runtime::RuntimeError::diagnostic(astra_core::Diagnostic::blocking(
                    "ASTRA_RUNTIME_MODULE_BINDING_MISSING",
                    "runtime module slot has no binding context",
                ))
            })?
            .context;
        astra_runtime::ValidatedModuleBinding::validate(
            astra_runtime::EngineModuleSlot(slot.0.clone()),
            provider.provider_id.clone(),
            provider.capability.clone(),
            astra_runtime::ModuleBindingContext {
                package_id: context.package_id.clone(),
                target: context.target.clone(),
                profile: context.profile.clone(),
                engine_version: context.engine_version.clone(),
                rustc_fingerprint: context.rustc_fingerprint.clone(),
                feature_fingerprint: context.feature_fingerprint.clone(),
                abi_fingerprint: context.abi_fingerprint.clone(),
            },
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
                .map(|(slot, binding)| ProviderBinding {
                    slot: slot.clone(),
                    provider_id: binding.provider_id.clone(),
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
