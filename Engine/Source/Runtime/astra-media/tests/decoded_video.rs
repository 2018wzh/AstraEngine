use astra_core::Hash256;
use astra_media::{
    DecodedVideoFrame, DecodedVideoStream, DecodedVideoStreamCursor, DecodedVideoStreamCursorEnd,
    DecodedVideoStreamDescriptor, DecodedVideoStreamEnd, DECODED_VIDEO_STREAM_CURSOR_END_SCHEMA,
    DECODED_VIDEO_STREAM_CURSOR_SCHEMA, DECODED_VIDEO_STREAM_DESCRIPTOR_SCHEMA,
    DECODED_VIDEO_STREAM_END_SCHEMA, DECODED_VIDEO_STREAM_SCHEMA,
};

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

#[astra_headless_test::test]
fn streaming_descriptor_frame_and_end_are_independently_bounded() {
    let descriptor = DecodedVideoStreamDescriptor {
        schema: DECODED_VIDEO_STREAM_DESCRIPTOR_SCHEMA.into(),
        duration_us: 40_000,
        frame_count: 2,
        decoded_byte_count: 8,
        stream_hash: Hash256::from_sha256(b"stream"),
    };
    let encoded = descriptor.encode(2, 8).unwrap();
    assert_eq!(
        DecodedVideoStreamDescriptor::decode(&encoded, 2, 8).unwrap(),
        descriptor
    );

    let frame = stream().frames.remove(0);
    let encoded = frame.encode(1_024).unwrap();
    assert_eq!(DecodedVideoFrame::decode(&encoded, 1_024).unwrap(), frame);

    let end = DecodedVideoStreamEnd {
        schema: DECODED_VIDEO_STREAM_END_SCHEMA.into(),
        frame_count: descriptor.frame_count,
        decoded_byte_count: descriptor.decoded_byte_count,
        stream_hash: descriptor.stream_hash,
    };
    end.validate_against(&descriptor).unwrap();
    let mut tampered = end;
    tampered.frame_count += 1;
    assert!(tampered.validate_against(&descriptor).is_err());
}

#[astra_headless_test::test]
fn raw_cpu_frame_format_moves_pixels_without_postcard_payload() {
    let frame = stream().frames.remove(0);
    let format = frame.cpu_buffer_format();
    let hash = frame.content_hash.to_string();
    let bytes = frame.bgra8;
    let payload_ptr = bytes.as_ptr();
    let rebuilt = DecodedVideoFrame::from_cpu_buffer(&format, bytes, &hash, 1_024).unwrap();
    assert_eq!(rebuilt.sequence, 1);
    assert_eq!(rebuilt.width, 1);
    assert_eq!(rebuilt.height, 1);
    assert_eq!(rebuilt.content_hash, frame.content_hash);
    assert_eq!(rebuilt.bgra8.as_ptr(), payload_ptr);
}

#[astra_headless_test::test]
fn raw_cpu_frame_format_rejects_metadata_and_hash_drift() {
    let frame = stream().frames.remove(0);
    let bytes = frame.bgra8.clone();
    let mut fields = frame
        .cpu_buffer_format()
        .split(':')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    fields[4] = "2".into();
    let drifted_format = fields.join(":");
    assert!(DecodedVideoFrame::from_cpu_buffer(
        &drifted_format,
        bytes.clone(),
        &frame.content_hash.to_string(),
        1_024,
    )
    .is_err());
    assert!(DecodedVideoFrame::from_cpu_buffer(
        &frame.cpu_buffer_format(),
        bytes,
        &Hash256::from_sha256(b"wrong").to_string(),
        1_024,
    )
    .is_err());
}

#[astra_headless_test::test]
fn lazy_cursor_authenticates_source_and_final_totals() {
    let cursor = DecodedVideoStreamCursor {
        schema: DECODED_VIDEO_STREAM_CURSOR_SCHEMA.into(),
        source_hash: Hash256::from_sha256(b"encoded-movie"),
        width: 1920,
        height: 1080,
        max_frames: 120,
        max_decoded_byte_count: 120 * 1920 * 1080 * 4,
    };
    let encoded = cursor.encode().unwrap();
    assert_eq!(DecodedVideoStreamCursor::decode(&encoded).unwrap(), cursor);
    let end = DecodedVideoStreamCursorEnd {
        schema: DECODED_VIDEO_STREAM_CURSOR_END_SCHEMA.into(),
        source_hash: cursor.source_hash,
        frame_count: 2,
        decoded_byte_count: 2 * 1920 * 1080 * 4,
    };
    let encoded = end.encode(&cursor).unwrap();
    assert_eq!(DecodedVideoStreamCursorEnd::decode(&encoded).unwrap(), end);
    let mut tampered = end;
    tampered.source_hash = Hash256::from_sha256(b"other-movie");
    assert!(tampered.validate_against(&cursor).is_err());
}
