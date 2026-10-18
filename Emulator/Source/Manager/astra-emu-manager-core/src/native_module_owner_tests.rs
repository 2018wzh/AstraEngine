use crate::{
    FamilyProviderRegistry, NativeCancellation, NativeOpenRequest,
    model::{FramePoll, ProbeRequest, WindowState, allocate_session_epoch},
    no_host_services,
    test_native::{self, Fixture},
};

fn request() -> NativeOpenRequest {
    let epoch = allocate_session_epoch().unwrap();
    let cancellation = NativeCancellation::new(epoch);
    NativeOpenRequest {
        game_path: ".".into(),
        session_epoch: epoch,
        configuration: Vec::new(),
        initial_window: WindowState {
            width: 1,
            height: 1,
            scale_factor: 1.0,
            focused: true,
            visible: true,
            suspended: false,
        },
        host: no_host_services(&cancellation),
        cancellation,
        d3d12: None,
    }
}

#[test]
fn one_explicit_owner_spans_reopen_and_cpu_lease_outlives_its_destruction() {
    let mut fixture = Fixture::new("owner-lifetime", "fixture", "fixture");
    fixture.module_owner = true;
    let calls = fixture.calls.clone();
    let mut plugin = test_native::install(0, fixture);
    let mut retained = None;
    for _ in 0..2 {
        let mut session = plugin.open(request()).unwrap();
        assert_eq!(
            plugin.shutdown().unwrap_err().code(),
            "ASTRA_EMU_MODULE_OWNER_BUSY"
        );
        let FramePoll::Ready(frame) = session.acquire_frame(None).unwrap() else {
            panic!("fixture frame missing")
        };
        retained = Some(frame);
        session.close().unwrap();
        assert!(
            !calls
                .lock()
                .iter()
                .any(|call| call.name == "module_destroy")
        );
        // A closed Rust session object must no longer retain the module owner.
        drop(session);
    }
    plugin.shutdown().unwrap();
    plugin.shutdown().unwrap();
    assert_eq!(retained.unwrap().pixels().unwrap(), &[23, 45, 67, 255]);
    assert_eq!(
        plugin
            .probe(ProbeRequest {
                game_path: ".".into()
            })
            .unwrap_err()
            .code(),
        "ASTRA_EMU_MODULE_CLOSED"
    );
    let calls = calls.lock();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.name == "module_create")
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.name == "module_use")
            .count(),
        2
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.name == "module_destroy")
            .count(),
        1
    );
    assert_eq!(calls.last().unwrap().name, "module_destroy");
    assert!(
        calls
            .iter()
            .all(|call| call.thread == std::thread::current().id())
    );
}

#[test]
fn last_session_reference_defers_destroy_after_registry_handle_drop() {
    let mut fixture = Fixture::new("deferred-owner", "fixture", "fixture");
    fixture.module_owner = true;
    let calls = fixture.calls.clone();
    let mut plugin = test_native::install(0, fixture);
    let mut session = plugin.open(request()).unwrap();
    drop(plugin);
    assert!(
        !calls
            .lock()
            .iter()
            .any(|call| call.name == "module_destroy")
    );
    session.close().unwrap();
    let calls = calls.lock();
    let close = calls.iter().position(|call| call.name == "close").unwrap();
    let destroy = calls
        .iter()
        .position(|call| call.name == "module_destroy")
        .unwrap();
    assert!(close < destroy);
    assert_eq!(destroy + 1, calls.len());
}

#[test]
fn failed_create_destroys_nonnull_partial_owner_once() {
    let mut fixture = Fixture::new("failed-owner", "fixture", "fixture");
    fixture.module_owner = true;
    fixture.owner_create_error = true;
    let calls = fixture.calls.clone();
    let result = test_native::try_install(0, fixture);
    assert!(
        matches!(result, Err(crate::FamilyLoadError::ProviderError(error)) if error.code() == "ASTRA_TEST_MODULE_CREATE")
    );
    let calls = calls.lock();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.name == "module_destroy")
            .count(),
        1
    );
    assert!(!calls.iter().any(|call| call.name == "open"));
}

#[test]
fn destroy_error_consumes_owner_and_does_not_skip_other_modules() {
    let mut failing = Fixture::new("owner-a", "fixture", "fixture");
    failing.module_owner = true;
    failing.owner_destroy_error = true;
    let first = failing.calls.clone();
    let mut other = Fixture::new("owner-b", "fixture", "fixture");
    other.module_owner = true;
    let second = other.calls.clone();
    let mut registry = FamilyProviderRegistry::new();
    registry
        .register_provider(test_native::install(0, failing))
        .unwrap();
    registry
        .register_provider(test_native::install(1, other))
        .unwrap();
    assert_eq!(
        registry.shutdown().unwrap_err().diagnostic_code(),
        "ASTRA_TEST_MODULE_DESTROY"
    );
    registry.shutdown().unwrap();
    assert_eq!(
        first
            .lock()
            .iter()
            .filter(|call| call.name == "module_destroy")
            .count(),
        1
    );
    assert_eq!(
        second
            .lock()
            .iter()
            .filter(|call| call.name == "module_destroy")
            .count(),
        1
    );
    assert!(registry.descriptors().next().is_none());
}

#[test]
fn unsupported_extension_passes_null_without_creating_a_dummy_owner() {
    let fixture = Fixture::new("ownerless", "fixture", "fixture");
    let calls = fixture.calls.clone();
    let mut plugin = test_native::install(0, fixture);
    let mut session = plugin.open(request()).unwrap();
    session.close().unwrap();
    plugin.shutdown().unwrap();
    assert!(
        !calls
            .lock()
            .iter()
            .any(|call| call.name.starts_with("module_"))
    );
}

#[test]
fn close_error_still_releases_the_session_reference_before_owner_shutdown() {
    let mut fixture = Fixture::new("failed-close-owner", "fixture", "fixture");
    fixture.module_owner = true;
    fixture.close_error = true;
    let calls = fixture.calls.clone();
    let mut plugin = test_native::install(0, fixture);
    let mut session = plugin.open(request()).unwrap();
    assert_eq!(session.close().unwrap_err().code(), "ASTRA_TEST_CLOSE");
    plugin.shutdown().unwrap();
    assert_eq!(calls.lock().last().unwrap().name, "module_destroy");
    session.close().unwrap();
    assert_eq!(
        calls
            .lock()
            .iter()
            .filter(|call| call.name == "module_destroy")
            .count(),
        1
    );
}
