use super::*;
use astra_media::{BlendMode, RectI, SceneCommand, TextureFrame};
use astra_platform::SceneFrame;
use tokio::sync::oneshot;

#[test]
#[ignore = "requires explicit local video fixture, expected frame count, GStreamer plugins and hardware GPU"]
fn complete_local_movie_streams_through_worker_and_gpu() {
    let input = std::env::var_os("ASTRA_LINUX_VIDEO_TEST_FILE").expect("set a local video fixture");
    let expected_frames: u64 = std::env::var("ASTRA_LINUX_VIDEO_TEST_FRAMES")
        .unwrap()
        .parse()
        .unwrap();
    let expected_duration: u64 = std::env::var("ASTRA_LINUX_VIDEO_TEST_DURATION_US")
        .unwrap()
        .parse()
        .unwrap();
    let codec = std::env::var("ASTRA_LINUX_VIDEO_TEST_CODEC").unwrap();
    let bytes = std::fs::read(input).unwrap();
    let artifact_dir =
        std::env::var_os("ASTRA_LINUX_VIDEO_TEST_ARTIFACT_DIR").map(std::path::PathBuf::from);
    if let Some(dir) = artifact_dir.as_ref() {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut renderer = pollster::block_on(astra_platform_common::WgpuOffscreenRenderer::new(
        &astra_platform_common::native_wgpu_instance().unwrap(),
    ))
    .unwrap();
    assert_ne!(renderer.identity().device_type, "cpu");
    let session = DecodeSession::new(DecodeKind::Video).unwrap();
    let mut request = video::start(bytes);
    request.codec = codec;
    let (reply, result) = oneshot::channel();
    session.submit(request, reply);
    assert!(
        matches!(result.blocking_recv().unwrap().unwrap(), DecodeOutput::VideoStreamStart {
        duration_estimate_us: Some(duration), .. } if duration > 0)
    );
    let mut total_bytes = 0;
    let mut next_pts = 0;
    let mut last_pixels = None;
    let mut changed = false;
    for index in 0..expected_frames {
        let (reply, result) = oneshot::channel();
        session.submit(video::next(index + 2), reply);
        let DecodeOutput::VideoFrame {
            sequence,
            pts_us,
            duration_us,
            width,
            height,
            bgra8,
        } = result.blocking_recv().unwrap().unwrap()
        else {
            panic!("premature end");
        };
        assert_eq!(sequence, index + 1);
        assert_eq!(pts_us, next_pts);
        next_pts = pts_us + duration_us;
        total_bytes += bgra8.len() as u64;
        let mut rgba = bgra8.to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let capture = renderer
            .render(&SceneFrame {
                sequence,
                width,
                height,
                clear_rgba: [0, 0, 0, 255],
                commands: vec![SceneCommand::Texture {
                    id: "streamed-video".into(),
                    frame: TextureFrame::from_vec(width, height, rgba.clone()).unwrap(),
                    destination: RectI::new(0, 0, width, height),
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                }],
                semantics: None,
            })
            .unwrap();
        assert_eq!(capture.rgba8.as_ref(), rgba.as_slice());
        if [0, expected_frames / 2, expected_frames - 1].contains(&index) {
            if let Some(dir) = artifact_dir.as_ref() {
                std::fs::write(
                    dir.join(format!("frame-{index}-{width}x{height}.rgba")),
                    capture.rgba8.as_ref(),
                )
                .unwrap();
            }
        }
        if last_pixels
            .as_ref()
            .is_some_and(|previous| previous != &capture.rgba8)
        {
            changed = true;
        }
        last_pixels = Some(capture.rgba8);
    }
    assert_eq!(next_pts, expected_duration);
    assert!(changed, "fixture must contain changing frames");
    let (reply, result) = oneshot::channel();
    session.submit(video::next(expected_frames + 2), reply);
    assert_eq!(
        result.blocking_recv().unwrap().unwrap(),
        DecodeOutput::VideoStreamEnd {
            frame_count: expected_frames,
            decoded_byte_count: total_bytes
        }
    );
    session.close().unwrap();
}
