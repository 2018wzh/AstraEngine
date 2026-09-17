use super::*;

#[test]
fn manager_voice_preferences_keep_history_and_authored_voice_wait() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1 aya-missing.ogg speaker First\r\n.message 2 aya-tone.ogg speaker Second\\v\r\n.end\r\n");
    fixture::asset(root.path(), "voice", "aya-tone.ogg", &fixture::wave());
    let sink = Sink::default();
    let mut provider = MusicaProvider::default();
    let mut req = request(root.path(), sink.clone());
    req.configuration.push(ConfigEntry {
        id: "voice_aya".into(),
        value: ConfigValue::Bool(false),
    });
    let mut opened = provider.open(req).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened
        .session
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let saved = crate::storage::Storage::new(root.path())
        .unwrap()
        .read(0)
        .unwrap();
    let state = MusicaVm::decode_native_save(&saved.vm).unwrap();
    assert_eq!(state.backlog.len(), 2);
    assert_eq!(
        state.backlog[0].voice.as_ref().unwrap().resource_uri,
        "musica:/voice/aya-missing.ogg"
    );
    assert!(matches!(
        state.wait,
        Some(crate::MusicaWaitState::Voice {
            milliseconds: Some(100),
            ..
        })
    ));
    assert!(!state.audio[&4].playing);
    opened
        .session
        .advance(
            0,
            &[
                key(KeyCode::F9),
                key(KeyCode::PageUp),
                key(KeyCode::ArrowUp),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    opened.session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert_eq!(
        opened.session.advance(50_000_000, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    assert!(!sink.nonzero.load(Ordering::Acquire));
    assert_eq!(
        opened.session.advance(100_000_000, &[]).unwrap().status,
        FamilyStatus::Finished
    );
    opened.session.close().unwrap();
}

#[test]
fn voice_preferences_are_typed_and_resolved_from_descriptor() {
    let descriptor = crate::musica_descriptor();
    descriptor.validate().unwrap();
    let defaults = resolve_config(&descriptor.configuration, &[]).unwrap();
    let preferences = crate::voice_preferences::VoicePreferences::resolve(&defaults).unwrap();
    assert!(preferences.backlog_voice_playback);
    for prefix in ["ren", "sui", "aya", "tou", "mot", "other"] {
        assert!(preferences.enabled(&format!("musica:/voice/{prefix}-test.ogg")));
    }
    assert!(resolve_config(
        &descriptor.configuration,
        &[ConfigEntry {
            id: "voice_aya".into(),
            value: ConfigValue::String("false".into())
        }]
    )
    .is_err());
}

#[test]
fn current_manager_preferences_override_saved_voice_playback() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1 aya-tone.ogg speaker First\r\n.end\r\n",
    );
    fixture::asset(root.path(), "voice", "aya-tone.ogg", &fixture::wave());
    let mut provider = MusicaProvider::default();
    let mut initial = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    initial
        .session
        .advance(16_666_667, &[key(KeyCode::F5)])
        .unwrap();
    initial.session.close().unwrap();
    let sink = Sink::default();
    let mut req = request(root.path(), sink.clone());
    req.configuration.push(ConfigEntry {
        id: "voice_aya".into(),
        value: ConfigValue::Bool(false),
    });
    let mut loaded = provider.open(req).unwrap();
    loaded.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    loaded.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let saved = crate::storage::Storage::new(root.path())
        .unwrap()
        .read(0)
        .unwrap();
    assert!(
        !saved
            .sounds
            .iter()
            .find(|sound| sound.id == 4)
            .unwrap()
            .playing
    );
    loaded
        .session
        .advance(0, &[key(KeyCode::PageUp), key(KeyCode::Enter)])
        .unwrap();
    assert!(!sink.nonzero.load(Ordering::Acquire));
    loaded.session.close().unwrap();
}

#[test]
fn backlog_voice_preference_does_not_remove_history() {
    let source = b".message 1 aya-tone.ogg speaker First\r\n.end\r\n";
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        astra_core::Hash256::from_sha256(source),
        crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    vm.set_voice_preferences(crate::voice_preferences::VoicePreferences {
        backlog_voice_playback: false,
        ..Default::default()
    });
    vm.step(1).unwrap();
    vm.open_backlog().unwrap();
    let commands = vm.replay_backlog_voice().unwrap();
    assert!(!commands.iter().any(|command| matches!(
        command,
        crate::MusicaAudioCommand::Play { .. } | crate::MusicaAudioCommand::LoadResource { .. }
    )));
    assert_eq!(vm.state().backlog.len(), 1);
    assert!(vm.state().backlog[0].voice.is_some());
}

#[test]
fn manager_bus_mute_is_applied_before_first_pcm_and_survives_restore() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1 aya-tone.ogg speaker Voice\\v\r\n.end\r\n",
    );
    fixture::asset(root.path(), "voice", "aya-tone.ogg", &fixture::wave());
    let sink = Sink::default();
    let mut req = request(root.path(), sink.clone());
    req.configuration.push(ConfigEntry {
        id: "voice_muted".into(),
        value: ConfigValue::Bool(true),
    });
    let mut provider = MusicaProvider::default();
    let mut opened = provider.open(req).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let saved = crate::storage::Storage::new(root.path())
        .unwrap()
        .read(0)
        .unwrap();
    assert_eq!(saved.sounds.iter().find(|s| s.id == 4).unwrap().volume, 1.0);
    let state = MusicaVm::decode_native_save(&saved.vm).unwrap();
    assert_eq!(state.audio[&4].volume_milli, 1000);
    assert!(matches!(
        state.wait,
        Some(crate::MusicaWaitState::Voice {
            milliseconds: Some(100),
            ..
        })
    ));
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    assert!(!sink.nonzero.load(Ordering::Acquire));
    opened.session.close().unwrap();
}
