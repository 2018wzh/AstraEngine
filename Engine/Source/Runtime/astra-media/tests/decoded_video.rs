use astra_core::Hash256;
use astra_media::{DecodedVideoFrame, DecodedVideoStream, DECODED_VIDEO_STREAM_SCHEMA};

fn stream() -> DecodedVideoStream {
    let first = vec![1, 2, 3, 255];
    let second = vec![4, 5, 6, 255];
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
                content_hash: Hash256::from_sha256(&first),
                bgra8: first,
            },
            DecodedVideoFrame {
                sequence: 2,
                pts_us: 20_000,
                duration_us: 20_000,
                width: 1,
                height: 1,
                content_hash: Hash256::from_sha256(&second),
                bgra8: second,
            },
        ],
    }
}

#[astra_headless_test::test]
fn decoded_video_stream_round_trips_with_order_and_hash_validation() {
    let stream = stream();
    let encoded = stream.encode(2, 1_024).unwrap();
    assert_eq!(
        DecodedVideoStream::decode(&encoded, 2, 1_024).unwrap(),
        stream
    );
}

#[astra_headless_test::test]
fn decoded_video_stream_blocks_tamper_and_resource_overflow() {
    let mut invalid = stream();
    invalid.frames[1].sequence = 1;
    assert!(invalid.encode(2, 8).is_err());

    let mut invalid = stream();
    invalid.frames[0].content_hash = Hash256::from_sha256(b"tampered");
    assert!(invalid.encode(2, 8).is_err());

    assert!(stream().encode(1, 8).is_err());
    assert!(stream().encode(2, 7).is_err());
}
