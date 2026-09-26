use super::*;
use gstreamer::{prelude::*, ClockTime};

pub(super) const PTS: [u64; 4] = [0, 100_000, 350_000, 450_000];

// Real, synthetic VP8/WebM with variable PTS and a different solid color per frame.
// It is encoded through installed GStreamer plugins, never a commercial fixture.
pub(super) fn fixture() -> Vec<u8> {
    gstreamer::init().unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    let pipeline = gstreamer::parse::launch(
        "appsrc name=input format=time ! video/x-raw,format=RGB,width=32,height=24,framerate=10/1 ! videoconvert ! vp8enc deadline=1 ! webmmux ! filesink name=output"
    ).unwrap().downcast::<gstreamer::Pipeline>().unwrap();
    struct Stop(gstreamer::Pipeline);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.set_state(gstreamer::State::Null).unwrap();
        }
    }
    let _stop = Stop(pipeline.clone());
    pipeline
        .by_name("output")
        .unwrap()
        .set_property("location", file.path().to_str().unwrap());
    let source = pipeline
        .by_name("input")
        .unwrap()
        .downcast::<gstreamer_app::AppSrc>()
        .unwrap();
    pipeline.set_state(gstreamer::State::Playing).unwrap();
    for (index, pts) in PTS.into_iter().enumerate() {
        let color = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]][index];
        let mut buffer = gstreamer::Buffer::from_mut_slice(color.repeat(32 * 24));
        let buffer_ref = buffer.get_mut().unwrap();
        buffer_ref.set_pts(ClockTime::from_useconds(pts));
        buffer_ref.set_duration(ClockTime::from_useconds(100_000));
        source.push_buffer(buffer).unwrap();
    }
    source.end_of_stream().unwrap();
    let message = pipeline
        .bus()
        .unwrap()
        .timed_pop_filtered(
            ClockTime::from_seconds(10),
            &[gstreamer::MessageType::Eos, gstreamer::MessageType::Error],
        )
        .expect("encoder timed out");
    assert_eq!(
        message.type_(),
        gstreamer::MessageType::Eos,
        "fixture encoder failed"
    );
    pipeline.set_state(gstreamer::State::Null).unwrap();
    std::fs::read(file.path()).unwrap()
}

pub(super) fn start(bytes: Vec<u8>) -> PlatformDecodeRequest {
    let mut request = request(bytes, 1);
    request.kind = DecodeKind::Video;
    request.codec = "webm".into();
    request.stream_action = DecodeStreamAction::Start;
    request
}

pub(super) fn next(sequence: u64) -> PlatformDecodeRequest {
    let mut request = start(Vec::new());
    request.sequence = sequence;
    request.codec.clear();
    request.stream_action = DecodeStreamAction::Next;
    request.keyframe = false;
    request
}

#[test]
fn video_requires_stream_start_and_rejects_out_of_order_requests() {
    let mut decoder = DecodeResource::new(DecodeKind::Video).unwrap();
    assert!(decoder.decode(next(1)).is_err());
    let mut request = start(vec![1, 2, 3]);
    request.stream_action = DecodeStreamAction::OneShot;
    assert!(decoder.decode(request).is_err());
    assert_eq!(decoder.next_sequence, 1);
}

#[test]
#[ignore = "requires GStreamer VP8/WebM encoding and decoding plugins"]
fn variable_pts_complete_stream_counts_and_end_are_preserved() {
    let mut decoder = DecodeResource::new(DecodeKind::Video).unwrap();
    assert!(matches!(
        decoder.decode(start(fixture())).unwrap(),
        DecodeOutput::VideoStreamStart {
            duration_us: Some(550_000),
            ..
        }
    ));
    let mut colors = Vec::new();
    for (index, pts) in PTS.into_iter().enumerate() {
        let output = decoder.decode(next(index as u64 + 2)).unwrap();
        let DecodeOutput::VideoFrame {
            sequence,
            pts_us,
            duration_us,
            width,
            height,
            bgra8,
        } = output
        else {
            panic!("expected decoded frame");
        };
        assert_eq!(sequence, index as u64 + 1);
        assert_eq!(pts_us, pts);
        assert_eq!(
            duration_us,
            PTS.get(index + 1).copied().unwrap_or(550_000) - pts
        );
        assert_eq!((width, height, bgra8.len()), (32, 24, 32 * 24 * 4));
        colors.push(bgra8[0..4].to_vec());
        assert!(decoder.decode(next(index as u64 + 2)).is_err());
    }
    assert!(colors.windows(2).all(|pair| pair[0] != pair[1]));
    assert_eq!(
        decoder.decode(next(6)).unwrap(),
        DecodeOutput::VideoStreamEnd {
            frame_count: 4,
            decoded_byte_count: 4 * 32 * 24 * 4
        }
    );
    assert!(decoder.decode(next(7)).is_err());
}

#[test]
#[ignore = "requires GStreamer VP8/WebM plugins"]
fn closing_backpressured_stream_stops_pipeline_and_removes_input() {
    let mut decoder = DecodeResource::new(DecodeKind::Video).unwrap();
    decoder.decode(start(fixture())).unwrap();
    let stream = decoder.video.as_ref().unwrap();
    let pipeline = stream.test_pipeline();
    let path = stream.test_input_path();
    assert!(path.exists());
    let started = Instant::now();
    drop(decoder);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(pipeline.current_state(), gstreamer::State::Null);
    assert!(!path.exists());
}

#[test]
#[ignore = "requires GStreamer VP8/WebM plugins and a native hardware GPU"]
fn every_decoded_frame_reaches_the_hardware_renderer() {
    use astra_media::TextureFrame;
    use astra_platform::SceneFrame;
    // Renderer accepts media-core commands through astra-media's reexports.
    use astra_media::{BlendMode, RectI, SceneCommand};
    let mut renderer =
        pollster::block_on(astra_platform_common::WgpuOffscreenRenderer::new()).unwrap();
    assert_ne!(renderer.identity().device_type, "cpu");
    let mut decoder = DecodeResource::new(DecodeKind::Video).unwrap();
    decoder.decode(start(fixture())).unwrap();
    for sequence in 2..6 {
        let DecodeOutput::VideoFrame {
            width,
            height,
            bgra8,
            ..
        } = decoder.decode(next(sequence)).unwrap()
        else {
            panic!("expected frame");
        };
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
                    id: "decoded-video".into(),
                    frame: TextureFrame::from_vec(width, height, rgba.clone()).unwrap(),
                    destination: RectI::new(0, 0, width, height),
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                }],
                semantics: None,
            })
            .unwrap();
        assert_eq!(capture.rgba8.as_ref(), rgba.as_slice());
    }
    assert!(matches!(
        decoder.decode(next(6)).unwrap(),
        DecodeOutput::VideoStreamEnd { frame_count: 4, .. }
    ));
}

#[test]
#[ignore = "requires GStreamer VP8/WebM plugins"]
fn truncated_video_never_reports_a_complete_stream() {
    let mut bytes = fixture();
    bytes.truncate(bytes.len() / 2);
    let mut decoder = DecodeResource::new(DecodeKind::Video).unwrap();
    if decoder.decode(start(bytes)).is_err() {
        return;
    }
    for sequence in 2..10 {
        match decoder.decode(next(sequence)) {
            Err(_) => {
                assert!(decoder.video.is_none());
                assert!(decoder.failed);
                return;
            }
            Ok(DecodeOutput::VideoFrame { .. }) => {}
            _ => panic!("truncated stream must not report successful completion"),
        }
    }
    panic!("truncated stream did not terminate");
}
