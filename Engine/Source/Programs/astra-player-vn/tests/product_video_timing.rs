use astra_platform::{
    host_channel, DecodeKind, DecodeOutput, DecodeSessionHandle, DecodeStreamAction, HostCommand,
    PlatformHostProfile, SurfaceHandle,
};
use astra_player_core::{PlatformCommandSink, PlayerHostCommandExecutor, PlayerHostResourceId};
use astra_player_vn::{NativeVnHostCommandSource, NativeVnProductMediaHost};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

mod support;

fn source(looping: bool) -> NativeVnHostCommandSource {
    let mut source = support::source_for_video(&format!(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    stage viewport:320x180 safe_area:16:9 #@id stage.main\n    layer id:video kind:video z:100 blend:normal clip:stage #@id layer.video\n    movie layer:video asset:asset:/video/intro loop:{looping} end:wait fence:movie.intro.end fallback:asset:/video/intro-fallback interrupt:reject #@id movie.intro\n    text key:line.after #@id line.after\n"
    ));
    source.launch().unwrap();
    source
}

// Exercise the actual Player host command channel, including reopen/restore,
// with native decoder replies whose estimate is independent of their frames.
fn backend(
    estimates: Vec<Option<u64>>,
    ends: Vec<u64>,
) -> (
    PlayerHostCommandExecutor<PlatformCommandSink>,
    Arc<AtomicU64>,
    tokio::task::JoinHandle<()>,
) {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let opens = Arc::new(AtomicU64::new(0));
    let opened = opens.clone();
    let task = tokio::spawn(async move {
        let mut cycle = 0;
        while let Some(command) = backend.next_command().await {
            match command {
                HostCommand::OpenDecode { kind, reply } => {
                    assert_eq!(kind, DecodeKind::Video);
                    cycle = opened.fetch_add(1, Ordering::SeqCst) as usize;
                    reply
                        .send(Ok(
                            DecodeSessionHandle::from_parts(cycle as u32 + 1, 1).unwrap()
                        ))
                        .unwrap();
                }
                HostCommand::Decode { request, reply, .. } => {
                    let output = match request.stream_action {
                        DecodeStreamAction::Start => DecodeOutput::VideoStreamStart {
                            duration_estimate_us: estimates[cycle.min(estimates.len() - 1)],
                            frame_count: None,
                            decoded_byte_count: None,
                        },
                        DecodeStreamAction::Next => match request.sequence {
                            2 => DecodeOutput::VideoFrame {
                                sequence: 1,
                                pts_us: 0,
                                duration_us: 20_000,
                                width: 1,
                                height: 1,
                                bgra8: vec![1, 2, 3, 255].into(),
                            },
                            3 => DecodeOutput::VideoFrame {
                                sequence: 2,
                                pts_us: 20_000,
                                duration_us: ends[cycle.min(ends.len() - 1)] - 20_000,
                                width: 1,
                                height: 1,
                                bgra8: vec![4, 5, 6, 255].into(),
                            },
                            4 => DecodeOutput::VideoStreamEnd {
                                frame_count: 2,
                                decoded_byte_count: 8,
                            },
                            _ => panic!("unexpected decoder cursor"),
                        },
                        _ => panic!("unexpected one-shot video"),
                    };
                    reply.send(Ok(output)).unwrap();
                }
                HostCommand::PresentScene { reply, .. } => {
                    reply.send(Ok(())).unwrap();
                }
                HostCommand::CloseDecode { reply, .. } => {
                    reply.send(Ok(())).unwrap();
                }
                command => panic!("unexpected command: {}", command.operation()),
            }
        }
    });
    let mut sink = PlatformCommandSink::new(client);
    sink.bind_surface(
        PlayerHostResourceId(1),
        SurfaceHandle::from_parts(7, 1).unwrap(),
    )
    .unwrap();
    (PlayerHostCommandExecutor::new(sink), opens, task)
}

#[tokio::test]
async fn completion_uses_actual_tail_with_smaller_larger_or_missing_estimate() {
    for estimate in [Some(10_000), Some(100_000), None] {
        let mut source = source(false);
        let mut media = NativeVnProductMediaHost::default();
        let (mut executor, opens, task) = backend(vec![estimate], vec![40_333]);
        media
            .process(&mut source, &mut executor, 0, Vec::new())
            .await
            .unwrap();
        let before_eos = media.snapshot();
        assert_eq!(before_eos.active_videos[0].frame_end_us, 20_000);
        assert_eq!(before_eos.active_videos[0].loop_duration_us, None);
        media
            .poll_and_process(&mut source, &mut executor, 20)
            .await
            .unwrap();
        assert_eq!(
            media.snapshot().active_videos[0].loop_duration_us,
            Some(40_333)
        );
        media
            .poll_and_process(&mut source, &mut executor, 40)
            .await
            .unwrap();
        assert!(media.has_active_video());
        media
            .poll_and_process(&mut source, &mut executor, 41)
            .await
            .unwrap();
        assert!(!media.has_active_video());
        assert_eq!(opens.load(Ordering::SeqCst), 1);
        media.shutdown(&mut source, &mut executor).await.unwrap();
        drop(executor);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn loop_and_restore_use_completed_duration_and_presented_cursor() {
    let mut source = source(true);
    let mut media = NativeVnProductMediaHost::default();
    let (mut executor, opens, task) =
        backend(vec![Some(10_000), None, Some(999_000)], vec![40_333]);
    media
        .process(&mut source, &mut executor, 0, Vec::new())
        .await
        .unwrap();
    let saved = media.snapshot();
    assert_eq!(saved.active_videos[0].loop_duration_us, None);
    media.restore(saved).unwrap();
    media
        .poll_and_process(&mut source, &mut executor, 1_000)
        .await
        .unwrap();
    assert_eq!(media.snapshot().active_videos[0].next_frame, 1);
    assert_eq!(media.snapshot().active_videos[0].frame_end_us, 20_000);
    media
        .poll_and_process(&mut source, &mut executor, 1_020)
        .await
        .unwrap();
    let saved = media.snapshot();
    assert_eq!(saved.active_videos[0].loop_duration_us, Some(40_333));
    media.restore(saved).unwrap();
    media
        .poll_and_process(&mut source, &mut executor, 2_000)
        .await
        .unwrap();
    assert_eq!(media.snapshot().active_videos[0].next_frame, 2);
    media
        .poll_and_process(&mut source, &mut executor, 2_020)
        .await
        .unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), 3);
    media
        .poll_and_process(&mut source, &mut executor, 2_021)
        .await
        .unwrap();
    let saved = media.snapshot();
    assert_eq!(saved.active_videos[0].loop_index, 1);
    assert_eq!(saved.active_videos[0].loop_duration_us, Some(40_333));
    media.restore(saved).unwrap();
    media
        .poll_and_process(&mut source, &mut executor, 3_000)
        .await
        .unwrap();
    assert_eq!(media.snapshot().active_videos[0].next_frame, 1);
    media
        .poll_and_process(&mut source, &mut executor, 3_019)
        .await
        .unwrap();
    assert_eq!(media.snapshot().active_videos[0].next_frame, 1);
    media
        .poll_and_process(&mut source, &mut executor, 3_020)
        .await
        .unwrap();
    assert_eq!(media.snapshot().active_videos[0].next_frame, 2);
    media
        .poll_and_process(&mut source, &mut executor, 3_039)
        .await
        .unwrap();
    assert_eq!(media.snapshot().active_videos[0].loop_index, 1);
    media
        .poll_and_process(&mut source, &mut executor, 3_040)
        .await
        .unwrap();
    assert_eq!(media.snapshot().active_videos[0].loop_index, 2);
    media.shutdown(&mut source, &mut executor).await.unwrap();
    drop(executor);
    task.await.unwrap();
}

#[tokio::test]
async fn unknown_counts_still_enforce_frame_and_byte_budgets() {
    for (frames, bytes) in [(1, 16), (10, 4)] {
        let mut source = source(true);
        let mut media =
            NativeVnProductMediaHost::with_video_limits(256, frames, bytes, bytes, false).unwrap();
        let (mut executor, _, task) = backend(vec![None], vec![40_333]);
        let error = media
            .process(&mut source, &mut executor, 0, Vec::new())
            .await
            .unwrap_err();
        assert!(
            error
                .message
                .contains("ASTRA_PLAYER_VIDEO_FRAME_IDENTITY_MISMATCH")
                || error
                    .message
                    .contains("ASTRA_PLAYER_VIDEO_DECODED_BYTES_BUDGET")
        );
        media.shutdown(&mut source, &mut executor).await.unwrap();
        drop(executor);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn a_reopened_loop_cannot_change_its_completed_duration() {
    let mut source = source(true);
    let mut media = NativeVnProductMediaHost::default();
    let (mut executor, _, task) = backend(vec![None], vec![40_333, 40_000]);
    media
        .process(&mut source, &mut executor, 0, Vec::new())
        .await
        .unwrap();
    media
        .poll_and_process(&mut source, &mut executor, 20)
        .await
        .unwrap();
    media
        .poll_and_process(&mut source, &mut executor, 41)
        .await
        .unwrap();
    media
        .poll_and_process(&mut source, &mut executor, 42)
        .await
        .unwrap();
    let error = media
        .poll_and_process(&mut source, &mut executor, 61)
        .await
        .unwrap_err();
    assert!(error
        .message
        .contains("ASTRA_PLAYER_VIDEO_STREAM_ENDED_EARLY"));
    media.shutdown(&mut source, &mut executor).await.unwrap();
    drop(executor);
    task.await.unwrap();
}
