use astra_media::{DecodedVideoFrame, DecodedVideoStream, DECODED_VIDEO_STREAM_SCHEMA};

fn stream() -> DecodedVideoStream {
    DecodedVideoStream {
        schema: DECODED_VIDEO_STREAM_SCHEMA.into(),
        duration_us: 40_000,
        frames: vec![
            DecodedVideoFrame {
                sequence: 1,
                pts_us: 0,
                duration_us: 20_000,
                width: 1,
                height: 1,
                bgra8: vec![1, 2, 3, 255].into(),
            },
            DecodedVideoFrame {
                sequence: 2,
                pts_us: 20_000,
                duration_us: 20_000,
                width: 1,
                height: 1,
                bgra8: vec![4, 5, 6, 255].into(),
            },
        ],
    }
}

#[astra_headless_test::test]
fn decoded_video_validates_typed_owned_frames() {
    let stream = stream();
    let pointer = stream.frames[0].bgra8.as_ptr();
    stream.validate(2, 8).unwrap();
    assert_eq!(stream.frames[0].bgra8.as_ptr(), pointer);
}

#[astra_headless_test::test]
fn decoded_video_rejects_invalid_order_dimensions_and_budget() {
    let mut invalid = stream();
    invalid.frames[1].sequence = 1;
    assert!(invalid.validate(2, 8).is_err());

    let mut invalid = stream();
    invalid.frames[0].width = 2;
    assert!(invalid.validate(2, 8).is_err());

    assert!(stream().validate(1, 8).is_err());
    assert!(stream().validate(2, 7).is_err());
}
