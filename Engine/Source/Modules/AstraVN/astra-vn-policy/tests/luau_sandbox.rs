use astra_vn_policy::{LuauPolicy, VnPolicyState};

#[test]
fn luau_policy_exposes_only_capability_api() {
    let mut policy = LuauPolicy::new().unwrap();
    let mut state = VnPolicyState::default();

    let result = policy
        .eval_bool(
            r#"
            astra.var.set("project", "affinity", 2)
            return astra.var.get("project", "affinity") == 2
                and io == nil
                and os == nil
                and require == nil
            "#,
            &mut state,
        )
        .unwrap();

    assert!(result);
    assert_eq!(state.var("project", "affinity"), Some(2));
}

#[test]
fn luau_policy_blocks_file_and_module_escape_attempts() {
    let mut policy = LuauPolicy::new().unwrap();
    let mut state = VnPolicyState::default();

    let err = policy
        .eval_bool(r#"return require("fs") ~= nil"#, &mut state)
        .unwrap_err();

    assert_eq!(err.code(), "ASTRA_VN_LUAU_SANDBOX");
}
