use super::*;

#[test]
fn translation_glyph_failure_poisoning_differs_from_service_failure() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1   Hello\r\n.end\r\n");
    let translation = Translation::default();
    let sink = Sink::default();
    let mut req = request(root.path(), sink.clone());
    req.host.text_replacement = ROption::RSome(TextReplacementService_TO::from_value(
        translation.clone(),
        TD_Opaque,
    ));
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_session(req).unwrap();
    session.advance(16_666_667, &[]).unwrap();

    translation.mode.store(2, Ordering::Release);
    session.advance(0, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Hello");
    assert!(!session.poisoned);

    session
        .message("Original after timeout".into(), None)
        .unwrap();
    translation.mode.store(0, Ordering::Release);
    session.pending.as_mut().unwrap().started = Instant::now() - Duration::from_secs(16);
    session.advance(0, &[]).unwrap();
    assert_eq!(
        session.message.as_ref().unwrap().0,
        "Original after timeout"
    );
    assert_eq!(translation.cancellations.load(Ordering::Acquire), 1);
    assert!(!session.poisoned);

    session
        .message("Original before glyph failure".into(), None)
        .unwrap();
    translation.mode.store(3, Ordering::Release);
    let failure = session.advance(0, &[]).unwrap_err();
    assert_eq!(failure.code(), "ASTRA_TEXT_GLYPH_MISSING");
    assert!(session.poisoned);
    assert_eq!(
        session.message.as_ref().unwrap().0,
        "Original before glyph failure"
    );
    let pc = session.vm.state().pc_line;
    assert_eq!(
        session.advance(0, &[key(KeyCode::F5)]).unwrap_err().code(),
        "ASTRA_EMU_MUSICA_POISONED"
    );
    assert_eq!(session.vm.state().pc_line, pc);
    assert!(session.storage.read(0).is_err());
    Box::new(session).close().unwrap();
    assert!(sink.cancelled.load(Ordering::Acquire));
}
