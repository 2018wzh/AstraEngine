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
    let binding = binding(SOURCE);
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
    }
    assert!(LuauPolicy::action_descriptors(
        &format!("{SOURCE}\n{SOURCE}"),
        PolicyExecutionBudget::default()
    )
    .is_err());
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
