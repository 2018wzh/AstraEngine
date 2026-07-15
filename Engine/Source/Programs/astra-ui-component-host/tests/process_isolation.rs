use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use astra_core::Hash256;
use astra_ui_component_host::{dylib_filename, UiComponentProcess, UiComponentProcessConfig};
use astra_ui_core::{
    UiFrameRequest, UiInputFrame, UiInsets, UiThemeManifest, UiThemeValue, UiViewport,
};
use astra_ui_plugin_abi::{
    UiComponentManifest, UiComponentRequest, UiComponentResponse, UI_COMPONENT_MANIFEST_SCHEMA,
};
use ed25519_dalek::SigningKey;

#[astra_headless_test::test]
fn signed_component_process_supports_frame_snapshot_restore_and_shutdown() {
    let fixture = Fixture::prepare(Duration::from_secs(2));
    let mut process = fixture.spawn();
    assert_eq!(
        process
            .invoke(UiComponentRequest::Open {
                session_id: "fixture.ok".into(),
                component_id: "fixture.component".into(),
                initial_state: b"initial".to_vec(),
            })
            .expect("open"),
        UiComponentResponse::Opened
    );
    let frame = process
        .invoke(UiComponentRequest::Frame {
            request: frame_request(),
        })
        .expect("frame");
    let UiComponentResponse::Frame {
        render, semantics, ..
    } = frame
    else {
        panic!("frame response expected");
    };
    assert_eq!(render.session_id, "fixture.ok");
    assert_eq!(semantics.root_id, "fixture.root");
    assert_eq!(
        process
            .invoke(UiComponentRequest::Snapshot)
            .expect("snapshot"),
        UiComponentResponse::Snapshot {
            state: b"initial".to_vec()
        }
    );
    assert_eq!(
        process
            .invoke(UiComponentRequest::Restore {
                state: b"restored".to_vec(),
            })
            .expect("restore"),
        UiComponentResponse::Restored
    );
    assert_eq!(
        process
            .invoke(UiComponentRequest::Snapshot)
            .expect("restored snapshot"),
        UiComponentResponse::Snapshot {
            state: b"restored".to_vec()
        }
    );
    assert_eq!(
        process
            .invoke(UiComponentRequest::Shutdown)
            .expect("shutdown"),
        UiComponentResponse::Shutdown
    );
    assert!(process.invoke(UiComponentRequest::Snapshot).is_err());
}

#[astra_headless_test::test]
fn hung_component_terminates_the_entire_component_session() {
    let fixture = Fixture::prepare(Duration::from_millis(50));
    let mut process = fixture.spawn();
    let error = process
        .invoke(UiComponentRequest::Open {
            session_id: "fixture.hang".into(),
            component_id: "fixture.component".into(),
            initial_state: Vec::new(),
        })
        .expect_err("hang must terminate");
    assert!(error.to_string().contains("TIMEOUT") || error.to_string().contains("I/O"));
    assert!(process.invoke(UiComponentRequest::Snapshot).is_err());
}

#[astra_headless_test::test]
fn panicking_component_process_cannot_continue_the_ui_session() {
    let fixture = Fixture::prepare(Duration::from_secs(2));
    let mut process = fixture.spawn();
    let error = process
        .invoke(UiComponentRequest::Open {
            session_id: "fixture.panic".into(),
            component_id: "fixture.component".into(),
            initial_state: Vec::new(),
        })
        .expect_err("aborted component must fail");
    assert!(
        error.to_string().contains("I/O")
            || error.to_string().contains("EXIT")
            || error.to_string().contains("TIMEOUT")
    );
    assert!(process.invoke(UiComponentRequest::Snapshot).is_err());
}

struct Fixture {
    _directory: tempfile::TempDir,
    config: UiComponentProcessConfig,
}

impl Fixture {
    fn prepare(deadline: Duration) -> Self {
        let workspace = workspace_root();
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.join("target"));
        let host = target.join("debug").join(if cfg!(windows) {
            "astra-ui-component-host.exe"
        } else {
            "astra-ui-component-host"
        });
        let artifact = target
            .join("debug")
            .join(dylib_filename("ui_component_provider"));
        assert!(
            host.is_file(),
            "component host binary must be built in the checkout-bound target"
        );
        assert!(
            artifact.is_file(),
            "component fixture dylib must be built in the checkout-bound target"
        );
        let artifact_bytes = std::fs::read(&artifact).expect("fixture artifact");
        let signing = SigningKey::from_bytes(&[19; 32]);
        let mut manifest = UiComponentManifest {
            schema: UI_COMPONENT_MANIFEST_SCHEMA.into(),
            component_id: "fixture.component".into(),
            component_version: "1.0.0".into(),
            signer_id: "fixture.test_signer".into(),
            engine_version: "0.1.0".into(),
            rustc_fingerprint: "rustc.test".into(),
            feature_fingerprint: "features.test".into(),
            abi_fingerprint: "astra.ui_component_abi.v1".into(),
            artifact_hash: Hash256::from_sha256(&artifact_bytes),
            input_schema: "astra.ui_component_request.v1".into(),
            output_schema: "astra.ui_component_response.v1".into(),
            capabilities: BTreeSet::from(["ui.render_frame".into()]),
            signature: Vec::new(),
        };
        manifest.sign(&signing).expect("test manifest signature");
        let directory = tempfile::tempdir().expect("fixture directory");
        let manifest_path = directory.path().join("manifest.postcard");
        let allowlist_path = directory.path().join("allowlist.postcard");
        std::fs::write(
            &manifest_path,
            postcard::to_allocvec(&manifest).expect("manifest postcard"),
        )
        .expect("write manifest");
        std::fs::write(
            &allowlist_path,
            postcard::to_allocvec(&BTreeMap::from([(
                "fixture.test_signer".to_string(),
                signing.verifying_key().to_bytes(),
            )]))
            .expect("allowlist postcard"),
        )
        .expect("write allowlist");
        Self {
            config: UiComponentProcessConfig {
                host_binary: host,
                manifest: manifest_path,
                artifact,
                allowlist: allowlist_path,
                deadline,
            },
            _directory: directory,
        }
    }

    fn spawn(&self) -> UiComponentProcess {
        UiComponentProcess::spawn(self.config.clone()).expect("spawn component process")
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("workspace root")
        .to_path_buf()
}

fn frame_request() -> UiFrameRequest {
    let mut theme = UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "fixture.theme".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        content_hash: Hash256::from_sha256(&[]),
    };
    theme.content_hash = theme.compute_hash().expect("theme hash");
    UiFrameRequest {
        schema: "astra.ui_frame_request.v1".into(),
        session_id: "fixture.ok".into(),
        generation: 1,
        viewport: UiViewport {
            physical_width: 1280,
            physical_height: 720,
            scale_factor: 1.0,
            font_scale: 1.0,
            safe_area_points: UiInsets {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            },
        },
        fixed_time_ns: 0,
        input: UiInputFrame {
            schema: "astra.ui_input_frame.v1".into(),
            events: Vec::new(),
        },
        theme,
        model_schema: "fixture.model.v1".into(),
        model_payload: Vec::new(),
    }
}
