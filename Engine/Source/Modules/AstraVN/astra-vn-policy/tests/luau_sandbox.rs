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

#[test]
fn host_owned_outputs_are_bounded_before_copy_and_failure_does_not_commit() {
    use astra_vn_policy::{
        PolicyIoCapabilities, PolicyIoContext, PolicyIoState, PolicyQueryContext, VnPolicyState,
    };
    let budget = astra_vn_policy::PolicyExecutionBudget {
        memory_bytes: 512 * 1024,
        output_limit: 128,
        ..Default::default()
    };
    for operation in [
        "astra.snapshot.set(tostring(i), payload)",
        "astra.trace.event('item', payload)",
        "astra.command.emit('item', payload)",
    ] {
        let source = format!("astra.mutate.set_var('project','count',1); astra.host.read_text('note','asset:/text/note'); local payload = string.rep('x', 16384); for i=1,100 do {operation} end; return true");
        let mut state = VnPolicyState::default();
        let mut io = PolicyIoState::default();
        let capabilities =
            PolicyIoCapabilities::read_text(["asset:/text/note".parse().unwrap()], 4096).unwrap();
        let error = astra_vn_policy::LuauPolicy::new()
            .unwrap()
            .eval_bool_with_host(
                &source,
                &mut state,
                &PolicyQueryContext::default(),
                budget,
                PolicyIoContext {
                    state: &mut io,
                    capabilities: &capabilities,
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("HOST_MEMORY_BUDGET"), "{error}");
        assert_eq!(state, VnPolicyState::default());
        assert!(io.requests.is_empty());
    }
}

#[test]
fn snapshot_replacement_reuses_budget_and_caught_limit_is_still_fatal() {
    use astra_vn_policy::{LuauPolicy, PolicyExecutionBudget, PolicyQueryContext, VnPolicyState};
    let mut policy = LuauPolicy::new().unwrap();
    let mut state = VnPolicyState::default();
    let budget = PolicyExecutionBudget {
        memory_bytes: 512 * 1024,
        output_limit: 128,
        ..Default::default()
    };
    assert!(policy.eval_bool_with_context("local payload = string.rep('x', 16384); for i=1,100 do astra.snapshot.set('same',payload) end; return true", &mut state, &PolicyQueryContext::default(), budget).unwrap());
    assert_eq!(state.snapshots.len(), 1);
    let before = state.clone();
    let budget = PolicyExecutionBudget {
        output_limit: 2,
        ..budget
    };
    let error = policy.eval_bool_with_context("for i=1,8 do pcall(function() astra.snapshot.set('same','small') end) end; return true", &mut state, &PolicyQueryContext::default(), budget).unwrap_err();
    assert!(error.to_string().contains("BUDGET"));
    assert_eq!(state, before);
}
