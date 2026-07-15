use astra_vn_policy::{
    LuauPolicy, VnPolicyBundleManifest, VnPolicyBundleSourceCache, VnPolicyState,
};

#[astra_headless_test::test]
fn standard_policy_bundle_carries_source_cache_and_executes() {
    let manifest = VnPolicyBundleManifest::standard();
    let cache = VnPolicyBundleSourceCache::standard();
    let report = manifest.validate_standard_with_cache(&cache);
    assert!(report.passed, "{report:?}");

    let mut policy = LuauPolicy::new().unwrap();
    let mut state = VnPolicyState::default();
    assert!(policy
        .eval_bool(VnPolicyBundleSourceCache::standard_source(), &mut state)
        .unwrap());

    assert!(state.command_trace.iter().any(|entry| {
        entry.api == "astra.command.register" && entry.name == "astra.vn.message.show"
    }));
    assert!(state.command_trace.iter().any(|entry| {
        entry.api == "astra.command.register" && entry.name == "astra.vn.system.open"
    }));
    assert!(state.snapshot("astra.policy.standard").is_some());
    assert!(state
        .trace_events
        .iter()
        .any(|entry| entry.kind == "policy.loaded"));
}

#[astra_headless_test::test]
fn standard_policy_bundle_blocks_source_cache_hash_mismatch() {
    let manifest = VnPolicyBundleManifest::standard();
    let mut cache = VnPolicyBundleSourceCache::standard();
    cache.bundles[0].source.push_str("\nreturn false");

    let report = manifest.validate_standard_with_cache(&cache);
    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_POLICY_CACHE_HASH"));
}
