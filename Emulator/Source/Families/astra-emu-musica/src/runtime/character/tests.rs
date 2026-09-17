use super::*;
use crate::{parse_sc, ScOpcodeCatalog};
#[test]
fn character_stage_retention_and_native_restore() {
    let source = b".char load -1 Stand.png\r\n.char pos 1 16 4\r\n.stage * * 0 0\r\n.char keep 1\r\n.stage * * 0 0\r\n.stage * * 0 0\r\n.end\r\n";
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    for tick in 1..=3 {
        vm.step(tick).unwrap();
    }
    let character = &vm.state().characters[&1];
    assert!(!character.positive_orientation);
    assert_eq!(character.anchor_position, [16, 4]);
    assert!(!character.pending_stage);
    vm.step(4).unwrap();
    let saved = vm.encode_native_save().unwrap();
    let mut restored = make_vm();
    restored.restore_native_save(&saved, 5).unwrap();
    vm.step(5).unwrap();
    restored.step(5).unwrap();
    assert_eq!(vm.state().characters, restored.state().characters);
    assert!(!vm.state().characters[&1].keep_once);
    vm.step(6).unwrap();
    assert!(vm.state().characters.is_empty());
}
