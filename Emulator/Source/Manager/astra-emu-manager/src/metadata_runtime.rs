use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use astra_emu_metadata::{
    BangumiPlayUpdate, CoreCatalogEntry, CoverAsset, MetadataLicenseManifest, MetadataProvider,
    MetadataProviderId, MetadataRecord, MetadataRelease, MetadataSearchQuery, MetadataSyncClient,
    ReleaseUse, SyncOutcome, VndbProvider, VndbProviderConfig, MAX_CORE_PACKAGE_BYTES,
};
use sha2::{Digest, Sha256};
// bangumi-api still links native-tls, which has no Android OpenSSL provider;
// see astra-emu-metadata's Cargo.toml for the target gate.
#[cfg(not(target_os = "android"))]
use astra_emu_metadata::{BangumiProvider, BangumiProviderConfig};

const COMMAND_CAPACITY: usize = 64;

#[derive(Debug)]
pub enum MetadataCommandKind {
    Search(MetadataSearchQuery),
    Fetch(String),
    #[cfg_attr(target_os = "android", allow(dead_code))]
    SyncBangumi(BangumiPlayUpdate),
    /// Synchronize the AstraEMU-metadata index. Not tied to a provider; the
    /// command's `provider` field is a placeholder for this variant.
    Sync {
        base_url: String,
        etag: Option<String>,
        cached: BTreeMap<String, String>,
    },
    /// Download a released core package and stage it under `pending`.
    InstallCore {
        entry: Box<CoreCatalogEntry>,
        pending: PathBuf,
    },
    /// Fetch the concrete VNDB releases (rIDs) of a visual novel so a local
    /// installation can be pinned to a specific game version. VNDB is the
    /// authoritative source; Bangumi returns an empty list for this.
    FetchReleases(String),
}

#[derive(Debug)]
pub struct MetadataCommand {
    pub request_id: String,
    pub case_identity: String,
    pub provider: MetadataProviderId,
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub access_token: Option<String>,
    pub allow_sensitive_cover: bool,
    pub kind: MetadataCommandKind,
}

#[derive(Debug)]
pub enum MetadataPayload {
    Search(Vec<MetadataRecord>),
    Fetch {
        record: Box<MetadataRecord>,
        cover: Option<CoverAsset>,
    },
    #[cfg_attr(target_os = "android", allow(dead_code))]
    BangumiPlaySynced,
    Synced(Box<SyncOutcome>),
    CoreStaged {
        stem: String,
        version: String,
    },
    Releases(Vec<MetadataRelease>),
}

#[derive(Debug)]
pub struct MetadataCompletion {
    pub request_id: String,
    pub case_identity: String,
    pub provider: MetadataProviderId,
    pub result: Result<MetadataPayload, String>,
}

pub struct MetadataRuntime {
    commands: mpsc::SyncSender<MetadataCommand>,
    completions: mpsc::Receiver<MetadataCompletion>,
    wake: Arc<Mutex<Option<MetadataWake>>>,
}

pub type MetadataWake = Arc<dyn Fn() + Send + Sync + 'static>;

impl MetadataRuntime {
    pub fn start() -> Result<Self, String> {
        let (commands, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (completion_tx, completions) = mpsc::channel();
        let wake = Arc::new(Mutex::new(None));
        let worker_wake = Arc::clone(&wake);
        thread::Builder::new()
            .name("astra-emu-metadata".into())
            .spawn(move || worker(command_rx, completion_tx, worker_wake))
            .map_err(|_| "ASTRA_EMU_METADATA_WORKER_START".to_owned())?;
        Ok(Self {
            commands,
            completions,
            wake,
        })
    }

    pub fn set_wake(&mut self, wake: MetadataWake) {
        if let Ok(mut current) = self.wake.lock() {
            *current = Some(wake);
        }
        // Close the startup race: a completion may have been queued before the
        // UI installed its wake callback.  One explicit edge lets the host
        // drain it without reintroducing a timer.
        if let Ok(current) = self.wake.lock() {
            if let Some(callback) = current.as_ref() {
                callback();
            }
        }
    }

    pub fn submit(&self, command: MetadataCommand) -> Result<(), String> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "ASTRA_EMU_METADATA_QUEUE_FULL".into(),
                mpsc::TrySendError::Disconnected(_) => {
                    "ASTRA_EMU_METADATA_WORKER_DISCONNECTED".into()
                }
            })
    }

    pub fn try_recv(&self) -> Result<Option<MetadataCompletion>, String> {
        match self.completions.try_recv() {
            Ok(completion) => Ok(Some(completion)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("ASTRA_EMU_METADATA_WORKER_DISCONNECTED".into())
            }
        }
    }

    /// A runtime without a worker: tests read the submitted commands and
    /// inject completions through the returned channel ends.
    #[cfg(test)]
    pub fn scripted() -> (
        Self,
        mpsc::Receiver<MetadataCommand>,
        mpsc::Sender<MetadataCompletion>,
    ) {
        let (commands, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (completion_tx, completions) = mpsc::channel();
        let runtime = Self {
            commands,
            completions,
            wake: Arc::new(Mutex::new(None)),
        };
        (runtime, command_rx, completion_tx)
    }
}

fn worker(
    commands: mpsc::Receiver<MetadataCommand>,
    completions: mpsc::Sender<MetadataCompletion>,
    wake: Arc<Mutex<Option<MetadataWake>>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return,
    };
    // One VNDB client for the worker's lifetime, so its request delay also
    // spaces consecutive commands, such as an automatic linking batch.
    let mut vndb = None;
    while let Ok(command) = commands.recv() {
        let result = runtime.block_on(execute(&command, &mut vndb));
        if completions
            .send(MetadataCompletion {
                request_id: command.request_id,
                case_identity: command.case_identity,
                provider: command.provider,
                result,
            })
            .is_err()
        {
            break;
        }
        if let Some(callback) = wake.lock().ok().and_then(|guard| guard.clone()) {
            callback();
        }
    }
}

async fn execute(
    command: &MetadataCommand,
    vndb: &mut Option<VndbProvider>,
) -> Result<MetadataPayload, String> {
    match &command.kind {
        MetadataCommandKind::Sync {
            base_url,
            etag,
            cached,
        } => {
            let client = MetadataSyncClient::new(base_url, Duration::from_secs(20))
                .map_err(|error| error.to_string())?;
            let outcome = client
                .sync(etag.as_deref(), cached)
                .await
                .map_err(|error| error.to_string())?;
            return Ok(MetadataPayload::Synced(Box::new(outcome)));
        }
        MetadataCommandKind::InstallCore { entry, pending } => {
            let stem = download_core(entry, pending).await?;
            return Ok(MetadataPayload::CoreStaged {
                stem,
                version: entry.version.clone(),
            });
        }
        _ => {}
    }
    match command.provider {
        MetadataProviderId::Vndb => {
            let provider = match vndb {
                Some(provider) => provider,
                slot => slot.insert(
                    VndbProvider::new(VndbProviderConfig {
                        license: MetadataLicenseManifest {
                            release_use: ReleaseUse::NonCommercial,
                            vndb_commercial_license_id: None,
                        },
                        timeout: Duration::from_secs(20),
                        minimum_request_delay: Duration::from_millis(1_000),
                    })
                    .map_err(|error| error.to_string())?,
                ),
            };
            execute_provider(provider, command).await
        }
        MetadataProviderId::Bangumi => {
            #[cfg(target_os = "android")]
            {
                Err(
                    "ASTRA_EMU_ANDROID_BANGUMI_UNAVAILABLE: Bangumi progress sync requires a \
                     native-tls OpenSSL provider that Android does not ship"
                        .to_string(),
                )
            }
            #[cfg(not(target_os = "android"))]
            {
                let provider = BangumiProvider::new(BangumiProviderConfig {
                    access_token: command.access_token.clone(),
                    timeout: Duration::from_secs(20),
                })
                .map_err(|error| error.to_string())?;
                if let MetadataCommandKind::SyncBangumi(update) = &command.kind {
                    provider
                        .sync_play_status(update)
                        .await
                        .map_err(|error| error.to_string())?;
                    return Ok(MetadataPayload::BangumiPlaySynced);
                }
                execute_provider(&provider, command).await
            }
        }
    }
}

async fn execute_provider(
    provider: &impl MetadataProvider,
    command: &MetadataCommand,
) -> Result<MetadataPayload, String> {
    match &command.kind {
        MetadataCommandKind::Search(query) => provider
            .search(query)
            .await
            .map(MetadataPayload::Search)
            .map_err(|error| error.to_string()),
        MetadataCommandKind::Fetch(remote_id) => {
            let record = provider
                .fetch_by_id(remote_id)
                .await
                .map_err(|error| error.to_string())?;
            let cover =
                if record.cover.is_some() && (!record.sensitive || command.allow_sensitive_cover) {
                    Some(
                        provider
                            .fetch_cover(&record, command.allow_sensitive_cover)
                            .await
                            .map_err(|error| error.to_string())?,
                    )
                } else {
                    None
                };
            Ok(MetadataPayload::Fetch {
                record: Box::new(record),
                cover,
            })
        }
        MetadataCommandKind::SyncBangumi(_)
        | MetadataCommandKind::Sync { .. }
        | MetadataCommandKind::InstallCore { .. } => {
            Err("ASTRA_EMU_METADATA_PROVIDER_MISMATCH".into())
        }
        MetadataCommandKind::FetchReleases(remote_id) => provider
            .fetch_releases(remote_id)
            .await
            .map(MetadataPayload::Releases)
            .map_err(|error| error.to_string()),
    }
}

const CORE_REDIRECT_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];
const MAX_CORE_REDIRECTS: usize = 10;

/// GitHub release assets redirect to its asset CDN; nothing else is followed.
pub(crate) fn core_redirect_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url
            .host_str()
            .is_some_and(|host| CORE_REDIRECT_HOSTS.contains(&host))
}

/// Running length/SHA-256 check of a core package download against its
/// catalog entry.
pub(crate) struct DownloadCheck<'a> {
    entry: &'a CoreCatalogEntry,
    received: u64,
    hasher: Sha256,
}

impl<'a> DownloadCheck<'a> {
    pub(crate) fn new(entry: &'a CoreCatalogEntry, announced: Option<u64>) -> Result<Self, String> {
        if entry.size > MAX_CORE_PACKAGE_BYTES
            || announced.is_some_and(|length| length != entry.size)
        {
            return Err("ASTRA_EMU_CORE_DOWNLOAD_SIZE".into());
        }
        Ok(Self {
            entry,
            received: 0,
            hasher: Sha256::new(),
        })
    }

    pub(crate) fn chunk(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.received = self.received.saturating_add(bytes.len() as u64);
        if self.received > self.entry.size {
            return Err("ASTRA_EMU_CORE_DOWNLOAD_SIZE".into());
        }
        self.hasher.update(bytes);
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), String> {
        if self.received != self.entry.size {
            return Err("ASTRA_EMU_CORE_DOWNLOAD_SIZE".into());
        }
        let digest: String = self
            .hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if digest != self.entry.sha256 {
            return Err("ASTRA_EMU_CORE_DOWNLOAD_HASH_MISMATCH".into());
        }
        Ok(())
    }
}

async fn download_core(entry: &CoreCatalogEntry, pending: &Path) -> Result<String, String> {
    let network = |error: reqwest::Error| {
        format!("ASTRA_EMU_CORE_DOWNLOAD_NETWORK: {}", error.without_url())
    };
    let url = reqwest::Url::parse(&entry.url).map_err(|_| "ASTRA_EMU_CORE_DOWNLOAD_URL")?;
    if !core_redirect_allowed(&url) {
        return Err("ASTRA_EMU_CORE_DOWNLOAD_URL".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_CORE_REDIRECTS {
                attempt.error("ASTRA_EMU_CORE_DOWNLOAD_REDIRECT")
            } else if core_redirect_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("ASTRA_EMU_CORE_DOWNLOAD_REDIRECT")
            }
        }))
        .build()
        .map_err(network)?;
    let mut response = client.get(url).send().await.map_err(network)?;
    if !response.status().is_success() {
        return Err(format!(
            "ASTRA_EMU_CORE_DOWNLOAD_NETWORK: http-{}",
            response.status().as_u16()
        ));
    }
    let mut check = DownloadCheck::new(entry, response.content_length())?;
    std::fs::create_dir_all(pending).map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
    let mut archive = tempfile::Builder::new()
        .prefix(".download-")
        .tempfile_in(pending)
        .map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
    while let Some(chunk) = response.chunk().await.map_err(network)? {
        check.chunk(&chunk)?;
        archive
            .write_all(&chunk)
            .map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
    }
    check.finish()?;
    archive
        .as_file()
        .sync_all()
        .map_err(|_| "ASTRA_EMU_CORE_INSTALL_COPY")?;
    let (stem, _) =
        crate::core_package::stage_package(archive.path(), Some(&entry.plugin_id), pending)?;
    tracing::info!(
        event = "astra.emu.manager.core_downloaded",
        plugin_id = %entry.plugin_id,
        version = %entry.version,
        core = %stem
    );
    Ok(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(bytes: &[u8]) -> CoreCatalogEntry {
        CoreCatalogEntry {
            plugin_id: "astra.emu.musica".into(),
            display_name: "Astra Musica".into(),
            family_name: "Musica".into(),
            version: "0.2.0".into(),
            abi: "astra.emu.native.c.v3".into(),
            platform: "windows-x86_64".into(),
            url: "https://github.com/2018wzh/AstraEngine/releases/download/emu-v0.2.0/core.zip"
                .into(),
            sha256: crate::metadata_cache::sha256_hex(bytes),
            size: bytes.len() as u64,
            license: "MIT".into(),
            release_tag: "emu-v0.2.0".into(),
        }
    }

    #[test]
    fn redirects_are_limited_to_github_release_hosts_over_https() {
        for allowed in [
            "https://github.com/2018wzh/AstraEngine/releases/download/x/y.zip",
            "https://objects.githubusercontent.com/github-production-release-asset/1",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1",
        ] {
            assert!(core_redirect_allowed(&reqwest::Url::parse(allowed).unwrap()), "{allowed}");
        }
        for rejected in [
            "http://github.com/x",
            "https://example.com/core.zip",
            "https://github.com.evil.example/x",
            "https://user:pass@github.com/x",
        ] {
            assert!(!core_redirect_allowed(&reqwest::Url::parse(rejected).unwrap()), "{rejected}");
        }
    }

    #[test]
    fn download_check_enforces_size_and_sha256() {
        let entry = entry(b"package bytes");
        let mut check = DownloadCheck::new(&entry, Some(entry.size)).unwrap();
        check.chunk(b"package ").unwrap();
        check.chunk(b"bytes").unwrap();
        check.finish().unwrap();

        assert_eq!(
            DownloadCheck::new(&entry, Some(entry.size + 1)).err().unwrap(),
            "ASTRA_EMU_CORE_DOWNLOAD_SIZE"
        );
        let mut long = DownloadCheck::new(&entry, None).unwrap();
        assert_eq!(
            long.chunk(b"package bytes plus").unwrap_err(),
            "ASTRA_EMU_CORE_DOWNLOAD_SIZE"
        );
        let mut tampered = DownloadCheck::new(&entry, None).unwrap();
        tampered.chunk(b"package BYTES").unwrap();
        assert_eq!(
            tampered.finish().unwrap_err(),
            "ASTRA_EMU_CORE_DOWNLOAD_HASH_MISMATCH"
        );
    }
}
