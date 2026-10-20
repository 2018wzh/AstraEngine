//! AstraEMU-metadata synchronization and the per-game views derived from it:
//! compatibility matching, recommended configuration and patch marking.

use super::*;
use crate::metadata_cache::{MetadataCatalog, MetadataSource};
use astra_emu_manager_ui_slint::{CompatViewModel, NetworkViewModel, RecipeViewModel};
use astra_emu_metadata::{Preset, SyncOutcome};
use serde::{Deserialize, Serialize};

pub(super) const METADATA_SYNC_KEY: &str = "metadata.sync";
const REPORT_URL: &str =
    "https://github.com/2018wzh/AstraEMU-metadata/issues/new?template=compatibility.yml";
pub(super) const RELEASE_HINT: &str = "选择发行版本以查看兼容性";
pub(super) const VNDB_LINK_HINT: &str = "先关联 VNDB 作品";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SyncRecord {
    pub(super) etag: Option<String>,
    pub(super) synced_at_unix_ms: Option<i64>,
}

#[derive(Debug, Default)]
pub(super) struct SyncState {
    pub(super) record: SyncRecord,
    pub(super) request: Option<String>,
}

/// The release a game is matched by: the pinned rID, otherwise the only rID
/// recorded for its vID. `None` when undecided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CompatibilityMatch<'a> {
    NotLinked,
    ReleaseUndecided,
    NoEmulator { release_id: String },
    Found(&'a CompatibilityEntry),
    NoRecord { release_id: String },
}

impl AstraEmuManagerController {
    fn vndb_id(identities: &[astra_emu_manager_core::ExternalIdentityRecord]) -> Option<&str> {
        identities
            .iter()
            .find(|identity| identity.provider == MetadataProviderId::Vndb.as_str())
            .map(|identity| identity.remote_id.as_str())
    }

    /// rID used for matching: the pinned one, or the only rID with records for
    /// this vID in either database.
    fn matched_release(&self, game_id: &str, vn_id: &str) -> Option<String> {
        if let Some(pinned) = self.pinned_releases.get(game_id) {
            return Some(pinned.clone());
        }
        let catalog = self.metadata_catalog.as_ref()?;
        let releases: BTreeSet<&str> = catalog
            .compatibility
            .entries
            .iter()
            .filter(|entry| entry.vn_id == vn_id)
            .map(|entry| entry.release_id.as_str())
            .chain(
                catalog
                    .presets
                    .entries
                    .iter()
                    .filter(|preset| preset.vn_id == vn_id)
                    .map(|preset| preset.release_id.as_str()),
            )
            .collect();
        let mut releases = releases.into_iter();
        match (releases.next(), releases.next()) {
            (Some(only), None) => Some(only.to_owned()),
            _ => None,
        }
    }

    fn game_emulator(&self, game_id: &str) -> Option<&str> {
        self.candidates
            .get(game_id)
            .map(|candidate| candidate.report.plugin_id.as_str())
    }

    pub(super) fn compatibility_match(
        &self,
        game_id: &str,
        identities: &[astra_emu_manager_core::ExternalIdentityRecord],
    ) -> CompatibilityMatch<'_> {
        let Some(vn_id) = Self::vndb_id(identities) else {
            return CompatibilityMatch::NotLinked;
        };
        let Some(release_id) = self.matched_release(game_id, vn_id) else {
            return CompatibilityMatch::ReleaseUndecided;
        };
        let Some(emulator) = self.game_emulator(game_id) else {
            return CompatibilityMatch::NoEmulator { release_id };
        };
        match self
            .metadata_catalog
            .as_ref()
            .and_then(|catalog| catalog.compatibility.entry(&release_id, emulator))
        {
            Some(entry) => CompatibilityMatch::Found(entry),
            None => CompatibilityMatch::NoRecord { release_id },
        }
    }

    /// Card badge and filter value: the matched status, empty when unrated.
    pub(super) fn compatibility_status(
        &self,
        game_id: &str,
        identities: &[astra_emu_manager_core::ExternalIdentityRecord],
    ) -> String {
        match self.compatibility_match(game_id, identities) {
            CompatibilityMatch::Found(entry) => entry.status.as_str().into(),
            _ => String::new(),
        }
    }

    pub(super) fn report_url(
        &self,
        game_id: &str,
        identities: &[astra_emu_manager_core::ExternalIdentityRecord],
    ) -> String {
        let Some(vn_id) = Self::vndb_id(identities) else {
            return String::new();
        };
        let release_id = self.matched_release(game_id, vn_id).unwrap_or_default();
        let candidate = self.candidates.get(game_id);
        let emulator = candidate
            .map(|candidate| candidate.report.plugin_id.as_str())
            .unwrap_or_default();
        let version = candidate
            .map(|candidate| candidate.descriptor.version.as_str())
            .unwrap_or_default();
        let encode = |value: &str| -> String {
            url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
        };
        format!(
            "{REPORT_URL}&vid={}&rid={}&emulator={}&platform=windows&emulator_version={}&manager_version={}",
            encode(vn_id),
            encode(&release_id),
            encode(emulator),
            encode(version),
            encode(env!("CARGO_PKG_VERSION")),
        )
    }

    pub(super) fn compat_view(
        &self,
        game_id: &str,
        identities: &[astra_emu_manager_core::ExternalIdentityRecord],
        now_ms: i64,
    ) -> CompatViewModel {
        let emulator_label = self
            .candidates
            .get(game_id)
            .map(|candidate| Self::emulator_label(&candidate.descriptor))
            .unwrap_or_default();
        let report_url = self.report_url(game_id, identities);
        let mut view = CompatViewModel {
            emulator_label,
            report_url,
            ..CompatViewModel::default()
        };
        match self.compatibility_match(game_id, identities) {
            CompatibilityMatch::NotLinked => view.hint = VNDB_LINK_HINT.into(),
            CompatibilityMatch::ReleaseUndecided => view.hint = RELEASE_HINT.into(),
            CompatibilityMatch::NoEmulator { release_id }
            | CompatibilityMatch::NoRecord { release_id } => view.release_id = release_id,
            CompatibilityMatch::Found(entry) => {
                view.status = entry.status.as_str().into();
                view.notes = entry.notes.clone().unwrap_or_default();
                view.updated = human_relative(entry.updated_at_unix_ms, now_ms);
                view.release_id.clone_from(&entry.release_id);
            }
        }
        view
    }

    /// Preset for the game's release and current emulator.
    pub(super) fn recommended_preset(&self, game_id: &str) -> Result<Option<&Preset>, String> {
        let identities = self
            .library
            .external_identities(game_id)
            .map_err(|error| error.to_string())?;
        let Some(vn_id) = Self::vndb_id(&identities) else {
            return Ok(None);
        };
        let (Some(release_id), Some(emulator)) =
            (self.matched_release(game_id, vn_id), self.game_emulator(game_id))
        else {
            return Ok(None);
        };
        Ok(self
            .metadata_catalog
            .as_ref()
            .and_then(|catalog| catalog.presets.preset(&release_id, emulator)))
    }

    /// Patch recipes from the metadata cache, marking the preset's patch.
    pub(super) fn recipe_views(&self, game_id: Option<&str>) -> Result<Vec<RecipeViewModel>, String> {
        let Some(catalog) = &self.metadata_catalog else {
            return Ok(Vec::new());
        };
        let recommended = match game_id {
            Some(game_id) => self
                .recommended_preset(game_id)?
                .and_then(|preset| preset.patch.clone()),
            None => None,
        };
        Ok(catalog
            .recipes
            .iter()
            .map(|recipe| RecipeViewModel {
                id: recipe.id.clone(),
                title: recipe.title.clone(),
                recommended: recommended.as_deref() == Some(recipe.id.as_str()),
            })
            .collect())
    }

    /// Write every preset entry as a game override. Any invalid entry restores
    /// the overrides stored before the call.
    pub(super) fn apply_recommended_config(
        &mut self,
        game_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let preset = self
            .recommended_preset(game_id)?
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_PRESET_UNAVAILABLE".to_owned())?;
        let plugin = preset.emulator.clone();
        let schema = self.registry.configuration(&plugin)?;
        let original = self
            .library
            .family_overrides(&plugin, game_id)
            .map_err(|error| error.to_string())?;
        let applied = preset.configuration.iter().try_for_each(|(field, raw)| {
            let entry = family_config::parse_config_entries(&schema, [(field.as_str(), raw.as_str())])?
                .pop()
                .ok_or_else(|| "ASTRA_EMU_PRESET_INVALID".to_owned())?;
            self.library
                .set_family_override(&plugin, game_id, &schema, entry)
                .map_err(|error| error.to_string())
        });
        if let Err(error) = applied {
            tracing::warn!(
                event = "astra.emu.manager.preset_rejected",
                plugin_id = %plugin,
                diagnostic_code = %error
            );
            let restored = if original.is_empty() {
                self.library
                    .family_overrides(&plugin, game_id)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .try_for_each(|entry| {
                        self.library
                            .remove_family_override(&plugin, game_id, &entry.id)
                            .map(drop)
                    })
            } else {
                self.library
                    .save_family_configuration(&plugin, game_id, &schema, &original)
            };
            restored.map_err(|error| error.to_string())?;
            return Err("ASTRA_EMU_PRESET_INVALID".into());
        }
        self.model()
    }

    /// Queue a metadata synchronization. Requires network permission.
    pub(super) fn sync_metadata(&mut self) -> Result<ManagerViewModel, String> {
        self.require_network()?;
        if self.metadata_sync.request.is_some() {
            return self.model();
        }
        let cached = self
            .metadata_catalog
            .as_ref()
            .filter(|catalog| catalog.source == MetadataSource::Synchronized)
            .map(MetadataCatalog::cached_files)
            .unwrap_or_default();
        let etag = if cached.is_empty() {
            None
        } else {
            self.metadata_sync.record.etag.clone()
        };
        let request_id = self.next_metadata_id("sync")?;
        self.metadata.submit(MetadataCommand {
            request_id: request_id.clone(),
            case_identity: String::new(),
            provider: MetadataProviderId::Vndb,
            access_token: None,
            allow_sensitive_cover: false,
            kind: MetadataCommandKind::Sync {
                base_url: DEFAULT_METADATA_BASE_URL.into(),
                etag,
                cached,
            },
        })?;
        self.pending_metadata.insert(
            request_id.clone(),
            PendingMetadata {
                game_id: None,
                core: None,
            },
        );
        self.metadata_sync.request = Some(request_id);
        self.model()
    }

    pub(super) fn apply_sync_outcome(&mut self, outcome: SyncOutcome) -> Result<(), String> {
        let now = unix_time_ms()?;
        let etag = match outcome {
            SyncOutcome::Unchanged => self.metadata_sync.record.etag.clone(),
            SyncOutcome::Updated { index, etag, files } => {
                let current = self
                    .metadata_catalog
                    .as_ref()
                    .filter(|catalog| catalog.source == MetadataSource::Synchronized);
                let catalog = metadata_cache::store_synchronized(&self.data_dir, current, &index, files)?;
                let changed = self
                    .metadata_catalog
                    .as_ref()
                    .is_none_or(|previous| previous.index != catalog.index);
                if catalog.index.generated_at_unix_ms
                    >= self
                        .metadata_catalog
                        .as_ref()
                        .map_or(i64::MIN, |previous| previous.index.generated_at_unix_ms)
                {
                    self.metadata_catalog = Some(catalog);
                }
                if changed {
                    self.show_snackbar("元数据已更新", None);
                }
                etag
            }
        };
        self.metadata_sync.record = SyncRecord {
            etag,
            synced_at_unix_ms: Some(now),
        };
        self.library
            .set_app_state(METADATA_SYNC_KEY, &self.metadata_sync.record)
            .map_err(|error| error.to_string())
    }

    pub(super) fn network_view(&self, now_ms: i64) -> NetworkViewModel {
        let sync_status = if self.metadata_sync.request.is_some() {
            "正在同步…".into()
        } else {
            match (&self.metadata_catalog, self.metadata_sync.record.synced_at_unix_ms) {
                (None, _) => "未同步".into(),
                (Some(_), Some(synced)) => format!("已同步 · {}", human_relative(synced, now_ms)),
                (Some(catalog), None) if catalog.source == MetadataSource::Bundled => {
                    "使用随包数据".into()
                }
                (Some(_), None) => "使用本地缓存".into(),
            }
        };
        NetworkViewModel {
            allowed: self.network_allowed,
            syncing: self.metadata_sync.request.is_some(),
            sync_status,
            source: DEFAULT_METADATA_BASE_URL.into(),
        }
    }

    pub(super) fn compatibility_summary(&self) -> String {
        match &self.metadata_catalog {
            Some(catalog) => format!(
                "已收录 {} 条兼容性记录、{} 份推荐配置",
                catalog.compatibility.entries.len(),
                catalog.presets.entries.len()
            ),
            None => "未同步".into(),
        }
    }
}

#[cfg(test)]
#[path = "metadata_sync_tests.rs"]
mod tests;
