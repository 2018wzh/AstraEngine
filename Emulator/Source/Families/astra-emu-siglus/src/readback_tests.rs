use super::*;
use astra_emu_family_api::{FrameLease, FramePoll};
use siglus_scene_vm::{
    assets::RgbaImage,
    image_manager::ImageManager,
    layer::{RenderSprite, Sprite},
};

fn draw(
    renderer: &mut Renderer,
    images: &mut ImageManager,
    width: u32,
    height: u32,
    seed: u8,
) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&[seed, x as u8, y as u8, 255]);
        }
    }
    let image = images.insert_image(RgbaImage {
        width,
        height,
        center_x: 0,
        center_y: 0,
        rgba: rgba.clone(),
    });
    let sprite = RenderSprite::new(
        None,
        None,
        Sprite {
            image_id: Some(image),
            visible: true,
            ..Sprite::default()
        },
    );
    renderer.render_sprites(images, &[sprite]).unwrap();
    rgba
}

fn capture(frame: &mut FrameReadback, renderer: &mut Renderer) -> FrameLease {
    while frame.capture(renderer).unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(1));
    }
    let FramePoll::Ready(lease) = frame.published.poll(None) else {
        panic!("completed native frame")
    };
    lease
}

fn owner() -> FrameReadback {
    FrameReadback::new(
        FramePublisher::new(astra_emu_family_api::allocate_session_epoch().unwrap()).unwrap(),
    )
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn native_gpu_readback_preserves_pixels_across_padded_resize() {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(63, 47)).unwrap();
    eprintln!("Siglus adapter: {:?}", renderer.adapter.get_info());
    let mut images = ImageManager::new(std::path::PathBuf::new());
    let mut frame = owner();
    let mut previous_generation = 0;
    for (width, height, seed) in [(63, 47, 23), (65, 49, 199), (63, 47, 23)] {
        renderer.resize(width, height);
        let expected = draw(&mut renderer, &mut images, width, height, seed);
        let captured_generation = renderer.hosted_frame_generation();
        assert!(frame.capture(&mut renderer).unwrap().is_none());
        // A newer draw must not relabel the already queued copy.
        draw(&mut renderer, &mut images, width, height, seed + 1);
        let lease = capture(&mut frame, &mut renderer);
        assert_eq!((lease.info.width, lease.info.height), (width, height));
        assert_eq!(lease.pixels(), expected);
        assert_eq!(frame.generation, captured_generation);
        assert!(frame.generation > previous_generation);
        assert!(frame.generation < renderer.hosted_frame_generation());
        previous_generation = frame.generation;
    }
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn held_leases_apply_backpressure_and_reuse_only_released_storage() {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(63, 47)).unwrap();
    let mut images = ImageManager::new(std::path::PathBuf::new());
    let mut frame = owner();
    let mut leases = Vec::new();
    let mut expected = Vec::new();
    for seed in 1..=3 {
        expected.push(draw(&mut renderer, &mut images, 63, 47, seed));
        leases.push(Some(capture(&mut frame, &mut renderer)));
    }
    let pointers: Vec<_> = leases
        .iter()
        .map(|lease| lease.as_ref().unwrap().pixels().as_ptr())
        .collect();
    assert_ne!(pointers[0], pointers[1]);
    assert_ne!(pointers[1], pointers[2]);
    let latest = draw(&mut renderer, &mut images, 63, 47, 4);
    for _ in 0..10 {
        assert!(frame.capture(&mut renderer).unwrap().is_none());
        assert!(frame.reserved.is_none());
        assert!(frame.deadline.is_none());
    }
    for (lease, expected) in leases.iter().zip(&expected) {
        assert_eq!(lease.as_ref().unwrap().pixels(), expected);
    }
    leases[0].take();
    let replacement = capture(&mut frame, &mut renderer);
    assert_eq!(replacement.pixels(), latest);
    assert_eq!(replacement.pixels().as_ptr(), pointers[0]);
    for i in 1..3 {
        assert_eq!(leases[i].as_ref().unwrap().pixels(), expected[i]);
    }
    drop(replacement);
    drop(leases);
    let capacities: Vec<_> = frame
        .pixels
        .iter()
        .map(|slot| slot.as_ref().unwrap().capacity())
        .collect();
    for seed in 5..=25 {
        let expected = draw(&mut renderer, &mut images, 63, 47, seed);
        let lease = capture(&mut frame, &mut renderer);
        assert!(pointers.contains(&lease.pixels().as_ptr()));
        assert_eq!(lease.pixels(), expected);
    }
    assert_eq!(
        capacities,
        frame
            .pixels
            .iter()
            .map(|slot| slot.as_ref().unwrap().capacity())
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "requires a hardware GPU adapter"]
fn pending_resize_keeps_deadline_and_cancel_discards_old_completion() {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(63, 47)).unwrap();
    let mut images = ImageManager::new(std::path::PathBuf::new());
    let mut frame = owner();
    let original = draw(&mut renderer, &mut images, 63, 47, 1);
    let lease = capture(&mut frame, &mut renderer);
    draw(&mut renderer, &mut images, 63, 47, 2);
    assert!(frame.capture(&mut renderer).unwrap().is_none());
    let deadline = frame.deadline;
    renderer.resize(65, 49);
    let resized_pixels = draw(&mut renderer, &mut images, 65, 49, 3);
    assert!(frame.capture(&mut renderer).unwrap().is_none());
    assert_eq!(frame.deadline, deadline);
    let resized = capture(&mut frame, &mut renderer);
    assert_eq!((resized.info.width, resized.info.height), (65, 49));
    assert_eq!(resized.pixels(), resized_pixels);
    assert_eq!(lease.pixels(), original);
    let identity = resized.identity;
    draw(&mut renderer, &mut images, 65, 49, 4);
    assert!(frame.capture(&mut renderer).unwrap().is_none());
    frame.deadline = Some(Instant::now() - Duration::from_secs(1));
    let failure = frame.capture(&mut renderer).unwrap_err();
    assert_eq!(failure.code(), "ASTRA_EMU_SIGLUS_READBACK_TIMEOUT");
    assert!(frame.reserved.is_none());
    assert!(matches!(
        frame.published.poll(Some(identity)),
        FramePoll::Unchanged(_)
    ));
    // Reset/close cancellation invalidates a submitted map before any poll.
    assert!(frame.capture(&mut renderer).unwrap().is_none());
    frame.cancel(&mut renderer);
    assert!(frame.deadline.is_none());
    let expected = draw(&mut renderer, &mut images, 65, 49, 5);
    let recovered = capture(&mut frame, &mut renderer);
    assert_eq!(recovered.pixels(), expected);
    assert_eq!(resized.pixels(), resized_pixels);
    assert!(frame.capture(&mut renderer).unwrap().is_none());
    frame.cancel(&mut renderer);
    drop(renderer);
}
