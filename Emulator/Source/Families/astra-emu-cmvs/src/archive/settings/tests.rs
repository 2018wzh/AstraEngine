use super::*;

#[test]
fn caption_charset_reads_native_initial_config_default_and_boolean() {
    let root = tempfile::tempdir().unwrap();
    assert!(font_shift_jis(root.path()).unwrap());
    for (value, expected) in [(0, false), (1, true), (-1, true), (2, true)] {
        std::fs::write(
            root.path().join("initial.cfg"),
            format!("[CMVS_INIT_CFG]\nFONT_SHIFT_JIS={value}\n"),
        )
        .unwrap();
        assert_eq!(font_shift_jis(root.path()).unwrap(), expected);
    }
}

#[derive(Default)]
struct Values {
    mode: Option<u32>,
    font: Option<String>,
}
impl Registry for Values {
    fn number(&self, key: &str, value: &str) -> Result<Option<u32>, CoreError> {
        assert_eq!((key, value), ("Fixture", "ConfigPut"));
        Ok(self.mode)
    }
    fn string(&self, key: &str, value: &str) -> Result<Option<String>, CoreError> {
        assert_eq!((key, value), ("Fixture\\param", "MI_Font"));
        Ok(self.font.clone())
    }
}

fn initial(root: &Path, mode: i32, save: u32) {
    std::fs::write(
        root.join("initial.cfg"),
        format!("[CMVS_INIT_CFG]\nREG=Fixture\nCONFIG_PUT={mode}\nSAVE_FOLDER={save}\n"),
    )
    .unwrap();
}

#[test]
fn config_defaults_and_registry_override_follow_native_selection() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        DEFAULT_FONT
    );
    std::fs::write(
        root.path().join("cmvs.cfg"),
        b"[MESSAGE_INFO]\nFONT=\"Local Font\"\n",
    )
    .unwrap();
    for mode in [0, 2, 4, -4] {
        initial(root.path(), mode, 0);
        assert_eq!(
            resolve_font(
                root.path(),
                &Values {
                    mode: None,
                    font: Some("Registry Font".into())
                }
            )
            .unwrap(),
            "Local Font"
        );
    }
    initial(root.path(), 1, 0);
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        "Local Font"
    );
    assert_eq!(
        resolve_font(
            root.path(),
            &Values {
                mode: None,
                font: Some("Registry Font".into())
            }
        )
        .unwrap(),
        "Registry Font"
    );
    assert_eq!(
        resolve_font(
            root.path(),
            &Values {
                mode: None,
                font: Some(String::new())
            }
        )
        .unwrap(),
        DEFAULT_FONT
    );
    initial(root.path(), 3, 0);
    assert_eq!(
        resolve_font(
            root.path(),
            &Values {
                mode: Some(1),
                font: Some("Selected Font".into())
            }
        )
        .unwrap(),
        "Selected Font"
    );
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        "Local Font"
    );
}

#[test]
fn cp932_font_name_and_utf16_ini_are_decoded_without_substitution() {
    let root = tempfile::tempdir().unwrap();
    let text = format!("[message_info]\nfont={DEFAULT_FONT}\n");
    let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&text);
    assert!(!invalid);
    std::fs::write(root.path().join("cmvs.cfg"), &bytes).unwrap();
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        DEFAULT_FONT
    );
    let bytes = [0xff, 0xfe]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
    std::fs::write(root.path().join("cmvs.cfg"), bytes).unwrap();
    assert_eq!(
        resolve_font(root.path(), &Values::default()).unwrap(),
        DEFAULT_FONT
    );
}

#[test]
fn external_config_root_and_invalid_values_do_not_fall_back() {
    let root = tempfile::tempdir().unwrap();
    initial(root.path(), 2, 1);
    assert_eq!(
        resolve_font(root.path(), &Values::default())
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_CONFIG_ROOT_UNBOUND"
    );
    initial(root.path(), 0, 0);
    std::fs::write(root.path().join("cmvs.cfg"), b"[MESSAGE_INFO]\nFONT=\x81").unwrap();
    assert!(resolve_font(root.path(), &Values::default()).is_err());
    std::fs::write(
        root.path().join("cmvs.cfg"),
        format!("[MESSAGE_INFO]\nFONT={}\n", "x".repeat(64)),
    )
    .unwrap();
    assert!(resolve_font(root.path(), &Values::default()).is_err());
}
