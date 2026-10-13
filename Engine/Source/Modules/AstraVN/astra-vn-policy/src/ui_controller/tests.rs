use super::*;

#[test]
fn controller_runs_in_fresh_sandbox_and_returns_typed_effects() {
    let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
    host.register_source(
        r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "none",
}, {
    on_action = function(ctx, model, action)
        return { astra.ui.effect.close_modal() }
    end,
})
"#,
    )
    .expect("register");
    let mut session = VnUiSessionState::default();
    let effects = host
        .invoke_action(
            "controller.test",
            "astra.vn.ui_model.test.v1",
            &UiValue::Map(BTreeMap::new()),
            &VnUiAction::Advance,
            &mut session,
        )
        .expect("invoke");
    assert_eq!(effects, vec![VnUiControllerEffect::CloseModal]);
    assert!(session.values().is_empty());
}

#[test]
fn tsuinosora_save_focus_skips_protected_slots() {
    let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
    host.register_source(include_str!(
        "../../../../../../../Examples/TsuiNoSora/ProjectTemplate/Controllers/tsui_ui.luau"
    ))
    .expect("register controllers");
    for (controller, prefix, suffix, back) in [
        (
            "tsui.system.save.classic",
            "root/window/body/slots/slot/",
            "",
            "root/window/body/back",
        ),
        (
            "tsui.system.save.modern",
            "root/gold/content/slots/slot/",
            "/write/slot.02",
            "root/gold/content/back",
        ),
    ] {
        for writable in [true, false] {
            let slots = [("slot.01", false), ("slot.02", writable)]
                .into_iter()
                .map(|(id, can_write)| {
                    UiValue::Map(BTreeMap::from([
                        ("slot_id".into(), UiValue::String(id.into())),
                        ("can_write".into(), UiValue::Bool(can_write)),
                    ]))
                })
                .collect();
            let model = UiValue::Map(BTreeMap::from([("slots".into(), UiValue::List(slots))]));
            let effects = host
                .invoke_open(
                    controller,
                    "astra.vn.ui_model.save.v1",
                    &model,
                    &mut VnUiSessionState::default(),
                )
                .expect("open save page");
            assert_eq!(
                effects,
                vec![VnUiControllerEffect::Focus {
                    semantic_id: if writable {
                        format!("{prefix}slot.02{suffix}")
                    } else {
                        back.into()
                    },
                }]
            );
        }
    }
}

#[test]
fn tsuinosora_modern_save_controller_requires_overwrite_confirmation() {
    let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
    host.register_source(include_str!(
        "../../../../../../../Examples/TsuiNoSora/ProjectTemplate/Controllers/tsui_ui.luau"
    ))
    .expect("register TsuiNoSora controllers");
    let slot = UiValue::Map(BTreeMap::from([
        ("slot_id".into(), UiValue::String("slot.01".into())),
        ("occupied".into(), UiValue::Bool(true)),
    ]));
    let model = UiValue::Map(BTreeMap::from([(
        "slots".into(),
        UiValue::List(vec![slot]),
    )]));

    let effects = host
        .invoke_action(
            "tsui.system.save.modern",
            "astra.vn.ui_model.save.v1",
            &model,
            &VnUiAction::RequestSave {
                slot_id: "slot.01".into(),
            },
            &mut VnUiSessionState::default(),
        )
        .expect("invoke save controller");

    assert_eq!(
        effects,
        vec![VnUiControllerEffect::OpenModal {
            view_id: "ui.tsui.modern.save_confirm".into(),
            model: UiValue::Map(BTreeMap::from([(
                "slot_id".into(),
                UiValue::String("slot.01".into()),
            )])),
        }]
    );
}

#[test]
fn snapshot_none_controller_cannot_write_session_state() {
    let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
    host.register_source(
        r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "none",
}, {
    on_action = function(ctx, model, action)
        return { astra.ui.effect.set_session_state("open", { bool = true }) }
    end,
})
"#,
    )
    .expect("register");
    let error = host
        .invoke_action(
            "controller.test",
            "astra.vn.ui_model.test.v1",
            &UiValue::Map(BTreeMap::new()),
            &VnUiAction::Advance,
            &mut VnUiSessionState::default(),
        )
        .expect_err("authority violation must fail");
    assert!(error.to_string().contains("snapshot=none"));
}

#[test]
fn optional_lifecycle_handlers_are_noops() {
    let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
    host.register_source(
        r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "none",
}, {
    on_action = function(ctx, model, action)
        return { astra.ui.effect.forward(action) }
    end,
})
"#,
    )
    .expect("register");
    let mut session = VnUiSessionState::default();
    let model = UiValue::Map(BTreeMap::new());
    assert!(host
        .invoke_open(
            "controller.test",
            "astra.vn.ui_model.test.v1",
            &model,
            &mut session,
        )
        .expect("open")
        .is_empty());
    assert!(host
        .invoke_update(
            "controller.test",
            "astra.vn.ui_model.test.v1",
            &model,
            &VnUiControllerUpdate {
                fixed_time_ns: 100,
                delta_ns: 16,
                generation: 1,
            },
            &mut session,
        )
        .expect("update")
        .is_empty());
}

#[test]
fn missing_optional_handler_does_not_reexecute_controller_source() {
    let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
    host.register_source(
        r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "none",
}, {
    on_action = function(ctx, model, action)
        return { astra.ui.effect.forward(action) }
    end,
})
"#,
    )
    .expect("register");
    host.controllers
        .get_mut("controller.test")
        .expect("registered controller")
        .source = Arc::from("this is not valid Luau");

    let effects = host
        .invoke_update(
            "controller.test",
            "astra.vn.ui_model.test.v1",
            &UiValue::Map(BTreeMap::new()),
            &VnUiControllerUpdate {
                fixed_time_ns: 100,
                delta_ns: 16,
                generation: 1,
            },
            &mut VnUiSessionState::default(),
        )
        .expect("missing optional handler must bypass the VM");
    assert!(effects.is_empty());
}

#[test]
fn lifecycle_handlers_receive_fixed_update_and_commit_session_state() {
    let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
    host.register_source(
        r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "session",
}, {
    on_open = function(ctx, model)
        return { astra.ui.effect.set_session_state("opened", true) }
    end,
    on_update = function(ctx, model, update)
        if update.fixed_time_ns ~= 100 or update.delta_ns ~= 16 or update.generation ~= 7 then
            error("fixed update mismatch")
        end
        return { astra.ui.effect.set_session_state("updated", true) }
    end,
    on_action = function(ctx, model, action)
        return { astra.ui.effect.forward(action) }
    end,
})
"#,
    )
    .expect("register");
    let mut session = VnUiSessionState::default();
    let model = UiValue::Map(BTreeMap::new());
    host.invoke_open(
        "controller.test",
        "astra.vn.ui_model.test.v1",
        &model,
        &mut session,
    )
    .expect("open");
    host.invoke_update(
        "controller.test",
        "astra.vn.ui_model.test.v1",
        &model,
        &VnUiControllerUpdate {
            fixed_time_ns: 100,
            delta_ns: 16,
            generation: 7,
        },
        &mut session,
    )
    .expect("update");
    assert_eq!(session.values().get("opened"), Some(&UiValue::Bool(true)));
    assert_eq!(session.values().get("updated"), Some(&UiValue::Bool(true)));
}

#[test]
fn non_function_optional_handler_is_rejected_during_registration() {
    let mut host = LuauUiControllerHost::new(PolicyExecutionBudget::default()).expect("host");
    let error = host
        .register_source(
            r#"
astra.ui.controller.register("controller.test", {
    schema = "astra.vn.ui_controller.v1",
    view = "ui.test",
    model_schema = "astra.vn.ui_model.test.v1",
    snapshot = "none",
}, {
    on_open = "invalid",
    on_action = function(ctx, model, action) return {} end,
})
"#,
        )
        .expect_err("invalid handler must fail registration");
    assert!(error.to_string().contains("on_open must be a function"));
}
