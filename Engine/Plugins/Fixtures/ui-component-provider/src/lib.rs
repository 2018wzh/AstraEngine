//! Test-only signed UI component. It is never eligible for product pages.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use abi_stable::{prefix_type::PrefixTypeTrait, std_types::RVec};
use astra_core::Hash256;
use astra_ui_core::{
    UiPoint, UiRect, UiRenderFrame, UiSemanticNode, UiSemanticRole, UiSemanticSnapshot,
    UiTextureDelta,
};
use astra_ui_plugin_abi::{
    FfiUiComponentResult, UiComponentManifest, UiComponentModule, UiComponentModuleRef,
    UiComponentRequest, UiComponentResponse, UI_COMPONENT_MANIFEST_SCHEMA,
};

#[derive(Default)]
struct FixtureState {
    session_id: Option<String>,
    snapshot: Vec<u8>,
}

fn state() -> &'static Mutex<FixtureState> {
    static STATE: OnceLock<Mutex<FixtureState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(FixtureState::default()))
}

fn embedded_manifest() -> UiComponentManifest {
    UiComponentManifest {
        schema: UI_COMPONENT_MANIFEST_SCHEMA.into(),
        component_id: "fixture.component".into(),
        component_version: "1.0.0".into(),
        signer_id: "fixture.test_signer".into(),
        engine_version: "0.1.0".into(),
        rustc_fingerprint: "rustc.test".into(),
        feature_fingerprint: "features.test".into(),
        abi_fingerprint: "astra.ui_component_abi.v1".into(),
        artifact_hash: Hash256::from_bytes([0; 32]),
        input_schema: "astra.ui_component_request.v1".into(),
        output_schema: "astra.ui_component_response.v1".into(),
        capabilities: BTreeSet::from(["ui.render_frame".into()]),
        signature: Vec::new(),
    }
}

extern "C" fn manifest_postcard() -> RVec<u8> {
    postcard::to_allocvec(&embedded_manifest())
        .expect("fixture manifest encodes")
        .into()
}

extern "C" fn create(payload: RVec<u8>) -> FfiUiComponentResult {
    invoke(payload, |request| match request {
        UiComponentRequest::Open {
            session_id,
            component_id,
            initial_state,
        } if component_id == "fixture.component" => {
            if session_id == "fixture.hang" {
                std::thread::sleep(Duration::from_secs(5));
            }
            if session_id == "fixture.panic" {
                std::process::abort();
            }
            let mut state = state().lock().expect("fixture state lock");
            state.session_id = Some(session_id);
            state.snapshot = initial_state;
            UiComponentResponse::Opened
        }
        _ => failed("ASTRA_UI_COMPONENT_FIXTURE_OPEN"),
    })
}

extern "C" fn frame(payload: RVec<u8>) -> FfiUiComponentResult {
    invoke(payload, |request| match request {
        UiComponentRequest::Frame { request } => {
            let root = UiSemanticNode {
                id: "fixture.root".into(),
                parent_id: None,
                role: UiSemanticRole::Group,
                bounds_points: UiRect {
                    min: UiPoint { x: 0.0, y: 0.0 },
                    max: UiPoint { x: 1.0, y: 1.0 },
                },
                name: Some("Test fixture".into()),
                description: None,
                value: None,
                enabled: true,
                hidden: false,
                focused: false,
                selected: false,
                checked: None,
                actions: BTreeSet::new(),
                properties: BTreeMap::new(),
            };
            let mut semantics = UiSemanticSnapshot {
                schema: "astra.ui_semantic_snapshot.v1".into(),
                session_id: request.session_id.clone(),
                generation: request.generation,
                root_id: root.id.clone(),
                nodes: vec![root],
                hash: Hash256::from_sha256(&[]),
            };
            semantics.hash = semantics.compute_hash().expect("fixture semantic hash");
            UiComponentResponse::Frame {
                render: Box::new(UiRenderFrame {
                    schema: "astra.ui_render_frame.v1".into(),
                    session_id: request.session_id,
                    generation: request.generation,
                    viewport: request.viewport,
                    textures: UiTextureDelta {
                        uploads: Vec::new(),
                        releases: Vec::new(),
                        full_resync: false,
                    },
                    primitives: Vec::new(),
                }),
                semantics: Box::new(semantics),
                actions: Vec::new(),
            }
        }
        _ => failed("ASTRA_UI_COMPONENT_FIXTURE_FRAME"),
    })
}

extern "C" fn snapshot(payload: RVec<u8>) -> FfiUiComponentResult {
    invoke(payload, |request| match request {
        UiComponentRequest::Snapshot => UiComponentResponse::Snapshot {
            state: state().lock().expect("fixture state lock").snapshot.clone(),
        },
        _ => failed("ASTRA_UI_COMPONENT_FIXTURE_SNAPSHOT"),
    })
}

extern "C" fn restore(payload: RVec<u8>) -> FfiUiComponentResult {
    invoke(payload, |request| match request {
        UiComponentRequest::Restore { state: next } => {
            state().lock().expect("fixture state lock").snapshot = next;
            UiComponentResponse::Restored
        }
        _ => failed("ASTRA_UI_COMPONENT_FIXTURE_RESTORE"),
    })
}

extern "C" fn shutdown(payload: RVec<u8>) -> FfiUiComponentResult {
    invoke(payload, |request| match request {
        UiComponentRequest::Shutdown => {
            *state().lock().expect("fixture state lock") = FixtureState::default();
            UiComponentResponse::Shutdown
        }
        _ => failed("ASTRA_UI_COMPONENT_FIXTURE_SHUTDOWN"),
    })
}

fn invoke(
    payload: RVec<u8>,
    handler: impl FnOnce(UiComponentRequest) -> UiComponentResponse,
) -> FfiUiComponentResult {
    match postcard::from_bytes::<UiComponentRequest>(&payload) {
        Ok(request) => match postcard::to_allocvec(&handler(request)) {
            Ok(payload) => FfiUiComponentResult {
                ok: true,
                payload: payload.into(),
                diagnostic: "".into(),
            },
            Err(error) => ffi_failure(error.to_string()),
        },
        Err(error) => ffi_failure(error.to_string()),
    }
}

fn failed(code: &str) -> UiComponentResponse {
    UiComponentResponse::Failed {
        code: code.into(),
        message: "test fixture rejected request".into(),
    }
}

fn ffi_failure(message: String) -> FfiUiComponentResult {
    FfiUiComponentResult {
        ok: false,
        payload: RVec::new(),
        diagnostic: message.into(),
    }
}

#[abi_stable::export_root_module]
pub fn astra_ui_component_root_module() -> UiComponentModuleRef {
    UiComponentModule {
        manifest_postcard,
        create,
        frame,
        snapshot,
        restore,
        shutdown,
    }
    .leak_into_prefix()
}
