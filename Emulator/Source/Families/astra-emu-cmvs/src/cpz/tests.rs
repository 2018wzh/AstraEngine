use super::*;

fn scheme() -> Cpz5Scheme {
    Cpz5Scheme {
        secret: std::array::from_fn(|index| 0x1020_3040u32.wrapping_add(index as u32)),
        index_addend: 0x5566_7788,
        index_subtrahend: 0x1d,
        index_seed: 0x1234_5678,
        directory_key_addend: [0, 1, 2, 3],
    }
}

#[test]
fn cpz5_index_transforms_accept_unaligned_buffers() {
    let scheme = scheme();
    let mut data = vec![0x51; 13];
    scheme.decrypt_index_stage_1(&mut data, 0x22);
    scheme.decrypt_directory(&mut data, [1, 2, 3, 4], 5);
    scheme.decrypt_entries(&mut data, [6, 7, 8, 9], 10);
    assert_ne!(data, vec![0x51; 13]);
}

#[test]
fn rejects_unknown_magic() {
    let error = parse_cpz_header(b"NOPE", 4).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_CPZ_MAGIC");
}

#[test]
fn cpz1_rejects_index_past_source() {
    let mut source = vec![0; 0x10];
    source[..4].copy_from_slice(b"CPZ1");
    source[4..8].copy_from_slice(&1u32.to_le_bytes());
    source[8..12].copy_from_slice(&1u32.to_le_bytes());
    let error = parse_cpz_header(&source, source.len() as u64).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_CPZ_HEADER");
}
