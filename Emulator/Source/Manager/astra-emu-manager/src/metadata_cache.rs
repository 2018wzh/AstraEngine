//! Local AstraEMU-metadata cache: `<data_dir>/metadata/` from synchronization
//! or the bundled `<exe_dir>/metadata/` snapshot, whichever is newer.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use astra_emu_metadata::{
    parse_compatibility_database, parse_core_catalog, parse_metadata_index, parse_preset_database,
    CompatibilityDatabase, CoreCatalog, MetadataIndex, PresetDatabase, MAX_METADATA_FILE_BYTES,
    METADATA_INDEX_FILE,
};
use astra_emu_patch::RecipeListing;
use sha2::{Digest, Sha256};

pub(crate) const METADATA_DIRECTORY: &str = "metadata";
const STAGING_DIRECTORY: &str = "metadata.staging";
const PREVIOUS_DIRECTORY: &str = "metadata.old";
const COMPATIBILITY_FILE: &str = "compatibility.json";
const PRESETS_FILE: &str = "presets.json";
const CORES_FILE: &str = "cores.json";
const PATCH_CATALOG_FILE: &str = "patches/catalog.json";
const PATCH_DIRECTORY: &str = "patches";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetadataSource {
    Synchronized,
    Bundled,
}

#[derive(Debug, Clone)]
pub(crate) struct MetadataCatalog {
    pub(crate) root: PathBuf,
    pub(crate) source: MetadataSource,
    pub(crate) index: MetadataIndex,
    pub(crate) compatibility: CompatibilityDatabase,
    pub(crate) presets: PresetDatabase,
    pub(crate) cores: CoreCatalog,
    pub(crate) recipes: Vec<RecipeListing>,
}

impl MetadataCatalog {
    /// Read and verify every indexed file, then parse the databases and the
    /// patch catalog. Any failure rejects the whole directory.
    pub(crate) fn load(root: &Path, source: MetadataSource) -> Result<Self, String> {
        let index = parse_metadata_index(&read_bounded(&root.join(METADATA_INDEX_FILE))?)
            .map_err(|error| error.to_string())?;
        let mut contents = BTreeMap::new();
        for file in &index.files {
            let bytes = read_bounded(&indexed_path(root, &file.path))?;
            if bytes.len() as u64 != file.size || sha256_hex(&bytes) != file.sha256 {
                return Err(format!("ASTRA_EMU_METADATA_HASH_MISMATCH: {}", file.path));
            }
            if matches!(
                file.path.as_str(),
                COMPATIBILITY_FILE | PRESETS_FILE | CORES_FILE
            ) {
                contents.insert(file.path.as_str(), bytes);
            }
        }
        let required = |name: &str| {
            contents
                .get(name)
                .ok_or_else(|| format!("ASTRA_EMU_METADATA_CACHE_INVALID: {name}"))
        };
        let compatibility = parse_compatibility_database(required(COMPATIBILITY_FILE)?)
            .map_err(|error| error.to_string())?;
        let presets =
            parse_preset_database(required(PRESETS_FILE)?).map_err(|error| error.to_string())?;
        let cores = parse_core_catalog(required(CORES_FILE)?).map_err(|error| error.to_string())?;
        let recipes = if index
            .files
            .iter()
            .any(|file| file.path == PATCH_CATALOG_FILE)
        {
            astra_emu_patch::list_recipes(&root.join(PATCH_DIRECTORY))
                .map_err(|error| format!("ASTRA_EMU_METADATA_CACHE_INVALID: {error}"))?
        } else {
            Vec::new()
        };
        Ok(Self {
            root: root.to_owned(),
            source,
            index,
            compatibility,
            presets,
            cores,
            recipes,
        })
    }

    /// `path → sha256` of the files present in this catalog's directory.
    pub(crate) fn cached_files(&self) -> BTreeMap<String, String> {
        self.index
            .files
            .iter()
            .map(|file| (file.path.clone(), file.sha256.clone()))
            .collect()
    }

    pub(crate) fn patch_directory(&self) -> PathBuf {
        self.root.join(PATCH_DIRECTORY)
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn indexed_path(root: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(root.to_owned(), |path, part| path.join(part))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|_| "ASTRA_EMU_METADATA_CACHE_READ".to_owned())?;
    if !metadata.is_file() || metadata.len() > MAX_METADATA_FILE_BYTES {
        return Err("ASTRA_EMU_METADATA_CACHE_READ".into());
    }
    fs::read(path).map_err(|_| "ASTRA_EMU_METADATA_CACHE_READ".into())
}

pub(crate) fn bundled_directory() -> Option<PathBuf> {
    Some(
        std::env::current_exe()
            .ok()?
            .parent()?
            .join(METADATA_DIRECTORY),
    )
}

fn load_optional(root: &Path, source: MetadataSource) -> Option<MetadataCatalog> {
    if !root.join(METADATA_INDEX_FILE).is_file() {
        return None;
    }
    match MetadataCatalog::load(root, source) {
        Ok(catalog) => Some(catalog),
        Err(error) => {
            tracing::warn!(
                event = "astra.emu.metadata.cache_rejected",
                source = ?source,
                diagnostic_code = %error
            );
            None
        }
    }
}

/// The synchronized cache wins when it is at least as new as the bundled one.
pub(crate) fn select_catalog(data_dir: &Path, bundled: Option<&Path>) -> Option<MetadataCatalog> {
    let synchronized = load_optional(
        &data_dir.join(METADATA_DIRECTORY),
        MetadataSource::Synchronized,
    );
    let bundled = bundled.and_then(|root| load_optional(root, MetadataSource::Bundled));
    match (synchronized, bundled) {
        (Some(synchronized), Some(bundled))
            if bundled.index.generated_at_unix_ms > synchronized.index.generated_at_unix_ms =>
        {
            Some(bundled)
        }
        (Some(synchronized), _) => Some(synchronized),
        (None, bundled) => bundled,
    }
}

fn remove_directory(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn cache_write(_: io::Error) -> String {
    "ASTRA_EMU_METADATA_CACHE_WRITE".into()
}

/// Write a synchronized index into `metadata.staging/`, verify it as a whole
/// and swap it in for `<data_dir>/metadata/`. Files not downloaded are copied
/// from `current`. On any failure the previous cache stays in place.
pub(crate) fn store_synchronized(
    data_dir: &Path,
    current: Option<&MetadataCatalog>,
    index: &MetadataIndex,
    files: Vec<(String, Vec<u8>)>,
) -> Result<MetadataCatalog, String> {
    let staging = data_dir.join(STAGING_DIRECTORY);
    remove_directory(&staging).map_err(cache_write)?;
    let result = (|| {
        fs::create_dir_all(&staging).map_err(cache_write)?;
        let mut downloaded: BTreeMap<String, Vec<u8>> = files.into_iter().collect();
        let reusable = current
            .map(MetadataCatalog::cached_files)
            .unwrap_or_default();
        for file in &index.files {
            let bytes = match downloaded.remove(&file.path) {
                Some(bytes) => bytes,
                None => match current.filter(|_| reusable.get(&file.path) == Some(&file.sha256)) {
                    Some(catalog) => read_bounded(&indexed_path(&catalog.root, &file.path))?,
                    None => return Err(format!("ASTRA_EMU_METADATA_HASH_MISMATCH: {}", file.path)),
                },
            };
            let destination = indexed_path(&staging, &file.path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(cache_write)?;
            }
            fs::write(&destination, bytes).map_err(cache_write)?;
        }
        let encoded =
            serde_json::to_vec(index).map_err(|_| "ASTRA_EMU_METADATA_CACHE_WRITE".to_owned())?;
        fs::write(staging.join(METADATA_INDEX_FILE), encoded).map_err(cache_write)?;
        MetadataCatalog::load(&staging, MetadataSource::Synchronized)
    })();
    let mut catalog = match result {
        Ok(catalog) => catalog,
        Err(error) => {
            let _ = remove_directory(&staging);
            return Err(error);
        }
    };
    let target = data_dir.join(METADATA_DIRECTORY);
    let previous = data_dir.join(PREVIOUS_DIRECTORY);
    let swapped = (|| {
        remove_directory(&previous)?;
        let had_previous = target.exists();
        if had_previous {
            fs::rename(&target, &previous)?;
        }
        if let Err(error) = fs::rename(&staging, &target) {
            if had_previous {
                fs::rename(&previous, &target)?;
            }
            return Err(error);
        }
        remove_directory(&previous)
    })();
    if let Err(error) = swapped {
        let _ = remove_directory(&staging);
        return Err(cache_write(error));
    }
    catalog.root = target;
    Ok(catalog)
}

#[cfg(test)]
#[path = "metadata_cache_tests.rs"]
pub(crate) mod tests;
