use super::*;

#[test]
fn loaded_texture_survives_metadata_observation_until_native_gpu_upload() {
    use rfvp::script::opcode::Opcode;
    let root = tempfile::tempdir().unwrap();
    let mut texture = b"hzc1".to_vec();
    texture.extend_from_slice(&32_u32.to_le_bytes());
    texture.extend_from_slice(&32_u32.to_le_bytes());
    texture.extend_from_slice(b"NVSG");
    for field in [0_u16, 1, 4, 2, 0, 0, 0, 0] {
        texture.extend_from_slice(&field.to_le_bytes());
    }
    texture.extend_from_slice(&[0; 12]);
    texture.extend_from_slice(&[120, 156, 251, 255, 31, 63, 0, 0, 14, 46, 31, 225]);
    let mut pack = Vec::new();
    for value in [1_u32, 11, 0, 31, texture.len() as u32] {
        pack.extend_from_slice(&value.to_le_bytes());
    }
    pack.extend_from_slice(b"white.nvsg\0");
    pack.extend_from_slice(&texture);
    std::fs::write(root.path().join("graph.bin"), pack).unwrap();

    fn push(hcb: &mut Vec<u8>, value: i16) {
        hcb.push(Opcode::PushI16 as u8);
        hcb.extend_from_slice(&value.to_le_bytes());
    }
    fn call(hcb: &mut Vec<u8>, id: u16) {
        hcb.push(Opcode::Syscall as u8);
        hcb.extend_from_slice(&id.to_le_bytes());
    }
    let mut hcb = vec![0; 4];
    push(&mut hcb, 0);
    hcb.extend_from_slice(&[Opcode::PushString as u8, 17]);
    hcb.extend_from_slice(b"graph/white.nvsg\0");
    call(&mut hcb, 1);
    for value in [1, 0, 0, 0] {
        push(&mut hcb, value);
    }
    call(&mut hcb, 2);
    for value in [1, 0] {
        push(&mut hcb, value);
    }
    call(&mut hcb, 3);
    let wait = hcb.len() as u32;
    call(&mut hcb, 0);
    hcb.push(Opcode::Jmp as u8);
    hcb.extend_from_slice(&wait.to_le_bytes());
    let descriptor = hcb.len() as u32;
    hcb[..4].copy_from_slice(&descriptor.to_le_bytes());
    hcb.extend_from_slice(&4_u32.to_le_bytes());
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    hcb.extend_from_slice(&[0, 0, 1, 0]);
    hcb.extend_from_slice(&4_u16.to_le_bytes());
    for (args, name) in [
        (0, "ThreadNext"),
        (2, "GraphLoad"),
        (4, "PrimSetSprt"),
        (2, "PrimGroupIn"),
    ] {
        hcb.extend_from_slice(&[args, (name.len() + 1) as u8]);
        hcb.extend_from_slice(name.as_bytes());
        hcb.push(0);
    }
    hcb.extend_from_slice(&0_u16.to_le_bytes());
    std::fs::write(root.path().join("fixture.hcb"), hcb).unwrap();
    let mut session = open(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    let frame = await_frame(&mut session, None);
    for y in 0..2 {
        let begin = y * frame.info.stride as usize;
        assert!(frame.pixels()[begin..begin + 16]
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [255; 3]));
    }
    assert_eq!(&frame.pixels()[16..19], &[0; 3]);
    for _ in 0..3 {
        session.advance(16_666_667, &[]).unwrap();
        assert!(
            matches!(session.acquire_frame(Some(frame.identity)).unwrap(), FramePoll::Unchanged(id) if id == frame.identity)
        );
    }
    Box::new(session).close().unwrap();
    assert_eq!(&frame.pixels()[..3], &[255; 3]);
}
