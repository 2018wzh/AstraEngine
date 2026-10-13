use super::*;

pub(super) struct ProductAudioTestLane {
    pub(super) consumed_samples: Arc<AtomicU64>,
    pub(super) underflow_count: u64,
    pub(super) dropped: Arc<AtomicBool>,
    pub(super) close_requested: Arc<AtomicBool>,
    pub(super) submissions_after_close: Arc<AtomicU64>,
}

impl ProductAudioTestLane {
    pub(super) fn new(consumed_samples: Arc<AtomicU64>, underflow_count: u64) -> Self {
        Self {
            consumed_samples,
            underflow_count,
            dropped: Arc::new(AtomicBool::new(false)),
            close_requested: Arc::new(AtomicBool::new(false)),
            submissions_after_close: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Drop for ProductAudioTestLane {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

impl AudioOutputLane for ProductAudioTestLane {
    fn wait_for_capacity(
        &mut self,
        _requested_samples: usize,
        _stop: &AtomicBool,
    ) -> Result<(), PlatformError> {
        Ok(())
    }

    fn submit(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlatformError> {
        if self.close_requested.load(Ordering::Acquire) {
            self.submissions_after_close.fetch_add(1, Ordering::Release);
        }
        self.consumed_samples
            .fetch_add(samples.len() as u64, Ordering::Release);
        Ok(samples)
    }

    fn consumed_samples(&self) -> u64 {
        self.consumed_samples.load(Ordering::Acquire)
    }

    fn underflow_count(&self) -> u64 {
        self.underflow_count
    }
}

pub(super) fn source() -> NativeVnHostCommandSource {
    support::source_for(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line speaker:hero #@id line.one\n",
    )
}

pub(super) fn assert_endpoint_consumed_is_monotonic(
    consumed_samples: &Arc<AtomicU64>,
    previous: &mut u64,
) {
    let current = consumed_samples.load(Ordering::Acquire);
    assert!(
        current >= *previous,
        "audio endpoint consumed sample count regressed from {} to {}",
        *previous,
        current
    );
    *previous = current;
}

#[tokio::test]
pub(super) async fn shared_product_media_host_completes_timeline_fence_and_presents_runtime_result()
{
    let mut source = support::source_for(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    timeline id:intro target:camera property:x keyframes:0=0,120=1 join:block fence:timeline.intro.complete budget_ms:2 #@id timeline.intro\n    text key:line.after #@id line.after\n",
    );
    source.launch().unwrap();

    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 4, 4).unwrap();
    let native_surface = SurfaceHandle::from_parts(7, 1).unwrap();
    let backend_task = tokio::spawn(async move {
        match backend.next_command().await.unwrap() {
            HostCommand::PresentScene {
                surface,
                frame,
                reply,
            } => {
                assert_eq!(surface, native_surface);
                assert_eq!((frame.width, frame.height), (320, 180));
                assert!(!frame.commands.is_empty());
                reply.send(Ok(())).unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
    });
    let mut sink = PlatformCommandSink::new(client);
    sink.bind_surface(PlayerHostResourceId(1), native_surface)
        .unwrap();
    let mut executor = PlayerHostCommandExecutor::new(sink);
    let mut host = NativeVnProductMediaHost::default();

    host.process(&mut source, &mut executor, 0, Vec::new())
        .await
        .unwrap();
    assert!(host.is_active());
    host.poll_and_process(&mut source, &mut executor, 120)
        .await
        .unwrap();
    assert!(!host.is_active());
    assert_ne!(
        source.pending_wait().map(|wait| wait.fence.as_str()),
        Some("timeline.intro.complete")
    );
    backend_task.await.unwrap();
}

#[tokio::test]
pub(super) async fn product_media_host_restores_uncommitted_timeline_tasks_after_capacity_failure()
{
    let mut source = support::source_for(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    timeline id:intro target:camera property:x keyframes:0=0,120=1 join:block fence:timeline.intro.complete budget_ms:2 #@id timeline.intro\n",
    );
    source.launch().unwrap();
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, _backend, _events) = host_channel(profile, 1, 1).unwrap();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    let mut host = NativeVnProductMediaHost::new(0);

    let error = host
        .process(&mut source, &mut executor, 0, Vec::new())
        .await
        .unwrap_err();

    assert_eq!(error.operation, "player.timeline.schedule");
    let tasks = source.take_timeline_tasks();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "intro");
}

#[tokio::test]
pub(super) async fn product_media_host_presents_every_video_frame_and_restores_by_asset_identity() {
    let mut source = support::source_for_video(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    stage viewport:320x180 safe_area:16:9 #@id stage.main\n    layer id:video kind:video z:100 blend:normal clip:stage #@id layer.video\n    movie layer:video asset:asset:/video/intro loop:true end:wait fence:movie.intro.end fallback:asset:/video/intro-fallback interrupt:reject #@id movie.intro\n    text key:line.after #@id line.after\n",
    );
    source.launch().unwrap();
    let first = vec![1, 2, 3, 255];
    let second = vec![4, 5, 6, 255];
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let decode = DecodeSessionHandle::from_parts(9, 1).unwrap();
    let surface = SurfaceHandle::from_parts(7, 1).unwrap();
    let backend_task = tokio::spawn(async move {
        for cycle in 0..2 {
            let HostCommand::OpenDecode { kind, reply } = backend.next_command().await.unwrap()
            else {
                panic!("expected decode open for cycle {cycle}");
            };
            assert_eq!(kind, DecodeKind::Video);
            reply.send(Ok(decode)).unwrap();

            let HostCommand::Decode { request, reply, .. } = backend.next_command().await.unwrap()
            else {
                panic!("expected stream start for cycle {cycle}");
            };
            assert_eq!(
                request.stream_action,
                astra_platform::DecodeStreamAction::Start
            );
            reply
                .send(Ok(DecodeOutput::VideoStreamStart {
                    duration_estimate_us: Some(40_000),
                    frame_count: Some(2),
                    decoded_byte_count: Some(8),
                }))
                .unwrap();

            let HostCommand::Decode { request, reply, .. } = backend.next_command().await.unwrap()
            else {
                panic!("expected first frame for cycle {cycle}");
            };
            assert_eq!(
                request.stream_action,
                astra_platform::DecodeStreamAction::Next
            );
            reply
                .send(Ok(DecodeOutput::VideoFrame {
                    sequence: 1,
                    pts_us: 0,
                    duration_us: 20_000,
                    width: 1,
                    height: 1,
                    bgra8: first.clone().into(),
                }))
                .unwrap();

            {
                let HostCommand::PresentScene { frame, reply, .. } =
                    backend.next_command().await.unwrap()
                else {
                    panic!("expected first frame presentation");
                };
                assert!(frame.commands.iter().any(|command| matches!(command,
                    astra_media_core::SceneCommand::VideoFrame { id, frame, .. }
                    if id == "vn.movie.video" && frame.rgba8.to_vec() == vec![3, 2, 1, 255]
                )));
                reply.send(Ok(())).unwrap();
            }

            let HostCommand::Decode { request, reply, .. } = backend.next_command().await.unwrap()
            else {
                panic!("expected second frame for cycle {cycle}");
            };
            assert_eq!(
                request.stream_action,
                astra_platform::DecodeStreamAction::Next
            );
            reply
                .send(Ok(DecodeOutput::VideoFrame {
                    sequence: 2,
                    pts_us: 20_000,
                    duration_us: 20_000,
                    width: 1,
                    height: 1,
                    bgra8: second.clone().into(),
                }))
                .unwrap();

            if cycle == 1 {
                let HostCommand::PresentScene { frame, reply, .. } =
                    backend.next_command().await.unwrap()
                else {
                    panic!("expected restored second frame presentation");
                };
                assert!(frame.commands.iter().any(|command| matches!(command,
                    astra_media_core::SceneCommand::VideoFrame { id, frame, .. }
                    if id == "vn.movie.video" && frame.rgba8.to_vec() == vec![6, 5, 4, 255]
                )));
                reply.send(Ok(())).unwrap();
                let HostCommand::Decode { request, reply, .. } =
                    backend.next_command().await.unwrap()
                else {
                    panic!("expected restored stream end");
                };
                assert_eq!(
                    request.stream_action,
                    astra_platform::DecodeStreamAction::Next
                );
                reply
                    .send(Ok(DecodeOutput::VideoStreamEnd {
                        frame_count: 2,
                        decoded_byte_count: 8,
                    }))
                    .unwrap();
            }

            let HostCommand::CloseDecode { reply, .. } = backend.next_command().await.unwrap()
            else {
                panic!("expected decode close for cycle {cycle}");
            };
            reply.send(Ok(())).unwrap();
        }
        match backend.next_command().await.unwrap() {
            HostCommand::PresentScene { frame, reply, .. } => {
                assert!(!frame.commands.is_empty());
                reply.send(Ok(())).unwrap();
            }
            command => panic!("unexpected completion command: {}", command.operation()),
        }
    });
    let mut sink = PlatformCommandSink::new(client);
    sink.bind_surface(PlayerHostResourceId(1), surface).unwrap();
    let mut executor = PlayerHostCommandExecutor::new(sink);
    let mut media = NativeVnProductMediaHost::default();
    media
        .process(&mut source, &mut executor, 0, Vec::new())
        .await
        .unwrap();
    assert!(media.is_active());
    let snapshot = media.snapshot();
    media.shutdown(&mut source, &mut executor).await.unwrap();
    let mut restored = NativeVnProductMediaHost::default();
    restored.restore(snapshot).unwrap();
    restored
        .poll_and_process(&mut source, &mut executor, 10_000)
        .await
        .unwrap();
    assert_eq!(restored.snapshot().playback_time_ms, 0);
    assert_eq!(restored.snapshot().active_videos[0].next_frame, 1);
    restored
        .poll_and_process(&mut source, &mut executor, 10_019)
        .await
        .unwrap();
    assert_eq!(restored.snapshot().active_videos[0].next_frame, 1);
    restored
        .poll_and_process(&mut source, &mut executor, 10_020)
        .await
        .unwrap();
    assert_eq!(restored.snapshot().active_videos[0].next_frame, 2);
    assert!(restored.has_active_video());
    source
        .prepare_ui_input(astra_ui_core::UiInputEventKind::Keyboard {
            physical_key: "Enter".into(),
            logical_key: "Enter".into(),
            state: astra_ui_core::UiButtonState::Pressed,
            repeat: false,
            modifiers: 0,
        })
        .unwrap();
    restored
        .poll_and_process(&mut source, &mut executor, 10_021)
        .await
        .unwrap();
    assert!(!restored.has_active_video());
    assert_ne!(
        source.pending_wait().map(|wait| wait.fence.as_str()),
        Some("movie.intro.end")
    );
    backend_task.await.unwrap();
}
