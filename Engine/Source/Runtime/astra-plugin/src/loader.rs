use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use abi_stable::library::{AbiHeaderRef, ROOT_MODULE_LOADER_NAME_WITH_NUL};
use astra_core::Hash256;
use libloading::Library;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::{
    install_actions, AstraPluginModuleRef, EngineModuleSlot, FfiPluginShutdown, LoadPhase,
    LoadedFfiAction, PluginDescriptor, PluginError, PluginGate, PluginRegistrar,
    RegisteredProvider,
};
use astra_plugin_abi::GAME_RUNTIME_PROVIDER_SLOT;
use astra_runtime::RuntimeWorld;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginLoadReport {
    pub schema: String,
    pub plugin: String,
    pub status: String,
    pub registered_slots: Vec<String>,
    pub callbacks_released: bool,
    pub diagnostics: Vec<String>,
}

pub struct LoadedPlugin {
    descriptor: PluginDescriptor,
    module: AstraPluginModuleRef,
    _library: Library,
    report: PluginLoadReport,
    registered_providers: Vec<RegisteredProvider>,
    registered_actions: Vec<LoadedFfiAction>,
}

impl LoadedPlugin {
    pub fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    pub fn report(&self) -> &PluginLoadReport {
        &self.report
    }

    pub fn install_runtime_actions(&self, world: &mut RuntimeWorld) -> Result<(), PluginError> {
        info!(
            plugin_id = %self.descriptor.id,
            action_count = self.registered_actions.len(),
            "plugin.action.install"
        );
        install_actions(&self.registered_actions, world)
    }

    pub fn unload_from(
        mut self,
        registrar: &mut PluginRegistrar,
    ) -> Result<PluginLoadReport, PluginError> {
        info!(
            plugin_id = %self.descriptor.id,
            provider_count = self.registered_providers.len(),
            "plugin.unload.start"
        );
        for provider in &self.registered_providers {
            registrar.unregister_provider(provider);
        }
        let shutdown: FfiPluginShutdown = (self.module.shutdown())();
        self.report.status = "unloaded".to_string();
        self.report.callbacks_released = shutdown.callbacks_released;
        info!(
            plugin_id = %self.descriptor.id,
            callbacks_released = shutdown.callbacks_released,
            "plugin.unload"
        );
        Ok(self.report)
    }

    pub fn unload_from_runtime(
        self,
        registrar: &mut PluginRegistrar,
        world: &mut RuntimeWorld,
    ) -> Result<PluginLoadReport, PluginError> {
        for action in &self.registered_actions {
            world.unregister_action_provider(action.provider_id());
        }
        self.unload_from(registrar)
    }
}

pub struct PluginLoader {
    gate: PluginGate,
}

impl PluginLoader {
    pub fn new(gate: PluginGate) -> Self {
        Self { gate }
    }

    pub fn load(
        &self,
        path: impl AsRef<Path>,
        registrar: &mut PluginRegistrar,
    ) -> Result<LoadedPlugin, PluginError> {
        let path = path.as_ref();
        debug!("plugin.load.start");
        let library = unsafe { Library::new(path) }
            .map_err(|err| PluginError::Load(format!("{}: {err}", path.display())))?;
        let module = unsafe { root_module(&library)? };
        let descriptor_yaml = (module.descriptor_yaml())().to_string();
        let descriptor = PluginDescriptor::from_yaml(&descriptor_yaml)?;
        descriptor.validate(&self.gate)?;
        descriptor.validate_binary_hash(Hash256::from_sha256(
            &fs::read(path).map_err(|err| PluginError::Load(err.to_string()))?,
        ))?;
        let registration = (module.register())();
        let mut slots = Vec::new();
        let mut registered_providers = Vec::new();
        for provider in registration.providers {
            let provider = RegisteredProvider {
                slot: EngineModuleSlot(provider.slot.to_string()),
                provider_id: provider.provider_id.to_string(),
                capability: provider.capability.to_string(),
                phase: LoadPhase::from_str(&provider.phase.to_string())
                    .map_err(PluginError::Load)?,
                packaged: provider.packaged,
            };
            slots.push(provider.slot.0.clone());
            registered_providers.push(provider.clone());
            registrar.register_provider(provider);
        }
        for provider in registration.runtime_providers {
            let provider = RegisteredProvider {
                slot: EngineModuleSlot(GAME_RUNTIME_PROVIDER_SLOT.to_string()),
                provider_id: provider.provider_id.to_string(),
                capability: provider.capability.to_string(),
                phase: LoadPhase::from_str(&provider.phase.to_string())
                    .map_err(PluginError::Load)?,
                packaged: provider.packaged,
            };
            slots.push(provider.slot.0.clone());
            registered_providers.push(provider.clone());
            registrar.register_provider(provider);
        }
        let registered_actions: Vec<LoadedFfiAction> = registration
            .actions
            .into_iter()
            .map(LoadedFfiAction::from_registration)
            .collect();
        info!(
            plugin_id = %descriptor.id,
            provider_count = registered_providers.len(),
            action_count = registered_actions.len(),
            "plugin.load"
        );
        Ok(LoadedPlugin {
            descriptor: descriptor.clone(),
            module,
            _library: library,
            report: PluginLoadReport {
                schema: "astra.plugin_report.v1".to_string(),
                plugin: descriptor.id,
                status: "loaded".to_string(),
                registered_slots: slots,
                callbacks_released: registration.callbacks == 0,
                diagnostics: Vec::new(),
            },
            registered_providers,
            registered_actions,
        })
    }
}

unsafe fn root_module(library: &Library) -> Result<AstraPluginModuleRef, PluginError> {
    let header = library
        .get::<AbiHeaderRef>(ROOT_MODULE_LOADER_NAME_WITH_NUL.as_bytes())
        .map_err(|err| PluginError::Load(err.to_string()))?;
    let header = (*header)
        .upgrade()
        .map_err(|err| PluginError::Load(err.to_string()))?;
    header
        .init_root_module::<AstraPluginModuleRef>()
        .map_err(|err| PluginError::Load(err.to_string()))
}

pub fn dylib_path(root: &Path, name: &str) -> PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    dylib_path_for_target(root, target_dir.as_deref(), "debug", name)
}

pub fn dylib_path_for_target(
    root: &Path,
    target_dir: Option<&Path>,
    profile: &str,
    name: &str,
) -> PathBuf {
    let file = if cfg!(target_os = "windows") {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    };
    let target_dir = match target_dir {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => root.join(path),
        None => root.join("target"),
    };
    target_dir.join(profile).join(file)
}
