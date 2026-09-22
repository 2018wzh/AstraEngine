use super::*;
use crate::{MusicaVm, MusicaVmEvent, MusicaWaitState, ScOpcodeCatalog, ScriptEncoding};

fn field(name: &str, value: &str) -> EdenSaveField {
    EdenSaveField {
        name: name.into(),
        value: value.into(),
    }
}

pub(super) fn checkpoint_fixture() -> EdenSave {
    let mut save = tests::fixture();
    save.route = [0; 4];
    save.variables.clear();
    for name in ["angBG", "angFG"] {
        save.variables.push(field(name, "0000000000000000"));
    }
    for name in [
        "bgH",
        "bgS",
        "bgV",
        "stH",
        "stS",
        "stV",
        "stage_FGX",
        "stage_FGY",
        "rotBG_dest.x",
        "rotBG_dest.y",
        "rotBG_source.x",
        "rotBG_source.y",
        "rotFG_dest.x",
        "rotFG_dest.y",
        "rotFG_source.x",
        "rotFG_source.y",
        "tr_Method",
        "stage_BGX",
        "stage_BGY",
    ] {
        save.variables.push(field(name, "0"));
    }
    for name in ["eff_Count", "eff_Param", "eff_Speed", "frameParam"] {
        save.variables.push(field(name, "-1"));
    }
    for (name, value) in [
        ("font_Color", "16777215"),
        ("font_FontSize", "22"),
        ("font_RubySize", "10"),
        ("script_Filename", "fixture.sc"),
        ("script_Pointer", "2"),
        ("script_ID", "7"),
        ("stage_BGFilename", "background.png"),
        ("panel_Mode", "1"),
        ("bgm_Filename", ""),
        ("bgm_Volume", "100"),
        ("tr_Param", "30"),
    ] {
        save.variables.push(field(name, value));
    }
    let mut record = Vec::new();
    for name in [
        "LC", "CT3", "CT4", "CT6", "CT7", "bgH", "bgS", "bgV", "stH", "stS", "stV", "Ct1",
    ] {
        record.push(field(name, "0"));
    }
    for name in [
        "CT1", "Frm", "CO0", "CO1", "CO2", "Cs0", "Cs1", "Cs2", "Ce1", "Ce2", "Ct2", "CP2", "CB1",
        "CE1", "CE2", "L1",
    ] {
        record.push(field(name, ""));
    }
    for name in ["FrP", "Ce3", "Ce4", "Ce5"] {
        record.push(field(name, "-1"));
    }
    for (name, value) in [
        ("CF1", "16777215"),
        ("CF2", "22"),
        ("CF3", "10"),
        ("CT5", "10"),
        ("L0", "7"),
        ("CS3", "7"),
        ("CS1", "fixture.sc"),
        ("CS2", "2"),
        ("L2", "speaker"),
        ("L3", "saved"),
        ("CT2", "background.png"),
        ("CP1", "1"),
        ("CB1v", "100"),
        ("Ct3", "30"),
    ] {
        record.push(field(name, value));
    }
    save.backlog = vec![record];
    save
}

fn vm() -> MusicaVm {
    let bytes = b".set before = 99\n.message 7  speaker saved\n.set after = 3\n.message 8  speaker continued\n.end\n";
    vm_for(bytes)
}

fn vm_for(bytes: &[u8]) -> MusicaVm {
    let script = crate::parse_sc_with_encoding(
        bytes,
        &ScOpcodeCatalog::observed_musica(),
        ScriptEncoding::Gbk,
    )
    .unwrap();
    MusicaVm::new(
        "musica:/scr/fixture.sc".into(),
        astra_core::Hash256::from_sha256(bytes),
        script,
        42,
    )
    .unwrap()
}

#[test]
fn exports_new_message_state_after_internal_restore_without_reusing_raw_native_fields() {
    let mut vm = vm_for(b"; fixture\n.message 7  speaker saved\n.message 8  speaker second\n.message 9  speaker third\n.end\n");
    vm.restore_eden_save(&checkpoint_fixture(), |_| panic!("same script"), 1)
        .unwrap();
    for tick in 1..=2 {
        let Some(MusicaWaitState::Input { token_id }) = vm.state().wait.clone() else {
            panic!("message wait")
        };
        vm.resolve_wait(&token_id).unwrap();
        vm.step(tick).unwrap();
        let saved = vm.encode_native_save().unwrap();
        vm.restore_native_save(&saved, tick + 1).unwrap();
    }
    let exported = vm.export_eden_save().unwrap();
    assert_eq!(exported.variable("script_ID"), Some("9"));
    assert_eq!(exported.variable("script_Pointer"), Some("4"));
    assert_eq!(exported.backlog.len(), 3);
    let decoded = EdenSave::decode(
        &exported.encode().unwrap(),
        exported.edition,
        exported.encoding,
    )
    .unwrap();
    assert_eq!(decoded.checkpoint().unwrap().message_id, 9);
    let mut restored = vm_for(b"; fixture\n.message 7  speaker saved\n.message 8  speaker second\n.message 9  speaker third\n.end\n");
    restored
        .restore_eden_save(&decoded, |_| panic!("same script"), 1)
        .unwrap();
    assert_eq!(restored.state().backlog, vm.state().backlog);
}

#[test]
fn unrepresentable_execution_disables_export_without_stopping_gameplay() {
    let mut vm = vm();
    vm.restore_eden_save(&checkpoint_fixture(), |_| panic!("same script"), 1)
        .unwrap();
    assert!(vm.export_eden_save().is_ok());
    let Some(MusicaWaitState::Input { token_id }) = vm.state().wait.clone() else {
        panic!("message wait")
    };
    vm.resolve_wait(&token_id).unwrap();
    vm.step(1).unwrap();
    assert!(matches!(
        vm.state().eden_export,
        EdenExportState::Rejected(EdenExportRejection::Variables)
    ));
    assert!(vm.export_eden_save().is_err());
    assert!(vm.encode_native_save().is_ok());
}

#[test]
fn corrupted_interchange_history_cannot_replace_active_internal_state() {
    let mut vm = vm();
    vm.restore_eden_save(&checkpoint_fixture(), |_| panic!("same script"), 1)
        .unwrap();
    let original = vm.encode_native_save().unwrap();
    let mut changed = MusicaVm::decode_native_save(&original).unwrap();
    let EdenExportState::Ready { history, .. } = &mut changed.eden_export else {
        panic!("native history")
    };
    history[0].text = "different".into();
    let bytes = postcard::to_allocvec(&changed).unwrap();
    assert!(vm.restore_native_save(&bytes, 1).is_err());
    assert_eq!(vm.encode_native_save().unwrap(), original);
}

#[test]
fn native_checkpoint_resumes_at_saved_boundary_without_running_prior_commands() {
    let mut vm = vm();
    let event = vm
        .restore_eden_save(
            &checkpoint_fixture(),
            |_| panic!("current script is already loaded"),
            1,
        )
        .unwrap();
    assert!(matches!(event, MusicaVmEvent::Message { text, .. } if text == "saved"));
    assert!(!vm.state().variables.contains_key("before"));
    assert_eq!(vm.state().backlog.len(), 1);
    let Some(MusicaWaitState::Input { token_id }) = vm.state().wait.clone() else {
        panic!("message wait")
    };
    vm.resolve_wait(&token_id).unwrap();
    assert!(
        matches!(vm.step(1).unwrap(), Some(MusicaVmEvent::Message { text, .. }) if text == "continued")
    );
    assert_eq!(vm.state().variables.get("after"), Some(&3));
    assert_eq!(vm.state().backlog.len(), 2);
    let state = vm.encode_native_save().unwrap();
    vm.restore_native_save(&state, 2).unwrap();
    assert_eq!(vm.state().backlog.len(), 2);
}

#[test]
fn native_restore_failure_keeps_active_state_and_rejects_unmodeled_fields() {
    let mut vm = vm();
    let original = vm.encode_native_save().unwrap();
    for (name, value) in [
        ("font_FontSize", "23"),
        ("stage_BGX", "2"),
        ("script_ID", "8"),
        ("eff_Method", "Snow"),
        ("unknown", "0"),
    ] {
        let mut save = checkpoint_fixture();
        if let Some(f) = save.variables.iter_mut().find(|f| f.name == name) {
            f.value = value.into();
        } else {
            save.variables.push(field(name, value));
        }
        assert!(vm
            .restore_eden_save(&save, |_| panic!("no history read expected"), 1)
            .is_err());
        assert_eq!(vm.encode_native_save().unwrap(), original);
    }
    let mut save = checkpoint_fixture();
    save.backlog[0]
        .iter_mut()
        .find(|f| f.name == "L3")
        .unwrap()
        .value = "changed".into();
    assert!(vm
        .restore_eden_save(&save, |_| panic!("no history read expected"), 1)
        .is_err());
    assert_eq!(vm.encode_native_save().unwrap(), original);
}

#[test]
fn missing_history_script_does_not_discard_history_or_replace_active_vm() {
    let mut vm = vm();
    let original = vm.encode_native_save().unwrap();
    let mut save = checkpoint_fixture();
    let mut record = save.backlog[0].clone();
    record
        .iter_mut()
        .find(|field| field.name == "CS1")
        .unwrap()
        .value = "missing.sc".into();
    save.backlog.insert(0, record);
    let mut calls = 0;
    let result = vm.restore_eden_save(
        &save,
        |name| {
            assert_eq!(name, "missing.sc");
            calls += 1;
            Err(CoreError::invalid(
                "TEST_SCRIPT_MISSING",
                "missing history script",
            ))
        },
        1,
    );
    assert!(result.is_err());
    assert_eq!(calls, 1);
    assert_eq!(vm.encode_native_save().unwrap(), original);
}
