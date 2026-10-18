use super::*;
#[test]
fn rejects_video_without_fabricating_completion() {
    let events = HostEvents::new();
    events.set_enabled(true);
    events.push_media("video_play", "{}");
    assert!(drain(
        &events,
        &HostResources::new(),
        &mut WindowRequests::default()
    )
    .is_err());
    events.set_enabled(false);
}
#[test]
fn missing_audio_and_loop_resource_are_errors() {
    for payload in [
        r#"{"kind":"audio_bgm_play","payload":{"file":"missing.ogg"}}"#,
        r#"{"kind":"audio_set_volume","payload":{"channel":"unknown","value":1}}"#,
        r#"{"kind":"audio_bgm_pan","payload":{"pan":"bad"}}"#,
    ] {
        assert!(parse_media(payload.as_bytes(), &HostResources::new()).is_err());
    }
}
#[test]
fn script_gain_and_pan_use_native_thousandths() {
    assert_eq!(compat_gain(Some(1)), 0.001);
    assert_eq!(compat_gain(Some(1000)), 1.0);
    assert_eq!(compat_pan(Some(-1)), -0.001);
}
#[test]
fn native_caption_uses_the_host_window_title_contract() {
    let command = parse_ui(br#"{"kind":"caption","payload":{"data":"Native game"}}"#).unwrap();
    assert!(matches!(command, WindowCommand::Title(title) if title == "Native game"));
    assert!(parse_ui(br#"{"kind":"caption","payload":{"data":"bad\ncaption"}}"#).is_err());
    assert!(parse_ui(br#"{"kind":"caption","payload":{"data":42}}"#).is_err());
}
#[test]
fn unsupported_native_ui_still_fails_with_the_request_kind() {
    let error = parse_ui(br#"{"kind":"dialog_show","payload":{}}"#).unwrap_err();
    assert_eq!(error.code, "ASTRA_EMU_ARTEMIS_UI_UNSUPPORTED");
    assert!(error.to_string().contains("dialog_show"));
}

#[test]
fn native_mouse_preserves_nulls_and_pending_caption() {
    let mut pending = WindowRequests::default();
    pending
        .push(parse_ui(br#"{"kind":"caption","payload":{"data":"Native game"}}"#).unwrap())
        .unwrap();
    pending
        .push(
            parse_ui(
                br#"{"kind":"mouse","payload":{"left":12,"top":null,"hide":1,"autohide":1000}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    pending
        .push(
            parse_ui(
                br#"{"kind":"mouse","payload":{"left":null,"top":34,"hide":null,"autohide":0}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    assert!(matches!(pending.take(), Some(WindowCommand::Title(_))));
    assert!(matches!(
        pending.take(),
        Some(WindowCommand::Pointer {
            x: Some(12),
            y: None,
            visible: Some(false),
            autohide_ms: Some(1000)
        })
    ));
    assert!(matches!(
        pending.take(),
        Some(WindowCommand::Pointer {
            x: None,
            y: Some(34),
            visible: None,
            autohide_ms: Some(0)
        })
    ));
    assert!(pending.take().is_none());
    assert!(parse_ui(br#"{"kind":"mouse","payload":{"hide":"bad"}}"#).is_err());
}

#[test]
fn native_mouse_distinguishes_explicit_visibility_from_idle_policy() {
    for (payload, expected_visible, expected_autohide) in [
        (r#"{"hide":0}"#, Some(true), None),
        (r#"{"hide":1}"#, Some(false), None),
        (r#"{"hide":-1}"#, Some(false), None),
        (r#"{"autohide":1000}"#, None, Some(1000)),
        (r#"{"autohide":0}"#, None, Some(0)),
        (r#"{"hide":null,"autohide":null}"#, None, None),
    ] {
        let bytes = format!(r#"{{"kind":"mouse","payload":{payload}}}"#);
        let WindowCommand::Pointer {
            x,
            y,
            visible,
            autohide_ms,
        } = parse_ui(bytes.as_bytes()).unwrap()
        else {
            panic!("mouse configuration must remain a pointer command");
        };
        assert_eq!((x, y), (None, None));
        assert_eq!(visible, expected_visible);
        assert_eq!(autohide_ms, expected_autohide);
    }
}

#[test]
fn native_video_audio_keeps_an_open_source_after_temporary_cleanup() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("video.wav");
    std::fs::write(&path, b"native audio").unwrap();
    let payload = serde_json::json!({
        "kind": "video_audio_play",
        "payload": { "id": null, "path": path, "loop": true },
    });
    let commands = parse_media(
        &serde_json::to_vec(&payload).unwrap(),
        &HostResources::new(),
    )
    .unwrap();
    let MixerCommand::Play {
        id,
        channel,
        sources,
        loop_play,
        ..
    } = &commands[0]
    else {
        panic!("native video audio must enter the mixer");
    };
    assert!(id.is_none());
    assert_eq!(*channel, Channel::Video);
    assert!(*loop_play);
    assert!(!path.exists());
    let mut bytes = [0; 5];
    assert_eq!(sources.base.read_at(7, &mut bytes).unwrap(), 5);
    assert_eq!(&bytes, b"audio");
    let stop = parse_media(
        br#"{"kind":"video_audio_stop","payload":{"id":null}}"#,
        &HostResources::new(),
    )
    .unwrap();
    assert!(matches!(&stop[0], MixerCommand::StopVideo { id: None }));
    let all = parse_media(
        br#"{"kind":"video_stop_all","payload":{}}"#,
        &HostResources::new(),
    )
    .unwrap();
    assert!(matches!(all[0], MixerCommand::StopVideoAll));
}
