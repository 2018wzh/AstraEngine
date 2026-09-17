use super::*;

#[test]
fn native_gpu_gbk_script_guards_chain_choices_and_restore() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let first = b"[j].message 1   Wrong\r\n[e].chain next.sc\r\n.end\r\n";
    let (next, _, errors) = encoding_rs::GBK.encode(
        "[j].message 1   Wrong\r\n[e].message 2  姓名 中文剧情\r\n.select 继续:done\r\n.label done\r\n.end\r\n",
    );
    assert!(!errors);
    fixture::game(root.path(), first);
    fixture::assets(
        root.path(),
        "scr",
        &[("test.sc", first), ("next.sc", &next)],
    );
    let mut req = request(root.path(), Sink::default());
    req.configuration.push(ConfigEntry {
        id: "script_encoding".into(),
        value: ConfigValue::Enum("gbk".into()),
    });
    let (_, mut session) = MusicaProvider::default().open_session(req).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        session.message,
        Some(("中文剧情".into(), Some("姓名".into())))
    );
    let mut capture = Capture(Vec::new());
    session.visit_frame(&mut capture).unwrap();
    let dialogue = capture.0.clone();
    assert!(dialogue
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[..3] != [0, 0, 0]));
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    let (labels, _) = session.vm.choice_display().unwrap().unwrap();
    assert_eq!(labels, ["继续"]);
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    session.visit_frame(&mut capture).unwrap();
    assert!(
        capture.0 == dialogue,
        "GBK dialogue must survive native restore"
    );
    assert_eq!(
        session.message,
        Some(("中文剧情".into(), Some("姓名".into())))
    );
    Box::new(session).close().unwrap();
}
