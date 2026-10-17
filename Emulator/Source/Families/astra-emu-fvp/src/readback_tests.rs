use super::*;
use rfvp::script::opcode::Opcode;

fn changing_tiles() -> tempfile::TempDir {
    fn call(hcb: &mut Vec<u8>, id: u16, args: &[i16]) {
        for value in args {
            hcb.push(Opcode::PushI16 as u8);
            hcb.extend_from_slice(&value.to_le_bytes());
        }
        hcb.push(Opcode::Syscall as u8);
        hcb.extend_from_slice(&id.to_le_bytes());
    }
    let root = tempfile::tempdir().unwrap();
    let mut hcb = vec![0; 4];
    call(&mut hcb, 2, &[1, 1, 0, 0, 640, 480]);
    call(&mut hcb, 3, &[1, 0]);
    let loop_pc = hcb.len() as u32;
    for [r, g, b] in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]] {
        call(&mut hcb, 1, &[1, r, g, b, 255]);
        call(&mut hcb, 0, &[]);
    }
    hcb.push(Opcode::Jmp as u8);
    hcb.extend_from_slice(&loop_pc.to_le_bytes());
    let descriptor = hcb.len() as u32;
    hcb[..4].copy_from_slice(&descriptor.to_le_bytes());
    hcb.extend_from_slice(&4_u32.to_le_bytes());
    hcb.extend_from_slice(&[0; 4]);
    hcb.extend_from_slice(&[0, 0, 1, 0]);
    hcb.extend_from_slice(&4_u16.to_le_bytes());
    for (args, name) in [
        (0, "ThreadNext"),
        (5, "ColorSet"),
        (6, "PrimSetTile"),
        (2, "PrimGroupIn"),
    ] {
        hcb.extend_from_slice(&[args, (name.len() + 1) as u8]);
        hcb.extend_from_slice(name.as_bytes());
        hcb.push(0);
    }
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    std::fs::write(root.path().join("fixture.hcb"), hcb).unwrap();
    root
}

fn assert_color(frame: &astra_emu_family_api::FrameLease, rgb: [u8; 3]) {
    assert!(frame
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[..3] == rgb));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn three_held_frames_backpressure_before_map_and_release_resumes_without_advance() {
    let root = changing_tiles();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut session = open(root.path(), Arc::clone(&cancelled)).unwrap();
    let boot = await_frame(&mut session, None);
    let boot_pointer = boot.pixels().as_ptr();
    assert_color(&boot, [0, 0, 0]);
    session.advance(16_666_667, &[]).unwrap();
    let red = await_frame(&mut session, Some(boot.identity));
    assert_color(&red, [255, 0, 0]);
    session.advance(16_666_667, &[]).unwrap();
    let green = await_frame(&mut session, Some(red.identity));
    assert_color(&green, [0, 255, 0]);
    session.advance(16_666_667, &[]).unwrap();
    for _ in 0..10 {
        assert!(matches!(
            session.acquire_frame(Some(green.identity)).unwrap(),
            FramePoll::Pending(_)
        ));
        assert!(
            !session.gpu.native().frame_pending(),
            "no map can time out while waiting for a CPU slot"
        );
    }
    assert_color(&boot, [0, 0, 0]);
    assert_color(&red, [255, 0, 0]);
    assert_color(&green, [0, 255, 0]);
    drop(boot);
    let blue = await_frame(&mut session, Some(green.identity));
    assert_eq!(blue.pixels().as_ptr(), boot_pointer);
    assert_color(&blue, [0, 0, 255]);
    assert_color(&red, [255, 0, 0]);
    assert_color(&green, [0, 255, 0]);
    session.advance(16_666_667, &[]).unwrap();
    assert!(matches!(
        session.acquire_frame(Some(blue.identity)).unwrap(),
        FramePoll::Pending(_)
    ));
    Box::new(session).close().unwrap();
    assert!(cancelled.load(Ordering::Acquire));
    assert_color(&blue, [0, 0, 255]);
    assert_color(&green, [0, 255, 0]);

    // Closing an actually submitted map must also return without polling it.
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut reopened = open(root.path(), Arc::clone(&cancelled)).unwrap();
    reopened.advance(16_666_667, &[]).unwrap();
    assert!(reopened.gpu.native().frame_pending());
    Box::new(reopened).close().unwrap();
    assert!(cancelled.load(Ordering::Acquire));
}
