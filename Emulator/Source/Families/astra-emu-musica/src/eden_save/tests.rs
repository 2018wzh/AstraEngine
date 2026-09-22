use super::*;
pub(super) fn fixture() -> EdenSave {
    let f = |name: &str, value: &str| EdenSaveField {
        name: name.into(),
        value: value.into(),
    };
    EdenSave {
        edition: EdenEdition::English,
        encoding: EdenSaveEncoding::Gbk,
        comment: "测试".into(),
        route: [0, 1, 2, 3],
        variables: vec![
            f("script_Filename", "fixture.sc"),
            f("script_Pointer", "12"),
        ],
        backlog: vec![vec![
            f("LC", "2"),
            f("L1_0", "voice.ogg"),
            f("L3_0", "测试正文"),
            f("L3_1", ""),
        ]],
    }
}
#[test]
fn preserves_order_and_multilingual_backlog_without_default_fields() {
    let save = fixture();
    let bytes = save.encode().unwrap();
    let read = EdenSave::decode(&bytes, save.edition, save.encoding).unwrap();
    assert!(read == save);
    assert_eq!(read.variable("script_Pointer"), Some("12"));
}
#[test]
fn rejects_wrong_edition_encoding_and_unrepresentable_export() {
    let mut save = fixture();
    let bytes = save.encode().unwrap();
    assert!(EdenSave::decode(&bytes, EdenEdition::Japanese, save.encoding).is_err());
    save.comment = "💡".into();
    assert!(save.encode().is_err());
    save.comment = "ok".into();
    save.variables.push(save.variables[0].clone());
    assert!(save.encode().is_err());
}
#[test]
fn rejects_truncation_trailing_data_and_corruption() {
    let save = fixture();
    let bytes = save.encode().unwrap();
    for cut in 0..bytes.len() {
        assert!(
            EdenSave::decode(&bytes[..cut], save.edition, save.encoding).is_err(),
            "accepted truncation at {cut}"
        );
    }
    let mut corrupt = bytes.clone();
    corrupt.push(0);
    assert!(EdenSave::decode(&corrupt, save.edition, save.encoding).is_err());
    let last = corrupt.len() - 2;
    corrupt[last] ^= 1;
    assert!(EdenSave::decode(&corrupt, save.edition, save.encoding).is_err());
}
#[test]
fn rejects_delimiter_injection_and_incomplete_sections() {
    let mut save = fixture();
    save.variables[0].value = "a\n!extra\tx".into();
    assert!(save.encode().is_err());
    for body in [
        "<begin variables>\n!x\ty\n",
        "<begin variables>\n!x\ty\n<end variables>\n<<begin backlog>>\nL0\t1\n<<end backlog>>\n",
    ] {
        assert!(body::parse(body).is_err());
    }
}
#[test]
fn bounds_inflated_data() {
    let save = fixture();
    let mut bytes = save.edition.magic().to_vec();
    bytes.extend_from_slice(save.edition.signature());
    bytes.extend_from_slice(&[0; 6]);
    let mut z = ZlibEncoder::new(bytes, Compression::default());
    z.write_all(&vec![b'x'; MAX_BODY + 1]).unwrap();
    assert_eq!(
        EdenSave::decode(&z.finish().unwrap(), save.edition, save.encoding)
            .err()
            .unwrap()
            .code(),
        "ASTRA_EMU_EDEN_SAVE_BOUND"
    );
}

#[test]
fn edition_magic_and_explicit_encoding_are_independent() {
    let mut save = fixture();
    save.edition = EdenEdition::Japanese;
    save.encoding = EdenSaveEncoding::ShiftJis;
    save.comment = "test".into();
    save.backlog.clear();
    let bytes = save.encode().unwrap();
    assert_eq!(&bytes[..4], b";\n!\xaa");
    assert!(EdenSave::decode(&bytes, save.edition, save.encoding).unwrap() == save);
    save.edition = EdenEdition::English;
    save.encoding = EdenSaveEncoding::Windows1252;
    save.comment = "€".into();
    let bytes = save.encode().unwrap();
    assert_eq!(&bytes[..4], &[0; 4]);
    assert!(EdenSave::decode(&bytes, save.edition, save.encoding).unwrap() == save);
}
