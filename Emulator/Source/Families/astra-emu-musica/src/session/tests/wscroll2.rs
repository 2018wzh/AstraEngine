use super::*;

#[test]
fn native_gpu_wscroll2_family_input_restores_and_suspends_the_animation() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".stage BG.png BG.png 0 0\r\n.effect WScroll2 sync:walk.txt 60 -8\r\n.wait 1000\r\n.effect end\r\n.end\r\n");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_fn(1280, 720, |x, _| {
        image::Rgba([(x % 251) as u8, 30, 90, 255])
    })
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    fixture::asset(root.path(), "bg", "BG.png", png.get_ref());
    fixture::asset(root.path(), "st", "walk.txt", b"13\r\n16\r\n");
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut frame = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        opened.session.advance(40_000_000, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    opened.session.visit_frame(&mut frame).unwrap();
    let saved = frame.0.clone();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    let later = frame.0.clone();
    assert_ne!(later, saved);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, saved);
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, later);
    opened
        .session
        .advance(0, &[FamilyEvent::WindowSuspended { suspended: true }])
        .unwrap();
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_eq!(frame.0, later);
    opened
        .session
        .advance(0, &[FamilyEvent::WindowSuspended { suspended: false }])
        .unwrap();
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut frame).unwrap();
    assert_ne!(frame.0, later);
    opened.session.close().unwrap();
}
