use super::video::{fixture, next, start};
use super::*;
use tokio::sync::oneshot;

#[test]
#[ignore = "requires GStreamer VP8/WebM plugins"]
fn worker_stream_closes_and_reopens_without_late_results() {
    let bytes = fixture();
    for _ in 0..3 {
        let session = DecodeSession::new(DecodeKind::Video).unwrap();
        let (reply, result) = oneshot::channel();
        session.submit(start(bytes.clone()), reply);
        assert!(matches!(
            result.blocking_recv().unwrap().unwrap(),
            DecodeOutput::VideoStreamStart { .. }
        ));
        let (reply, result) = oneshot::channel();
        session.submit(next(2), reply);
        assert!(matches!(
            result.blocking_recv().unwrap().unwrap(),
            DecodeOutput::VideoFrame { sequence: 1, .. }
        ));
        let started = Instant::now();
        session.close().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[test]
#[ignore = "requires GStreamer VP8/WebM plugins"]
fn cancelled_start_reply_does_not_leave_a_usable_stream() {
    let session = DecodeSession::new(DecodeKind::Video).unwrap();
    let (reply, result) = oneshot::channel();
    drop(result);
    session.submit(start(fixture()), reply);
    // One-slot queue may still contain the abandoned start. Once dequeued, the
    // decoder is permanently failed; no Next can return a late video frame.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let (reply, result) = oneshot::channel();
        session.submit(next(2), reply);
        let error = result.blocking_recv().unwrap().unwrap_err();
        if error.message != "decode request queue is full" {
            assert_eq!(error.message, "failed decoder must be closed");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    session.close().unwrap();
}
