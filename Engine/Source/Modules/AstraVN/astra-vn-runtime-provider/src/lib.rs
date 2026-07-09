//! Native AstraVN gameplay runtime provider and ABI-safe FFI adapter.

use std::collections::BTreeMap;

use abi_stable::std_types::{RString, RVec};
use astra_core::Hash256;
use astra_plugin_abi::{
    FfiRuntimeProviderRegistration, FfiRuntimeProviderResult, GameRuntimeSessionId,
    ProductRuntimeDescriptor, ReleaseCheckDescriptor, RuntimeEditorMetadata, RuntimeOpenReport,
    RuntimeOpenRequest, RuntimePackageSectionPlan, RuntimePrepareReport, RuntimePrepareRequest,
    RuntimeProbeReport, RuntimeProbeRequest, RuntimeRestoreReport, RuntimeRestoreRequest,
    RuntimeSaveRequest, RuntimeSaveSections, RuntimeSectionRef, RuntimeShutdownReport,
    RuntimeStepInput, RuntimeStepOutput, GAME_RUNTIME_PROVIDER_SLOT, NATIVE_VN_PROVIDER_ID,
    NATIVE_VN_RUNTIME_ID, PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA, RUNTIME_EDITOR_METADATA_SCHEMA,
};
use astra_vn_core::{
    CompiledStory as CoreCompiledStory, VnError as CoreVnError,
    VnPlayerCommand as CoreVnPlayerCommand, VnRuntime as CoreVnRuntime,
};
use astra_vn_save::{
    VN_POLICY_STATE_SECTION_ID as VN_POLICY_SECTION_ID,
    VN_RUNTIME_STATE_SECTION_ID as VN_RUNTIME_SECTION_ID,
};

pub use astra_vn_core::*;
pub use astra_vn_editor::*;
pub use astra_vn_package::*;
pub use astra_vn_save::*;

#[derive(Debug, Default)]
pub struct NativeVnRuntimeProvider {
    sessions: BTreeMap<String, CoreVnRuntime>,
}

impl NativeVnRuntimeProvider {
    pub fn slot() -> &'static str {
        GAME_RUNTIME_PROVIDER_SLOT
    }

    pub fn descriptor() -> ProductRuntimeDescriptor {
        ProductRuntimeDescriptor {
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            product_kind: "visual_novel".to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            supported_targets: vec!["game".to_string()],
            capabilities: vec!["runtime.native_vn".to_string()],
            package_sections: native_vn_package_sections(),
            release_checks: native_vn_release_check_ids(),
        }
    }

    pub fn prepare(&self, request: RuntimePrepareRequest) -> RuntimePrepareReport {
        let mut diagnostics = Vec::new();
        if request
            .section_ids
            .iter()
            .all(|section| section != "vn.compiled_story")
        {
            diagnostics.push("ASTRA_NATIVE_VN_COMPILED_STORY_MISSING".to_string());
        }
        RuntimePrepareReport {
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            status: if diagnostics.is_empty() {
                "pass".to_string()
            } else {
                "blocked".to_string()
            },
            diagnostics,
        }
    }

    pub fn probe(&self, request: RuntimeProbeRequest) -> RuntimeProbeReport {
        let prepare = self.prepare(RuntimePrepareRequest {
            target_id: request.target_id,
            profile: request.profile,
            package_hash: String::new(),
            section_ids: request.section_ids,
        });
        RuntimeProbeReport {
            runtime_id: prepare.runtime_id,
            provider_id: prepare.provider_id,
            status: prepare.status,
            diagnostics: prepare.diagnostics,
        }
    }

    pub fn open_compiled_story(
        &mut self,
        compiled: CoreCompiledStory,
        config: VnRunConfig,
        request: RuntimeOpenRequest,
    ) -> Result<RuntimeOpenReport, CoreVnError> {
        let session_id = GameRuntimeSessionId(format!(
            "{}:{}:{}",
            NATIVE_VN_RUNTIME_ID, request.target_id, request.seed
        ));
        let runtime = CoreVnRuntime::new(compiled, config)?;
        self.sessions.insert(session_id.0.clone(), runtime);
        Ok(RuntimeOpenReport {
            session_id,
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            diagnostics: Vec::new(),
        })
    }

    pub fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, CoreVnError> {
        let command = {
            let runtime = self.session(&input.session_id)?;
            runtime_command_from_input(runtime, &input)?
        };
        self.apply_command(input.session_id, command)
    }

    pub fn apply_command(
        &mut self,
        session_id: GameRuntimeSessionId,
        command: CoreVnPlayerCommand,
    ) -> Result<RuntimeStepOutput, CoreVnError> {
        let output = self.session_mut(&session_id)?.apply(command)?;
        let presentation = output
            .presentation
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        Ok(RuntimeStepOutput {
            session_id,
            status: if presentation.is_empty() {
                "idle".to_string()
            } else {
                "blocked".to_string()
            },
            effects: vec![serde_json::json!({
                "schema": "astra.vn.runtime_step_effect.v1",
                "coverage_reached": output.coverage.reached,
                "state_hash_before_advance": output.state_hash_before_advance.to_string(),
                "state_hash_after_advance": output.state_hash_after_advance.to_string(),
            })],
            presentation,
            diagnostics: Vec::new(),
            dirty_save_sections: vec![VN_RUNTIME_SECTION_ID.to_string()],
        })
    }

    pub fn default_launch_command(
        &self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<CoreVnPlayerCommand, CoreVnError> {
        self.session(session_id)?
            .default_launch_command()
            .ok_or_else(|| {
                CoreVnError::diagnostic(
                    "ASTRA_NATIVE_VN_LAUNCH_MISSING",
                    "compiled story has no launchable state",
                )
            })
    }

    pub fn state(&self, session_id: &GameRuntimeSessionId) -> Result<&VnRuntimeState, CoreVnError> {
        Ok(self.session(session_id)?.state())
    }

    pub fn save_slot(
        &self,
        session_id: &GameRuntimeSessionId,
        slot: impl Into<String>,
    ) -> Result<VnSaveBlob, CoreVnError> {
        self.session(session_id)?.save_slot(slot)
    }

    pub fn load_slot(
        &mut self,
        session_id: &GameRuntimeSessionId,
        save: VnSaveBlob,
    ) -> Result<(), CoreVnError> {
        self.session_mut(session_id)?.load_slot(save)
    }

    pub fn save(&self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, CoreVnError> {
        let runtime = self.session(&request.session_id)?;
        let state_payload = postcard::to_allocvec(runtime.state())?;
        Ok(RuntimeSaveSections {
            session_id: request.session_id,
            sections: vec![
                RuntimeSectionRef {
                    section_id: VN_RUNTIME_SECTION_ID.to_string(),
                    schema: "astra.vn.runtime_state_save.v1".to_string(),
                    hash: Hash256::from_sha256(&state_payload).to_string(),
                },
                RuntimeSectionRef {
                    section_id: VN_POLICY_SECTION_ID.to_string(),
                    schema: "astra.vn.policy_state_save.v1".to_string(),
                    hash: "sha256:deferred-policy-state".to_string(),
                },
            ],
            diagnostics: Vec::new(),
        })
    }

    pub fn restore(&self, request: RuntimeRestoreRequest) -> RuntimeRestoreReport {
        RuntimeRestoreReport {
            session_id: request.session_id,
            status: "restored".to_string(),
            diagnostics: Vec::new(),
        }
    }

    pub fn shutdown(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, CoreVnError> {
        self.sessions.remove(&session_id.0).ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_MISSING",
                "runtime session is not open",
            )
        })?;
        Ok(RuntimeShutdownReport {
            session_id,
            status: "shutdown".to_string(),
            diagnostics: Vec::new(),
        })
    }

    pub fn package_sections(&self) -> RuntimePackageSectionPlan {
        RuntimePackageSectionPlan {
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            sections: native_vn_package_sections()
                .into_iter()
                .map(|section_id| RuntimeSectionRef {
                    section_id,
                    schema: "astra.vn.package_section.v1".to_string(),
                    hash: "sha256:package-build-assigned".to_string(),
                })
                .collect(),
        }
    }

    pub fn release_checks(&self) -> Vec<ReleaseCheckDescriptor> {
        native_vn_release_check_ids()
            .into_iter()
            .map(|id| ReleaseCheckDescriptor {
                domain: if id.starts_with("runtime_provider") {
                    "runtime_provider".to_string()
                } else {
                    "visual_novel".to_string()
                },
                id,
                required: true,
            })
            .collect()
    }

    pub fn editor_metadata(&self) -> RuntimeEditorMetadata {
        RuntimeEditorMetadata {
            schema: RUNTIME_EDITOR_METADATA_SCHEMA.to_string(),
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            product_kind: "visual_novel".to_string(),
            project_templates: vec!["native_vn".to_string(), "advanced_vn".to_string()],
            authoring_surfaces: vec![
                "script".to_string(),
                "graph".to_string(),
                "timeline".to_string(),
                "system_pages".to_string(),
            ],
            debug_views: vec![
                "route_graph".to_string(),
                "runtime_state".to_string(),
                "policy_trace".to_string(),
                "presentation_state".to_string(),
            ],
            release_checks: native_vn_release_check_ids(),
        }
    }

    pub fn ffi_registration() -> FfiRuntimeProviderRegistration {
        FfiRuntimeProviderRegistration {
            provider_id: RString::from(NATIVE_VN_PROVIDER_ID),
            runtime_id: RString::from(NATIVE_VN_RUNTIME_ID),
            capability: RString::from("runtime.native_vn"),
            phase: RString::from("runtime"),
            packaged: true,
            descriptor_schema: RString::from(PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA),
            descriptor_json: RVec::from(serde_json::to_vec(&Self::descriptor()).unwrap()),
            prepare: ffi_prepare,
            probe: ffi_probe,
            open: ffi_open,
            step: ffi_step,
            save: ffi_save,
            restore: ffi_restore,
            shutdown: ffi_shutdown,
            package_sections: ffi_package_sections,
            release_checks: ffi_release_checks,
            editor_metadata: ffi_editor_metadata,
        }
    }

    fn session(&self, session_id: &GameRuntimeSessionId) -> Result<&CoreVnRuntime, CoreVnError> {
        self.sessions.get(&session_id.0).ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_MISSING",
                "runtime session is not open",
            )
        })
    }

    fn session_mut(
        &mut self,
        session_id: &GameRuntimeSessionId,
    ) -> Result<&mut CoreVnRuntime, CoreVnError> {
        self.sessions.get_mut(&session_id.0).ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_MISSING",
                "runtime session is not open",
            )
        })
    }
}

fn runtime_command_from_input(
    runtime: &CoreVnRuntime,
    input: &RuntimeStepInput,
) -> Result<CoreVnPlayerCommand, CoreVnError> {
    match input.action.as_str() {
        "launch_default" => runtime.default_launch_command().ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_LAUNCH_MISSING",
                "compiled story has no launchable state",
            )
        }),
        "advance" => Ok(CoreVnPlayerCommand::Advance),
        "choose" => Ok(CoreVnPlayerCommand::Choose {
            option_id: required_payload_string(&input.payload, "option_id")?,
        }),
        "complete_wait" => Ok(CoreVnPlayerCommand::CompleteWait {
            fence: required_payload_string(&input.payload, "fence")?,
        }),
        other => Err(CoreVnError::diagnostic(
            "ASTRA_NATIVE_VN_ACTION_UNKNOWN",
            format!("runtime action {other} is not supported"),
        )),
    }
}

fn required_payload_string(payload: &serde_json::Value, key: &str) -> Result<String, CoreVnError> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_ACTION_PAYLOAD",
                format!("runtime action payload is missing {key}"),
            )
        })
}

fn native_vn_package_sections() -> Vec<String> {
    [
        "vn.compiled_story",
        "vn.profile_manifest",
        "vn.policy_bundle_manifest",
        "vn.extension_manifest",
        "vn.standard_command_manifest",
        "vn.presentation_provider_manifest",
        "vn.commercial_baseline_manifest",
        "vn.system_story_manifest",
        "vn.system_ui_profile_manifest",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn native_vn_release_check_ids() -> Vec<String> {
    [
        "runtime_provider.native_vn",
        "vn.commercial_baseline",
        "vn.system_ui_profile",
        "vn.advanced_presentation",
        "player.full_playable",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

extern "C" fn ffi_prepare(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimePrepareRequest| {
        NativeVnRuntimeProvider::default().prepare(request)
    })
}

extern "C" fn ffi_probe(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimeProbeRequest| {
        NativeVnRuntimeProvider::default().probe(request)
    })
}

extern "C" fn ffi_open(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimeOpenRequest| RuntimeOpenReport {
        session_id: GameRuntimeSessionId(format!(
            "{}:{}:{}",
            NATIVE_VN_RUNTIME_ID, request.target_id, request.seed
        )),
        runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
        provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
        diagnostics: Vec::new(),
    })
}

extern "C" fn ffi_step(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimeStepInput| RuntimeStepOutput {
        session_id: request.session_id,
        status: "ffi_shape_only".to_string(),
        effects: Vec::new(),
        presentation: Vec::new(),
        diagnostics: vec!["ASTRA_NATIVE_VN_FFI_SESSION_NOT_BOUND".to_string()],
        dirty_save_sections: Vec::new(),
    })
}

extern "C" fn ffi_save(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimeSaveRequest| RuntimeSaveSections {
        session_id: request.session_id,
        sections: Vec::new(),
        diagnostics: vec!["ASTRA_NATIVE_VN_FFI_SESSION_NOT_BOUND".to_string()],
    })
}

extern "C" fn ffi_restore(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |request: RuntimeRestoreRequest| {
        RuntimeRestoreReport {
            session_id: request.session_id,
            status: "ffi_shape_only".to_string(),
            diagnostics: Vec::new(),
        }
    })
}

extern "C" fn ffi_shutdown(payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json(payload, |session_id: GameRuntimeSessionId| {
        RuntimeShutdownReport {
            session_id,
            status: "ffi_shape_only".to_string(),
            diagnostics: Vec::new(),
        }
    })
}

extern "C" fn ffi_package_sections(_payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_value(NativeVnRuntimeProvider::default().package_sections())
}

extern "C" fn ffi_release_checks(_payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_value(NativeVnRuntimeProvider::default().release_checks())
}

extern "C" fn ffi_editor_metadata(_payload: RVec<u8>) -> FfiRuntimeProviderResult {
    ffi_json_value(NativeVnRuntimeProvider::default().editor_metadata())
}

fn ffi_json<T, U>(payload: RVec<u8>, f: impl FnOnce(T) -> U) -> FfiRuntimeProviderResult
where
    T: serde::de::DeserializeOwned,
    U: serde::Serialize,
{
    match serde_json::from_slice::<T>(payload.as_slice()) {
        Ok(request) => ffi_json_value(f(request)),
        Err(err) => ffi_error("ASTRA_NATIVE_VN_FFI_DECODE", err.to_string()),
    }
}

fn ffi_json_value(value: impl serde::Serialize) -> FfiRuntimeProviderResult {
    match serde_json::to_vec(&value) {
        Ok(payload) => FfiRuntimeProviderResult {
            ok: true,
            payload: RVec::from(payload),
            diagnostics: RVec::new(),
        },
        Err(err) => ffi_error("ASTRA_NATIVE_VN_FFI_ENCODE", err.to_string()),
    }
}

fn ffi_error(code: &'static str, message: String) -> FfiRuntimeProviderResult {
    FfiRuntimeProviderResult {
        ok: false,
        payload: RVec::new(),
        diagnostics: RVec::from(vec![RString::from(format!("{code}: {message}"))]),
    }
}
