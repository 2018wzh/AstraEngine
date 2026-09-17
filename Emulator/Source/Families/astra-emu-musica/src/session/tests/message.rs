use super::*;

#[test]
fn native_gpu_message_inline_load_restores_mid_crossfade() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".char load 1 A.png\r\n.char pos 1 16 0\r\n.message 1   Body\\x{load,30,1,B.png,100,255}\r\n.end\r\n");
    let png = |color| {
        let mut data = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
            .write_to(&mut data, image::ImageFormat::Png)
            .unwrap();
        data.into_inner()
    };
    let red = png([255, 0, 0, 255]);
    let green = png([0, 255, 0, 255]);
    fixture::assets(root.path(), "st", &[("A.png", &red), ("B.png", &green)]);
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut capture = Capture(Vec::new());
    for _ in 0..3 {
        opened.session.advance(16_666_667, &[]).unwrap();
    }
    opened.session.advance(30_000_000, &[]).unwrap();
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let saved = capture.0.clone();
    let index = (704 * 1280 + 8) * 4;
    assert!(saved[index] > 0 && saved[index + 1] > 0);
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let later = capture.0.clone();
    assert_ne!(later, saved);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, saved);
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, later);
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[key(KeyCode::Enter)])
            .unwrap()
            .status,
        FamilyStatus::Finished
    );
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_message_voice_wait_queries_existing_decoded_audio() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1 tone.ogg  Body\\v\r\n.end\r\n");
    fixture::asset(root.path(), "voice", "tone.ogg", &fixture::wave());
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    assert_eq!(
        opened.session.advance(16_666_667, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    let mut finished = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if opened.session.advance(16_666_667, &[]).unwrap().status == FamilyStatus::Finished {
            finished = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        finished,
        "voice duration response did not release message wait"
    );
    opened.session.close().unwrap();
}
