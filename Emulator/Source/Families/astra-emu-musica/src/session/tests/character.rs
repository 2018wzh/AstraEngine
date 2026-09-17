use super::*;
#[test]
fn native_gpu_character_mirror_transition_wait_and_save_restore() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".char load -1 Stand.png\r\n.char pos 1 16 0\r\n.char trans 1 100 0\r\n.wait 1000\r\n.end\r\n");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_fn(16, 16, |x, _| {
        image::Rgba(if x < 8 {
            [255, 0, 0, 255]
        } else {
            [0, 255, 0, 255]
        })
    })
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    fixture::stand(root.path(), png.get_ref());
    let mut provider = MusicaProvider::default();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut capture = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let index = (704 * 1280 + 8) * 4;
    assert_eq!(&capture.0[index..index + 4], &[0, 255, 0, 255]);
    opened.session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        opened.session.advance(40_000_000, &[]).unwrap().status,
        FamilyStatus::Waiting
    );
    opened.session.visit_frame(&mut capture).unwrap();
    let saved = capture.0.clone();
    assert!(saved[index + 1] > 0 && saved[index + 1] < 255);
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    let later = capture.0.clone();
    assert!(later[index + 1] < saved[index + 1]);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, saved);
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(capture.0, later);
    opened.session.advance(40_000_000, &[]).unwrap();
    opened.session.visit_frame(&mut capture).unwrap();
    assert_eq!(&capture.0[index..index + 4], &[0, 0, 0, 255]);
    opened.session.close().unwrap();
}
