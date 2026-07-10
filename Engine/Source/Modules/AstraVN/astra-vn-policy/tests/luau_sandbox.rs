use astra_vn_policy::{LuauPolicy, PolicyExecutionBudget, PolicyQueryContext, VnPolicyState};

#[test]
fn luau_policy_blocks_removed_authority_bypass_api() {
    let mut policy = LuauPolicy::new().unwrap();
    let mut state = VnPolicyState::default();

    let error = policy
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
        .unwrap_err();

    assert_eq!(error.code(), "ASTRA_VN_LUAU_AUTHORITY_API");
    assert_eq!(state.var("project", "affinity"), None);
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

#[test]
fn luau_policy_blocks_execution_past_the_deterministic_budget() {
    let mut policy = LuauPolicy::new().unwrap();
    let mut state = VnPolicyState::default();
    let error = policy
        .eval_bool_with_context(
            "while true do end return true",
            &mut state,
            &PolicyQueryContext::default(),
            PolicyExecutionBudget {
                interrupt_limit: 1,
                ..PolicyExecutionBudget::default()
            },
        )
        .unwrap_err();

    assert_eq!(error.code(), "ASTRA_VN_LUAU_INSTRUCTION_BUDGET");
}
