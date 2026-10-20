use super::*;
use astra_emu_manager_core::{is_family_library_path, FamilyCapability};
use astra_emu_manager_ui_slint::{describe_error, CoreViewModel, CoresViewModel};
use std::io::{self, BufRead, Seek, SeekFrom};

const PENDING: &str = ".pending";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Install,
    Remove,
}

fn pending_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cores").join(PENDING)
}

fn pending_entries(data_dir: &Path) -> Result<BTreeMap<String, PendingKind>, String> {
    let directory = pending_dir(data_dir);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("ASTRA_EMU_CORE_PENDING_APPLY: {}", error.kind())),
    };
    let mut pending = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("ASTRA_EMU_CORE_PENDING_APPLY: {}", error.kind()))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(file) = name.strip_suffix(".remove") {
            if is_family_library_path(Path::new(file)) {
                pending.insert(file.to_owned(), PendingKind::Remove);
            }
        } else if is_family_library_path(Path::new(name)) {
            pending.entry(name.to_owned()).or_insert(PendingKind::Install);
        }
    }
    Ok(pending)
}

fn pending_apply_error(file: &str, error: &io::Error) -> (String, String) {
    tracing::error!(
        event = "astra.emu.manager.core_pending_failed",
        core = %file,
        error = %error
    );
    (
        file.to_owned(),
        format!("ASTRA_EMU_CORE_PENDING_APPLY: {}", error.kind()),
    )
}

pub(super) fn apply_pending(cores: &Path) -> Vec<(String, String)> {
    let pending = cores.join(PENDING);
    let entries = match fs::read_dir(&pending) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            return vec![pending_apply_error("<pending>", &error)];
        }
    };
    let mut removes = Vec::new();
    let mut installs = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(file) = name.strip_suffix(".remove") {
            if is_family_library_path(Path::new(file)) {
                removes.push((file.to_owned(), pending.join(name)));
            }
        } else if is_family_library_path(Path::new(name)) {
            installs.push((name.to_owned(), pending.join(name)));
        }
    }
    let mut failures = Vec::new();
    for (file, marker) in removes {
        let target = cores.join(&file);
        match fs::remove_file(&target) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                failures.push(pending_apply_error(&file, &error));
                continue;
            }
        }
        if let Err(error) = fs::remove_file(&marker) {
            failures.push(pending_apply_error(&file, &error));
        }
    }
    for (file, source) in installs {
        let target = cores.join(&file);
        match fs::rename(&source, &target) {
            Ok(()) => {}
            Err(error) => failures.push(pending_apply_error(&file, &error)),
        }
    }
    failures
}

fn capability_label(capability: FamilyCapability) -> &'static str {
    match capability {
        FamilyCapability::CpuFrame => "软件渲染",
        FamilyCapability::RenderExtent => "可调渲染分辨率",
        FamilyCapability::TextReplacement => "支持实时翻译",
    }
}

fn monogram(family_id: &str) -> String {
    family_id.chars().take(3).collect::<String>().to_uppercase()
}

impl AstraEmuManagerController {
    pub(super) fn cores_view(&self) -> Result<CoresViewModel, String> {
        let pending = pending_entries(&self.data_dir)?;
        let mut loaded = Vec::new();
        let mut loaded_files = BTreeSet::new();
        for meta in self.registry.core_metadata() {
            loaded_files.insert(meta.file.clone());
            let state = match pending.get(&meta.file) {
                Some(PendingKind::Remove) => "pending-removal",
                Some(PendingKind::Install) => "pending-update",
                None => "loaded",
            };
            loaded.push(CoreViewModel {
                key: meta.descriptor.plugin_id.clone(),
                name: meta.descriptor.display_name.clone(),
                monogram: monogram(&meta.descriptor.family_id),
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
                file: meta.file.clone(),
                message: String::new(),
                removable: !meta.bundled,
            });
        }
        loaded.sort_by(|left, right| left.name.cmp(&right.name));
        let mut items = loaded;
        for (file, raw) in &self.plugin_errors {
            let described = describe_error(raw);
            items.push(CoreViewModel {
                key: file.clone(),
                name: file.clone(),
                monogram: String::new(),
                version: String::new(),
                capabilities: Vec::new(),
                state: "failed".into(),
                source: "user".into(),
                file: file.clone(),
                message: described.title,
                removable: self.data_dir.join("cores").join(file).is_file(),
            });
        }
        for (file, kind) in &pending {
            if *kind == PendingKind::Install && !loaded_files.contains(file) {
                items.push(CoreViewModel {
                    key: file.clone(),
                    name: file.clone(),
                    monogram: String::new(),
                    version: String::new(),
                    capabilities: Vec::new(),
                    state: "pending-install".into(),
                    source: "user".into(),
                    file: file.clone(),
                    message: String::new(),
                    removable: true,
                });
            }
        }
        Ok(CoresViewModel {
            items,
            restart_required: !pending.is_empty() && self.cores_started(),
            defaults: Default::default(),
        })
    }

    pub(super) fn install_core(&mut self, source: &Path) -> Result<ManagerViewModel, String> {
        if !is_family_library_path(source) {
            return Err("ASTRA_EMU_CORE_INSTALL_NAME".into());
        }
        let file = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "ASTRA_EMU_CORE_INSTALL_NAME".to_owned())?;
        let pending = pending_dir(&self.data_dir);
        fs::create_dir_all(&pending).map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
        let mut input = fs::File::open(source).map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
        let mut staged = tempfile::NamedTempFile::new_in(&pending)
            .map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
        io::copy(&mut input, staged.as_file_mut()).map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
        staged.as_file().sync_all().map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
        staged.persist(pending.join(file)).map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
        match fs::remove_file(pending.join(format!("{file}.remove"))) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err("ASTRA_EMU_CORE_INSTALL_COPY".into()),
        }
        self.model()
    }

    pub(super) fn uninstall_core(&mut self, file: &str) -> Result<ManagerViewModel, String> {
        if file.is_empty()
            || file.contains(['/', '\\'])
            || file.split(['/', '\\']).any(|part| part == "..")
            || file.contains("..")
        {
            return Err("ASTRA_EMU_CORE_INSTALL_NAME".into());
        }
        let pending = pending_dir(&self.data_dir);
        let cores = self.data_dir.join("cores");
        let pending_file = pending.join(file);
        let installed = cores.join(file);
        if pending_file.exists() && !installed.exists() {
            fs::remove_file(&pending_file).map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
            return self.model();
        }
        if installed.exists() {
            fs::create_dir_all(&pending).map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
            match fs::remove_file(&pending_file) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err("ASTRA_EMU_CORE_INSTALL_COPY".into()),
            }
            fs::write(pending.join(format!("{file}.remove")), []).map_err(|_| {
                "ASTRA_EMU_CORE_INSTALL_COPY"
            })?;
            return self.model();
        }
        Err("ASTRA_EMU_CORE_UNINSTALL_BUNDLED".into())
    }

    pub(super) fn log_text(&self, core: &str) -> Result<(String, usize), String> {
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
    fn apply_pending_renames_installs_and_honors_remove_markers() {
        let root = tempfile::tempdir().unwrap();
        let cores = root.path().join("cores");
        fs::create_dir_all(cores.join(PENDING)).unwrap();
        fs::write(cores.join("astra_emu_y.dll"), b"old").unwrap();
        fs::write(cores.join(PENDING).join("astra_emu_x.dll"), b"new").unwrap();
        fs::write(cores.join(PENDING).join("astra_emu_y.dll.remove"), b"").unwrap();
        let failures = apply_pending(&cores);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(fs::read(cores.join("astra_emu_x.dll")).unwrap(), b"new");
        assert!(!cores.join("astra_emu_y.dll").exists());
        assert!(!cores.join(PENDING).join("astra_emu_x.dll").exists());
        assert!(!cores.join(PENDING).join("astra_emu_y.dll.remove").exists());
    }

    #[cfg(windows)]
    #[test]
    fn failed_update_preserves_installed_core_and_pending_bytes() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let cores = root.path().join("cores");
        let pending = cores.join(PENDING);
        fs::create_dir_all(&pending).unwrap();
        let name = "astra_emu_fixture.dll";
        fs::write(cores.join(name), b"installed").unwrap();
        fs::write(pending.join(name), b"replacement").unwrap();
        fs::write(pending.join(".interrupted-copy"), b"partial").unwrap();
        let locked = fs::OpenOptions::new().read(true).share_mode(0)
            .open(pending.join(name)).unwrap();
        assert_eq!(apply_pending(&cores).len(), 1);
        assert_eq!(fs::read(cores.join(name)).unwrap(), b"installed");
        drop(locked);
        assert_eq!(fs::read(pending.join(name)).unwrap(), b"replacement");
        assert!(apply_pending(&cores).is_empty());
        assert_eq!(fs::read(cores.join(name)).unwrap(), b"replacement");
        assert!(!cores.join(".interrupted-copy").exists());
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
                r#"{"timestamp":"2024-01-02T03:04:06.000Z","level":"ERROR","event":"plugin","fields":{"core":"astra_emu_fvp.dll"}}"#,
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
        let (_, count) = app.log_text("astra_emu_fvp.dll").unwrap();
        assert_eq!(count, 1);
        let (_, count_all) = app.log_text("").unwrap();
        assert_eq!(count_all, 3);
    }
}
