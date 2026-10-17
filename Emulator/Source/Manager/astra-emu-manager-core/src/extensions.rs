//! Local trusted Luau extensions; no remote installation or resource hooks.
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
const MAX_SOURCE: u64 = 256 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalExtension {
    pub id: String,
    pub enabled: bool,
    pub configuration: serde_json::Value,
    pub source: String,
}
pub struct ExtensionStore { directory: PathBuf }
impl ExtensionStore {
    pub fn new(data: &Path) -> Self { Self { directory: data.join("extensions") } }
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)) { return Err("ASTRA_EMU_EXTENSION_ID".into()); }
        Ok(self.directory.join(format!("{id}.json")))
    }
    pub fn list(&self) -> Result<Vec<LocalExtension>, String> {
        let entries = match fs::read_dir(&self.directory) { Ok(v) => v, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()), Err(e) => return Err(e.to_string()) };
        let mut result = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") { continue; }
            if result.len() == 64 { return Err("ASTRA_EMU_EXTENSION_LIMIT".into()); }
            let mut bytes = Vec::new();
            fs::File::open(&path).map_err(|e| e.to_string())?.take(MAX_SOURCE * 4 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_SOURCE * 4 { return Err("ASTRA_EMU_EXTENSION_SIZE".into()); }
            let extension: LocalExtension = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if self.path(&extension.id)? != path || extension.source.len() as u64 > MAX_SOURCE || !extension.configuration.is_object() { return Err("ASTRA_EMU_EXTENSION_INVALID".into()); }
            result.push(extension);
        }
        result.sort_by(|a,b| a.id.cmp(&b.id));
        Ok(result)
    }
    pub fn install(&self, id: &str, source: &Path) -> Result<(), String> {
        let target = self.path(id)?;
        if target.exists() { return Err("ASTRA_EMU_EXTENSION_ALREADY_INSTALLED".into()); }
        let mut text = String::new();
        fs::File::open(source).map_err(|e| e.to_string())?.take(MAX_SOURCE + 1).read_to_string(&mut text).map_err(|e| e.to_string())?;
        if text.len() as u64 > MAX_SOURCE { return Err("ASTRA_EMU_EXTENSION_SIZE".into()); }
        self.save(&LocalExtension { id: id.into(), enabled: false, configuration: serde_json::json!({}), source: text })
    }
    pub fn configure(&self, id: &str, enabled: Option<bool>, config: Option<serde_json::Value>) -> Result<(), String> {
        let mut entry = self.list()?.into_iter().find(|v| v.id == id).ok_or("ASTRA_EMU_EXTENSION_MISSING")?;
        if let Some(enabled) = enabled { entry.enabled = enabled; }
        if let Some(config) = config {
            if !config.is_object() || serde_json::to_vec(&config).map_err(|e| e.to_string())?.len() > 16384 { return Err("ASTRA_EMU_EXTENSION_CONFIG".into()); }
            entry.configuration = config;
        }
        self.save(&entry)
    }
    fn save(&self, entry: &LocalExtension) -> Result<(), String> {
        fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        let target = self.path(&entry.id)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut temporary, entry).map_err(|e| e.to_string())?;
        temporary.flush().map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        temporary.persist(target).map_err(|e| e.to_string())?;
        Ok(())
    }
}
