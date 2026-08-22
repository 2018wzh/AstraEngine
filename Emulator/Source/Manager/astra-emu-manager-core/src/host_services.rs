use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use astra_byte_source::{OwnedByteBuffer, OwnedWritableByteBuffer};
use astra_emu_family_api::{
    validate_relative_writable_path, LegacyFamilyHostServicesV9, LegacyHookHostV1,
    LegacyHookInvocationV1, LegacyHookResultV1, LegacyHookStatusV1, LegacyProviderError,
    LegacySurfaceCommitV9, LegacySurfaceFormatV9, LegacySurfaceHostV9, LegacySurfaceLeaseV9,
    LegacyVfsReader, LegacyWritableFileEntryV1, LegacyWritableFileHostV1,
    LegacyWritableFileRequestV1, LegacyWritableFileResultV1,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SurfaceKey {
    session_id: String,
    surface_id: String,
}

struct SurfaceSlot {
    generation: u64,
    width: u32,
    height: u32,
    stride: u32,
    format: LegacySurfaceFormatV9,
    available: Option<OwnedWritableByteBuffer>,
    leased: Option<(u64, u64)>,
    staged: Option<(u64, u64, OwnedWritableByteBuffer)>,
    published_step: Option<u64>,
}

#[derive(Default)]
pub struct FamilySurfaceHost {
    slots: Mutex<BTreeMap<SurfaceKey, SurfaceSlot>>,
}

pub struct PublishedFamilySurface {
    pub session_id: String,
    pub surface_id: String,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: LegacySurfaceFormatV9,
    pub pixels: OwnedWritableByteBuffer,
}

impl FamilySurfaceHost {
    pub fn publish_step(
        &self,
        session_id: &str,
        fixed_step: u64,
        generations: &BTreeMap<String, u64>,
    ) -> Result<(), LegacyProviderError> {
        let mut slots = self.slots.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_LOCK", "surface pool lock poisoned")
        })?;
        for (surface_id, generation) in generations {
            let key = SurfaceKey {
                session_id: session_id.to_owned(),
                surface_id: surface_id.clone(),
            };
            let slot = slots.get(&key).ok_or_else(|| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_SURFACE_NOT_COMMITTED",
                    "layer transaction references an unknown surface",
                )
            })?;
            let Some((step, staged_generation, _)) = slot.staged.as_ref() else {
                if slot.generation == *generation && slot.published_step.is_some() {
                    continue;
                }
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_SURFACE_NOT_STAGED",
                    "layer transaction references a generation that was not staged",
                ));
            };
            if *step != fixed_step || *staged_generation != *generation {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_SURFACE_GENERATION_MISMATCH",
                    "staged surface generation does not match the layer transaction",
                ));
            }
        }
        if slots.iter().any(|(key, slot)| {
            key.session_id == session_id
                && slot.staged.as_ref().is_some_and(|(step, generation, _)| {
                    *step == fixed_step && generations.get(&key.surface_id) != Some(generation)
                })
        }) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_UNREFERENCED_COMMIT",
                "step committed a surface generation absent from its layer transaction",
            ));
        }
        for (key, slot) in slots.iter_mut() {
            if key.session_id != session_id {
                continue;
            }
            if let Some((step, generation, buffer)) = slot.staged.take() {
                if step == fixed_step {
                    slot.generation = generation;
                    slot.available = Some(buffer);
                    slot.published_step = Some(fixed_step);
                } else {
                    slot.staged = Some((step, generation, buffer));
                }
            }
        }
        Ok(())
    }

    pub fn rollback_step(&self, session_id: &str, fixed_step: u64) {
        let Ok(mut slots) = self.slots.lock() else {
            return;
        };
        for (key, slot) in slots.iter_mut() {
            if key.session_id != session_id {
                continue;
            }
            if slot
                .staged
                .as_ref()
                .is_some_and(|(step, _, _)| *step == fixed_step)
            {
                let (_, _, buffer) = slot.staged.take().expect("staged checked above");
                slot.available = Some(buffer);
            }
        }
    }

    pub fn with_published<R>(
        &self,
        session_id: &str,
        surface_id: &str,
        generation: u64,
        reader: impl FnOnce(&[u8], u32, u32, u32, LegacySurfaceFormatV9) -> R,
    ) -> Result<R, LegacyProviderError> {
        let slots = self.slots.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_LOCK", "surface pool lock poisoned")
        })?;
        let slot = slots
            .get(&SurfaceKey {
                session_id: session_id.to_owned(),
                surface_id: surface_id.to_owned(),
            })
            .ok_or_else(|| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_SURFACE_UNKNOWN",
                    "published surface does not exist",
                )
            })?;
        if slot.generation != generation || slot.published_step.is_none() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_GENERATION_MISMATCH",
                "requested generation is not published",
            ));
        }
        let bytes = slot.available.as_ref().ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_LEASED",
                "published surface is currently leased for writing",
            )
        })?;
        Ok(reader(
            bytes.as_slice(),
            slot.width,
            slot.height,
            slot.stride,
            slot.format,
        ))
    }

    pub fn take_published(
        &self,
        session_id: &str,
        surface_id: &str,
        generation: u64,
    ) -> Result<PublishedFamilySurface, LegacyProviderError> {
        let mut slots = self.slots.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_LOCK", "surface pool lock poisoned")
        })?;
        let slot = slots
            .get_mut(&SurfaceKey {
                session_id: session_id.to_owned(),
                surface_id: surface_id.to_owned(),
            })
            .ok_or_else(|| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_SURFACE_UNKNOWN",
                    "published surface does not exist",
                )
            })?;
        if slot.generation != generation || slot.published_step.is_none() || slot.leased.is_some() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_GENERATION_MISMATCH",
                "requested generation is not available for presentation",
            ));
        }
        let pixels = slot.available.take().ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_PRESENTING",
                "published surface is already owned by a presenter",
            )
        })?;
        Ok(PublishedFamilySurface {
            session_id: session_id.to_owned(),
            surface_id: surface_id.to_owned(),
            generation,
            width: slot.width,
            height: slot.height,
            stride: slot.stride,
            format: slot.format,
            pixels,
        })
    }

    pub fn return_published(
        &self,
        surface: PublishedFamilySurface,
    ) -> Result<(), LegacyProviderError> {
        let mut slots = self.slots.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_LOCK", "surface pool lock poisoned")
        })?;
        let slot = slots
            .get_mut(&SurfaceKey {
                session_id: surface.session_id,
                surface_id: surface.surface_id,
            })
            .ok_or_else(|| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_SURFACE_UNKNOWN",
                    "returned presentation surface does not exist",
                )
            })?;
        if slot.generation != surface.generation
            || slot.available.is_some()
            || slot.leased.is_some()
            || slot.staged.is_some()
            || slot.width != surface.width
            || slot.height != surface.height
            || slot.stride != surface.stride
            || slot.format != surface.format
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_RETURN_OWNERSHIP",
                "returned presentation surface does not match its pool slot",
            ));
        }
        slot.available = Some(surface.pixels);
        Ok(())
    }

    pub fn release_session(&self, session_id: &str) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.retain(|key, _| key.session_id != session_id);
        }
    }
}

impl LegacySurfaceHostV9 for FamilySurfaceHost {
    fn acquire(
        &self,
        session_id: &str,
        fixed_step: u64,
        surface_id: &str,
        width: u32,
        height: u32,
        format: LegacySurfaceFormatV9,
    ) -> Result<LegacySurfaceLeaseV9, LegacyProviderError> {
        if session_id.is_empty() || surface_id.is_empty() || width == 0 || height == 0 {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_ACQUIRE",
                "surface acquire identity and dimensions must be non-empty",
            ));
        }
        let stride = width.checked_mul(4).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_STRIDE", "surface stride overflow")
        })?;
        let byte_len = usize::try_from(
            u64::from(stride)
                .checked_mul(u64::from(height))
                .ok_or_else(|| {
                    LegacyProviderError::invalid("ASTRA_EMU_SURFACE_SIZE", "surface size overflow")
                })?,
        )
        .map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_SIZE", "surface size exceeds usize")
        })?;
        let key = SurfaceKey {
            session_id: session_id.to_owned(),
            surface_id: surface_id.to_owned(),
        };
        let mut slots = self.slots.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_LOCK", "surface pool lock poisoned")
        })?;
        let slot = slots.entry(key).or_insert_with(|| SurfaceSlot {
            generation: 0,
            width,
            height,
            stride,
            format,
            available: None,
            leased: None,
            staged: None,
            published_step: None,
        });
        if slot.staged.is_some()
            || slot.leased.is_some()
            || (slot.generation != 0 && slot.available.is_none())
        {
            tracing::warn!(
                target: "astra_emu_manager_core::surface",
                event = "astra.emu.surface.pool_exhausted",
                fixed_step,
                "surface lease remains outstanding; retry the same generation"
            );
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_POOL_EXHAUSTED",
                "surface buffer is already leased",
            ));
        }
        if slot.generation != 0
            && (slot.width != width
                || slot.height != height
                || slot.stride != stride
                || slot.format != format)
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_RECONFIGURE",
                "surface dimensions and format are immutable for a live session",
            ));
        }
        let buffer = match slot.available.take() {
            Some(buffer) => buffer,
            None => {
                let mut bytes = Vec::new();
                bytes.try_reserve_exact(byte_len).map_err(|_| {
                    LegacyProviderError::invalid(
                        "ASTRA_EMU_SURFACE_ALLOCATION",
                        "surface allocation failed",
                    )
                })?;
                bytes.resize(byte_len, 0);
                OwnedWritableByteBuffer::from_vec(bytes)
            }
        };
        let generation = slot.generation.checked_add(1).ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_GENERATION",
                "surface generation overflow",
            )
        })?;
        slot.leased = Some((fixed_step, generation));
        Ok(LegacySurfaceLeaseV9 {
            lease_id: format!("surface.{fixed_step}.{generation}"),
            surface_id: surface_id.to_owned(),
            generation,
            width,
            height,
            stride,
            format,
            pixels: buffer,
        })
    }

    fn commit(
        &self,
        session_id: &str,
        fixed_step: u64,
        commit: LegacySurfaceCommitV9,
    ) -> Result<(), LegacyProviderError> {
        commit.validate()?;
        let key = SurfaceKey {
            session_id: session_id.to_owned(),
            surface_id: commit.lease.surface_id.clone(),
        };
        let mut slots = self.slots.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_LOCK", "surface pool lock poisoned")
        })?;
        let slot = slots.get_mut(&key).ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_UNKNOWN",
                "surface commit has no matching acquire",
            )
        })?;
        let expected_generation = slot.generation.checked_add(1).ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_GENERATION",
                "surface generation overflow",
            )
        })?;
        if slot.staged.is_some()
            || slot.leased != Some((fixed_step, expected_generation))
            || commit.lease.generation != expected_generation
            || commit.lease.width != slot.width
            || commit.lease.height != slot.height
            || commit.lease.stride != slot.stride
            || commit.lease.format != slot.format
            || commit.lease.lease_id != format!("surface.{fixed_step}.{expected_generation}")
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_OWNERSHIP",
                "surface commit does not match its outstanding lease",
            ));
        }
        slot.leased = None;
        slot.staged = Some((fixed_step, expected_generation, commit.lease.pixels));
        Ok(())
    }
}

pub trait SynchronousFamilyHookProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn invoke(
        &self,
        invocation: &LegacyHookInvocationV1,
    ) -> Result<LegacyHookResultV1, LegacyProviderError>;
}

struct HookBinding {
    provider_id: String,
    timeout_ms: u32,
    provider: Arc<dyn SynchronousFamilyHookProvider>,
}

#[derive(Default)]
pub struct FamilyHookHost {
    bindings: Mutex<BTreeMap<(String, String), HookBinding>>,
}

impl FamilyHookHost {
    pub fn bind(
        &self,
        family_id: impl Into<String>,
        family_game_id: impl Into<String>,
        timeout_ms: u32,
        provider: Arc<dyn SynchronousFamilyHookProvider>,
    ) -> Result<(), LegacyProviderError> {
        let family_id = family_id.into();
        let family_game_id = family_game_id.into();
        if family_id.is_empty() || family_game_id.is_empty() || provider.provider_id().is_empty() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_HOOK_BINDING",
                "hook binding identities must be non-empty",
            ));
        }
        let key = (family_id, family_game_id);
        let mut bindings = self.bindings.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_HOOK_LOCK", "hook binding lock poisoned")
        })?;
        if bindings.contains_key(&key) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_HOOK_BINDING_DUPLICATE",
                "family game already has a hook provider binding",
            ));
        }
        bindings.insert(
            key,
            HookBinding {
                provider_id: provider.provider_id().to_owned(),
                timeout_ms,
                provider,
            },
        );
        Ok(())
    }
}

impl LegacyHookHostV1 for FamilyHookHost {
    fn invoke(
        &self,
        mut invocation: LegacyHookInvocationV1,
    ) -> Result<LegacyHookResultV1, LegacyProviderError> {
        let bindings = self.bindings.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_HOOK_LOCK", "hook binding lock poisoned")
        })?;
        let Some(binding) = bindings.get(&(
            invocation.family_id.clone(),
            invocation.family_game_id.clone(),
        )) else {
            return Ok(LegacyHookResultV1 {
                status: LegacyHookStatusV1::Unbound,
                payload: OwnedByteBuffer::from_vec(Vec::new()),
                diagnostics: Vec::new(),
            });
        };
        if invocation.timeout_ms != binding.timeout_ms {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_HOOK_TIMEOUT_BINDING",
                "hook invocation timeout differs from the explicit game binding",
            ));
        }
        invocation.timeout_ms = binding.timeout_ms;
        let result = binding.provider.invoke(&invocation)?;
        if matches!(result.status, LegacyHookStatusV1::Unbound) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_HOOK_PROVIDER_UNBOUND",
                format!(
                    "bound hook provider {} returned unbound",
                    binding.provider_id
                ),
            ));
        }
        Ok(result)
    }
}

#[derive(Default)]
pub struct FamilyWritableFileHost {
    sessions: Mutex<BTreeMap<String, WritableSession>>,
    games: Mutex<BTreeMap<(String, String), String>>,
}

struct WritableSession {
    family_id: String,
    family_game_id: String,
    root: PathBuf,
}

impl FamilyWritableFileHost {
    pub fn bind_session(
        &self,
        session_id: impl Into<String>,
        family_id: impl Into<String>,
        family_game_id: impl Into<String>,
        root: impl AsRef<Path>,
    ) -> Result<(), LegacyProviderError> {
        let session_id = session_id.into();
        let family_id = family_id.into();
        let family_game_id = family_game_id.into();
        if session_id.is_empty() || family_id.is_empty() || family_game_id.is_empty() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_WRITABLE_BINDING",
                "writable session identities must be non-empty",
            ));
        }
        fs::create_dir_all(root.as_ref()).map_err(file_error)?;
        let root = fs::canonicalize(root.as_ref()).map_err(file_error)?;
        let key = (family_id.clone(), family_game_id.clone());
        let mut games = self.games.lock().map_err(|_| lock_error())?;
        if let Some(owner) = games.get(&key) {
            if owner != &session_id {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_WRITABLE_SESSION_CONFLICT",
                    "family game already has a writable session",
                ));
            }
        }
        let mut sessions = self.sessions.lock().map_err(|_| lock_error())?;
        if sessions.contains_key(&session_id) {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_WRITABLE_SESSION_DUPLICATE",
                "writable session is already bound",
            ));
        }
        games.insert(key, session_id.clone());
        sessions.insert(
            session_id,
            WritableSession {
                family_id,
                family_game_id,
                root,
            },
        );
        Ok(())
    }

    pub fn release_session(&self, session_id: &str) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        let Some(session) = sessions.remove(session_id) else {
            return;
        };
        if let Ok(mut games) = self.games.lock() {
            games.remove(&(session.family_id, session.family_game_id));
        }
    }

    fn resolve(&self, session_id: &str, relative: &str) -> Result<PathBuf, LegacyProviderError> {
        validate_relative_writable_path(relative)?;
        let sessions = self.sessions.lock().map_err(|_| lock_error())?;
        let session = sessions.get(session_id).ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_WRITABLE_SESSION_MISSING",
                "writable session is not bound",
            )
        })?;
        let path = session.root.join(relative.replace('\\', "/"));
        ensure_no_symlink_ancestor(&session.root, &path)?;
        Ok(path)
    }
}

impl LegacyWritableFileHostV1 for FamilyWritableFileHost {
    fn execute(
        &self,
        session_id: &str,
        request: LegacyWritableFileRequestV1,
    ) -> Result<LegacyWritableFileResultV1, LegacyProviderError> {
        request.validate()?;
        let empty = || LegacyWritableFileResultV1 {
            exists: false,
            is_file: false,
            length: 0,
            entries: Vec::new(),
            bytes: OwnedByteBuffer::from_vec(Vec::new()),
            written: 0,
        };
        match request {
            LegacyWritableFileRequestV1::Stat { path } => {
                let path = self.resolve(session_id, &path)?;
                match fs::metadata(path) {
                    Ok(metadata) => Ok(LegacyWritableFileResultV1 {
                        exists: true,
                        is_file: metadata.is_file(),
                        length: metadata.len(),
                        ..empty()
                    }),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(empty()),
                    Err(error) => Err(file_error(error)),
                }
            }
            LegacyWritableFileRequestV1::List { path } => {
                let path = self.resolve(session_id, &path)?;
                let mut entries = Vec::new();
                for entry in fs::read_dir(path).map_err(file_error)? {
                    let entry = entry.map_err(file_error)?;
                    let metadata = entry.metadata().map_err(file_error)?;
                    let name = entry.file_name().into_string().map_err(|_| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_WRITABLE_NAME_UTF8",
                            "writable entry name is not UTF-8",
                        )
                    })?;
                    entries.push(LegacyWritableFileEntryV1 {
                        name,
                        is_file: metadata.is_file(),
                        length: metadata.len(),
                    });
                }
                entries.sort_by(|left, right| left.name.cmp(&right.name));
                Ok(LegacyWritableFileResultV1 {
                    exists: true,
                    entries,
                    ..empty()
                })
            }
            LegacyWritableFileRequestV1::CreateDir { path } => {
                fs::create_dir_all(self.resolve(session_id, &path)?).map_err(file_error)?;
                Ok(LegacyWritableFileResultV1 {
                    exists: true,
                    ..empty()
                })
            }
            LegacyWritableFileRequestV1::ReadRange {
                path,
                offset,
                length,
            } => {
                let mut file = File::open(self.resolve(session_id, &path)?).map_err(file_error)?;
                let metadata = file.metadata().map_err(file_error)?;
                let end = offset.checked_add(length).ok_or_else(range_error)?;
                if end > metadata.len() {
                    return Err(range_error());
                }
                let length = usize::try_from(length).map_err(|_| range_error())?;
                let mut bytes = Vec::new();
                bytes.try_reserve_exact(length).map_err(|_| {
                    LegacyProviderError::invalid(
                        "ASTRA_EMU_WRITABLE_ALLOCATION",
                        "file range allocation failed",
                    )
                })?;
                bytes.resize(length, 0);
                file.seek(SeekFrom::Start(offset)).map_err(file_error)?;
                file.read_exact(&mut bytes).map_err(file_error)?;
                Ok(LegacyWritableFileResultV1 {
                    exists: true,
                    is_file: true,
                    length: metadata.len(),
                    bytes: OwnedByteBuffer::from_vec(bytes),
                    ..empty()
                })
            }
            LegacyWritableFileRequestV1::WriteRange {
                path,
                offset,
                bytes,
            } => {
                let path = self.resolve(session_id, &path)?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(file_error)?;
                }
                let mut file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(path)
                    .map_err(file_error)?;
                file.seek(SeekFrom::Start(offset)).map_err(file_error)?;
                file.write_all(bytes.as_slice()).map_err(file_error)?;
                Ok(LegacyWritableFileResultV1 {
                    exists: true,
                    is_file: true,
                    written: u64::try_from(bytes.len()).map_err(|_| range_error())?,
                    ..empty()
                })
            }
            LegacyWritableFileRequestV1::SetLength { path, length } => {
                let path = self.resolve(session_id, &path)?;
                let file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(path)
                    .map_err(file_error)?;
                file.set_len(length).map_err(file_error)?;
                Ok(LegacyWritableFileResultV1 {
                    exists: true,
                    is_file: true,
                    length,
                    ..empty()
                })
            }
            LegacyWritableFileRequestV1::Remove { path } => {
                let path = self.resolve(session_id, &path)?;
                let metadata = fs::metadata(&path).map_err(file_error)?;
                if metadata.is_file() {
                    fs::remove_file(path).map_err(file_error)?;
                } else {
                    fs::remove_dir(path).map_err(file_error)?;
                }
                Ok(empty())
            }
            LegacyWritableFileRequestV1::AtomicReplace {
                temporary_path,
                destination_path,
            } => {
                let temporary = self.resolve(session_id, &temporary_path)?;
                let destination = self.resolve(session_id, &destination_path)?;
                let temporary_file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&temporary)
                    .map_err(file_error)?;
                temporary_file.sync_all().map_err(file_error)?;
                drop(temporary_file);
                atomic_replace(&temporary, &destination).map_err(file_error)?;
                sync_parent(&destination).map_err(file_error)?;
                Ok(LegacyWritableFileResultV1 {
                    exists: true,
                    is_file: true,
                    length: fs::metadata(destination).map_err(file_error)?.len(),
                    ..empty()
                })
            }
        }
    }
}

#[derive(Clone)]
pub struct AstraEmuFamilyHost {
    pub surfaces: Arc<FamilySurfaceHost>,
    pub hooks: Arc<FamilyHookHost>,
    pub writable_files: Arc<FamilyWritableFileHost>,
    vfs: Arc<dyn LegacyVfsReader>,
    services: LegacyFamilyHostServicesV9,
}

impl AstraEmuFamilyHost {
    pub fn new(vfs: Arc<dyn LegacyVfsReader>) -> Self {
        let surfaces = Arc::new(FamilySurfaceHost::default());
        let hooks = Arc::new(FamilyHookHost::default());
        let writable_files = Arc::new(FamilyWritableFileHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs: vfs.clone(),
            surfaces: surfaces.clone(),
            hooks: hooks.clone(),
            writable_files: writable_files.clone(),
        };
        Self {
            surfaces,
            hooks,
            writable_files,
            vfs,
            services,
        }
    }

    pub fn services(&self) -> LegacyFamilyHostServicesV9 {
        self.services.clone()
    }

    pub fn vfs(&self) -> Arc<dyn LegacyVfsReader> {
        self.vfs.clone()
    }

    pub fn release_session(&self, session_id: &str) {
        self.surfaces.release_session(session_id);
        self.writable_files.release_session(session_id);
    }
}

fn ensure_no_symlink_ancestor(root: &Path, path: &Path) -> Result<(), LegacyProviderError> {
    if !path.starts_with(root) {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_WRITABLE_ESCAPE",
            "writable path escapes its root",
        ));
    }
    let mut current = root.to_path_buf();
    let relative = path.strip_prefix(root).map_err(|_| {
        LegacyProviderError::invalid(
            "ASTRA_EMU_WRITABLE_ESCAPE",
            "writable path escapes its root",
        )
    })?;
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_WRITABLE_SYMLINK",
                    "writable path traverses a symbolic link",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(file_error(error)),
        }
    }
    Ok(())
}

fn file_error(error: std::io::Error) -> LegacyProviderError {
    LegacyProviderError::remote("ASTRA_EMU_WRITABLE_IO", error.to_string())
}

fn lock_error() -> LegacyProviderError {
    LegacyProviderError::invalid("ASTRA_EMU_WRITABLE_LOCK", "writable host lock poisoned")
}

fn range_error() -> LegacyProviderError {
    LegacyProviderError::invalid(
        "ASTRA_EMU_WRITABLE_RANGE",
        "file range is outside its bounds",
    )
}

#[cfg(not(windows))]
fn atomic_replace(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn atomic_replace(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::GetLastError,
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
    };
    if !destination.exists() {
        return fs::rename(temporary, destination);
    }
    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    // SAFETY: both paths are valid, NUL-terminated UTF-16 buffers for the duration of the call.
    let result = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        // SAFETY: GetLastError has no preconditions.
        return Err(std::io::Error::from_raw_os_error(
            unsafe { GetLastError() } as i32
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> std::io::Result<()> {
    File::open(path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no parent",
        )
    })?)?
    .sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::{LegacyDamageRectV9, LegacySurfaceDamageV9};

    #[test]
    fn surface_pool_preserves_allocation_and_publishes_atomically() {
        let host = FamilySurfaceHost::default();
        let mut lease = host
            .acquire(
                "session",
                1,
                "main",
                2,
                1,
                LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            )
            .unwrap();
        let allocation = lease.pixels.as_slice().as_ptr();
        lease
            .pixels
            .as_mut_slice()
            .copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        host.commit(
            "session",
            1,
            LegacySurfaceCommitV9 {
                lease,
                damage: LegacySurfaceDamageV9::Full,
            },
        )
        .unwrap();
        assert!(host.take_published("session", "main", 1).is_err());
        host.publish_step("session", 1, &BTreeMap::from([("main".into(), 1)]))
            .unwrap();
        let surface = host.take_published("session", "main", 1).unwrap();
        assert_eq!(surface.pixels.as_slice().as_ptr(), allocation);
        assert_eq!(surface.pixels.as_slice(), [1, 2, 3, 4, 5, 6, 7, 8]);
        host.return_published(surface).unwrap();

        let lease = host
            .acquire(
                "session",
                2,
                "main",
                2,
                1,
                LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            )
            .unwrap();
        assert_eq!(lease.generation, 2);
        assert_eq!(lease.pixels.as_slice().as_ptr(), allocation);
        host.commit(
            "session",
            2,
            LegacySurfaceCommitV9 {
                lease,
                damage: LegacySurfaceDamageV9::Rects(vec![LegacyDamageRectV9 {
                    x: 1,
                    y: 0,
                    width: 1,
                    height: 1,
                }]),
            },
        )
        .unwrap();
        host.rollback_step("session", 2);
        let lease = host
            .acquire(
                "session",
                2,
                "main",
                2,
                1,
                LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            )
            .unwrap();
        assert_eq!(lease.generation, 2);
        assert_eq!(lease.pixels.as_slice().as_ptr(), allocation);
    }

    struct EchoHook;

    impl SynchronousFamilyHookProvider for EchoHook {
        fn provider_id(&self) -> &str {
            "translation.test"
        }

        fn invoke(
            &self,
            invocation: &LegacyHookInvocationV1,
        ) -> Result<LegacyHookResultV1, LegacyProviderError> {
            Ok(LegacyHookResultV1 {
                status: LegacyHookStatusV1::Completed,
                payload: invocation.payload.clone(),
                diagnostics: Vec::new(),
            })
        }
    }

    #[test]
    fn hook_binding_is_unique_and_timeout_is_exact() {
        let host = FamilyHookHost::default();
        let invocation = || LegacyHookInvocationV1 {
            session_id: "session".into(),
            invocation_id: "invocation".into(),
            family_id: "fvp".into(),
            family_game_id: "game".into(),
            hook_id: "astra.emu.translation.text.v1".into(),
            timeout_ms: 2_000,
            payload: OwnedByteBuffer::from_vec("原文".as_bytes().to_vec()),
        };
        assert_eq!(
            host.invoke(invocation()).unwrap().status,
            LegacyHookStatusV1::Unbound
        );
        host.bind("fvp", "game", 2_000, Arc::new(EchoHook)).unwrap();
        assert_eq!(
            host.invoke(invocation()).unwrap().payload.as_slice(),
            "原文".as_bytes()
        );
        assert!(host.bind("fvp", "game", 2_000, Arc::new(EchoHook)).is_err());
        let mut wrong_timeout = invocation();
        wrong_timeout.timeout_ms = 1;
        assert_eq!(
            host.invoke(wrong_timeout).unwrap_err().code(),
            "ASTRA_EMU_HOOK_TIMEOUT_BINDING"
        );
    }

    #[test]
    fn writable_root_supports_ranges_atomic_replace_and_single_writer() {
        let root = tempfile::tempdir().unwrap();
        let host = FamilyWritableFileHost::default();
        host.bind_session("session", "fvp", "game", root.path())
            .unwrap();
        assert_eq!(
            host.bind_session("other", "fvp", "game", root.path())
                .unwrap_err()
                .code(),
            "ASTRA_EMU_WRITABLE_SESSION_CONFLICT"
        );
        host.execute(
            "session",
            LegacyWritableFileRequestV1::WriteRange {
                path: "save/tmp.dat".into(),
                offset: 0,
                bytes: vec![1, 2, 3, 4],
            },
        )
        .unwrap();
        host.execute(
            "session",
            LegacyWritableFileRequestV1::AtomicReplace {
                temporary_path: "save/tmp.dat".into(),
                destination_path: "save/slot.dat".into(),
            },
        )
        .unwrap();
        let read = host
            .execute(
                "session",
                LegacyWritableFileRequestV1::ReadRange {
                    path: "save/slot.dat".into(),
                    offset: 1,
                    length: 2,
                },
            )
            .unwrap();
        assert_eq!(read.bytes.as_slice(), [2, 3]);
        assert_eq!(
            host.execute(
                "session",
                LegacyWritableFileRequestV1::Stat {
                    path: "../escape".into(),
                },
            )
            .unwrap_err()
            .code(),
            "ASTRA_EMU_WRITABLE_PATH"
        );
    }
}
