//! Registry and probe policy for static and dynamically loaded Family providers.

use std::{collections::BTreeMap, path::Path};

use astra_emu_family_api::{
    FamilyCapability as AbiCapability, FamilyDescriptor as AbiDescriptor, FamilyOpen,
    FamilyProvider, OpenRequest, ProbeRequest,
};

use crate::family::{
    FamilyCapability, FamilyPluginDescriptor, FamilyPluginRegistry, FamilyProbeReport,
    FamilyProbeSelection,
};
use crate::family_loader::{FamilyLoadError, LoadedFamilyPlugin};

/// Runtime registry used by Manager and CLI. Static and dynamic providers are
/// registered through the same descriptor validation and probe-selection path.
pub struct FamilyProviderRegistry {
    providers: BTreeMap<String, Box<dyn FamilyProvider>>,
    descriptors: FamilyPluginRegistry,
}

impl Default for FamilyProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FamilyProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: BTreeMap::new(),
            descriptors: FamilyPluginRegistry::new(),
        }
    }

    pub fn register_provider<P>(&mut self, provider: P) -> Result<(), FamilyLoadError>
    where
        P: FamilyProvider + 'static,
    {
        let descriptor = provider
            .descriptor()
            .map_err(|_| FamilyLoadError::Descriptor)?;
        descriptor
            .validate()
            .map_err(|_| FamilyLoadError::Descriptor)?;
        let manager = manager_descriptor(&descriptor)?;
        let plugin_id = manager.plugin_id.clone();
        if self.providers.contains_key(&plugin_id) {
            return Err(FamilyLoadError::DuplicatePlugin);
        }
        self.descriptors
            .register(manager)
            .map_err(|_| FamilyLoadError::DuplicatePlugin)?;
        self.providers.insert(plugin_id, Box::new(provider));
        Ok(())
    }

    pub fn load_dynamic(&mut self, path: impl AsRef<Path>) -> Result<(), FamilyLoadError> {
        self.register_provider(LoadedFamilyPlugin::load(path)?)
    }

    pub fn remove(&mut self, plugin_id: &str) -> bool {
        self.descriptors.remove(plugin_id);
        self.providers.remove(plugin_id).is_some()
    }

    pub fn descriptor(&self, plugin_id: &str) -> Option<&FamilyPluginDescriptor> {
        self.descriptors.descriptor(plugin_id)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &FamilyPluginDescriptor> {
        self.descriptors.descriptors()
    }

    pub fn probe(
        &self,
        request: &ProbeRequest,
        preferred_plugin_id: Option<&str>,
        preferred_family_id: Option<&str>,
    ) -> Result<FamilyProbeSelection, FamilyLoadError> {
        request.validate().map_err(|_| FamilyLoadError::Probe)?;
        let mut reports = Vec::new();
        for (plugin_id, provider) in &self.providers {
            let Some(report) = provider
                .probe(request.clone())
                .map_err(|_| FamilyLoadError::Probe)?
            else {
                continue;
            };
            reports.push(FamilyProbeReport {
                plugin_id: plugin_id.clone(),
                family_id: report.family_id.to_string(),
                game_id: report.game_id.to_string(),
                format: report.format.to_string(),
                confidence_permyriad: report.confidence_permyriad,
            });
        }
        self.descriptors
            .select_probe(reports, preferred_plugin_id, preferred_family_id)
            .map_err(|_| FamilyLoadError::Policy)
    }

    pub fn open_selected(
        &mut self,
        candidate: &crate::family::FamilyProbeCandidate,
        request: OpenRequest,
    ) -> Result<FamilyOpen, FamilyLoadError> {
        let checked = self
            .descriptors
            .choose_probe(
                std::slice::from_ref(candidate),
                &candidate.report.plugin_id,
                &candidate.report.game_id,
            )
            .map_err(|_| FamilyLoadError::Policy)?;
        let provider = self
            .providers
            .get_mut(&checked.report.plugin_id)
            .ok_or(FamilyLoadError::Provider)?;
        provider
            .open(request)
            .map_err(|_| FamilyLoadError::Provider)
    }
}

pub(crate) fn manager_descriptor(
    descriptor: &AbiDescriptor,
) -> Result<FamilyPluginDescriptor, FamilyLoadError> {
    let capabilities = descriptor
        .capabilities
        .iter()
        .map(|capability| match capability {
            AbiCapability::CpuFrame => FamilyCapability::CpuFrame,
            AbiCapability::PcmAudio => FamilyCapability::PcmAudio,
            AbiCapability::NativeSave => FamilyCapability::NativeSave,
            AbiCapability::TextReplacement => FamilyCapability::TextReplacement,
        })
        .collect();
    let result = FamilyPluginDescriptor {
        family_id: descriptor.family_id.to_string(),
        plugin_id: descriptor.plugin_id.to_string(),
        abi_fingerprint: descriptor.abi_fingerprint.to_string(),
        version: descriptor.version.to_string(),
        capabilities,
        supported_formats: descriptor
            .supported_formats
            .iter()
            .map(ToString::to_string)
            .collect(),
    };
    result.validate().map_err(|_| FamilyLoadError::Descriptor)?;
    Ok(result)
}

#[cfg(test)]
#[path = "family_registry_tests.rs"]
mod tests;
