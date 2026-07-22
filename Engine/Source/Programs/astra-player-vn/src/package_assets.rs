use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use astra_asset::{AssetCatalog, VfsManifest, VfsSourceRef};
use astra_core::Hash256;
use astra_media_core::TextureFrame;
use astra_package::PackageReader;
use astra_ui_core::UiValidationError;
use astra_ui_yakui::UiImageResourceProvider;

use crate::NativeVnHostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone)]
struct AssetDescriptor {
    section_id: String,
    hash: Hash256,
    decoded_length: u64,
    kind: AssetKind,
    codec: Option<String>,
}

#[derive(Debug, Clone)]
enum CachedAsset {
    Image(TextureFrame),
    Media(Arc<[u8]>),
}

impl CachedAsset {
    fn bytes(&self) -> usize {
        match self {
            Self::Image(frame) => frame.rgba8.len(),
            Self::Media(bytes) => bytes.len(),
        }
    }
}

#[derive(Debug)]
struct CacheEntry {
    value: CachedAsset,
    last_used: u64,
}

#[derive(Debug)]
struct AssetCache {
    entries: BTreeMap<String, CacheEntry>,
    pinned: BTreeSet<String>,
    bytes: usize,
    max_bytes: usize,
    clock: u64,
}

impl AssetCache {
    fn get(&mut self, asset_id: &str) -> Option<CachedAsset> {
        self.clock = self.clock.saturating_add(1);
        let entry = self.entries.get_mut(asset_id)?;
        entry.last_used = self.clock;
        Some(entry.value.clone())
    }

    fn insert(
        &mut self,
        asset_id: String,
        value: CachedAsset,
    ) -> Result<Vec<CachedAsset>, NativeVnHostError> {
        let bytes = value.bytes();
        if bytes == 0 || bytes > self.max_bytes {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_ASSET_CACHE_ENTRY_BUDGET".into(),
            ));
        }
        self.clock = self.clock.saturating_add(1);
        let mut retired = Vec::new();
        if let Some(previous) = self.entries.remove(&asset_id) {
            self.bytes = self.bytes.saturating_sub(previous.value.bytes());
            retired.push(previous.value);
        }
        self.entries.insert(
            asset_id,
            CacheEntry {
                value,
                last_used: self.clock,
            },
        );
        self.bytes = self.bytes.saturating_add(bytes);
        while self.bytes > self.max_bytes {
            let evict_id = self
                .entries
                .iter()
                .filter(|(id, _)| !self.pinned.contains(*id))
                .min_by_key(|(id, entry)| (entry.last_used, *id))
                .map(|(id, _)| id.clone())
                .ok_or_else(|| {
                    NativeVnHostError::Asset(
                        "ASTRA_PLAYER_ASSET_CACHE_PINNED_BUDGET_EXCEEDED".into(),
                    )
                })?;
            let evicted = self
                .entries
                .remove(&evict_id)
                .ok_or_else(|| NativeVnHostError::Asset("ASTRA_PLAYER_ASSET_CACHE_STATE".into()))?;
            self.bytes = self.bytes.saturating_sub(evicted.value.bytes());
            retired.push(evicted.value);
            tracing::debug!(
                event = "player.asset.cache.evicted",
                asset_id = evict_id,
                cache_bytes = self.bytes,
                cache_budget_bytes = self.max_bytes,
                "evicted a package asset from the bounded CPU cache"
            );
        }
        Ok(retired)
    }

    fn insert_without_eviction(
        &mut self,
        asset_id: String,
        value: CachedAsset,
    ) -> Result<bool, NativeVnHostError> {
        let bytes = value.bytes();
        if bytes == 0 || bytes > self.max_bytes {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_ASSET_CACHE_ENTRY_BUDGET".into(),
            ));
        }
        if !self.can_insert_without_eviction(&asset_id, bytes) {
            return Ok(false);
        }
        self.clock = self.clock.saturating_add(1);
        if let Some(previous) = self.entries.remove(&asset_id) {
            self.bytes = self.bytes.saturating_sub(previous.value.bytes());
        }
        self.entries.insert(
            asset_id,
            CacheEntry {
                value,
                last_used: self.clock,
            },
        );
        self.bytes = self.bytes.saturating_add(bytes);
        Ok(true)
    }

    fn can_insert_without_eviction(&self, asset_id: &str, bytes: usize) -> bool {
        if bytes == 0 || bytes > self.max_bytes {
            return false;
        }
        let previous_bytes = self
            .entries
            .get(asset_id)
            .map_or(0, |entry| entry.value.bytes());
        self.bytes
            .saturating_sub(previous_bytes)
            .saturating_add(bytes)
            <= self.max_bytes
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedMediaAsset {
    pub codec: String,
    pub bytes: Arc<[u8]>,
    pub hash: Hash256,
}

#[derive(Debug)]
pub(crate) struct PackageAssetStore {
    package: PackageReader,
    descriptors: BTreeMap<String, AssetDescriptor>,
    cache: Mutex<AssetCache>,
}

enum ImagePrefetchCommand {
    Load(String),
    Shutdown,
}

struct ImagePrefetchCompletion {
    asset_id: String,
    result: Result<(), String>,
}

pub(crate) type ImagePrefetchResult = (String, Result<(), String>);

pub(crate) struct PackageImagePrefetcher {
    commands: SyncSender<ImagePrefetchCommand>,
    completions: Receiver<ImagePrefetchCompletion>,
    workers: Vec<JoinHandle<()>>,
}

impl PackageImagePrefetcher {
    const QUEUE_CAPACITY: usize = 32;
    const WORKER_COUNT: usize = 2;

    pub fn start(store: Arc<PackageAssetStore>) -> Result<Self, NativeVnHostError> {
        let (commands, worker_commands) = mpsc::sync_channel(Self::QUEUE_CAPACITY);
        let (worker_completions, completions) = mpsc::channel();
        let worker_commands = Arc::new(Mutex::new(worker_commands));
        let mut workers: Vec<JoinHandle<()>> = Vec::with_capacity(Self::WORKER_COUNT);
        for worker_index in 0..Self::WORKER_COUNT {
            let worker_store = Arc::clone(&store);
            let worker_commands = Arc::clone(&worker_commands);
            let worker_completions = worker_completions.clone();
            let worker = match thread::Builder::new()
                .name(format!("astra-image-prefetch-{worker_index}"))
                .spawn(move || loop {
                    let command = worker_commands
                        .lock()
                        .expect("image prefetch command queue was poisoned")
                        .recv();
                    match command {
                        Ok(ImagePrefetchCommand::Load(asset_id)) => {
                            let result = worker_store
                                .load_image(&asset_id)
                                .map(|_| ())
                                .map_err(|error| error.to_string());
                            if worker_completions
                                .send(ImagePrefetchCompletion { asset_id, result })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(ImagePrefetchCommand::Shutdown) | Err(_) => break,
                    }
                }) {
                Ok(worker) => worker,
                Err(error) => {
                    for _ in 0..workers.len() {
                        let _ = commands.send(ImagePrefetchCommand::Shutdown);
                    }
                    for worker in workers {
                        let _ = worker.join();
                    }
                    return Err(NativeVnHostError::Asset(format!(
                        "ASTRA_PLAYER_IMAGE_PREFETCH_THREAD: {error}"
                    )));
                }
            };
            workers.push(worker);
        }
        drop(worker_completions);
        Ok(Self {
            commands,
            completions,
            workers,
        })
    }

    pub fn try_schedule(&self, asset_id: String) -> Result<bool, NativeVnHostError> {
        match self.commands.try_send(ImagePrefetchCommand::Load(asset_id)) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_IMAGE_PREFETCH_DISCONNECTED".into(),
            )),
        }
    }

    fn try_recv(&self) -> Result<Option<ImagePrefetchCompletion>, NativeVnHostError> {
        match self.completions.try_recv() {
            Ok(completion) => Ok(Some(completion)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_IMAGE_PREFETCH_COMPLETION_DISCONNECTED".into(),
            )),
        }
    }

    pub fn drain_completions(&self) -> Result<Vec<ImagePrefetchResult>, NativeVnHostError> {
        let mut completed = Vec::new();
        while let Some(completion) = self.try_recv()? {
            completed.push((completion.asset_id, completion.result));
        }
        Ok(completed)
    }

    pub fn shutdown(&mut self) -> Result<Vec<ImagePrefetchResult>, NativeVnHostError> {
        if self.workers.is_empty() {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_IMAGE_PREFETCH_SHUTDOWN_ORDER".into(),
            ));
        }
        for _ in 0..self.workers.len() {
            self.commands
                .send(ImagePrefetchCommand::Shutdown)
                .map_err(|_| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_IMAGE_PREFETCH_DISCONNECTED".into())
                })?;
        }
        for worker in self.workers.drain(..) {
            worker.join().map_err(|_| {
                NativeVnHostError::Asset("ASTRA_PLAYER_IMAGE_PREFETCH_PANICKED".into())
            })?;
        }
        let mut completed = Vec::new();
        while let Ok(completion) = self.completions.try_recv() {
            completed.push((completion.asset_id, completion.result));
        }
        Ok(completed)
    }
}

impl PackageAssetStore {
    pub fn index(
        package: &PackageReader,
        max_cache_bytes: u64,
    ) -> Result<Arc<Self>, NativeVnHostError> {
        let max_cache_bytes = usize::try_from(max_cache_bytes).map_err(|_| {
            NativeVnHostError::Asset("ASTRA_PLAYER_ASSET_CACHE_BUDGET_RANGE".into())
        })?;
        if max_cache_bytes == 0 {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_ASSET_CACHE_BUDGET_ZERO".into(),
            ));
        }
        let catalog: AssetCatalog = serde_json::from_slice(
            &package
                .container()
                .read_bounded("asset.catalog", 16 * 1024 * 1024)
                .map_err(|error| NativeVnHostError::Package(error.to_string()))?,
        )
        .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        let manifest: VfsManifest = serde_json::from_slice(
            &package
                .container()
                .read_bounded("asset.vfs_manifest", 32 * 1024 * 1024)
                .map_err(|error| NativeVnHostError::Package(error.to_string()))?,
        )
        .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        let mut descriptors = BTreeMap::new();
        for asset in catalog.assets {
            let kind = if catalog_asset_has_type(&asset, "image.") {
                AssetKind::Image
            } else if catalog_asset_has_type(&asset, "video.") {
                AssetKind::Video
            } else if catalog_asset_has_type(&asset, "audio.")
                || catalog_asset_has_type(&asset, "voice")
            {
                AssetKind::Audio
            } else {
                continue;
            };
            let matches = manifest
                .entries
                .iter()
                .filter(|entry| entry.uri == asset.uri)
                .collect::<Vec<_>>();
            let [entry] = matches.as_slice() else {
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_ASSET_VFS_AMBIGUOUS: {}",
                    asset.asset_id
                )));
            };
            let VfsSourceRef::PackageSection { section_id } = &entry.source else {
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_ASSET_SOURCE: {}",
                    asset.asset_id
                )));
            };
            let section = package
                .container()
                .entries()
                .iter()
                .find(|section| section.id == *section_id)
                .ok_or_else(|| {
                    NativeVnHostError::Asset(format!(
                        "ASTRA_PLAYER_ASSET_SECTION_MISSING: {}",
                        asset.asset_id
                    ))
                })?;
            if section.hash != entry.hash || section.decoded_length != entry.size {
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_ASSET_SECTION_IDENTITY: {}",
                    asset.asset_id
                )));
            }
            if descriptors
                .insert(
                    asset.asset_id.clone(),
                    AssetDescriptor {
                        section_id: section_id.clone(),
                        hash: entry.hash,
                        decoded_length: entry.size,
                        kind,
                        codec: entry.codec.clone(),
                    },
                )
                .is_some()
            {
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_ASSET_ID_DUPLICATE: {}",
                    asset.asset_id
                )));
            }
        }
        Ok(Arc::new(Self {
            package: package.clone(),
            descriptors,
            cache: Mutex::new(AssetCache {
                entries: BTreeMap::new(),
                pinned: BTreeSet::new(),
                bytes: 0,
                max_bytes: max_cache_bytes,
                clock: 0,
            }),
        }))
    }

    pub fn contains_image(&self, asset_id: &str) -> bool {
        self.descriptors
            .get(asset_id)
            .is_some_and(|descriptor| descriptor.kind == AssetKind::Image)
    }

    pub fn contains_media(&self, asset_id: &str) -> bool {
        self.descriptors.get(asset_id).is_some_and(|descriptor| {
            matches!(descriptor.kind, AssetKind::Audio | AssetKind::Video)
        })
    }

    pub fn contains_audio(&self, asset_id: &str) -> bool {
        self.descriptors
            .get(asset_id)
            .is_some_and(|descriptor| descriptor.kind == AssetKind::Audio)
    }

    pub fn cache_bytes(&self) -> u64 {
        self.cache
            .lock()
            .map(|cache| cache.bytes as u64)
            .unwrap_or(u64::MAX)
    }

    pub fn is_image_cached(&self, asset_id: &str) -> Result<bool, NativeVnHostError> {
        self.cache
            .lock()
            .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_ASSET_CACHE_POISONED".into()))
            .map(|cache| {
                matches!(
                    cache.entries.get(asset_id).map(|entry| &entry.value),
                    Some(CachedAsset::Image(_))
                )
            })
    }

    pub fn pin_image_working_set(&self, asset_ids: &[String]) -> Result<(), NativeVnHostError> {
        if let Some(asset_id) = asset_ids
            .iter()
            .find(|asset_id| !self.contains_image(asset_id))
        {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_IMAGE_PIN_KIND: asset_hash={}",
                Hash256::from_sha256(asset_id.as_bytes())
            )));
        }
        let pinned = asset_ids.iter().cloned().collect::<BTreeSet<_>>();
        if pinned.len() != asset_ids.len() {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_IMAGE_PIN_DUPLICATE".into(),
            ));
        }
        self.cache
            .lock()
            .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_ASSET_CACHE_LOCK".into()))?
            .pinned = pinned;
        Ok(())
    }

    pub fn load_image(&self, asset_id: &str) -> Result<TextureFrame, NativeVnHostError> {
        if let Some(CachedAsset::Image(frame)) = self.cache_get(asset_id)? {
            return Ok(frame);
        }
        let frame = self.decode_image(asset_id)?;
        self.cache_insert(asset_id, CachedAsset::Image(frame.clone()))?;
        Ok(frame)
    }

    pub fn prewarm_image_prefix(&self, asset_ids: &[String]) -> Result<usize, NativeVnHostError> {
        let mut retained = 0usize;
        for asset_id in asset_ids {
            if matches!(self.cache_get(asset_id)?, Some(CachedAsset::Image(_))) {
                retained += 1;
                continue;
            }
            let frame = self.decode_image(asset_id)?;
            let inserted = self
                .cache
                .lock()
                .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_ASSET_CACHE_LOCK".into()))?
                .insert_without_eviction(asset_id.clone(), CachedAsset::Image(frame))?;
            if !inserted {
                break;
            }
            retained += 1;
        }
        Ok(retained)
    }

    pub fn load_media(&self, asset_id: &str) -> Result<LoadedMediaAsset, NativeVnHostError> {
        let descriptor = self
            .descriptors
            .get(asset_id)
            .filter(|descriptor| matches!(descriptor.kind, AssetKind::Audio | AssetKind::Video))
            .ok_or_else(|| {
                NativeVnHostError::Asset(format!("ASTRA_PLAYER_MEDIA_ASSET_MISSING: {asset_id}"))
            })?;
        let bytes = match self.cache_get(asset_id)? {
            Some(CachedAsset::Media(bytes)) => bytes,
            Some(CachedAsset::Image(_)) => {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_ASSET_CACHE_KIND_MISMATCH".into(),
                ));
            }
            None => {
                let bytes: Arc<[u8]> = self.read_asset(asset_id, descriptor)?.into();
                self.cache_insert(asset_id, CachedAsset::Media(Arc::clone(&bytes)))?;
                bytes
            }
        };
        let codec = descriptor
            .codec
            .as_deref()
            .and_then(normalize_codec)
            .or_else(|| match descriptor.kind {
                AssetKind::Audio => sniff_audio_codec(&bytes),
                AssetKind::Video => sniff_video_codec(&bytes),
                AssetKind::Image => None,
            })
            .ok_or_else(|| {
                NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_MEDIA_CODEC_UNSUPPORTED: {asset_id}"
                ))
            })?;
        Ok(LoadedMediaAsset {
            codec: codec.into(),
            bytes,
            hash: descriptor.hash,
        })
    }

    fn descriptor(
        &self,
        asset_id: &str,
        expected_kind: AssetKind,
    ) -> Result<&AssetDescriptor, NativeVnHostError> {
        self.descriptors
            .get(asset_id)
            .filter(|descriptor| descriptor.kind == expected_kind)
            .ok_or_else(|| {
                NativeVnHostError::Asset(format!("ASTRA_PLAYER_ASSET_MISSING: {asset_id}"))
            })
    }

    fn decode_image(&self, asset_id: &str) -> Result<TextureFrame, NativeVnHostError> {
        let descriptor = self.descriptor(asset_id, AssetKind::Image)?;
        let encoded = self.read_asset(asset_id, descriptor)?;
        let decoded = image::load_from_memory(&encoded)
            .map_err(|error| {
                NativeVnHostError::Asset(format!("ASTRA_PLAYER_ASSET_DECODE: {error}"))
            })?
            .into_rgba8();
        let (width, height) = decoded.dimensions();
        let rgba8: Arc<[u8]> = decoded.into_raw().into();
        TextureFrame::from_rgba8(width, height, rgba8).map_err(|error| {
            NativeVnHostError::Asset(format!("ASTRA_PLAYER_ASSET_TEXTURE: {error}"))
        })
    }

    fn read_asset(
        &self,
        asset_id: &str,
        descriptor: &AssetDescriptor,
    ) -> Result<Vec<u8>, NativeVnHostError> {
        let max_bytes = usize::try_from(descriptor.decoded_length).map_err(|_| {
            NativeVnHostError::Asset(format!("ASTRA_PLAYER_ASSET_SIZE_RANGE: {asset_id}"))
        })?;
        let bytes = self
            .package
            .container()
            .read_bounded(&descriptor.section_id, max_bytes)
            .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        if bytes.len() as u64 != descriptor.decoded_length
            || Hash256::from_sha256(&bytes) != descriptor.hash
        {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_HASH: {asset_id}"
            )));
        }
        Ok(bytes)
    }

    fn cache_get(&self, asset_id: &str) -> Result<Option<CachedAsset>, NativeVnHostError> {
        self.cache
            .lock()
            .map_err(|_| NativeVnHostError::Asset("ASTRA_PLAYER_ASSET_CACHE_POISONED".into()))
            .map(|mut cache| cache.get(asset_id))
    }

    fn cache_insert(&self, asset_id: &str, value: CachedAsset) -> Result<(), NativeVnHostError> {
        let retired = {
            let mut cache = self.cache.lock().map_err(|_| {
                NativeVnHostError::Asset("ASTRA_PLAYER_ASSET_CACHE_POISONED".into())
            })?;
            cache.insert(asset_id.into(), value)?
        };
        drop(retired);
        Ok(())
    }
}

impl UiImageResourceProvider for PackageAssetStore {
    fn load_image(&self, asset: &str) -> Result<TextureFrame, UiValidationError> {
        PackageAssetStore::load_image(self, asset).map_err(|error| {
            UiValidationError::invalid("ASTRA_UI_IMAGE_RESOURCE_LOAD", error.to_string())
        })
    }
}

fn catalog_asset_has_type(asset: &astra_asset::AssetCatalogEntry, prefix: &str) -> bool {
    let mime_prefix = prefix.strip_suffix('.').map(|value| format!("{value}/"));
    asset.media_kind.starts_with(prefix)
        || mime_prefix
            .as_deref()
            .is_some_and(|prefix| asset.media_kind.starts_with(prefix))
        || asset.tags.iter().any(|tag| {
            tag.starts_with(prefix)
                || mime_prefix
                    .as_deref()
                    .is_some_and(|prefix| tag.starts_with(prefix))
        })
}

fn normalize_codec(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "mp3" | "audio/mpeg" => Some("mp3"),
        "ogg" | "audio/ogg" => Some("ogg"),
        "flac" | "audio/flac" => Some("flac"),
        "wav" | "audio/wav" | "audio/x-wav" => Some("wav"),
        "webm" | "video/webm" => Some("webm"),
        "mp4" | "video/mp4" => Some("mp4"),
        _ => None,
    }
}

fn sniff_audio_codec(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"ID3")
        || bytes
            .get(..2)
            .is_some_and(|prefix| prefix[0] == 0xff && prefix[1] & 0xe0 == 0xe0)
    {
        Some("mp3")
    } else if bytes.starts_with(b"OggS") {
        Some("ogg")
    } else if bytes.starts_with(b"fLaC") {
        Some("flac")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        Some("wav")
    } else {
        None
    }
}

fn sniff_video_codec(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        Some("webm")
    } else if bytes.get(4..8) == Some(b"ftyp") {
        Some("mp4")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(bytes: &[u8]) -> CachedAsset {
        CachedAsset::Media(Arc::from(bytes))
    }

    #[test]
    fn decoded_cache_evicts_least_recently_used_entry_within_bound() {
        let mut cache = AssetCache {
            entries: BTreeMap::new(),
            pinned: BTreeSet::new(),
            bytes: 0,
            max_bytes: 6,
            clock: 0,
        };
        drop(cache.insert("a".into(), media(b"aaa")).unwrap());
        drop(cache.insert("b".into(), media(b"bbb")).unwrap());
        assert!(cache.get("a").is_some());
        drop(cache.insert("c".into(), media(b"ccc")).unwrap());
        assert!(cache.entries.contains_key("a"));
        assert!(!cache.entries.contains_key("b"));
        assert!(cache.entries.contains_key("c"));
        assert_eq!(cache.bytes, 6);
    }

    #[test]
    fn decoded_cache_rejects_single_entry_over_budget() {
        let mut cache = AssetCache {
            entries: BTreeMap::new(),
            pinned: BTreeSet::new(),
            bytes: 0,
            max_bytes: 2,
            clock: 0,
        };
        let error = cache.insert("oversized".into(), media(b"abc")).unwrap_err();
        assert!(error.to_string().contains("CACHE_ENTRY_BUDGET"));
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn prewarm_insert_never_evicts_an_authored_prefix() {
        let mut cache = AssetCache {
            entries: BTreeMap::new(),
            pinned: BTreeSet::new(),
            bytes: 0,
            max_bytes: 6,
            clock: 0,
        };
        assert!(cache
            .insert_without_eviction("a".into(), media(b"aaaa"))
            .unwrap());
        assert!(!cache
            .insert_without_eviction("b".into(), media(b"bbb"))
            .unwrap());
        assert!(cache.entries.contains_key("a"));
        assert!(!cache.entries.contains_key("b"));
        assert_eq!(cache.bytes, 4);
    }

    #[test]
    fn background_admission_accounts_for_replacement_without_evicting_neighbors() {
        let mut cache = AssetCache {
            entries: BTreeMap::new(),
            pinned: BTreeSet::new(),
            bytes: 0,
            max_bytes: 8,
            clock: 0,
        };
        drop(cache.insert("a".into(), media(b"aaaa")).unwrap());
        drop(cache.insert("b".into(), media(b"bbbb")).unwrap());

        assert!(cache.can_insert_without_eviction("a", 4));
        assert!(!cache.can_insert_without_eviction("a", 5));
        assert!(!cache
            .insert_without_eviction("a".into(), media(b"aaaaa"))
            .unwrap());
        assert_eq!(cache.bytes, 8);
        assert!(cache.entries.contains_key("a"));
        assert!(cache.entries.contains_key("b"));
    }

    #[test]
    fn decoded_cache_never_evicts_the_explicit_working_set() {
        let mut cache = AssetCache {
            entries: BTreeMap::new(),
            pinned: BTreeSet::new(),
            bytes: 0,
            max_bytes: 6,
            clock: 0,
        };
        drop(cache.insert("a".into(), media(b"aaa")).unwrap());
        drop(cache.insert("b".into(), media(b"bbb")).unwrap());
        cache.pinned.insert("a".into());
        drop(cache.insert("c".into(), media(b"ccc")).unwrap());
        assert!(cache.entries.contains_key("a"));
        assert!(!cache.entries.contains_key("b"));
        assert!(cache.entries.contains_key("c"));
    }
}
