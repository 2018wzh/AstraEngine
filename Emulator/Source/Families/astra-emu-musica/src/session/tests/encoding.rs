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

#[test]
fn native_gpu_mixed_encoding_chain_keeps_primary_and_restores_another_encoding() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (first, _, _) = encoding_rs::SHIFT_JIS
        .encode("[e].message 1   Wrong\r\n[j].message 2   ｱ\r\n.chain next.sc\r\n");
    let (next, _, _) = encoding_rs::GBK
        .encode("[j].message 1   Wrong\r\n[e].message 2  姓名 中文剧情\r\n.chain tail.sc\r\n");
    let (tail, _, _) = encoding_rs::SHIFT_JIS.encode("[j].message 3   ｲ\r\n.chain ascii.sc\r\n");
    let ascii = b"[j].message 1   Wrong\r\n[e].message 2   Primary\r\n.end\r\n";
    fixture::game(root.path(), &first);
    fixture::assets(
        root.path(),
        "scr",
        &[
            ("test.sc", &first),
            ("next.sc", &next),
            ("tail.sc", &tail),
            ("ascii.sc", ascii),
        ],
    );
    let open = || {
        let mut req = request(root.path(), Sink::default());
        req.configuration.push(ConfigEntry {
            id: "script_encoding".into(),
            value: ConfigValue::Enum("gbk".into()),
        });
        MusicaProvider::default().open_session(req).unwrap().1
    };
    let mut session = open();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "ｱ");
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "中文剧情");
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let mut capture = Capture(Vec::new());
    session.visit_frame(&mut capture).unwrap();
    let chinese = capture.0.clone();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "ｲ");
    assert_eq!(
        session.vm.state().script_encoding,
        crate::ScriptEncoding::ShiftJis
    );
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    session.visit_frame(&mut capture).unwrap();
    assert!(
        capture.0 == chinese,
        "restore must rebuild the saved Chinese font binding"
    );
    Box::new(session).close().unwrap();
    let mut session = open();
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    session.visit_frame(&mut capture).unwrap();
    assert!(
        capture.0 == chinese,
        "cold reopen must load a different detected encoding"
    );
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Primary");
    assert_eq!(
        session.vm.state().script_encoding,
        crate::ScriptEncoding::Gbk
    );
    Box::new(session).close().unwrap();
}
