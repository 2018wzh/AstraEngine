use std::io::Cursor;
use wmv_decoder::AsfWmv2Decoder;

#[test]
fn successive_reads_preserve_every_video_object_in_one_asf_packet() {
    // Noncommercial fixture encoded with FFmpeg 7.1:
    // ffmpeg -f lavfi -i testsrc2=size=16x16:rate=5:duration=1 -an -c:v wmv2 -f asf multi-payload.wmv
    let mut decoder =
        AsfWmv2Decoder::open(Cursor::new(include_bytes!("fixtures/multi-payload.wmv"))).unwrap();
    assert_eq!(decoder.presentation_duration_ns().unwrap(), 1_000_000_000);
    let mut timestamps = Vec::new();
    while let Some(frame) = decoder.next_frame().unwrap() {
        timestamps.push(frame.pts_ms);
        assert_eq!((frame.frame.width, frame.frame.height), (16, 16));
    }
    assert_eq!(timestamps, [0, 200, 400, 600, 800]);
    assert!(decoder.next_frame().unwrap().is_none());
}
