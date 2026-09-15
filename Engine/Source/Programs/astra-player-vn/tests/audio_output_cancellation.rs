use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use astra_platform::{
    host_channel, AudioDeviceFormat, AudioOutputHandle, AudioOutputLane, HostCommand,
    OpenedAudioOutput, PlatformError, PlatformErrorCode, PlatformHostProfile,
};
use astra_player_core::{PlatformCommandSink, PlayerHostCommandExecutor};
use astra_player_vn::NativeVnProductAudioHost;
use astra_runtime::{TaskOutcome, TaskScope};

mod support;

struct UnusedLane(Arc<AtomicBool>);
impl Drop for UnusedLane {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl AudioOutputLane for UnusedLane {
    fn wait_for_capacity(&mut self, _: usize, _: &AtomicBool) -> Result<(), PlatformError> {
        panic!("cleanup must not start a mixer")
    }
    fn submit(&mut self, _: Vec<f32>) -> Result<Vec<f32>, PlatformError> {
        panic!("cleanup must not submit audio")
    }
    fn consumed_samples(&self) -> u64 {
        0
    }
    fn underflow_count(&self) -> u64 {
        0
    }
}

fn output(rate: u32, dropped: Arc<AtomicBool>) -> OpenedAudioOutput {
    OpenedAudioOutput {
        handle: AudioOutputHandle::from_parts(3, 1).unwrap(),
        format: AudioDeviceFormat {
            sample_rate: rate,
            channels: 2,
        },
        lane: Box::new(UnusedLane(dropped)),
        capture: None,
    }
}

#[tokio::test]
async fn cancelled_open_and_shutdown_keep_responses_until_endpoint_is_closed() {
    let mut source = support::source_for(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line speaker:hero #@id line.one\n",
    );
    let (client, mut backend, _) = host_channel(
        PlatformHostProfile::windows_release("nativevn-game", "com.example.game"),
        8,
        8,
    )
    .unwrap();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    let mut audio = NativeVnProductAudioHost::default();
    let scope = TaskScope::new();
    let reply = {
        let opening = scope.run(audio.ensure_open(&mut source, &mut executor));
        tokio::pin!(opening);
        let reply = tokio::select! {
            command = backend.next_command() => {
                let HostCommand::OpenAudioOutput { reply, .. } = command.unwrap() else { panic!("expected open") };
                reply
            },
            result = &mut opening => panic!("open completed early: {result:?}"),
        };
        scope.cancel();
        assert_eq!(opening.await, TaskOutcome::Cancelled);
        reply
    };
    let dropped = Arc::new(AtomicBool::new(false));
    reply.send(Ok(output(48_000, dropped.clone()))).unwrap();
    let close_reply = {
        let shutdown = audio.shutdown(&mut source, &mut executor);
        tokio::pin!(shutdown);
        tokio::select! {
            command = backend.next_command() => {
                let HostCommand::CloseAudio { output, reply } = command.unwrap() else { panic!("expected close without another open") };
                assert_eq!(output, AudioOutputHandle::from_parts(3, 1).unwrap());
                reply
            },
            result = &mut shutdown => panic!("shutdown completed early: {result:?}"),
        }
    };
    assert!(dropped.load(Ordering::Acquire));
    close_reply.send(Ok(())).unwrap();
    audio.shutdown(&mut source, &mut executor).await.unwrap();
    // Idempotent cleanup consumes the original close response, without sending another command.
    audio.shutdown(&mut source, &mut executor).await.unwrap();
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[tokio::test]
async fn resuming_open_uses_original_response_and_failed_close_remains_retryable() {
    let mut source = support::source_for(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line speaker:hero #@id line.one\n",
    );
    let (client, mut backend, _) = host_channel(
        PlatformHostProfile::windows_release("nativevn-game", "com.example.game"),
        8,
        8,
    )
    .unwrap();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    let mut audio = NativeVnProductAudioHost::default();
    let reply = {
        let opening = audio.ensure_open(&mut source, &mut executor);
        tokio::pin!(opening);
        tokio::select! {
            command = backend.next_command() => {
                let HostCommand::OpenAudioOutput { reply, .. } = command.unwrap() else { panic!("expected open") };
                reply
            },
            result = &mut opening => panic!("open completed early: {result:?}"),
        }
    };
    let dropped = Arc::new(AtomicBool::new(false));
    reply.send(Ok(output(44_100, dropped.clone()))).unwrap();
    let error = audio
        .ensure_open(&mut source, &mut executor)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("FORMAT_DRIFT"));
    assert!(dropped.load(Ordering::Acquire));
    assert!(audio
        .ensure_open(&mut source, &mut executor)
        .await
        .unwrap_err()
        .to_string()
        .contains("REQUIRES_CLEANUP"));
    for attempt in 0..2 {
        let shutdown = audio.shutdown(&mut source, &mut executor);
        tokio::pin!(shutdown);
        tokio::select! {
            command = backend.next_command() => {
                let HostCommand::CloseAudio { output, reply } = command.unwrap() else { panic!("expected close retry") };
                assert_eq!(output, AudioOutputHandle::from_parts(3, 1).unwrap());
                reply.send(if attempt == 0 {
                    Err(PlatformError::new(PlatformErrorCode::Io, "audio.close", "test close failure"))
                } else { Ok(()) }).unwrap();
            },
            result = &mut shutdown => panic!("shutdown completed early: {result:?}"),
        }
        assert_eq!(shutdown.await.is_err(), attempt == 0);
    }
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}
