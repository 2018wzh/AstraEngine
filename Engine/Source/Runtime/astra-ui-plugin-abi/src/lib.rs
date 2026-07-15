//! Signed, process-isolated UI component protocol.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};

#[cfg(feature = "ffi")]
use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RString, RVec},
    StableAbi,
};
use astra_core::Hash256;
use astra_ui_core::{
    UiActionEnvelope, UiFrameRequest, UiRenderFrame, UiSemanticSnapshot, ValidateUi, MAX_DTO_BYTES,
    MAX_EFFECTS_PER_CALL, MAX_SESSION_STATE_BYTES,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const UI_COMPONENT_SLOT: &str = "ui_component";
pub const UI_COMPONENT_MANIFEST_SCHEMA: &str = "astra.ui_component_manifest.v1";
pub const UI_COMPONENT_PROTOCOL_VERSION: u16 = 1;
pub const UI_COMPONENT_FRAME_MAGIC: [u8; 4] = *b"AUI1";
pub const UI_COMPONENT_MAX_FRAME_BYTES: usize = MAX_DTO_BYTES;

#[derive(Debug, Error)]
pub enum UiComponentError {
    #[error("{0}")]
    Invalid(String),
    #[error("component I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("component codec failed: {0}")]
    Codec(String),
    #[error("component signature failed")]
    Signature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiComponentManifest {
    pub schema: String,
    pub component_id: String,
    pub component_version: String,
    pub signer_id: String,
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
    pub artifact_hash: Hash256,
    pub input_schema: String,
    pub output_schema: String,
    pub capabilities: BTreeSet<String>,
    pub signature: Vec<u8>,
}

impl UiComponentManifest {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, UiComponentError> {
        #[derive(Serialize)]
        struct Canonical<'a> {
            schema: &'a str,
            component_id: &'a str,
            component_version: &'a str,
            signer_id: &'a str,
            engine_version: &'a str,
            rustc_fingerprint: &'a str,
            feature_fingerprint: &'a str,
            abi_fingerprint: &'a str,
            artifact_hash: Hash256,
            input_schema: &'a str,
            output_schema: &'a str,
            capabilities: &'a BTreeSet<String>,
        }
        postcard::to_allocvec(&Canonical {
            schema: &self.schema,
            component_id: &self.component_id,
            component_version: &self.component_version,
            signer_id: &self.signer_id,
            engine_version: &self.engine_version,
            rustc_fingerprint: &self.rustc_fingerprint,
            feature_fingerprint: &self.feature_fingerprint,
            abi_fingerprint: &self.abi_fingerprint,
            artifact_hash: self.artifact_hash,
            input_schema: &self.input_schema,
            output_schema: &self.output_schema,
            capabilities: &self.capabilities,
        })
        .map_err(|error| UiComponentError::Codec(error.to_string()))
    }

    pub fn sign(&mut self, key: &SigningKey) -> Result<(), UiComponentError> {
        self.validate_unsigned()?;
        self.signature = key.sign(&self.canonical_bytes()?).to_bytes().to_vec();
        Ok(())
    }

    pub fn verify(
        &self,
        artifact: &[u8],
        allowlist: &BTreeMap<String, [u8; 32]>,
    ) -> Result<(), UiComponentError> {
        self.validate_unsigned()?;
        if Hash256::from_sha256(artifact) != self.artifact_hash {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_ARTIFACT_HASH: artifact hash mismatch".to_string(),
            ));
        }
        let public = allowlist
            .get(&self.signer_id)
            .ok_or(UiComponentError::Signature)?;
        let key = VerifyingKey::from_bytes(public).map_err(|_| UiComponentError::Signature)?;
        let signature_bytes: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| UiComponentError::Signature)?;
        key.verify(
            &self.canonical_bytes()?,
            &Signature::from_bytes(&signature_bytes),
        )
        .map_err(|_| UiComponentError::Signature)
    }

    /// Verifies the descriptor exported by the dylib without creating an
    /// impossible self-referential artifact hash. The embedded descriptor must
    /// deliberately leave the artifact hash and signature empty; the signed
    /// sidecar remains the sole trust authority for those fields.
    pub fn verify_embedded_descriptor(&self, embedded: &Self) -> Result<(), UiComponentError> {
        embedded.validate_unsigned()?;
        if embedded.artifact_hash != Hash256::from_bytes([0; 32]) || !embedded.signature.is_empty()
        {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_EMBEDDED_TRUST_FIELDS: embedded descriptor must not claim artifact trust"
                    .to_string(),
            ));
        }
        let mut expected = self.clone();
        expected.artifact_hash = Hash256::from_bytes([0; 32]);
        expected.signature.clear();
        if &expected != embedded {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_EMBEDDED_MANIFEST: embedded descriptor differs from signed sidecar identity"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn validate_unsigned(&self) -> Result<(), UiComponentError> {
        if self.schema != UI_COMPONENT_MANIFEST_SCHEMA
            || !safe_id(&self.component_id)
            || !safe_id(&self.signer_id)
            || self.input_schema != "astra.ui_component_request.v1"
            || self.output_schema != "astra.ui_component_response.v1"
        {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_MANIFEST: invalid manifest identity or schema".to_string(),
            ));
        }
        if self.capabilities.len() > 64 || self.capabilities.iter().any(|value| value.len() > 256) {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_CAPABILITY_LIMIT: capability set exceeds limits".to_string(),
            ));
        }
        for value in [
            &self.component_version,
            &self.engine_version,
            &self.rustc_fingerprint,
            &self.feature_fingerprint,
            &self.abi_fingerprint,
        ] {
            if value.is_empty() || value.len() > 256 {
                return Err(UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_FINGERPRINT: version and fingerprint fields must contain 1..=256 bytes"
                        .to_string(),
                ));
            }
        }
        if self.capabilities.iter().any(|value| !safe_id(value)) {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_CAPABILITY_ID: capability is not a safe identifier".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiComponentRequest {
    Open {
        session_id: String,
        component_id: String,
        initial_state: Vec<u8>,
    },
    Frame {
        request: Box<UiFrameRequest>,
    },
    Snapshot,
    Restore {
        state: Vec<u8>,
    },
    Shutdown,
}

impl UiComponentRequest {
    pub fn validate(&self) -> Result<(), UiComponentError> {
        match self {
            Self::Open {
                session_id,
                component_id,
                initial_state,
            } => {
                if !safe_id(session_id) || !safe_id(component_id) {
                    return Err(UiComponentError::Invalid(
                        "ASTRA_UI_COMPONENT_OPEN_ID: session and component IDs must be safe identifiers"
                            .to_string(),
                    ));
                }
                if initial_state.len() > MAX_SESSION_STATE_BYTES {
                    return Err(UiComponentError::Invalid(
                        "ASTRA_UI_COMPONENT_STATE_LIMIT: session state exceeds 1 MiB".to_string(),
                    ));
                }
            }
            Self::Restore {
                state: initial_state,
            } => {
                if initial_state.len() > MAX_SESSION_STATE_BYTES {
                    return Err(UiComponentError::Invalid(
                        "ASTRA_UI_COMPONENT_STATE_LIMIT: session state exceeds 1 MiB".to_string(),
                    ));
                }
            }
            Self::Frame { request } => request
                .validate()
                .map_err(|error| UiComponentError::Invalid(error.to_string()))?,
            Self::Snapshot | Self::Shutdown => {}
        }
        validate_postcard_size(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiComponentResponse {
    Opened,
    Frame {
        render: Box<UiRenderFrame>,
        semantics: Box<UiSemanticSnapshot>,
        actions: Vec<UiActionEnvelope>,
    },
    Snapshot {
        state: Vec<u8>,
    },
    Restored,
    Shutdown,
    Failed {
        code: String,
        message: String,
    },
}

impl UiComponentResponse {
    pub fn validate(&self) -> Result<(), UiComponentError> {
        match self {
            Self::Frame {
                render,
                semantics,
                actions,
            } => {
                if actions.len() > MAX_EFFECTS_PER_CALL {
                    return Err(UiComponentError::Invalid(
                        "ASTRA_UI_COMPONENT_EFFECT_LIMIT: component returned too many actions"
                            .to_string(),
                    ));
                }
                render
                    .validate()
                    .map_err(|error| UiComponentError::Invalid(error.to_string()))?;
                semantics
                    .validate()
                    .map_err(|error| UiComponentError::Invalid(error.to_string()))?;
                if render.session_id != semantics.session_id
                    || render.generation != semantics.generation
                {
                    return Err(UiComponentError::Invalid(
                        "ASTRA_UI_COMPONENT_FRAME_IDENTITY: render and semantics mismatch"
                            .to_string(),
                    ));
                }
                for action in actions {
                    action
                        .validate()
                        .map_err(|error| UiComponentError::Invalid(error.to_string()))?;
                }
            }
            Self::Snapshot { state } if state.len() > MAX_SESSION_STATE_BYTES => {
                return Err(UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_STATE_LIMIT: session state exceeds 1 MiB".to_string(),
                ));
            }
            Self::Failed { code, message }
                if !safe_id(code) || code.len() > 256 || message.len() > 4096 =>
            {
                return Err(UiComponentError::Invalid(
                    "ASTRA_UI_COMPONENT_DIAGNOSTIC_LIMIT: diagnostic exceeds limits".to_string(),
                ));
            }
            Self::Opened
            | Self::Snapshot { .. }
            | Self::Restored
            | Self::Shutdown
            | Self::Failed { .. } => {}
        }
        validate_postcard_size(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiComponentFrame {
    pub kind: u16,
    pub sequence: u64,
    pub deadline_ns: u64,
    pub payload: Vec<u8>,
}

impl UiComponentFrame {
    pub fn encode<W: Write>(&self, writer: &mut W) -> Result<(), UiComponentError> {
        if self.payload.len() > UI_COMPONENT_MAX_FRAME_BYTES
            || self.deadline_ns == 0
            || self.sequence == 0
        {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_FRAME_LIMIT: invalid payload length or deadline".to_string(),
            ));
        }
        writer.write_all(&UI_COMPONENT_FRAME_MAGIC)?;
        writer.write_all(&UI_COMPONENT_PROTOCOL_VERSION.to_le_bytes())?;
        writer.write_all(&self.kind.to_le_bytes())?;
        writer.write_all(&self.sequence.to_le_bytes())?;
        writer.write_all(&self.deadline_ns.to_le_bytes())?;
        writer.write_all(&(self.payload.len() as u32).to_le_bytes())?;
        writer.write_all(Hash256::from_sha256(&self.payload).as_bytes())?;
        writer.write_all(&self.payload)?;
        writer.flush()?;
        Ok(())
    }

    pub fn decode<R: Read>(reader: &mut R) -> Result<Self, UiComponentError> {
        let mut magic = [0; 4];
        reader.read_exact(&mut magic)?;
        if magic != UI_COMPONENT_FRAME_MAGIC {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_FRAME_MAGIC: invalid frame magic".to_string(),
            ));
        }
        let version = read_u16(reader)?;
        if version != UI_COMPONENT_PROTOCOL_VERSION {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_FRAME_VERSION: unsupported protocol version".to_string(),
            ));
        }
        let kind = read_u16(reader)?;
        let sequence = read_u64(reader)?;
        let deadline_ns = read_u64(reader)?;
        let length = read_u32(reader)? as usize;
        if length > UI_COMPONENT_MAX_FRAME_BYTES || deadline_ns == 0 || sequence == 0 {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_FRAME_LIMIT: invalid payload length or deadline".to_string(),
            ));
        }
        let mut expected_hash = [0; 32];
        reader.read_exact(&mut expected_hash)?;
        let mut payload = vec![0; length];
        reader.read_exact(&mut payload)?;
        if Hash256::from_sha256(&payload).as_bytes() != &expected_hash {
            return Err(UiComponentError::Invalid(
                "ASTRA_UI_COMPONENT_FRAME_HASH: payload hash mismatch".to_string(),
            ));
        }
        Ok(Self {
            kind,
            sequence,
            deadline_ns,
            payload,
        })
    }
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        })
        && !value.contains("..")
        && !value.starts_with('/')
}

fn validate_postcard_size<T: Serialize>(value: &T) -> Result<(), UiComponentError> {
    let bytes =
        postcard::to_allocvec(value).map_err(|error| UiComponentError::Codec(error.to_string()))?;
    if bytes.len() > MAX_DTO_BYTES {
        return Err(UiComponentError::Invalid(
            "ASTRA_UI_COMPONENT_DTO_LIMIT: DTO exceeds 4 MiB".to_string(),
        ));
    }
    Ok(())
}

fn read_u16(reader: &mut impl Read) -> Result<u16, std::io::Error> {
    let mut bytes = [0; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}
fn read_u32(reader: &mut impl Read) -> Result<u32, std::io::Error> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn read_u64(reader: &mut impl Read) -> Result<u64, std::io::Error> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(StableAbi)]
pub struct FfiUiComponentResult {
    pub ok: bool,
    pub payload: RVec<u8>,
    pub diagnostic: RString,
}

#[cfg(feature = "ffi")]
pub type FfiUiComponentInvoke = extern "C" fn(RVec<u8>) -> FfiUiComponentResult;

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(
    prefix_ref = UiComponentModuleRef,
    prefix_fields = UiComponentModulePrefix
)))]
#[sabi(missing_field(panic))]
pub struct UiComponentModule {
    pub manifest_postcard: extern "C" fn() -> RVec<u8>,
    #[sabi(unsafe_opaque_field)]
    pub create: FfiUiComponentInvoke,
    #[sabi(unsafe_opaque_field)]
    pub frame: FfiUiComponentInvoke,
    #[sabi(unsafe_opaque_field)]
    pub snapshot: FfiUiComponentInvoke,
    #[sabi(unsafe_opaque_field)]
    pub restore: FfiUiComponentInvoke,
    #[sabi(last_prefix_field)]
    #[sabi(unsafe_opaque_field)]
    pub shutdown: FfiUiComponentInvoke,
}

#[cfg(feature = "ffi")]
impl RootModule for UiComponentModuleRef {
    abi_stable::declare_root_module_statics! {UiComponentModuleRef}
    const BASE_NAME: &'static str = "astra_ui_component_module";
    const NAME: &'static str = "astra-ui-component";
    const VERSION_STRINGS: VersionStrings = abi_stable::package_version_strings!();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(artifact_hash: Hash256) -> UiComponentManifest {
        UiComponentManifest {
            schema: UI_COMPONENT_MANIFEST_SCHEMA.into(),
            component_id: "fixture.component".into(),
            component_version: "1.0.0".into(),
            signer_id: "fixture.test_signer".into(),
            engine_version: "test".into(),
            rustc_fingerprint: "rustc.test".into(),
            feature_fingerprint: "features.test".into(),
            abi_fingerprint: "abi.test".into(),
            artifact_hash,
            input_schema: "astra.ui_component_request.v1".into(),
            output_schema: "astra.ui_component_response.v1".into(),
            capabilities: BTreeSet::from(["ui.render_frame".into()]),
            signature: Vec::new(),
        }
    }

    #[astra_headless_test::test]
    fn signed_manifest_binds_artifact_and_embedded_descriptor() {
        let artifact = b"test-only component artifact";
        let signing = SigningKey::from_bytes(&[7; 32]);
        let mut manifest = descriptor(Hash256::from_sha256(artifact));
        manifest.sign(&signing).expect("sign");
        manifest
            .verify(
                artifact,
                &BTreeMap::from([(
                    "fixture.test_signer".into(),
                    signing.verifying_key().to_bytes(),
                )]),
            )
            .expect("verify");
        let embedded = descriptor(Hash256::from_bytes([0; 32]));
        manifest
            .verify_embedded_descriptor(&embedded)
            .expect("embedded descriptor");
        assert!(manifest.verify(b"different", &BTreeMap::new()).is_err());
    }

    #[astra_headless_test::test]
    fn framed_ipc_rejects_hash_and_sequence_corruption() {
        let frame = UiComponentFrame {
            kind: 1,
            sequence: 1,
            deadline_ns: 1_000_000,
            payload: vec![1, 2, 3],
        };
        let mut bytes = Vec::new();
        frame.encode(&mut bytes).expect("encode");
        assert_eq!(
            UiComponentFrame::decode(&mut bytes.as_slice()).expect("decode"),
            frame
        );
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        assert!(UiComponentFrame::decode(&mut bytes.as_slice()).is_err());

        let invalid = UiComponentFrame {
            sequence: 0,
            ..frame
        };
        assert!(invalid.encode(&mut Vec::new()).is_err());
    }
}
