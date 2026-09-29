use astra_asset::VfsUri;
use astra_core::Hash256;
use astra_vn_policy::*;
use std::collections::BTreeMap;

const SOURCE: &str = r#"
astra.command.register("load_note", {
    schema = "demo.load_note.v1", fields = {resource = {kind = "asset_uri", required = true}}
}, function(args)
    astra.mutate.set_var("project", "count", (astra.var.get("project", "count") or 0) + 1)
    local text, status = astra.host.read_text("note", args.resource)
    if status == "pending" then
        astra.snapshot.set("phase", "reading")
        return astra.action.await("note")
    end
    if status == "ready" then assert(text == "hello") end
    return astra.action.complete()
end)
"#;

fn binding(source: &str) -> PolicyActionBinding {
    PolicyActionBinding {
        descriptor: LuauPolicy::action_descriptors(source, PolicyExecutionBudget::default())
            .unwrap()
            .remove(0),
        entry: "Policies/note.luau".into(),
        source: source.into(),
        source_hash: Hash256::from_sha256(source.as_bytes()).to_string(),
        read_text: vec![VfsUri::parse("asset:/text/note").unwrap()],
    }
}
fn continuation(binding: &PolicyActionBinding, instance: u64) -> PolicyActionContinuation {
    PolicyActionContinuation {
        source_id: "note.load".into(),
        instance,
        command: ExtensionPresentationCommand {
            command: binding.descriptor.command.clone(),
            provider_id: POLICY_ACTION_PROVIDER.into(),
            schema: binding.descriptor.schema.clone(),
            fields: BTreeMap::from([(
                "resource".into(),
                ExtensionValue::AssetUri("asset:/text/note".into()),
            )]),
        },
        waiting_key: None,
        state: BTreeMap::new(),
    }
}
fn invoke(
    binding: &PolicyActionBinding,
    continuation: &mut PolicyActionContinuation,
    state: &mut VnPolicyState,
    io: &mut PolicyIoState,
) -> Result<PolicyActionOutcome, VnError> {
    LuauPolicy::new().unwrap().invoke_action(
        binding,
        continuation,
        state,
        &PolicyQueryContext::default(),
        PolicyExecutionBudget::default(),
        PolicyIoContext {
            state: io,
            capabilities: &PolicyIoCapabilities::read_text(
                [VfsUri::parse("asset:/text/note").unwrap()],
                POLICY_IO_MAX_BYTES,
            )
            .unwrap(),
        },
    )
}

#[test]
fn action_await_is_transactional_and_repeated_source_has_distinct_io() {
    check_action_lifecycle(SOURCE);
}

#[test]
fn captured_api_survives_await_completion_restore_and_new_instances() {
    check_action_lifecycle(&format!(
        "local api = astra\n{}",
        SOURCE.replace("astra.", "api.")
    ));
}

fn check_action_lifecycle(source: &str) {
    let binding = binding(source);
    let mut first = continuation(&binding, 1);
    let mut state = VnPolicyState::default();
    let mut io = PolicyIoState::default();
    assert_eq!(
        invoke(&binding, &mut first, &mut state, &mut io).unwrap(),
        PolicyActionOutcome::Await {
            request_key: "note".into()
        }
    );
    assert_eq!(state.var("project", "count"), None);
    assert_eq!(
        first.state.get("phase"),
        Some(&PolicySnapshotValue::String("reading".into()))
    );
    io.requests.get_mut("a1.note").unwrap().status = PolicyIoStatus::Ready("hello".into());
    assert_eq!(
        invoke(&binding, &mut first, &mut state, &mut io).unwrap(),
        PolicyActionOutcome::Complete
    );
    assert_eq!(state.var("project", "count"), Some(1));
    let mut second = continuation(&binding, 2);
    assert!(matches!(
        invoke(&binding, &mut second, &mut state, &mut io).unwrap(),
        PolicyActionOutcome::Await { .. }
    ));
    assert_eq!(io.requests["a2.note"].status, PolicyIoStatus::Pending);
    assert_ne!(
        io.requests["a1.note"].request.id,
        io.requests["a2.note"].request.id
    );
    assert_eq!(state.var("project", "count"), Some(1));
    io.interrupt_pending();
    let mut second: PolicyActionContinuation =
        postcard::from_bytes(&postcard::to_allocvec(&second).unwrap()).unwrap();
    let mut io: PolicyIoState = postcard::from_bytes(&postcard::to_allocvec(&io).unwrap()).unwrap();
    assert_eq!(
        invoke(&binding, &mut second, &mut state, &mut io).unwrap(),
        PolicyActionOutcome::Complete
    );
    assert_eq!(state.var("project", "count"), Some(2));
}

#[test]
fn action_rejects_invalid_wait_boolean_and_ambient_grants_without_committing() {
    for source in [
        SOURCE.replace(
            "astra.action.await(\"note\")",
            "astra.action.await(\"missing\")",
        ),
        SOURCE.replace("astra.action.await(\"note\")", "true"),
        SOURCE.replace("astra.action.await(\"note\")", "astra.action.complete()"),
    ] {
        let binding = binding(&source);
        let mut continuation = continuation(&binding, 1);
        let mut state = VnPolicyState::default();
        let mut io = PolicyIoState::default();
        assert!(invoke(&binding, &mut continuation, &mut state, &mut io).is_err());
        assert!(io.requests.is_empty());
        assert_eq!(state, VnPolicyState::default());
        assert!(continuation.state.is_empty());
    }
    let mut binding = binding(SOURCE);
    binding.read_text.clear();
    assert!(invoke(
        &binding,
        &mut continuation(&binding, 1),
        &mut VnPolicyState::default(),
        &mut PolicyIoState::default()
    )
    .is_err());
}

#[test]
fn registration_has_no_gameplay_or_io_capabilities() {
    for prefix in [
        "astra.host.read_text('key','asset:/text/note')",
        "astra.mutate.set_var('project','count',1)",
    ] {
        assert!(LuauPolicy::action_descriptors(
            &format!("{prefix}\n{SOURCE}"),
            PolicyExecutionBudget::default()
        )
        .is_err());
        let mut binding = binding(SOURCE);
        binding.source = format!("{prefix}\n{SOURCE}");
        binding.source_hash = Hash256::from_sha256(binding.source.as_bytes()).to_string();
        assert!(invoke(
            &binding,
            &mut continuation(&binding, 1),
            &mut VnPolicyState::default(),
            &mut PolicyIoState::default()
        )
        .is_err());
    }
    assert!(LuauPolicy::action_descriptors(
        &format!("{SOURCE}\n{SOURCE}"),
        PolicyExecutionBudget::default()
    )
    .is_err());
}

#[test]
fn captured_registration_table_and_function_are_closed_inside_handler() {
    for (capture, call) in [
        ("local command = astra.command", "command.register"),
        ("local register = astra.command.register", "register"),
    ] {
        let source = format!(
            "{capture}\n{}",
            SOURCE.replace(
                "local text, status",
                &format!(
                    "{call}('late', {{schema='demo.late.v1',fields={{}}}}, function() end)\n    local text, status"
                )
            )
        );
        let binding = binding(&source);
        let mut continuation = continuation(&binding, 1);
        let mut state = VnPolicyState::default();
        let mut io = PolicyIoState::default();
        let error = invoke(&binding, &mut continuation, &mut state, &mut io).unwrap_err();
        assert!(error.to_string().contains("REGISTRATION_CLOSED"));
        assert_eq!(state, VnPolicyState::default());
        assert!(io.requests.is_empty());
        assert!(continuation.state.is_empty());
    }
}

#[test]
fn integer_arguments_do_not_truncate_to_luau_machine_integer() {
    let binding = binding(
        r#"
astra.command.register("integer", {schema="demo.integer.v1", fields={value={kind="integer",required=true}}}, function(args)
    assert(args.value == 1099511627776)
    return astra.action.complete()
end)
"#,
    );
    let mut continuation = continuation(&binding, 1);
    continuation.command.fields =
        BTreeMap::from([("value".into(), ExtensionValue::Integer(1_i64 << 40))]);
    assert_eq!(
        invoke(
            &binding,
            &mut continuation,
            &mut VnPolicyState::default(),
            &mut PolicyIoState::default()
        )
        .unwrap(),
        PolicyActionOutcome::Complete
    );
    continuation
        .command
        .fields
        .insert("value".into(), ExtensionValue::Integer(i64::MAX));
    assert!(invoke(
        &binding,
        &mut continuation,
        &mut VnPolicyState::default(),
        &mut PolicyIoState::default()
    )
    .is_err());
}

#[test]
fn action_integer_snapshot_roundtrips_after_ready_and_interrupted() {
    let source = r#"
astra.command.register("integer", {schema="demo.integer.v1", fields={value={kind="integer",required=true}}}, function(args)
    local _, status = astra.host.read_text("note", "asset:/text/note")
    if status == "pending" then
        astra.snapshot.set("value", args.value)
        return astra.action.await("note")
    end
    assert(astra.snapshot.get("value") == args.value)
    return astra.action.complete()
end)
"#;
    let binding = binding(source);
    for value in [1_i64 << 40, (1_i64 << 53) - 1, -((1_i64 << 53) - 1)] {
        for result in [
            PolicyIoStatus::Ready("hello".into()),
            PolicyIoStatus::Failed(PolicyIoFailure::Interrupted),
        ] {
            let mut continuation = continuation(&binding, 1);
            continuation.command.fields =
                BTreeMap::from([("value".into(), ExtensionValue::Integer(value))]);
            let mut state = VnPolicyState::default();
            let mut io = PolicyIoState::default();
            assert!(matches!(
                invoke(&binding, &mut continuation, &mut state, &mut io).unwrap(),
                PolicyActionOutcome::Await { .. }
            ));
            let bytes = postcard::to_allocvec(&continuation).unwrap();
            let mut continuation = postcard::from_bytes(&bytes).unwrap();
            io.requests.get_mut("a1.note").unwrap().status = result;
            assert_eq!(
                invoke(&binding, &mut continuation, &mut state, &mut io).unwrap(),
                PolicyActionOutcome::Complete
            );
        }
    }
    for value in [1_i64 << 53, -(1_i64 << 53), i64::MIN, i64::MAX] {
        let mut continuation = continuation(&binding, 1);
        continuation.command.fields =
            BTreeMap::from([("value".into(), ExtensionValue::Integer(value))]);
        assert!(invoke(
            &binding,
            &mut continuation,
            &mut VnPolicyState::default(),
            &mut PolicyIoState::default()
        )
        .is_err());
    }
}

#[test]
fn snapshot_rejects_ambiguous_numbers_and_out_of_range_saved_integers() {
    let mut policy = LuauPolicy::new().unwrap();
    for number in [
        "9007199254740992",
        "-9007199254740992",
        "9007199254740993",
        "9223372036854775807",
    ] {
        let mut state = VnPolicyState::default();
        assert!(policy
            .eval_bool(
                &format!("astra.snapshot.set('value', {number}); return true"),
                &mut state
            )
            .is_err());
        assert!(state.snapshots.is_empty());
    }
    for integer in [1_i64 << 53, -(1_i64 << 53), i64::MIN, i64::MAX] {
        let mut state = VnPolicyState::default();
        state
            .snapshots
            .insert("value".into(), PolicySnapshotValue::Integer(integer));
        assert!(policy
            .eval_bool(
                "local value = astra.snapshot.get('value'); return true",
                &mut state
            )
            .is_err());
    }
}
