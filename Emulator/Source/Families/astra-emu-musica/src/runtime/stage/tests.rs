use super::*;
use crate::{parse_sc, ScOpcodeCatalog};

fn vm(source: &[u8]) -> MusicaVm {
    MusicaVm::new(
        "musica:/scr/stage.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap()
}

#[test]
fn native_stage_sequence_and_stand_parameters_survive_save_and_restore() {
    let source = b".stage front.png:* back.png 0 -20 actor.png 640,1685\r\n.end\r\n";
    let mut live = vm(source);
    let Some(MusicaVmEvent::Stage(stage)) = live.step(1).unwrap() else {
        panic!("missing stage event");
    };
    assert_eq!(
        stage.resource_sequence,
        vec![Some("musica:/bg/front.png".into()), None]
    );
    assert_eq!(stage.reference_position, None);
    assert_eq!(stage.background.as_ref().unwrap().y, -20);
    assert_eq!(stage.stands[0].position, 640);
    assert_eq!(stage.stands[0].resource_parameter, 1685);
    assert_eq!(live.state().stage.as_ref(), Some(&stage));
    let saved = live.encode_native_save().unwrap();
    let decoded = MusicaVm::decode_native_save(&saved).unwrap();
    assert_eq!(decoded.stage.as_ref(), Some(&stage));
    let mut restored = vm(source);
    restored.restore_native_save(&saved, 1).unwrap();
    assert_eq!(restored.state().stage.as_ref(), Some(&stage));
}

#[test]
fn reference_coordinates_keep_full_precision_and_one_trailing_empty_field() {
    let mut vm = vm(b".stage front.png 2147483647 -2147483648 back.png 12 -34 actor.png 640 \r\n");
    vm.step(1).unwrap();
    let stage = vm.state().stage.as_ref().unwrap();
    assert_eq!(stage.reference_position, Some([i32::MAX, i32::MIN]));
    assert_eq!(stage.background.as_ref().unwrap().x, 12);
    assert_eq!(stage.stands[0].resource_parameter, 0);
}

#[test]
fn invalid_native_stage_does_not_replace_previous_scene() {
    for invalid in [
        "front.png: back.png 0 0",
        "front.png:*:* back.png 0 0",
        "* back.png 0 0 actor.png 640,",
        "* back.png 0 0 actor.png 640,1,2",
        "* back.png 0 0 actor.png ,1685",
        "* back.png 0 0 actor.png 2147483648",
        "* ../back.png 0 0",
        "* back.png 0 0  ",
    ] {
        let source = format!(".stage * back.png 0 0\r\n.stage {invalid}\r\n");
        let mut vm = vm(source.as_bytes());
        vm.step(1).unwrap();
        let previous = vm.state().stage.clone();
        assert!(vm.step(2).is_err(), "accepted invalid stage: {invalid}");
        assert_eq!(vm.state().stage, previous);
    }
}

#[test]
fn comma_in_resource_name_is_not_reinterpreted_as_a_stand_parameter() {
    let mut vm = vm(b".stage * back.png 0 0 actor.png,1 640\r\n");
    vm.step(1).unwrap();
    let stand = &vm.state().stage.as_ref().unwrap().stands[0];
    assert_eq!(stand.resource_uri, "musica:/st/actor.png,1");
    assert_eq!(stand.resource_parameter, 0);
}

#[test]
fn restore_rejects_invalid_stage_before_changing_live_state() {
    let source = b".stage * back.png 0 0\r\n.end\r\n";
    let mut live = vm(source);
    live.step(1).unwrap();
    let previous = live.state().clone();
    for variant in 0..4 {
        let mut invalid = previous.clone();
        let stage = invalid.stage.as_mut().unwrap();
        match variant {
            0 => stage.resource_sequence.clear(),
            1 => stage.resource_sequence = vec![None; 3],
            2 => stage.resource_sequence = vec![Some("musica:/bg/../private.png".into())],
            _ => invalid.schema = "astra.emu.musica.runtime_state.v7".into(),
        }
        let bytes = postcard::to_allocvec(&invalid).unwrap();
        assert!(MusicaVm::decode_native_save(&bytes).is_err());
        assert!(live.restore_native_save(&bytes, 2).is_err());
        assert_eq!(live.state(), &previous);
    }
}
