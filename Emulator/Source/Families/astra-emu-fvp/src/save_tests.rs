use rfvp::host_api::clock::CalendarTime;
use rfvp::script::parser::Nls;
use rfvp::subsystem::resources::save_manager::{SaveItem, SaveManager};
use rfvp::subsystem::resources::thread_manager::ThreadManager;
use rfvp::subsystem::save_state::{try_decode_state_chunk_v1, SaveStateSnapshotV1};
use rfvp::subsystem::world::GameData;

fn prepared_save() -> SaveManager {
    let mut save = SaveManager::new();
    save.set_thumb_size(1, 1);
    save.set_current_title("日本語".into());
    save.request_prepare_local_savedata();
    assert!(save.wants_vm_snapshot_capture());
    let mut snapshot =
        SaveStateSnapshotV1::capture_hosted(&GameData::default(), &ThreadManager::new());
    snapshot.vm.current_id = 7;
    save.prepare_hosted_save(
        vec![11, 22, 33, 255],
        &snapshot,
        Nls::ShiftJIS,
        CalendarTime {
            year: 2026,
            month: 9,
            day: 10,
            day_of_week: 4,
            hour: 13,
            minute: 2,
        },
    )
    .unwrap();
    assert!(!save.wants_vm_snapshot_capture());
    assert!(!save.test_save_slot(0));
    save
}

#[test]
fn save_write_preserves_pre_menu_capture_and_supports_cold_load() {
    let mut save = prepared_save();
    save.set_current_title("menu".into());
    save.asynchronously_save(12);
    assert!(!save.wants_vm_snapshot_capture());
    let (slot, bytes) = save.pending_save_write().unwrap().unwrap();
    assert_eq!(slot, 12);
    assert_eq!(&bytes[..7], &[234, 7, 9, 10, 4, 13, 2]);
    assert!(!save.test_save_slot(12));
    let item = SaveItem::load_from_mem(&bytes, Nls::ShiftJIS).unwrap();
    assert_eq!(item.title, "日本語");
    assert_eq!(item.thumb, [11, 22, 33, 255]);
    assert_eq!(
        try_decode_state_chunk_v1(&bytes)
            .unwrap()
            .unwrap()
            .vm
            .current_id,
        7
    );
    save.finalize_save_write(slot, bytes.clone(), Nls::ShiftJIS)
        .unwrap();
    assert!(save.test_save_slot(12));
    assert!(save.pending_save_write().unwrap().is_none());
    save.asynchronously_save(13);
    assert_eq!(save.pending_save_write().unwrap().unwrap().1, bytes);
    let mut cold = SaveManager::new();
    cold.load_slot_into_current_from_bytes(12, Nls::ShiftJIS, &bytes)
        .unwrap();
    cold.restore_current_metadata(12).unwrap();
    assert_eq!(cold.get_current_title(), "日本語");
    assert_eq!(cold.get_year(12), 2026);
}

#[test]
fn native_save_rejects_invalid_header_and_missing_state() {
    let mut save = prepared_save();
    save.asynchronously_save(0);
    let (_, bytes) = save.pending_save_write().unwrap().unwrap();
    for length in [0, 6, 7, 8, bytes.len() - 1] {
        assert!(SaveItem::load_from_mem(&bytes[..length], Nls::ShiftJIS).is_err());
    }
    let mut invalid = bytes.clone();
    invalid[2] = 13;
    assert!(SaveItem::load_from_mem(&invalid, Nls::ShiftJIS).is_err());
    invalid = bytes;
    invalid[7..9].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(SaveItem::load_from_mem(&invalid, Nls::ShiftJIS).is_err());
    assert!(SaveItem::load_from_mem(b"title=old fake save", Nls::UTF8).is_err());
}

#[test]
fn save_prepare_captures_after_all_contexts_yield_in_the_current_frame() {
    use rfvp::script::parser::{Parser, Syscall};
    use rfvp::vm_runner::VmRunner;
    use std::sync::Arc;

    // Context 0 requests SaveCreate(3, nil), then yields with ThreadNext.
    // Capture the entire cooperative frame, including later contexts' cleanup.
    let mut parser = Parser::default();
    parser.buffer = Arc::new(vec![12, 3, 8, 3, 0, 0, 3, 1, 0, 255, 3, 1, 0, 255]);
    parser.syscalls.insert(
        0,
        Syscall {
            args: 2,
            name: "SaveCreate".into(),
        },
    );
    parser.syscalls.insert(
        1,
        Syscall {
            args: 0,
            name: "ThreadNext".into(),
        },
    );
    let mut runner = VmRunner::new(ThreadManager::new());
    runner.start_main(0);
    runner.thread_manager_mut().thread_start(1, 10);
    let mut game = GameData::default();
    runner.tick(&mut game, &mut parser, 16).unwrap();
    assert_eq!(runner.thread_manager().contexts[0].get_pc(), 9);
    assert_eq!(runner.thread_manager().contexts[1].get_pc(), 13);
    // A second tick in the same host frame must also wait for persistence.
    runner.tick(&mut game, &mut parser, 0).unwrap();
    let snapshot = SaveStateSnapshotV1::capture_user_save(&mut game).unwrap();
    assert_eq!(snapshot.vm.current_id, 1);
    assert_eq!(snapshot.vm.contexts[0].cursor, 9);
    assert_eq!(snapshot.vm.contexts[1].cursor, 13);
    let mut restored = ThreadManager::new();
    restored.apply_snapshot_v1(&snapshot.vm);
    restored.signal_native_load();
    assert!(restored
        .capture_snapshot_v1()
        .contexts
        .iter()
        .all(|c| matches!(c.return_value, rfvp::script::Variant::True)));
    restored.contexts[0].push_return_value().unwrap();
    let consumed = restored.contexts[0].capture_snapshot_v1();
    assert!(matches!(consumed.return_value, rfvp::script::Variant::Nil));
    assert!(matches!(
        consumed.stack[consumed.cur_stack_base + consumed.cur_stack_pos - 1],
        rfvp::script::Variant::True
    ));
}

#[test]
fn hosted_vm_preserves_load_request_for_core_across_repeated_ticks() {
    use rfvp::script::parser::{Parser, Syscall};
    use rfvp::vm_runner::VmRunner;
    use std::sync::Arc;

    let mut parser = Parser::default();
    parser.buffer = Arc::new(vec![12, 1, 3, 0, 0, 255]);
    parser.syscalls.insert(
        0,
        Syscall {
            args: 1,
            name: "Load".into(),
        },
    );
    let mut runner = VmRunner::new(ThreadManager::new());
    runner.start_main(0);
    runner.thread_manager_mut().thread_start(1, 5);
    let mut game = GameData::default();
    runner.tick(&mut game, &mut parser, 16).unwrap();
    runner.tick(&mut game, &mut parser, 0).unwrap();
    runner.tick(&mut game, &mut parser, 0).unwrap();
    assert_eq!(runner.thread_manager().contexts[1].get_pc(), 5);
}

#[test]
fn hosted_save_refresh_blocks_queries_until_file_operations_complete() {
    use rfvp::script::parser::{Parser, Syscall};
    use rfvp::vm_runner::VmRunner;
    use std::sync::Arc;

    // SaveData(RefreshAll, nil, nil) must finish before any subsequent opcode
    // or coroutine can inspect slots. The next opcode is deliberately invalid.
    let mut parser = Parser::default();
    parser.buffer = Arc::new(vec![12, 0, 8, 8, 3, 0, 0, 255]);
    parser.syscalls.insert(
        0,
        Syscall {
            args: 3,
            name: "SaveData".into(),
        },
    );
    let mut runner = VmRunner::new(ThreadManager::new());
    runner.start_main(0);
    runner.thread_manager_mut().thread_start(1, 7);
    let mut game = GameData::default();
    runner.tick(&mut game, &mut parser, 16).unwrap();
    runner.tick(&mut game, &mut parser, 0).unwrap();
    assert_eq!(runner.thread_manager().contexts[0].get_pc(), 7);
    assert_eq!(runner.thread_manager().contexts[1].get_pc(), 7);
}
