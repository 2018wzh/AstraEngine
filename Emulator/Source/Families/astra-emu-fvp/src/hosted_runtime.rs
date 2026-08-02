//! Thread-confined RFVP hosted-session lifecycle.
//!
//! This is the concrete v5 execution boundary used by the dynamic provider:
//! the RFVP core and its non-`Send` VFS cursors remain on one worker while the
//! caller exchanges only bounded semantic deltas and opaque snapshots.

use std::collections::BTreeMap;

use astra_emu_family_api::{LegacyPreparedSceneCommitV1, LegacySceneResourceStateV1};
use rfvp_hosted::{
    hosted::{
        HostedBootConfig, HostedConfig, HostedLimits, HostedSession, HostedStepDelta,
        HostedStepInput, HostedTraceProfile,
    },
    script::parser::Nls,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    hosted::HostedScenePacketTranslator,
    hosted_host::HostedMemoryHost,
    hosted_worker::{HostedSessionWorker, HostedWorkerError, HostedWorkerStartError},
    FvpNls,
};

pub const MAX_HOSTED_CASE_FILES: usize = 65_536;
pub const MAX_HOSTED_HCB_BYTES: usize = 512 * 1024 * 1024;
const MAX_HOSTED_RUNTIME_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum HostedRuntimeError {
    #[error("hosted case script URI is invalid")]
    ScriptUri,
    #[error("hosted case script conflicts with a supplied file")]
    ScriptCollision,
    #[error("hosted session initialization failed: {0}")]
    Initialization(String),
    #[error(transparent)]
    Worker(#[from] HostedWorkerError),
    #[error("hosted core failed: {0:?}")]
    Core(rfvp_hosted::host_api::RfvpError),
    #[error("hosted scene delta is invalid: {0}")]
    Scene(String),
    #[error("hosted snapshot is invalid")]
    Snapshot,
}

struct HostedState {
    core: HostedSession,
    host: HostedMemoryHost,
    translator: HostedScenePacketTranslator,
}

/// Astra-owned envelope around the opaque fork snapshot.  The fork owns VM
/// state while this boundary owns incremental scene resource metadata; both
/// are required to resume the next semantic transaction faithfully.
#[derive(Debug, Serialize, Deserialize)]
struct HostedRuntimeSnapshotV1 {
    version: u16,
    core_bytes: Vec<u8>,
    scene_resources: LegacySceneResourceStateV1,
}

const HOSTED_RUNTIME_SNAPSHOT_VERSION: u16 = 1;

/// Sendable owner of one non-Send RFVP hosted-core session.
pub struct HostedFvpSession {
    worker: HostedSessionWorker<HostedState>,
}

impl HostedFvpSession {
    pub fn open_case(
        mut files: BTreeMap<String, Vec<u8>>,
        script_uri: String,
        script_bytes: Vec<u8>,
        nls: FvpNls,
        stage_width: u32,
        stage_height: u32,
    ) -> Result<Self, HostedRuntimeError> {
        if !script_uri.ends_with(".hcb") {
            return Err(HostedRuntimeError::ScriptUri);
        }
        if let Some(existing) = files.insert(script_uri, script_bytes.clone()) {
            if existing != script_bytes {
                return Err(HostedRuntimeError::ScriptCollision);
            }
        }
        let worker = HostedSessionWorker::try_spawn(move || {
            let mut host = HostedMemoryHost::new(files).map_err(HostedRuntimeError::Core)?;
            let mut core = HostedSession::new(
                HostedConfig {
                    virtual_width: stage_width,
                    virtual_height: stage_height,
                    ..HostedConfig::default()
                },
                HostedLimits::default(),
            )
            .map_err(HostedRuntimeError::Core)?;
            core.set_trace_profile(HostedTraceProfile::Shipping)
                .map_err(HostedRuntimeError::Core)?;
            core.boot(
                &mut host,
                HostedBootConfig {
                    asset_root: ".",
                    hcb_extension: "hcb",
                    max_hcb_bytes: MAX_HOSTED_HCB_BYTES,
                    max_manifest_entries: MAX_HOSTED_CASE_FILES,
                    nls: map_nls(nls),
                },
            )
            .map_err(HostedRuntimeError::Core)?;
            Ok::<_, HostedRuntimeError>(HostedState {
                core,
                host,
                translator: HostedScenePacketTranslator::default(),
            })
        })
        .map_err(map_start_error)?;
        Ok(Self { worker })
    }

    pub fn step(
        &self,
        delta_ns: u64,
        input: HostedStepInput,
    ) -> Result<(HostedStepDelta, Option<LegacyPreparedSceneCommitV1>), HostedRuntimeError> {
        self.worker.execute_result(move |state| {
            state
                .host
                .advance(delta_ns)
                .map_err(HostedRuntimeError::Core)?;
            let delta = state
                .core
                .step(&mut state.host, input)
                .map_err(HostedRuntimeError::Core)?;
            let prepared = state
                .translator
                .translate(&delta)
                .map_err(|error| HostedRuntimeError::Scene(error.to_string()))?;
            Ok((delta, prepared))
        })?
    }

    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, HostedRuntimeError> {
        self.worker.execute_result(|state| {
            let core_bytes = state
                .core
                .snapshot_bytes()
                .map_err(HostedRuntimeError::Core)?;
            let bytes = bincode::serialize(&HostedRuntimeSnapshotV1 {
                version: HOSTED_RUNTIME_SNAPSHOT_VERSION,
                core_bytes,
                scene_resources: state.translator.snapshot(),
            })
            .map_err(|_| HostedRuntimeError::Snapshot)?;
            if bytes.len() > MAX_HOSTED_RUNTIME_SNAPSHOT_BYTES {
                return Err(HostedRuntimeError::Snapshot);
            }
            Ok(bytes)
        })?
    }

    pub fn restore_bytes(&self, bytes: Vec<u8>) -> Result<(), HostedRuntimeError> {
        self.worker.execute_result(move |state| {
            if bytes.is_empty() || bytes.len() > MAX_HOSTED_RUNTIME_SNAPSHOT_BYTES {
                return Err(HostedRuntimeError::Snapshot);
            }
            let snapshot: HostedRuntimeSnapshotV1 =
                bincode::deserialize(&bytes).map_err(|_| HostedRuntimeError::Snapshot)?;
            if snapshot.version != HOSTED_RUNTIME_SNAPSHOT_VERSION {
                return Err(HostedRuntimeError::Snapshot);
            }
            state
                .core
                .restore_bytes(&snapshot.core_bytes)
                .map_err(HostedRuntimeError::Core)?;
            state.translator.restore(snapshot.scene_resources);
            Ok(())
        })?
    }

    pub fn read_resource(
        &self,
        resource_uri: String,
        max_bytes: usize,
    ) -> Result<Vec<u8>, HostedRuntimeError> {
        self.worker.execute_result(move |state| {
            state
                .core
                .read_resource(&resource_uri, max_bytes)
                .map_err(HostedRuntimeError::Core)
        })?
    }

    pub fn complete_video(&self) -> Result<(), HostedRuntimeError> {
        self.worker.execute_result(|state| {
            state
                .core
                .complete_video()
                .map_err(HostedRuntimeError::Core)
        })?
    }

    pub fn quit_requested(&self) -> Result<bool, HostedRuntimeError> {
        self.worker
            .execute_result(|state| Ok::<_, HostedRuntimeError>(state.core.quit_requested()))?
    }

    pub fn shutdown(self) -> Result<(), HostedRuntimeError> {
        self.worker.shutdown().map_err(HostedRuntimeError::Worker)
    }
}

fn map_start_error(error: HostedWorkerStartError<HostedRuntimeError>) -> HostedRuntimeError {
    match error {
        HostedWorkerStartError::Initialization(error) => error,
        HostedWorkerStartError::Worker(error) => HostedRuntimeError::Worker(error),
    }
}

fn map_nls(nls: FvpNls) -> Nls {
    match nls {
        FvpNls::ShiftJis => Nls::ShiftJIS,
        FvpNls::Gbk => Nls::GBK,
        FvpNls::Utf8 => Nls::UTF8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_lifecycle_emits_a_prepared_semantic_frame() {
        let session = HostedFvpSession::open_case(
            BTreeMap::from([(
                "default.ttf".into(),
                include_bytes!(
                    "../../../../../Engine/Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf"
                )
                .to_vec(),
            )]),
            "script.hcb".into(),
            terminal_hcb(),
            FvpNls::Utf8,
            1024,
            768,
        )
        .expect("hosted case must boot");
        let (delta, prepared) = session
            .step(16_666_667, HostedStepInput::default())
            .expect("hosted case must step");
        assert_eq!(delta.tick.frame_index, 1);
        assert!(prepared.is_some());
        let snapshot = session.snapshot_bytes().expect("snapshot must capture");
        session
            .restore_bytes(snapshot)
            .expect("snapshot must restore");
        assert!(!session
            .quit_requested()
            .expect("quit state must be readable"));
        assert!(session.read_resource("missing.bin".into(), 16).is_err());
        session.shutdown().expect("worker must stop");
    }

    fn terminal_hcb() -> Vec<u8> {
        let mut bytes = 8u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0x04, 0, 0, 0]);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[8, 0, 2, b'X', 0]);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }
}
