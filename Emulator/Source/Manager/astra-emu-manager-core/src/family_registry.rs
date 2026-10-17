//! Registry and probe policy for static and dynamically loaded Family providers.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::model::ProbeRequest;

use crate::family::{
    FamilyPluginDescriptor, FamilyPluginRegistry, FamilyProbeReport,
    FamilyProbeSelection,
};
use crate::family_loader::{FamilyLoadError, LoadedFamilyPlugin, NativeOpenRequest, NativeSession};

/// Result of one deterministic scan of a Manager `cores` directory.
///
/// A bad core is isolated to its file.  The caller can surface every error
/// while still using providers that passed the ABI and descriptor gates.
#[derive(Debug, Default)]
pub struct FamilyDirectoryLoadReport {
    pub loaded: usize,
    pub errors: Vec<FamilyDirectoryLoadError>,
}

#[derive(Debug)]
pub struct FamilyDirectoryLoadError {
    pub path: PathBuf,
    pub error: FamilyLoadError,
}

/// Family dynamic libraries use the same stable basename across platforms.
/// Dependencies such as FFmpeg and the MSVC runtime intentionally do not use
/// this prefix, so placing a release directory under `cores` cannot make the
/// Manager probe every dependency DLL.
pub fn is_family_library_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    if !extension.eq_ignore_ascii_case(std::env::consts::DLL_EXTENSION) {
        return false;
    }
    let name = file_name.to_ascii_lowercase();
    name.starts_with("astra_emu_") || name.starts_with("libastra_emu_")
}

fn discover_family_libraries(directory: &Path) -> Result<Vec<PathBuf>, FamilyLoadError> {
    let entries = fs::read_dir(directory).map_err(|_| FamilyLoadError::Directory)?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| FamilyLoadError::Directory)?;
        let file_type = entry.file_type().map_err(|_| FamilyLoadError::Directory)?;
        if file_type.is_file() && is_family_library_path(&entry.path()) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Runtime registry used by Manager and CLI. Static and dynamic providers are
/// registered through the same descriptor validation and probe-selection path.
pub struct FamilyProviderRegistry {
    providers: BTreeMap<String, LoadedFamilyPlugin>,
    descriptors: FamilyPluginRegistry,
    rejected_ids: BTreeSet<String>,
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
            rejected_ids: BTreeSet::new(),
        }
    }

    pub fn register_provider(&mut self, provider: LoadedFamilyPlugin) -> Result<(), FamilyLoadError> {
        let manager = provider.manager_descriptor().clone();
        let plugin_id = manager.plugin_id.clone();
        if self.providers.contains_key(&plugin_id) || self.rejected_ids.contains(&plugin_id) {
            self.rejected_ids.insert(plugin_id.clone());
            self.remove(&plugin_id);
            return Err(FamilyLoadError::DuplicatePlugin);
        }
        self.descriptors.register(manager).map_err(|_| FamilyLoadError::DuplicatePlugin)?;
        self.providers.insert(plugin_id, provider);
        Ok(())
    }

    /// Static factory returns the same process-lifetime C table as a DLL.
    pub unsafe fn register_static(&mut self, query: astra_emu_abi::Query) -> Result<(), FamilyLoadError> {
        self.register_provider(unsafe { LoadedFamilyPlugin::from_query(query) }?)
    }

    pub fn load_dynamic(&mut self, path: impl AsRef<Path>) -> Result<(), FamilyLoadError> {
        self.register_provider(LoadedFamilyPlugin::load(path)?)
    }

    /// Load every Family library in a directory using one deterministic pass.
    ///
    /// Descriptor collection happens before registration.  If two files
    /// advertise the same plugin ID, both files are rejected and the ID is
    /// left absent from the registry; this prevents a sorted filename from
    /// becoming an accidental provider selection policy.
    pub fn load_directory(
        &mut self,
        directory: impl AsRef<Path>,
    ) -> Result<FamilyDirectoryLoadReport, FamilyLoadError> {
        self.load_directories(&[directory.as_ref().to_owned()])
    }

    /// Reject duplicate plugin IDs across bundled and user-installed cores.
    pub fn load_directories(
        &mut self,
        directories: &[PathBuf],
    ) -> Result<FamilyDirectoryLoadReport, FamilyLoadError> {
        let mut paths = Vec::new();
        let mut identities = BTreeSet::new();
        for directory in directories {
            for path in discover_family_libraries(directory)? {
                let identity = fs::canonicalize(&path).map_err(|_| FamilyLoadError::InvalidPath)?;
                if identities.insert(identity) {
                    paths.push(path);
                }
            }
        }
        paths.sort();
        paths.dedup();
        let mut candidates = BTreeMap::<String, (PathBuf, LoadedFamilyPlugin)>::new();
        let mut duplicate_ids = BTreeSet::new();
        let mut report = FamilyDirectoryLoadReport::default();

        for path in paths {
            let plugin = match LoadedFamilyPlugin::load(&path) {
                Ok(plugin) => plugin,
                Err(error) => {
                    report.errors.push(FamilyDirectoryLoadError { path, error });
                    continue;
                }
            };
            let plugin_id = plugin.manager_descriptor().plugin_id.clone();
            if duplicate_ids.contains(&plugin_id) {
                report.errors.push(FamilyDirectoryLoadError {
                    path,
                    error: FamilyLoadError::DuplicatePlugin,
                });
                continue;
            }
            if let Some((first_path, _)) = candidates.remove(&plugin_id) {
                duplicate_ids.insert(plugin_id);
                report.errors.push(FamilyDirectoryLoadError {
                    path: first_path,
                    error: FamilyLoadError::DuplicatePlugin,
                });
                report.errors.push(FamilyDirectoryLoadError {
                    path,
                    error: FamilyLoadError::DuplicatePlugin,
                });
                continue;
            }
            candidates.insert(plugin_id, (path, plugin));
        }

        for plugin_id in &duplicate_ids {
            self.remove(plugin_id);
            self.rejected_ids.insert(plugin_id.clone());
        }
        for (plugin_id, (path, plugin)) in candidates {
            if self.providers.contains_key(&plugin_id) {
                self.remove(&plugin_id);
                self.rejected_ids.insert(plugin_id.clone());
                report.errors.push(FamilyDirectoryLoadError {
                    path,
                    error: FamilyLoadError::DuplicatePlugin,
                });
                continue;
            }
            match self.register_provider(plugin) {
                Ok(()) => report.loaded += 1,
                Err(error) => report.errors.push(FamilyDirectoryLoadError { path, error }),
            }
        }
        Ok(report)
    }

    pub fn remove(&mut self, plugin_id: &str) -> bool {
        self.descriptors.remove(plugin_id);
        self.providers.remove(plugin_id).is_some()
    }

    pub fn descriptor(&self, plugin_id: &str) -> Option<&FamilyPluginDescriptor> {
        self.descriptors.descriptor(plugin_id)
    }

    pub fn configuration(&self, plugin_id: &str) -> Result<Vec<crate::model::ConfigField>, FamilyLoadError> {
        Ok(self.providers.get(plugin_id).ok_or(FamilyLoadError::Provider)?.configuration().to_vec())
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
        request
            .validate()
            .map_err(FamilyLoadError::ProbeError)?;
        let mut reports = Vec::new();
        for (plugin_id, provider) in &self.providers {
            let Some(report) = provider
                .probe(request.clone())
                .map_err(FamilyLoadError::ProbeError)?
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
            .map_err(|error| FamilyLoadError::PolicyError(error.to_string()))
    }

    pub fn open_selected(
        &mut self,
        candidate: &crate::family::FamilyProbeCandidate,
        request: NativeOpenRequest,
    ) -> Result<NativeSession, FamilyLoadError> {
        let checked = self
            .descriptors
            .choose_probe(
                std::slice::from_ref(candidate),
                &candidate.report.plugin_id,
                &candidate.report.game_id,
            )
            .map_err(|error| FamilyLoadError::PolicyError(error.to_string()))?;
        let provider = self
            .providers
            .get_mut(&checked.report.plugin_id)
            .ok_or(FamilyLoadError::Provider)?;
        provider
            .open(request)
            .map_err(FamilyLoadError::ProviderError)
    }
}


#[cfg(test)]
#[path = "family_registry_tests.rs"]
mod tests;
