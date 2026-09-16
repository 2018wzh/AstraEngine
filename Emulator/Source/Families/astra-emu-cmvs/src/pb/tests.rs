use super::*;

#[test]
fn parses_pb3_metadata() {
    let mut source = vec![0_u8; PB3_DECRYPT_BYTES + PB3_TAIL_KEY_BYTES];
    source[..4].copy_from_slice(PB3_MAGIC);
    let input_size = u32::try_from(source.len()).expect("fixture length fits u32");
    source[4..8].copy_from_slice(&input_size.to_le_bytes());
    source[0x1c..0x1e].copy_from_slice(&1_u16.to_le_bytes());
    source[0x1e..0x20].copy_from_slice(&640_u16.to_le_bytes());
    source[0x20..0x22].copy_from_slice(&360_u16.to_le_bytes());
    source[0x22..0x24].copy_from_slice(&32_u16.to_le_bytes());
    let metadata = parse_pb3_metadata(&source).expect("PB3 fixture should parse");
    assert_eq!(metadata.width, 640);
    assert_eq!(metadata.bits_per_pixel, 32);
}

#[test]
fn decrypts_pb3_protected_metadata_prefix() {
    let mut source = vec![0_u8; PB3_DECRYPT_BYTES + PB3_TAIL_KEY_BYTES];
    source[..4].copy_from_slice(PB3_MAGIC);
    let source_size = u32::try_from(source.len()).unwrap();
    source[4..8].copy_from_slice(&source_size.to_le_bytes());
    source[0x1c..0x1e].copy_from_slice(&5_u16.to_le_bytes());
    source[0x1e..0x20].copy_from_slice(&1280_u16.to_le_bytes());
    source[0x20..0x22].copy_from_slice(&720_u16.to_le_bytes());
    source[0x22..0x24].copy_from_slice(&32_u16.to_le_bytes());
    let tail_offset = source.len() - PB3_TAIL_KEY_BYTES;
    for (index, byte) in source[tail_offset..].iter_mut().enumerate() {
        *byte = u8::try_from(index + 1).unwrap();
    }
    let tail = source[tail_offset..].to_vec();
    let key_offset = source.len() - 3;
    let key = u16::from_le_bytes(source[key_offset..key_offset + 2].try_into().unwrap());
    for (offset, key_byte) in (8..PB3_DECRYPT_BYTES).zip(&tail) {
        source[offset] = source[offset].wrapping_add(*key_byte);
    }
    for offset in (8..PB3_DECRYPT_BYTES).step_by(2) {
        let word = u16::from_le_bytes(source[offset..offset + 2].try_into().unwrap()) ^ key;
        source[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
    }

    let metadata = parse_pb3_metadata(&source).expect("PB3 protected header should parse");
    assert_eq!(
        (metadata.width, metadata.height, metadata.image_type),
        (1280, 720, 5)
    );
}

#[test]
fn decodes_pb3_type5_delta_channels() {
    let mut source = vec![0_u8; 128];
    source[..4].copy_from_slice(PB3_MAGIC);
    let source_size = u32::try_from(source.len()).unwrap();
    source[4..8].copy_from_slice(&source_size.to_le_bytes());
    source[0x1c..0x1e].copy_from_slice(&5_u16.to_le_bytes());
    source[0x1e..0x20].copy_from_slice(&1_u16.to_le_bytes());
    source[0x20..0x22].copy_from_slice(&1_u16.to_le_bytes());
    source[0x22..0x24].copy_from_slice(&32_u16.to_le_bytes());
    for channel in 0..4 {
        let table = 0x34 + channel * 8;
        source[table..table + 4]
            .copy_from_slice(&(u32::try_from(channel * 2).unwrap()).to_le_bytes());
        source[table + 4..table + 8]
            .copy_from_slice(&(u32::try_from(channel * 2 + 1).unwrap()).to_le_bytes());
        source[0x54 + channel * 2] = 0;
    }
    source[0x55] = 10;
    source[0x57] = 20;
    source[0x59] = 30;
    source[0x5b] = 40;
    encrypt_pb3_prefix_for_test(&mut source);

    let image = decode_pb3_type5(&source).expect("PB3 type-5 fixture should decode");
    assert_eq!(image.into_raw(), vec![30, 20, 10, 40]);
}

#[test]
fn decodes_pb3_type1_constant_blocks() {
    let mut source = vec![0_u8; 256];
    source[..4].copy_from_slice(PB3_MAGIC);
    let source_size = u32::try_from(source.len()).unwrap();
    source[4..8].copy_from_slice(&source_size.to_le_bytes());
    source[0x1c..0x1e].copy_from_slice(&1_u16.to_le_bytes());
    source[0x1e..0x20].copy_from_slice(&1_u16.to_le_bytes());
    source[0x20..0x22].copy_from_slice(&1_u16.to_le_bytes());
    source[0x22..0x24].copy_from_slice(&32_u16.to_le_bytes());
    source[0x2c..0x30].copy_from_slice(&84_u32.to_le_bytes());
    source[0x30..0x34].copy_from_slice(&200_u32.to_le_bytes());
    for channel in 0..4 {
        source[84 + channel * 4..88 + channel * 4].copy_from_slice(&14_u32.to_le_bytes());
        let header = 100 + channel * 14;
        source[header..header + 4].copy_from_slice(&1_u32.to_le_bytes());
        source[header + 12] = 0x80;
        source[header + 13] = [10, 20, 30, 40][channel];
    }
    encrypt_pb3_prefix_for_test(&mut source);

    let image = decode_pb3_type1(&source).expect("PB3 type-1 fixture should decode");
    assert_eq!(image.into_raw(), vec![30, 20, 10, 40]);
}

#[test]
fn decodes_pb3_type6_base_overlay() {
    struct Base;
    impl PbImageResolver for Base {
        fn resolve_pb3_base(&self, reference: &str) -> Result<PbDecodedImage, CoreError> {
            if reference != "base.pb3" {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_PB3_BASE_REFERENCE",
                    "fixture base mismatch",
                ));
            }
            RgbaImage::from_raw(1, 1, vec![1, 2, 3, 4]).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB3_PIXELS",
                    "fixture image construction failed",
                )
            })
        }
    }

    let mut source = vec![0_u8; 256];
    source[..4].copy_from_slice(PB3_MAGIC);
    source[4..8].copy_from_slice(&256_u32.to_le_bytes());
    source[0x0c..0x10].copy_from_slice(&100_u32.to_le_bytes());
    source[0x18..0x1c].copy_from_slice(&13_u32.to_le_bytes());
    source[0x1c..0x1e].copy_from_slice(&6_u16.to_le_bytes());
    source[0x1e..0x20].copy_from_slice(&1_u16.to_le_bytes());
    source[0x20..0x22].copy_from_slice(&1_u16.to_le_bytes());
    source[0x22..0x24].copy_from_slice(&32_u16.to_le_bytes());
    source[0x2c..0x30].copy_from_slice(&18_u32.to_le_bytes());
    const KEY: [u8; 16] = [
        0xa6, 0x75, 0xf3, 0x9c, 0xc5, 0x69, 0x78, 0xa3, 0x3e, 0xa5, 0x4f, 0x79, 0x59, 0xfe, 0x3a,
        0xc7,
    ];
    for (index, byte) in b"base\0".iter().enumerate() {
        source[0x34 + index] = *byte ^ KEY[index];
    }
    source[132] = 0;
    source[133] = 0;
    source[150..163].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 10, 20, 30, 40]);
    encrypt_pb3_prefix_for_test(&mut source);

    let image = decode_pb3_type6(&source, &Base).expect("PB3 type-6 fixture should decode");
    assert_eq!(image.into_raw(), vec![30, 20, 10, 40]);
}

#[test]
fn decodes_pb2_type1_planar_blocks() {
    let mut source = pb2_fixture(1, 1, 1, 24, 0x20, 0x21, 0x3e);
    source[0x20] = 0;
    source[0x21..0x24].copy_from_slice(&[1, 2, 0]);
    encrypt_pb2_header(&mut source);

    let image = decode_pb2_type1(&source).expect("PB2 type-1 fixture should decode");
    assert_eq!(image.as_raw(), &[0, 2, 1, 0xff]);
}

#[test]
fn decodes_pb2_type2_constant_blocks() {
    let mut source = pb2_fixture(2, 1, 1, 24, 0x20, 0x50, 0x100);
    source[0x38] = 0x80;
    encrypt_pb2_header(&mut source);

    let image = decode_pb2_type2(&source).expect("PB2 type-2 fixture should decode");
    assert_eq!(image.as_raw(), &[0x80, 0x80, 0x80, 0xff]);
}

#[test]
fn decodes_pb2_type6_xor_channels() {
    let mut source = pb2_fixture(6, 1, 1, 32, 0, 0, 0x60);
    encrypt_pb2_header(&mut source);

    let image = decode_pb2_type6(&source).expect("PB2 type-6 fixture should decode");
    assert_eq!(image.as_raw(), &[0, 0, 0, 0]);
}

#[test]
fn rejects_pb3_payload_larger_than_source() {
    let mut source = vec![0_u8; PB3_DECRYPT_BYTES + PB3_TAIL_KEY_BYTES];
    source[..4].copy_from_slice(PB3_MAGIC);
    source[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    source[0x1c..0x1e].copy_from_slice(&1_u16.to_le_bytes());
    source[0x1e..0x20].copy_from_slice(&1_u16.to_le_bytes());
    source[0x20..0x22].copy_from_slice(&1_u16.to_le_bytes());
    source[0x22..0x24].copy_from_slice(&24_u16.to_le_bytes());
    assert_eq!(
        parse_pb3_metadata(&source).unwrap_err().code(),
        "ASTRA_EMU_CMVS_PB3_INPUT_SIZE"
    );
}

fn encrypt_pb3_prefix_for_test(source: &mut [u8]) {
    let tail_offset = source.len() - PB3_TAIL_KEY_BYTES;
    let tail = source[tail_offset..].to_vec();
    for (offset, key_byte) in (8..PB3_DECRYPT_BYTES).zip(&tail) {
        source[offset] = source[offset].wrapping_add(*key_byte);
    }
    let key_offset = source.len() - 3;
    let key = u16::from_le_bytes(source[key_offset..key_offset + 2].try_into().unwrap());
    for offset in (8..PB3_DECRYPT_BYTES).step_by(2) {
        let word = u16::from_le_bytes(source[offset..offset + 2].try_into().unwrap()) ^ key;
        source[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
    }
}

fn pb2_fixture(
    image_type: u16,
    width: u16,
    height: u16,
    bits_per_pixel: u16,
    offset_1: u32,
    offset_2: u32,
    length: usize,
) -> Vec<u8> {
    let mut source = vec![0_u8; length];
    source[..4].copy_from_slice(PB2_MAGIC);
    source[4..8].copy_from_slice(&(u32::try_from(length).unwrap()).to_le_bytes());
    source[0x10..0x12].copy_from_slice(&image_type.to_le_bytes());
    source[0x12..0x14].copy_from_slice(&width.to_le_bytes());
    source[0x14..0x16].copy_from_slice(&height.to_le_bytes());
    source[0x16..0x18].copy_from_slice(&bits_per_pixel.to_le_bytes());
    source[0x18..0x1c].copy_from_slice(&offset_1.to_le_bytes());
    source[0x1c..0x20].copy_from_slice(&offset_2.to_le_bytes());
    source
}

fn encrypt_pb2_header(source: &mut [u8]) {
    let key_offset = source.len() - PB2_KEY_BYTES;
    let key = source[key_offset..].to_vec();
    for index in (8..PB2_HEADER_BYTES).step_by(2) {
        source[index] = source[index].wrapping_add(key[index - 8]);
        source[index] ^= key[24];
        source[index + 1] = source[index + 1].wrapping_add(key[index - 7]);
        source[index + 1] ^= key[25];
    }
}

#[test]
fn decodes_literal_pb_lzss_bytes() {
    let source = [0_u8, b'a', b'b', b'c'];
    assert_eq!(decode_pb_lzss(&source, 0, 1, 3).unwrap(), b"abc");
}

#[test]
fn rejects_pb_lzss_back_reference_past_output() {
    let source = [0x80_u8, 0, 0];
    assert_eq!(
        decode_pb_lzss(&source, 0, 1, 1).unwrap_err().code(),
        "ASTRA_EMU_CMVS_PB_LZSS_OUTPUT"
    );
}
