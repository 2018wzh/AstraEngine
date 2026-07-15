use std::collections::BTreeMap;

use astra_policy::{
    create_sandboxed_lua, PolicyBundleEntry, PolicyBundleManifest, PolicyBundleSourceCache,
    PolicyBundleSourceEntry, PolicyCommandRecord, PolicyExecutionBudget, PolicyValue, PolicyVm,
};

#[astra_headless_test::test]
fn policy_value_roundtrips_and_rejects_excessive_depth() {
    let value = PolicyValue::Object(BTreeMap::from([(
        "nested".to_string(),
        PolicyValue::Object(BTreeMap::from([(
            "answer".to_string(),
            PolicyValue::Integer(42),
        )])),
    )]));
    let bytes = postcard::to_allocvec(&value).unwrap();
    assert_eq!(postcard::from_bytes::<PolicyValue>(&bytes).unwrap(), value);
    assert!(value.validate_depth(1).is_err());
    assert!(value.validate_depth(2).is_ok());
}

#[astra_headless_test::test]
fn policy_snapshot_restores_state_and_serialized_command_records() {
    let mut vm = PolicyVm::new(PolicyExecutionBudget::default()).unwrap();
    vm.set_state("theme", PolicyValue::String("classic".into()))
        .unwrap();
    vm.record_command(PolicyCommandRecord {
        api: "astra.test".into(),
        name: "set_theme".into(),
        payload: PolicyValue::String("classic".into()),
        replay_event: "policy.command.1".into(),
    })
    .unwrap();
    let bytes = postcard::to_allocvec(&vm.snapshot()).unwrap();

    let mut restored = PolicyVm::new(PolicyExecutionBudget::default()).unwrap();
    restored
        .restore(postcard::from_bytes(&bytes).unwrap())
        .unwrap();
    assert_eq!(
        restored.state("theme"),
        Some(&PolicyValue::String("classic".into()))
    );
    assert_eq!(restored.snapshot().commands.len(), 1);
}

#[astra_headless_test::test]
fn policy_vm_owns_the_shared_sandbox_and_budget() {
    let vm = PolicyVm::new(PolicyExecutionBudget::default()).unwrap();
    assert!(vm.eval_bool("return io == nil and os == nil").unwrap());
    assert_eq!(vm.budget(), PolicyExecutionBudget::default());
}

#[astra_headless_test::test]
fn policy_budget_rejects_zero_or_unbounded_configuration() {
    assert!(PolicyExecutionBudget::default().validate().is_ok());
    assert!(PolicyExecutionBudget {
        interrupt_limit: 0,
        ..PolicyExecutionBudget::default()
    }
    .validate()
    .is_err());
    assert!(PolicyExecutionBudget {
        output_limit: 0,
        ..PolicyExecutionBudget::default()
    }
    .validate()
    .is_err());
}

#[astra_headless_test::test]
fn sandbox_removes_host_escape_globals() {
    let lua = create_sandboxed_lua(PolicyExecutionBudget::default()).unwrap();
    let denied: bool = lua
        .load(
            "return io == nil and os == nil and debug == nil and package == nil and require == nil",
        )
        .eval()
        .unwrap();
    assert!(denied);
}

#[astra_headless_test::test]
fn bundle_cache_is_hash_bound_and_path_safe() {
    let source = "return true";
    let hash = astra_core::Hash256::from_sha256(source.as_bytes()).to_string();
    let manifest = PolicyBundleManifest {
        schema: "astra.policy_bundle.v1".to_string(),
        bundles: vec![PolicyBundleEntry {
            id: "astra.policy.fixture".to_string(),
            entry: "Policies/fixture.luau".to_string(),
            capabilities: vec!["astra.fixture.read".to_string()],
            dependencies: Vec::new(),
            lock_hash: hash.clone(),
            source_hash: hash.clone(),
            byte_size: source.len() as u64,
            source_cache_section: "policy.bundle_source_cache".to_string(),
        }],
    };
    let cache = PolicyBundleSourceCache {
        schema: "astra.policy_bundle_source_cache.v1".to_string(),
        bundles: vec![PolicyBundleSourceEntry {
            id: "astra.policy.fixture".to_string(),
            entry: "Policies/fixture.luau".to_string(),
            source_hash: hash,
            byte_size: source.len() as u64,
            source: source.to_string(),
        }],
    };
    assert!(cache.validate(&manifest).passed);

    let mut leaking = cache;
    leaking.bundles[0].entry = "../fixture.luau".to_string();
    let report = leaking.validate(&manifest);
    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_POLICY_ENTRY_PATH"));
}
