use super::*;
use crate::core_package::{self, PendingKind};
use astra_emu_manager_core::{FamilyCapability, INDEPENDENT_FAMILY_ABI_FINGERPRINT};
use astra_emu_manager_ui_slint::{describe_error, CoreViewModel, CoresViewModel};
use astra_emu_metadata::CoreCatalogEntry;
use std::io::{self, BufRead, Seek, SeekFrom};

/// Release platform of this Manager build, e.g. `windows-x86_64`.
pub(super) fn core_platform() -> String {
    format!("{}-{}", env::consts::OS, env::consts::ARCH)
}

fn is_newer(candidate: &str, installed: &str) -> bool {
    match (
        semver::Version::parse(candidate),
        semver::Version::parse(installed),
    ) {
        (Ok(candidate), Ok(installed)) => candidate > installed,
        _ => false,
    }
}

fn capability_label(capability: FamilyCapability) -> &'static str {
    match capability {
        FamilyCapability::CpuFrame => "软件渲染",
        FamilyCapability::RenderExtent => "可调渲染分辨率",
        FamilyCapability::TextReplacement => "支持实时翻译",
    }
}

impl AstraEmuManagerController {
    fn cores_dir(&self) -> PathBuf {
        self.data_dir.join("cores")
    }

    /// Highest released version of a core for this platform and ABI.
    pub(super) fn online_core(&self, plugin_id: &str) -> Option<&CoreCatalogEntry> {
        self.metadata_catalog.as_ref()?.cores.latest(
            plugin_id,
            &core_platform(),
            INDEPENDENT_FAMILY_ABI_FINGERPRINT,
        )
    }

    /// Loaded, failed, pending and online-available cores in one list, plus each
    /// item's core directory stem (empty for online-only entries).
    fn core_items(&self) -> Result<(CoresViewModel, Vec<String>), String> {
        let cores = self.cores_dir();
        let pending = core_package::pending_entries(&cores)?;
        let mut items = Vec::new();
        let mut listed_plugins = BTreeSet::new();
        let mut loaded_stems = BTreeSet::new();
        for meta in self.registry.core_metadata() {
            loaded_stems.insert(meta.stem.clone());
            listed_plugins.insert(meta.descriptor.plugin_id.clone());
            let state = match pending.get(&meta.stem) {
                Some(PendingKind::Remove) => "pending-removal",
                Some(PendingKind::Install) => "pending-update",
                None => "loaded",
            };
            let update_version = self
                .online_core(&meta.descriptor.plugin_id)
                .filter(|entry| is_newer(&entry.version, &meta.descriptor.version))
                .map(|entry| entry.version.clone())
                .unwrap_or_default();
            items.push((
                CoreViewModel {
                    id: meta.descriptor.plugin_id.clone(),
                    name: meta.descriptor.display_name.clone(),
                    engine: meta.descriptor.family_name.clone(),
                    version: meta.descriptor.version.clone(),
                    capabilities: meta
                        .descriptor
                        .capabilities
                        .iter()
                        .copied()
                        .map(capability_label)
                        .map(str::to_owned)
                        .collect(),
                    state: state.into(),
                    source: if meta.bundled { "bundled" } else { "user" }.into(),
                    message: String::new(),
                    removable: !meta.bundled,
                    update_version,
                },
                meta.stem.clone(),
            ));
        }
        items.sort_by(|left, right| left.0.name.cmp(&right.0.name));
        let updates_available = items
            .iter()
            .filter(|(item, _)| !item.update_version.is_empty())
            .count();
        for (stem, raw) in &self.plugin_errors {
            let described = describe_error(raw);
            items.push((
                CoreViewModel {
                    id: stem.clone(),
                    name: stem.clone(),
                    state: "failed".into(),
                    source: "user".into(),
                    message: described.title,
                    removable: cores.join(stem).is_dir(),
                    ..CoreViewModel::default()
                },
                stem.clone(),
            ));
        }
        for (stem, kind) in &pending {
            if *kind != PendingKind::Install || loaded_stems.contains(stem) {
                continue;
            }
            let manifest =
                core_package::read_manifest(&cores.join(core_package::PENDING).join(stem));
            let plugin_id = manifest.as_ref().map(|manifest| manifest.plugin_id.clone());
            let online = plugin_id.as_deref().and_then(|id| self.online_core(id));
            let name = online.map_or_else(|| stem.clone(), |entry| entry.display_name.clone());
            let engine = online
                .map(|entry| entry.family_name.clone())
                .unwrap_or_default();
            listed_plugins.extend(plugin_id.clone());
            items.push((
                CoreViewModel {
                    id: plugin_id.unwrap_or_else(|| stem.clone()),
                    name,
                    engine,
                    version: manifest.map(|manifest| manifest.version).unwrap_or_default(),
                    state: "pending-install".into(),
                    source: "user".into(),
                    removable: true,
                    ..CoreViewModel::default()
                },
                stem.clone(),
            ));
        }
        if let Some(catalog) = &self.metadata_catalog {
            let available: BTreeSet<&str> = catalog
                .cores
                .entries
                .iter()
                .map(|entry| entry.plugin_id.as_str())
                .filter(|plugin_id| !listed_plugins.contains(*plugin_id))
                .collect();
            for entry in available.into_iter().filter_map(|id| self.online_core(id)) {
                items.push((
                    CoreViewModel {
                        id: entry.plugin_id.clone(),
                        name: entry.display_name.clone(),
                        engine: entry.family_name.clone(),
                        version: entry.version.clone(),
                        state: "available".into(),
                        source: "online".into(),
                        ..CoreViewModel::default()
                    },
                    String::new(),
                ));
            }
        }
        let (items, stems) = items.into_iter().unzip();
        Ok((
            CoresViewModel {
                items,
                restart_required: !pending.is_empty() && self.cores_started(),
                updates_available: i32::try_from(updates_available).unwrap_or(i32::MAX),
                ..CoresViewModel::default()
            },
            stems,
        ))
    }

    pub(super) fn cores_view(&self) -> Result<CoresViewModel, String> {
        self.core_items().map(|(view, _)| view)
    }

    /// Stage a local `.zip` core package or a single core library.
    pub(super) fn install_core(&mut self, source: &Path) -> Result<ManagerViewModel, String> {
        let pending = self.cores_dir().join(core_package::PENDING);
        let package = source
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"));
        if package {
            core_package::stage_package(source, None, &pending)?;
        } else {
            core_package::stage_library(source, &pending)?;
        }
        self.model()
    }

    /// Download the newest released package of a core into the pending area.
    pub(super) fn install_online_core(
        &mut self,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.require_network()?;
        let entry = self
            .online_core(plugin_id)
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_CORE_NOT_AVAILABLE".to_owned())?;
        if self
            .pending_metadata
            .values()
            .any(|pending| pending.core.as_deref() == Some(plugin_id))
        {
            return Err("ASTRA_EMU_CORE_DOWNLOAD_PENDING".into());
        }
        let request_id = self.next_metadata_id("core")?;
        self.metadata.submit(MetadataCommand {
            request_id: request_id.clone(),
            case_identity: String::new(),
            provider: MetadataProviderId::Vndb,
            access_token: None,
            allow_sensitive_cover: false,
            kind: MetadataCommandKind::InstallCore {
                entry: Box::new(entry),
                pending: self.cores_dir().join(core_package::PENDING),
            },
        })?;
        self.pending_metadata.insert(
            request_id,
            PendingMetadata {
                game_id: None,
                core: Some(plugin_id.into()),
            },
        );
        self.model()
    }

    pub(super) fn core_staged(&mut self, stem: &str, version: &str) {
        tracing::info!(event = "astra.emu.manager.core_staged", core = %stem, version = %version);
        self.show_snackbar(
            "核心已下载，重启后生效",
            Some(snackbar::SnackbarAction::Restart),
        );
    }

    /// Core directory stem of a core list entry (plugin id or stem).
    fn core_stem(&self, core_id: &str) -> Result<Option<String>, String> {
        let (view, stems) = self.core_items()?;
        Ok(view
            .items
            .iter()
            .zip(stems)
            .find(|(item, stem)| item.id == core_id && !stem.is_empty())
            .map(|(_, stem)| stem))
    }

    /// Cancel a pending install, or mark an installed user core for removal.
    pub(super) fn uninstall_core(&mut self, core_id: &str) -> Result<ManagerViewModel, String> {
        let stem = self
            .core_stem(core_id)?
            .ok_or("ASTRA_EMU_CORE_UNINSTALL_BUNDLED")?;
        let stem = stem.as_str();
        let cores = self.cores_dir();
        let staged = cores.join(core_package::PENDING).join(stem);
        let installed = cores.join(stem);
        if staged.is_dir() && !installed.is_dir() {
            core_package::cancel_pending_install(&cores, stem)?;
        } else if installed.is_dir() {
            core_package::mark_removal(&cores, stem)?;
        } else {
            return Err("ASTRA_EMU_CORE_UNINSTALL_BUNDLED".into());
        }
        self.model()
    }

    /// Diagnostics mentioning a core, addressed by plugin id or directory stem.
    pub(super) fn log_text(&self, core: &str) -> Result<(String, usize), String> {
        let stem = if core.is_empty() {
            None
        } else {
            self.core_stem(core)?
        };
        let core = stem.as_deref().unwrap_or(core);
        let path = self.data_dir.join("diagnostics").join("astra.jsonl");
        let mut file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((String::new(), 0));
            }
            Err(_) => return Err("ASTRA_EMU_LOG_READ".into()),
        };
        let len = file.metadata().map_err(|_| "ASTRA_EMU_LOG_READ")?.len();
        let start = len.saturating_sub(1024 * 1024);
        file.seek(SeekFrom::Start(start))
            .map_err(|_| "ASTRA_EMU_LOG_READ")?;
        let mut reader = io::BufReader::new(file);
        if start != 0 {
            let mut discarded = String::new();
            reader
                .read_line(&mut discarded)
                .map_err(|_| "ASTRA_EMU_LOG_READ")?;
        }
        let mut kept = Vec::new();
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => return Err("ASTRA_EMU_LOG_READ".into()),
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            if core.is_empty() || log_matches_core(&value, core) {
                kept.push(render_log_line(&value));
            }
        }
        let count = kept.len();
        let skip = count.saturating_sub(200);
        let mut lines = kept.split_off(skip);
        lines.reverse();
        Ok((lines.join("\n"), count))
    }

    pub(super) fn engine_action(&mut self, control: u32) -> Result<(), String> {
        let (epoch, plugin_id) = {
            let active = self
                .active
                .as_ref()
                .filter(|active| active.opened && active.closing.is_none())
                .ok_or_else(|| "ASTRA_EMU_ENGINE_ACTION_UNAVAILABLE".to_owned())?;
            let plugin_id = self
                .candidates
                .get(&active.game_id)
                .map(|candidate| candidate.descriptor.plugin_id.clone())
                .ok_or_else(|| "ASTRA_EMU_ENGINE_ACTION_UNAVAILABLE".to_owned())?;
            (active.epoch, plugin_id)
        };
        let listed = self.registry.core_metadata().any(|meta| {
            meta.descriptor.plugin_id == plugin_id
                && meta.menu.iter().any(|item| item.control == control)
        });
        if !listed {
            return Err("ASTRA_EMU_ENGINE_ACTION_UNAVAILABLE".into());
        }
        self.action_message.clear();
        self.registry.action(epoch, control)
    }
}

fn log_event(value: &serde_json::Value) -> Option<&str> {
    value
        .get("event")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("fields")
                .and_then(|fields| fields.get("event"))
                .and_then(serde_json::Value::as_str)
        })
}

fn log_fields(value: &serde_json::Value) -> Option<&serde_json::Value> {
    value.get("fields")
}

fn json_contains_core(value: &serde_json::Value, core: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text.contains(core),
        serde_json::Value::Array(items) => items.iter().any(|item| json_contains_core(item, core)),
        serde_json::Value::Object(map) => map.values().any(|item| json_contains_core(item, core)),
        _ => false,
    }
}

fn log_matches_core(value: &serde_json::Value, core: &str) -> bool {
    log_event(value).is_some_and(|event| event.contains(core))
        || log_fields(value).is_some_and(|fields| json_contains_core(fields, core))
}

fn render_field_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn render_log_line(value: &serde_json::Value) -> String {
    let timestamp = value
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let stamp = if timestamp.len() >= 19 {
        timestamp[5..19].replace('T', " ")
    } else {
        timestamp.replace('T', " ")
    };
    let level = value
        .get("level")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_uppercase();
    let event = log_event(value).unwrap_or("");
    let mut pairs = Vec::new();
    if let Some(fields) = log_fields(value).and_then(serde_json::Value::as_object) {
        let mut keys: Vec<_> = fields.keys().cloned().collect();
        keys.sort();
        for key in keys {
            if let Some(field) = fields.get(&key) {
                pairs.push(format!("{key}={}", render_field_value(field)));
            }
        }
    }
    let rendered = format!("{stamp} {level} {event}");
    if pairs.is_empty() {
        rendered
    } else {
        format!("{rendered} {}", pairs.join(" "))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cores_view_lists_online_cores_and_gates_downloads() {
        let root = tempfile::tempdir().unwrap();
        let mut app = AstraEmuManagerController::open_library(
            root.path().into(),
            FrameMailbox::new(),
            Vec::new(),
        )
        .unwrap();
        let entry = |version: &str| CoreCatalogEntry {
            plugin_id: "astra.emu.musica".into(),
            display_name: "Astra Musica".into(),
            family_name: "Musica".into(),
            version: version.into(),
            abi: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
            platform: core_platform(),
            url: format!(
                "https://github.com/2018wzh/AstraEngine/releases/download/emu-v{version}/core.zip"
            ),
            sha256: "a".repeat(64),
            size: 1,
            license: "MIT".into(),
            release_tag: format!("emu-v{version}"),
        };
        let mut catalog = crate::metadata_cache::tests::sample_catalog(root.path());
        catalog.cores.entries = vec![entry("0.2.0"), entry("0.3.0")];
        app.metadata_catalog = Some(catalog);
        let view = app.cores_view().unwrap();
        assert_eq!(view.items.len(), 1);
        assert_eq!(view.items[0].state, "available");
        assert_eq!(view.items[0].source, "online");
        assert_eq!(view.items[0].version, "0.3.0");
        assert_eq!(view.updates_available, 0);
        assert_eq!(
            app.install_online_core("astra.emu.musica").unwrap_err(),
            "ASTRA_EMU_NETWORK_PERMISSION_REQUIRED"
        );
        app.network_allowed = true;
        assert_eq!(
            app.install_online_core("astra.emu.other").unwrap_err(),
            "ASTRA_EMU_CORE_NOT_AVAILABLE"
        );
        assert!(is_newer("0.10.0", "0.9.1"));
        assert!(!is_newer("0.2.0", "0.2.0"));
    }

    #[test]
    fn log_text_filters_by_core_and_counts_all() {
        let root = tempfile::tempdir().unwrap();
        let diagnostics = root.path().join("diagnostics");
        fs::create_dir_all(&diagnostics).unwrap();
        fs::write(
            diagnostics.join("astra.jsonl"),
            concat!(
                r#"{"timestamp":"2024-01-02T03:04:05.000Z","level":"INFO","event":"alpha","fields":{"x":"1"}}"#,
                "\n",
                r#"{"timestamp":"2024-01-02T03:04:06.000Z","level":"ERROR","event":"plugin","fields":{"core":"astra_emu_rfvp.dll"}}"#,
                "\n",
                r#"{"timestamp":"2024-01-02T03:04:07.000Z","level":"WARN","event":"gamma","fields":{"y":"2"}}"#,
                "\n",
            ),
        )
        .unwrap();
        let app = AstraEmuManagerController::open_library(
            root.path().into(),
            FrameMailbox::new(),
            Vec::new(),
        )
        .unwrap();
        let (_, count) = app.log_text("astra_emu_rfvp.dll").unwrap();
        assert_eq!(count, 1);
        let (_, count_all) = app.log_text("").unwrap();
        assert_eq!(count_all, 3);
    }
}
