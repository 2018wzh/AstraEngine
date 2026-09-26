use super::*;

#[test]
fn retains_native_string_bytes_and_rejects_unbounded_or_truncated_pools() {
    let mut payload = layout_payload();
    payload.splice(44800..44801, [0x83, 0x61, 0]);
    payload.extend_from_slice(b"first\0second\0");
    let mut bytes = encode_container(&payload, 0);
    bytes[88..92].copy_from_slice(&1u32.to_le_bytes());
    bytes[84..88].copy_from_slice(&1u32.to_le_bytes());
    let save = parse_cmvs_system_save(&bytes, 0).unwrap();
    assert_eq!(save.title_slots[0], vec![0x83, 0x61]);
    assert_eq!(
        save.lists,
        [vec![b"first".to_vec()], vec![b"second".to_vec()]]
    );
    bytes[88..92].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse_cmvs_system_save(&bytes, 0).is_err());
    for value in [vec![b'a'; 1024], vec![0x81], vec![0xff]] {
        let mut payload = layout_payload();
        payload.splice(44800..44800, value);
        assert!(parse_cmvs_system_save(&encode_container(&payload, 0), 0).is_err());
    }
    let mut payload = layout_payload();
    payload.extend_from_slice(&[b'a'; 64]);
    payload.push(0);
    let mut bytes = encode_container(&payload, 0);
    bytes[88..92].copy_from_slice(&1u32.to_le_bytes());
    assert!(parse_cmvs_system_save(&bytes, 0).is_err());
}

/// Builds a valid synthetic container around one decompressed payload.
/// The payload is stored with an all-literal LZSS stream and the engine
/// cipher inverted from the recovered decrypt path.
pub(crate) fn encode_container(decompressed: &[u8], identity: u32) -> Vec<u8> {
    // All-literal LZSS: flag byte 0xff followed by up to eight literals.
    let mut plain = Vec::new();
    for chunk in decompressed.chunks(8) {
        plain.push(0xff_u8);
        plain.extend_from_slice(chunk);
    }
    let rot = 3_u32;
    let addend = 5_u8;
    let xor_base = 9_u8;
    let payload: Vec<u8> = plain
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            let key = xor_base.wrapping_add(SYSTEM_SAVE_KEY[index % SYSTEM_SAVE_KEY.len()]);
            let t = byte.wrapping_add(64) ^ key;
            let t = t.wrapping_add(addend);
            t.rotate_left(rot)
        })
        .collect();
    let total = SYSTEM_SAVE_HEADER_BYTES + payload.len() + 2;
    let mut source = vec![0_u8; total];
    source[68..72].copy_from_slice(&identity.to_le_bytes());
    source[76..78].copy_from_slice(&1_u16.to_le_bytes());
    // word@78 supplies the addend byte; keep the rotation derivation
    // stable: word@78 low bits choose shift 0, word@82 provides rot-2.
    source[78] = addend;
    source[79] = 0;
    source[80] = xor_base;
    let shift = u16::from((addend >> 1) & 7);
    let word82 = (((rot - 2) & 3) as u16) << shift;
    source[82..84].copy_from_slice(&word82.to_le_bytes());
    source[84..88].copy_from_slice(&0_u32.to_le_bytes()); // list B count
    source[88..92].copy_from_slice(&0_u32.to_le_bytes()); // list A count
    source[92..96].copy_from_slice(&(decompressed.len() as u32).to_le_bytes());
    source[96..100].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    source[100..104].copy_from_slice(&((total - 2) as u32).to_le_bytes());
    source[SYSTEM_SAVE_HEADER_BYTES..SYSTEM_SAVE_HEADER_BYTES + payload.len()]
        .copy_from_slice(&payload);
    let mut sum = 0_u16;
    for byte in &source[SYSTEM_SAVE_HEADER_BYTES..total - 2] {
        sum = sum.wrapping_add(u16::from(*byte));
    }
    source[total - 2..].copy_from_slice(&sum.to_le_bytes());
    source
}

pub(crate) fn layout_payload() -> Vec<u8> {
    let mut payload = vec![0_u8; TITLE_STRINGS_OFFSET + SYSTEM_SAVE_TITLE_SLOTS];
    for (index, byte) in payload[..SYSTEM_SAVE_FLAG_BYTES].iter_mut().enumerate() {
        *byte = (index % 251) as u8;
    }
    for index in 0..SYSTEM_SAVE_CG_WORDS {
        let word = (index as u32) | 0x1000_0000;
        payload[CG_TABLE_OFFSET + index * 4..CG_TABLE_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    for index in 0..SYSTEM_SAVE_BGM_WORDS {
        let word = (index as u32) | 0x2000_0000;
        payload[BGM_TABLE_OFFSET + index * 4..BGM_TABLE_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    // Zero-filled title slots parse as 64 empty strings.
    payload
}

#[test]
fn parses_a_valid_synthetic_system_save() {
    let payload = layout_payload();
    let container = encode_container(&payload, 0);
    let save = parse_cmvs_system_save(&container, 0).unwrap();
    assert_eq!(save.flags.len(), SYSTEM_SAVE_FLAG_BYTES);
    assert_eq!(save.flags[251], 0);
    assert_eq!(save.flags[250], 250);
    assert_eq!(save.cg_words.len(), SYSTEM_SAVE_CG_WORDS);
    assert_eq!(save.cg_words[3], 3 | 0x1000_0000);
    assert_eq!(save.bgm_words.len(), SYSTEM_SAVE_BGM_WORDS);
    assert_eq!(save.bgm_words[5], 5 | 0x2000_0000);
    assert_eq!(save.title_slots.len(), SYSTEM_SAVE_TITLE_SLOTS);
    assert_eq!(save.lists[0].len(), 0);
    assert_eq!(save.lists[1].len(), 0);
}

#[test]
fn rejects_a_corrupted_checksum() {
    let payload = layout_payload();
    let mut container = encode_container(&payload, 0);
    let last = container.len() - 3;
    container[last] ^= 0xff;
    let error = parse_cmvs_system_save(&container, 0).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_SYSTEM_SAVE_CHECKSUM");
}

#[test]
fn rejects_a_mismatched_identity() {
    let payload = layout_payload();
    let container = encode_container(&payload, 77);
    let error = parse_cmvs_system_save(&container, 7).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_SYSTEM_SAVE_IDENTITY");
    let save = parse_cmvs_system_save(&container, 77).unwrap();
    assert_eq!(save.cg_words.len(), SYSTEM_SAVE_CG_WORDS);
}

#[test]
fn rejects_a_truncated_header() {
    let error = parse_cmvs_system_save(&[0_u8; 16], 0).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS");
}

#[test]
fn decompressor_reproduces_all_literal_streams() {
    let payload = b"abcdefgh01234567".to_vec();
    let mut stream = Vec::new();
    for chunk in payload.chunks(8) {
        stream.push(0xff_u8);
        stream.extend_from_slice(chunk);
    }
    let decoded = decompress_system_save(&stream, payload.len()).unwrap();
    assert_eq!(decoded, payload);
}

#[test]
fn decompressor_reproduces_window_matches() {
    // Emit eight literals, then copy them back through the window. The
    // window starts at position 976, so the literals occupy 976..984.
    let mut stream = vec![0xff_u8];
    stream.extend_from_slice(b"abcdefgh");
    // The next token needs a fresh flag byte; bit 0 selects the match.
    stream.push(0xfe_u8);
    let offset = 976_usize;
    let copies = 8_usize;
    stream.push((offset & 0xff) as u8);
    stream.push((((offset >> 2) & 0xc0) | ((copies - 2) & 0x3f)) as u8);
    let decoded = decompress_system_save(&stream, 16).unwrap();
    assert_eq!(decoded, b"abcdefghabcdefgh".to_vec());
}
