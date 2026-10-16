use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn info(stride: u32, alpha: FrameAlpha) -> FrameInfo {
    FrameInfo {
        width: 1,
        height: 2,
        logical_width: 1,
        logical_height: 2,
        stride,
        format: FrameFormat::Rgba8Srgb { alpha },
        rows: FrameRowOrder::TopToBottom,
    }
}

fn ready(poll: FramePoll) -> FrameLease {
    match poll {
        FramePoll::Ready(frame) => frame,
        other => panic!("expected completed frame, got {other:?}"),
    }
}

struct Owner {
    pixels: Arc<Vec<u8>>,
    drops: Arc<AtomicUsize>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn leases_survive_producer_close_and_isolate_later_native_mutation() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut native = Arc::new(vec![10, 20, 30, 255, 40, 50, 60, 255]);
    let mut publisher = FramePublisher::new(7).unwrap();
    publisher
        .publish(
            Owner {
                pixels: native.clone(),
                drops: drops.clone(),
            },
            |owner| owner.pixels.as_slice(),
            info(4, FrameAlpha::Opaque),
        )
        .unwrap();
    let frame = ready(publisher.poll(None));
    let clone = frame.clone();
    drop(publisher);
    Arc::make_mut(&mut native)[0] = 90;
    assert_eq!(frame.pixels(), &[10, 20, 30, 255, 40, 50, 60, 255]);
    drop(frame);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    std::thread::spawn(move || {
        assert_eq!(clone.pixels()[0], 10);
        assert_eq!(
            clone.identity,
            FrameIdentity {
                session_epoch: 7,
                generation: 1
            }
        );
    })
    .join()
    .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn skipped_publications_keep_old_leases_and_return_latest_full_baseline() {
    let mut publisher = FramePublisher::new(9).unwrap();
    assert!(matches!(
        publisher.poll(None),
        FramePoll::Pending(FrameIdentity { generation: 0, .. })
    ));
    publisher
        .publish(
            vec![1, 2, 3, 255, 4, 5, 6, 255],
            Vec::as_slice,
            info(4, FrameAlpha::Opaque),
        )
        .unwrap();
    let first = ready(publisher.poll(None));
    assert!(
        matches!(publisher.poll(Some(first.identity)), FramePoll::Unchanged(id) if id == first.identity)
    );
    publisher
        .publish(vec![8; 8], Vec::as_slice, info(4, FrameAlpha::Ignored))
        .unwrap();
    publisher
        .publish(vec![9; 8], Vec::as_slice, info(4, FrameAlpha::Ignored))
        .unwrap();
    let latest = ready(publisher.poll(Some(first.identity)));
    assert_eq!(latest.identity.generation, 3);
    assert_eq!(latest.pixels(), &[9; 8]);
    assert_eq!(first.pixels(), &[1, 2, 3, 255, 4, 5, 6, 255]);
    assert_eq!(
        FramePoll::Ready(latest).validate(10).unwrap_err().code(),
        "ASTRA_EMU_FAMILY_FRAME_EPOCH"
    );
}

#[test]
fn rejected_frame_does_not_replace_completed_pixels_or_advance_generation() {
    let mut publisher = FramePublisher::new(11).unwrap();
    // Padding is not opacity; only actual pixels must be opaque.
    let pixels = vec![1, 2, 3, 255, 0, 0, 0, 0, 4, 5, 6, 255, 0, 0, 0, 0];
    publisher
        .publish(pixels.clone(), Vec::as_slice, info(8, FrameAlpha::Opaque))
        .unwrap();
    let first = ready(publisher.poll(None));
    assert_eq!(
        publisher
            .publish(vec![0; 15], Vec::as_slice, info(8, FrameAlpha::Opaque))
            .unwrap_err()
            .code(),
        "ASTRA_EMU_FAMILY_FRAME_BYTES"
    );
    assert_eq!(
        publisher
            .publish(vec![0; 16], Vec::as_slice, info(8, FrameAlpha::Opaque))
            .unwrap_err()
            .code(),
        "ASTRA_EMU_FAMILY_FRAME_ALPHA"
    );
    let retained = ready(publisher.poll(None));
    assert_eq!(retained.identity, first.identity);
    assert_eq!(retained.pixels(), pixels);
    publisher
        .publish(vec![0; 16], Vec::as_slice, info(8, FrameAlpha::Ignored))
        .unwrap();
    assert_eq!(ready(publisher.poll(None)).identity.generation, 2);
}
