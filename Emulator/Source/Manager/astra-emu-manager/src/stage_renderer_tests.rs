use super::*;
use astra_emu_manager_core::model::{
    FrameAlpha, FrameFormat, FrameInfo, FrameLease, FrameRowOrder,
};

fn info(width: u32, height: u32, stride: u32, alpha: FrameAlpha) -> FrameInfo {
    FrameInfo {
        width,
        height,
        logical_width: width,
        logical_height: height,
        stride,
        format: FrameFormat::Rgba8Srgb { alpha },
        rows: FrameRowOrder::TopToBottom,
    }
}

fn lease(pixels: Vec<u8>, info: FrameInfo, epoch: u64, generation: u64) -> FrameLease {
    FrameLease::from_vec(
        pixels,
        info,
        FrameIdentity {
            session_epoch: epoch,
            output_epoch: 1,
            generation,
        },
    )
    .unwrap()
}

#[test]
fn old_frame_and_shutdown_cannot_replace_or_clear_a_new_session_baseline() {
    let mailbox = FrameMailbox::new();
    mailbox.activate(1).unwrap();
    let old = lease(vec![1, 2, 3, 255], info(1, 1, 4, FrameAlpha::Opaque), 1, 7);
    mailbox.publish(old.clone()).unwrap();
    mailbox.clear(1).unwrap();
    mailbox.activate(2).unwrap();
    mailbox
        .publish(lease(
            vec![5, 6, 7, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            2,
            1,
        ))
        .unwrap();
    assert!(mailbox.publish(old).is_err());
    mailbox.clear(1).unwrap();
    let current = mailbox.snapshot().unwrap().unwrap();
    assert_eq!(
        current.identity,
        FrameIdentity {
            session_epoch: 2,
            output_epoch: 1,
            generation: 1
        }
    );
    assert_eq!(current.pixels().unwrap(), &[5, 6, 7, 255]);
}

#[test]
fn replacement_and_close_preserve_an_already_acquired_cpu_lease() {
    let mailbox = FrameMailbox::new();
    mailbox.activate(3).unwrap();
    mailbox
        .publish(lease(
            vec![3, 4, 5, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            3,
            1,
        ))
        .unwrap();
    let held = mailbox.snapshot().unwrap().unwrap();
    mailbox
        .publish(lease(
            vec![7, 8, 9, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            3,
            2,
        ))
        .unwrap();
    mailbox.clear(3).unwrap();
    assert!(mailbox.snapshot().unwrap().is_none());
    assert_eq!(held.pixels().unwrap(), &[3, 4, 5, 255]);
}

#[test]
fn filtered_output_keeps_the_complete_native_frame_interactive() {
    let content = super::content_viewport_for_output(1280, 720, 721, 1280, 1082, 1920).unwrap();
    assert_eq!(
        content,
        astra_media_core::Viewport2D {
            x: 0,
            y: 0,
            width: 1082,
            height: 1920,
        }
    );

    let content = super::content_viewport_for_output(1280, 720, 1920, 1200, 2880, 1800).unwrap();
    assert_eq!(
        content,
        astra_media_core::Viewport2D {
            x: 0,
            y: 0,
            width: 2880,
            height: 1800,
        }
    );
}
