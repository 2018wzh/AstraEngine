use abi_stable::{
    prefix_type::PrefixTypeTrait,
    std_types::{RString, RVec},
};
use astra_plugin_abi::{
    AstraPluginModule, AstraPluginModuleRef, FfiActionRegistration, FfiPluginRegistration,
    FfiPluginShutdown, FfiProviderRegistration,
};
use astra_runtime::{
    ActionCallRequest, ActionCallResult, ActionEffect, ActionTrace, BlackboardValue, EventPayload,
    EventSource, PresentationCommand,
};
use std::collections::BTreeMap;
use tracing::{debug, warn};

extern "C" fn descriptor_yaml() -> RString {
    r#"
id: astra.fixture.headless_presentation
version: 0.1.0
engine_version: 0.1.0
rustc_fingerprint: rustc-stable
feature_fingerprint: stage1-core
abi_style: abi_stable_rust
capabilities:
  - presentation.headless
  - action.fixture
permissions:
  - runtime.presentation
  - runtime.action
packaged: true
"#
    .into()
}

extern "C" fn run_fixture_action(request: RVec<u8>) -> RVec<u8> {
    let request: Vec<u8> = request.into_iter().collect();
    let result = match postcard::from_bytes::<ActionCallRequest>(&request) {
        Ok(request) => {
            debug!(
                step = request.step,
                action_id = %request.action_id,
                "fixture.action.run"
            );
            let mut payload = BTreeMap::new();
            payload.insert("fixture.action".to_string(), BlackboardValue::from("ran"));
            ActionCallResult::Ok {
                trace: ActionTrace {
                    action_id: request.action_id,
                    payload: payload.clone(),
                },
                effects: vec![
                    ActionEffect::SetBlackboard {
                        key: "fixture.action".to_string(),
                        value: BlackboardValue::from("ran"),
                    },
                    ActionEffect::EmitEvent {
                        source: EventSource::StateMachine,
                        payload: EventPayload::new("fixture.action.done"),
                    },
                    ActionEffect::Presentation {
                        command: PresentationCommand::Marker {
                            name: "ffi_action".to_string(),
                        },
                    },
                ],
            }
        }
        Err(err) => {
            warn!(
                diagnostic_code = "ASTRA_FIXTURE_ACTION_DECODE",
                "fixture.action.decode_failed"
            );
            ActionCallResult::Err {
                code: "ASTRA_FIXTURE_ACTION_DECODE".to_string(),
                message: err.to_string(),
            }
        }
    };
    let encoded = postcard::to_allocvec(&result).expect("fixture action result must encode");
    RVec::from(encoded)
}

extern "C" fn register() -> FfiPluginRegistration {
    debug!("fixture.register");
    FfiPluginRegistration {
        providers: RVec::from(vec![FfiProviderRegistration {
            slot: "presentation".into(),
            provider_id: "astra.fixture.headless_presentation".into(),
            capability: "presentation.headless".into(),
            phase: "runtime".into(),
            packaged: true,
        }]),
        actions: RVec::from(vec![FfiActionRegistration {
            provider_id: "astra.fixture.action_provider".into(),
            action_id: "astra.fixture.action.set_flag".into(),
            input_schema: "astra.fixture.action.set_flag.request.v1".into(),
            output_schema: "astra.action_trace.v1".into(),
            invoke: run_fixture_action,
        }]),
        callbacks: 0,
    }
}

extern "C" fn shutdown() -> FfiPluginShutdown {
    debug!("fixture.shutdown");
    FfiPluginShutdown {
        callbacks_released: true,
    }
}

#[abi_stable::export_root_module]
pub fn astra_plugin_root_module() -> AstraPluginModuleRef {
    AstraPluginModule {
        descriptor_yaml,
        register,
        shutdown,
    }
    .leak_into_prefix()
}
