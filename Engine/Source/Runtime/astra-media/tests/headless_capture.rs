use astra_media::{
    DrawCommand, HeadlessRendererProvider, RenderTargetFormat, Renderer2DProvider,
    RendererCreateRequest,
};

#[test]
fn headless_capture_hash_is_repeatable_and_descriptor_is_gateable() {
    let provider = HeadlessRendererProvider;
    let descriptor = provider.descriptor();
    assert!(descriptor.headless);
    assert!(descriptor.packaged_eligible);

    let request = RendererCreateRequest {
        width: 64,
        height: 32,
        format: RenderTargetFormat::Rgba8Srgb,
        profile: "desktop-release".to_string(),
    };
    let mut a = provider.create(request.clone()).unwrap();
    let mut b = provider.create(request).unwrap();
    let commands = vec![
        DrawCommand::clear([0, 0, 0, 255]),
        DrawCommand::rect("hero", 4, 4, 12, 8, [255, 64, 32, 255]),
    ];
    let first = a.capture_frame(&commands).unwrap();
    let second = b.capture_frame(&commands).unwrap();
    assert_eq!(first.hash, second.hash);
    assert_eq!(first.bytes.len(), 64 * 32 * 4);
    let hero_pixel = ((4 * 64 + 4) * 4) as usize;
    assert_eq!(
        &first.bytes[hero_pixel..hero_pixel + 4],
        &[255, 64, 32, 255]
    );
}
