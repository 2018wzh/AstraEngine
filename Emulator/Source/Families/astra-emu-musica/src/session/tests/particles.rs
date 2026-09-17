use super::*;

#[test]
fn native_gpu_particle_family_save_restore_and_pause() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".effect Firefly glow 20 1000\r\n.effect2 Snow\r\n.wait 1000\r\n.effect end\r\n.effect2 fadeout\r\n.end\r\n");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(32, 32, image::Rgba([220, 230, 250, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let entries = [
        "glowS.png",
        "glowM.png",
        "glowL.png",
        "snowS.png",
        "snowM.png",
        "snowL.png",
    ]
    .map(|name| (name, png.get_ref().as_slice()));
    fixture::assets(root.path(), "sys", &entries);
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut capture = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(64_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let saved = capture.0.clone();
    assert!(saved
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[..3] != [0, 0, 0]));
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened.session.advance(64_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let later = capture.0.clone();
    assert_ne!(saved, later);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(saved, capture.0);
    opened.session.advance(64_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(later, capture.0);
    opened
        .session
        .advance(0, &[FamilyEvent::WindowSuspended { suspended: true }])
        .unwrap();
    opened.session.advance(64_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(later, capture.0);
    opened.session.close().unwrap();
}
