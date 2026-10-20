//! Registry and probe policy for static and dynamically loaded Family providers.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::model::ProbeRequest;

use crate::family::{
    FamilyPluginDescriptor, FamilyPluginRegistry, FamilyProbeReport, FamilyProbeSelection,
    SessionMenuEntry,
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

/// Libraries found under `cores/<stem>/` plus the subdirectories rejected by
/// the layout rule.
#[derive(Debug, Default)]
pub struct FamilyDiscovery {
    pub libraries: Vec<PathBuf>,
    pub errors: Vec<FamilyDirectoryLoadError>,
}

/// Scan the one-level subdirectories of a `cores` directory. Each subdirectory
/// must hold exactly one Family library; dot-directories (the pending area)
/// and top-level files are not cores.
pub fn discover_family_libraries(directory: &Path) -> Result<FamilyDiscovery, FamilyLoadError> {
    let entries = fs::read_dir(directory).map_err(|_| FamilyLoadError::Directory)?;
    let mut discovery = FamilyDiscovery::default();
    let mut subdirectories = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| FamilyLoadError::Directory)?;
        let file_type = entry.file_type().map_err(|_| FamilyLoadError::Directory)?;
        let hidden = entry
            .file_name()
            .to_str()
            .is_none_or(|name| name.starts_with('.'));
        if file_type.is_dir() && !hidden {
            subdirectories.push(entry.path());
        }
    }
    subdirectories.sort();
    for subdirectory in subdirectories {
        let mut libraries = Vec::new();
        for entry in fs::read_dir(&subdirectory).map_err(|_| FamilyLoadError::Directory)? {
            let entry = entry.map_err(|_| FamilyLoadError::Directory)?;
            let file_type = entry.file_type().map_err(|_| FamilyLoadError::Directory)?;
            if file_type.is_file() && is_family_library_path(&entry.path()) {
                libraries.push(entry.path());
            }
        }
        match <[PathBuf; 1]>::try_from(libraries) {
            Ok([library]) => discovery.libraries.push(library),
            Err(_) => discovery.errors.push(FamilyDirectoryLoadError {
                path: subdirectory,
                error: FamilyLoadError::Layout,
            }),
        }
    }
    Ok(discovery)
}

/// Discover cores across directories in priority order: a library whose stem
/// appears in an earlier directory overrides the same stem in later ones.
pub fn discover_core_directories(
    directories: &[PathBuf],
) -> Result<FamilyDiscovery, FamilyLoadError> {
    let mut merged = FamilyDiscovery::default();
    let mut stems = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for directory in directories {
        let discovery = discover_family_libraries(directory)?;
        for library in discovery.libraries {
            let stem = library_stem(&library);
            let identity = fs::canonicalize(&library).map_err(|_| FamilyLoadError::InvalidPath)?;
            if !identities.insert(identity) {
                continue;
            }
            if stems.insert(stem.clone()) {
                merged.libraries.push(library);
            } else {
                tracing::info!(event = "astra.emu.manager.core_overridden", core = %stem);
            }
        }
        merged.errors.extend(discovery.errors);
    }
    Ok(merged)
}

/// Lowercase file stem identifying a core across user and bundled directories.
pub fn library_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
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

    pub fn register_provider(
        &mut self,
        provider: LoadedFamilyPlugin,
    ) -> Result<(), FamilyLoadError> {
        let manager = provider.manager_descriptor().clone();
        let plugin_id = manager.plugin_id.clone();
        if self.providers.contains_key(&plugin_id) || self.rejected_ids.contains(&plugin_id) {
            self.rejected_ids.insert(plugin_id.clone());
            self.remove(&plugin_id);
            return Err(FamilyLoadError::DuplicatePlugin);
        }
        self.descriptors
            .register(manager)
            .map_err(|_| FamilyLoadError::DuplicatePlugin)?;
        self.providers.insert(plugin_id, provider);
        Ok(())
    }

    /// Static factory returns the same process-lifetime C table as a DLL.
    /// # Safety
    /// The query must satisfy `LoadedFamilyPlugin::from_query`, including
    /// process-lifetime code/storage and the native ABI ownership contract.
    pub unsafe fn register_static(
        &mut self,
        query: astra_emu_abi::Query,
    ) -> Result<(), FamilyLoadError> {
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
        let discovery = discover_core_directories(directories)?;
        let mut report = self.load_libraries(discovery.libraries, |_, _| {})?;
        report.errors.extend(discovery.errors);
        Ok(report)
    }

    /// `progress(loaded, total)` runs after each library file is opened.
    pub fn load_libraries(
        &mut self,
        mut paths: Vec<PathBuf>,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<FamilyDirectoryLoadReport, FamilyLoadError> {
        paths.sort();
        paths.dedup();
        let total = paths.len();
        let mut candidates = BTreeMap::<String, (PathBuf, LoadedFamilyPlugin)>::new();
        let mut duplicate_ids = BTreeSet::new();
        let mut report = FamilyDirectoryLoadReport::default();

        for (index, path) in paths.into_iter().enumerate() {
            let loaded = LoadedFamilyPlugin::load(&path);
            progress(index + 1, total);
            let plugin = match loaded {
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

    /// Release every module's explicit resources after its sessions close.
    /// Failures never skip destruction of the remaining module owners.
    pub fn shutdown(&mut self) -> Result<(), FamilyLoadError> {
        self.descriptors = FamilyPluginRegistry::new();
        let mut failure = None;
        for (plugin_id, mut plugin) in std::mem::take(&mut self.providers) {
            if let Err(error) = plugin.shutdown() {
                tracing::error!(event = "astra.emu.module_owner.shutdown_failed", plugin_id, diagnostic_code = %error.code());
                if failure.is_none() {
                    failure = Some(FamilyLoadError::ProviderError(error));
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub fn descriptor(&self, plugin_id: &str) -> Option<&FamilyPluginDescriptor> {
        self.descriptors.descriptor(plugin_id)
    }

    pub fn configuration(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<crate::model::ConfigField>, FamilyLoadError> {
        Ok(self
            .providers
            .get(plugin_id)
            .ok_or(FamilyLoadError::Provider)?
            .configuration()
            .to_vec())
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &FamilyPluginDescriptor> {
        self.descriptors.descriptors()
    }
    pub fn location(&self, plugin_id: &str) -> Option<&Path> {
        self.providers
            .get(plugin_id)
            .map(LoadedFamilyPlugin::location)
    }

    pub fn session_menu(&self, plugin_id: &str) -> &[SessionMenuEntry] {
        self.providers
            .get(plugin_id)
            .map(LoadedFamilyPlugin::session_menu)
            .unwrap_or(&[])
    }

    pub fn probe(
        &self,
        request: &ProbeRequest,
        preferred_plugin_id: Option<&str>,
        preferred_family_id: Option<&str>,
    ) -> Result<FamilyProbeSelection, FamilyLoadError> {
        request.validate().map_err(FamilyLoadError::ProbeError)?;
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

    /// Freshly probe one current provider without invoking unrelated native cores.
    pub fn probe_selected(
        &self,
        request: &ProbeRequest,
        plugin_id: &str,
    ) -> Result<FamilyProbeSelection, FamilyLoadError> {
        request.validate().map_err(FamilyLoadError::ProbeError)?;
        let provider = self
            .providers
            .get(plugin_id)
            .ok_or(FamilyLoadError::Provider)?;
        let report = provider
            .probe(request.clone())
            .map_err(FamilyLoadError::ProbeError)?
            .map(|report| FamilyProbeReport {
                plugin_id: plugin_id.to_owned(),
                family_id: report.family_id.to_string(),
                game_id: report.game_id.to_string(),
                format: report.format.to_string(),
                confidence_permyriad: report.confidence_permyriad,
            });
        self.descriptors
            .select_probe(report, Some(plugin_id), None)
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
        if checked.descriptor != candidate.descriptor {
            return Err(FamilyLoadError::PolicyError(
                "ASTRA_EMU_FAMILY_PROVIDER_CHANGED".into(),
            ));
        }
        let fresh = self.probe_selected(
            &ProbeRequest {
                game_path: request.game_path.clone(),
            },
            &checked.report.plugin_id,
        )?;
        let FamilyProbeSelection::Selected(fresh) = fresh else {
            return Err(FamilyLoadError::PolicyError(
                "ASTRA_EMU_FAMILY_FRESH_PROBE_NO_MATCH".into(),
            ));
        };
        if fresh.report.game_id != checked.report.game_id
            || fresh.report.family_id != checked.report.family_id
            || fresh.report.format != checked.report.format
        {
            return Err(FamilyLoadError::PolicyError(
                "ASTRA_EMU_FAMILY_FRESH_PROBE_CHANGED".into(),
            ));
        }
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
