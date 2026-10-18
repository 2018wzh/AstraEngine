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

#[test]
fn saved_filter_is_prepared_without_a_device_or_compiler() {
    let mut renderer = ManagerStageRenderer::with_dxc_path(
        FrameMailbox::new(),
        PathBuf::from("unavailable/dxcompiler.dll"),
    );
    let config = FilterConfiguration {
        preset: FilterPreset::Anime4kRestoreUpscale,
        scale: 2.0,
        ..Default::default()
    };
    renderer.configure_filter_inner(&config, None).unwrap();
    assert!(renderer.take_stage_texture_update().unwrap().is_none());
    let prepared = renderer.filter_activation.take_pending().unwrap();
    assert_eq!(prepared.output_dimensions(721, 1280).unwrap(), (1442, 2560));
    assert!(!renderer.filter_activation.ready_for_frame().unwrap());
}

#[test]
fn invalid_reconfigure_preserves_the_pending_good_request() {
    let mut renderer = ManagerStageRenderer::new(FrameMailbox::new());
    let config = FilterConfiguration {
        preset: FilterPreset::Scale,
        scale: 1.5,
        ..Default::default()
    };
    renderer.configure_filter_inner(&config, None).unwrap();
    let invalid = FilterConfiguration {
        scale: f32::NAN,
        ..config.clone()
    };
    assert_eq!(
        renderer.configure_filter_inner(&invalid, None).unwrap_err(),
        "ASTRA_EMU_FILTER_PARAMETER_scale"
    );
    assert!(
        renderer
            .configure_filter_inner(&config, Some("invalid format-4 source"))
            .is_err()
    );
    let prepared = renderer.filter_activation.take_pending().unwrap();
    assert_eq!(prepared.output_dimensions(721, 1280).unwrap(), (1082, 1920));
}

#[test]
fn deferred_dimension_failure_reports_once_and_requires_reconfiguration() {
    let config = FilterConfiguration {
        preset: FilterPreset::Anime4kRestoreUpscale,
        scale: 2.0,
        ..Default::default()
    };
    let mut activation = filter::FilterActivation::default();
    activation.defer(PreparedFilter::new(&config, None).unwrap());
    let prepared = activation.take_pending().unwrap();
    let error = prepared
        .output_dimensions(u32::MAX, 720)
        .unwrap_err()
        .to_string();
    activation.complete(Err(error.clone()));
    assert_eq!(activation.ready_for_frame().unwrap_err(), error);
    for _ in 0..3 {
        assert!(activation.take_pending().is_none());
        assert!(!activation.ready_for_frame().unwrap());
    }
    activation.defer(PreparedFilter::new(&FilterConfiguration::default(), None).unwrap());
    let prepared = activation.take_pending().unwrap();
    assert_eq!(prepared.output_dimensions(1280, 720).unwrap(), (1280, 720));
    activation.complete(Ok(()));
    assert!(activation.ready_for_frame().unwrap());
}

#[test]
fn teardown_discards_pending_filter_without_activating_it() {
    let mut renderer = ManagerStageRenderer::new(FrameMailbox::new());
    let config = FilterConfiguration {
        preset: FilterPreset::Sharpen,
        ..Default::default()
    };
    renderer.configure_filter_inner(&config, None).unwrap();
    renderer.teardown().unwrap();
    assert!(renderer.filter_activation.take_pending().is_none());
    assert!(renderer.take_stage_texture_update().unwrap().is_none());
    renderer.configure_filter_inner(&config, None).unwrap();
    assert!(renderer.filter_activation.take_pending().is_some());
}
