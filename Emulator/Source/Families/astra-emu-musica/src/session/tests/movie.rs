use super::*;

#[test]
fn movie_control_skip_uses_movie_flag_independently_of_message_skip_preferences() {
    let _session = PROVIDER_SESSION.lock();
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../Engine/Fixtures/PublicDomainMedia/flower-roar.mp4"),
    )
    .unwrap();
    for skippable in [true, false] {
        let root = tempfile::tempdir().unwrap();
        fixture::game(
            root.path(),
            format!(
                ".pragma disable_control\r\n.movie 1 sample.mp4 1280 720 {}\r\n.end\r\n",
                if skippable { "t" } else { "f" }
            )
            .as_bytes(),
        );
        fixture::asset(root.path(), "mov", "sample.mp4", &bytes);
        let mut provider = fixture::provider();
        let mut opened = provider
            .open(request(root.path(), Sink::default()))
            .unwrap();
        opened.session.advance(16_666_667, &[]).unwrap();
        let result = opened
            .session
            .advance(16_666_667, &[key(KeyCode::ControlLeft)])
            .unwrap();
        assert_eq!(
            result.status,
            if skippable {
                FamilyStatus::Finished
            } else {
                FamilyStatus::Waiting
            }
        );
        opened.session.close().unwrap();
    }
}

#[test]
fn corrupt_movie_decode_fails_and_session_can_close() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".movie 1 broken.mp4 1280 720 t\r\n.end\r\n");
    fixture::asset(root.path(), "mov", "broken.mp4", b"not a movie");
    let mut provider = fixture::provider();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        if let Err(error) = opened.session.advance(16_666_667, &[]) {
            assert_eq!(error.code.as_str(), "ASTRA_EMU_VIDEO_DECODE");
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    opened.session.close().unwrap();
}

#[test]
fn movie_close_cancels_blocked_pcm_and_pending_decode_on_repeated_open() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".movie 1 sample.mp4 1280 720 t\r\n.end\r\n");
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../Engine/Fixtures/PublicDomainMedia/flower-roar.mp4"),
    )
    .unwrap();
    fixture::asset(root.path(), "mov", "sample.mp4", &bytes);
    for _ in 0..3 {
        let mut provider = fixture::provider();
        let sink = Sink {
            block: true,
            ..Default::default()
        };
        let mut opened = provider.open(request(root.path(), sink.clone())).unwrap();
        for _ in 0..20 {
            opened.session.advance(16_666_667, &[]).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        let start = Instant::now();
        opened.session.close().unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(sink.cancelled.load(Ordering::Acquire));
    }
}

#[test]
fn native_gpu_movie_plays_pcm_pauses_restores_and_completes() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".movie 1 sample.mp4 1280 720 t\r\n.end\r\n");
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../Engine/Fixtures/PublicDomainMedia/flower-roar.mp4"),
    )
    .unwrap();
    fixture::asset(root.path(), "mov", "sample.mp4", &bytes);
    std::fs::write(
        root.path().join("public.ttf"),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf"
        )),
    )
    .unwrap();
    std::fs::write(root.path().join("public.ass"), concat!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\nYCbCr Matrix: None\n",
        "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV\n",
        "Style: Default,Noto Sans JP Thin,24,&H000000FF,&H00000000,&H00000000,1,0,0,7,0,0,0\n",
        "[Events]\nFormat: Layer, Start, End, Style, Text\n",
        "Dialogue: 0,0:00:00.00,0:00:01.00,Default,{\\an7\\pos(10,10)\\p1}m 0 0 l 120 0 120 120 0 120\n",
        "Dialogue: 0,0:00:01.00,0:00:10.00,Default,{\\an7\\pos(10,10)\\c&H00FF00&\\p1}m 0 0 l 120 0 120 120 0 120\n",
    )).unwrap();
    std::fs::write(root.path().join("subtitles.json"), br#"{"version":1,"tracks":[{"clock":"movie","resource":"sample.mp4","ass":"public.ass","fonts":["public.ttf"]}]}"#).unwrap();
    let sink = Sink::default();
    let mut provider = fixture::provider();
    let mut req = request(root.path(), sink.clone());
    req.configuration.push(ConfigEntry {
        id: "subtitle_map".into(),
        value: ConfigValue::String("subtitles.json".into()),
    });
    let mut opened = provider.open(req).unwrap();
    let mut capture = Capture(Vec::new());
    let mut last = Vec::new();
    let mut changes = 0;
    let mut restored = false;
    let (mut seen_red, mut seen_green) = (false, false);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(Instant::now() < deadline, "movie did not complete");
        let result = opened.session.advance(16_666_667, &[]).unwrap();
        capture
            .read(opened.session.acquire_frame(None).unwrap())
            .unwrap();
        let pixel = &capture.0[(64 * 1280 + 64) * 4..(64 * 1280 + 64) * 4 + 3];
        seen_red |= pixel == [255, 0, 0];
        seen_green |= pixel == [0, 255, 0];
        if capture.0 != last {
            changes += 1;
            last.clone_from(&capture.0);
        }
        if changes == 10 && !restored {
            opened
                .session
                .advance(0, &[FamilyEvent::WindowSuspended { suspended: true }])
                .unwrap();
            opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
            for _ in 0..5 {
                opened.session.advance(16_666_667, &[]).unwrap();
            }
            capture
                .read(opened.session.acquire_frame(None).unwrap())
                .unwrap();
            assert_eq!(capture.0, last);
            opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
            opened
                .session
                .advance(0, &[FamilyEvent::WindowSuspended { suspended: false }])
                .unwrap();
            restored = true;
        }
        if result.status == FamilyStatus::Finished {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(restored && changes > 20);
    assert!(
        seen_red && seen_green,
        "native ASS content transitions must reach actual GPU pixels"
    );
    assert!(sink.nonzero.load(Ordering::Acquire));
    opened.session.close().unwrap();
    assert!(sink.cancelled.load(Ordering::Acquire));
}
