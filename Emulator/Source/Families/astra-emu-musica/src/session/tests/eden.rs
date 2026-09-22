use super::*;
use crate::eden_save::{EdenEdition, EdenHistoryMessage, EdenSave, EdenSaveEncoding};

#[test]
fn native_gpu_eden_import_continues_saves_and_exports_authoritative_message() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let script = b".message 7  speaker saved\n.message 8  speaker continued\n.end\n";
    fixture::game(root.path(), script);
    let mut panel = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 20, 20, 255]))
        .write_to(&mut panel, image::ImageFormat::Png)
        .unwrap();
    fixture::assets(root.path(), "sys", &[("msgPanel.png", panel.get_ref())]);
    let native = EdenSave::from_history(
        EdenEdition::English,
        EdenSaveEncoding::Gbk,
        &[EdenHistoryMessage {
            script: "test.sc".into(),
            next_line: 1,
            message_id: 7,
            voice: String::new(),
            speaker: "speaker".into(),
            text: "saved".into(),
            background: "BG.png".into(),
            background_position: [0, 0],
            panel_mode: 1,
            panel_resource: String::new(),
            bgm: String::new(),
            bgm_volume: 100,
            sound_effects: Default::default(),
            panel_fade: 0,
            transition_ticks: 30,
        }],
    )
    .unwrap()
    .encode()
    .unwrap();
    std::fs::write(root.path().join("native.sav"), &native).unwrap();
    let mut req = request(root.path(), Sink::default());
    req.configuration.extend([
        ConfigEntry {
            id: "logical_width".into(),
            value: ConfigValue::Integer(1024),
        },
        ConfigEntry {
            id: "logical_height".into(),
            value: ConfigValue::Integer(640),
        },
        ConfigEntry {
            id: "script_encoding".into(),
            value: ConfigValue::Enum("gbk".into()),
        },
        ConfigEntry {
            id: "eden_import_edition".into(),
            value: ConfigValue::Enum("english".into()),
        },
        ConfigEntry {
            id: "eden_import_file".into(),
            value: ConfigValue::String("native.sav".into()),
        },
    ]);
    let (_, mut session) = MusicaProvider::default().open_session(req).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "saved");
    let mut capture = Capture(Vec::new());
    session.visit_frame(&mut capture).unwrap();
    assert!(capture
        .0
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[..3] != [0, 0, 0]));
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "continued");
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let before = session.vm.encode_native_save().unwrap();
    let mut incompatible = session.storage.read(10).unwrap();
    incompatible.logical_extent = [1280, 720];
    session.storage.write(11, &incompatible).unwrap();
    let result = session.load(11).unwrap_err();
    assert_eq!(result.code(), "ASTRA_EMU_MUSICA_SAVE_CANVAS");
    assert_eq!(session.vm.encode_native_save().unwrap(), before);
    assert_eq!(session.message.as_ref().unwrap().0, "continued");
    assert!(crate::eden_save::export_slot(
        root.path(),
        std::path::Path::new(crate::MUSICA_PROFILE_FILE),
        11,
    )
    .is_err());
    Box::new(session).close().unwrap();
    let exported = crate::eden_save::export_slot(
        root.path(),
        std::path::Path::new(crate::MUSICA_PROFILE_FILE),
        10,
    )
    .unwrap();
    let thumbnail = image::load_from_memory(&exported.thumbnail_png).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (128, 80));
    let checkpoint = exported.save.checkpoint().unwrap();
    assert_eq!(checkpoint.message_id, 8);
    assert_eq!(checkpoint.history.len(), 2);
    assert_eq!(
        std::fs::read(root.path().join("native.sav")).unwrap(),
        native
    );
}
