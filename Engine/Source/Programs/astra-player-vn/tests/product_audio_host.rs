use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use astra_platform::{
    host_channel, AudioMeter, AudioOutputFormat, AudioOutputHandle, AudioOutputState, HostCommand,
    PlatformHostProfile,
};
use astra_player_core::{
    PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor, PlayerHostResourceId,
};
use astra_player_vn::{
    NativeVnAudioControlRequest, NativeVnAudioRequest, NativeVnHostCommandSource,
    NativeVnProductAudioHost,
};
use astra_vn_core::{compile_astra_sources, AstraSource, VnRunConfig};

fn source() -> NativeVnHostCommandSource {
    let compiled = compile_astra_sources([AstraSource::new(
        "audio.astra",
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line speaker:hero #@id line.one\n",
    )])
    .unwrap();
    NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap()
}

#[tokio::test]
async fn shared_product_audio_host_owns_format_queue_control_and_cleanup() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 16, 16).unwrap();
    let native_output = AudioOutputHandle::from_parts(3, 1).unwrap();
    let backend_task = tokio::spawn(async move {
        match backend.next_command().await.unwrap() {
            HostCommand::QueryAudioOutputFormat { reply } => reply
                .send(Ok(AudioOutputFormat {
                    sample_rate: 48_000,
                    channels: 2,
                }))
                .unwrap(),
            command => panic!("unexpected command: {}", command.operation()),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::OpenAudioOutput { request, reply } => {
                assert_eq!(request.sample_rate, 48_000);
                assert_eq!(request.channels, 2);
                reply.send(Ok(native_output)).unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::QueryAudio { output, reply } => {
                assert_eq!(output, native_output);
                reply
                    .send(Ok(AudioOutputState {
                        queued_frames: 0,
                        submitted_samples: 0,
                        consumed_samples: 0,
                        underflow_count: 64,
                        meter: AudioMeter {
                            sample_count: 0,
                            peak_dbfs: -120.0,
                            rms_dbfs: -120.0,
                        },
                    }))
                    .unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::SubmitAudio { packet, reply, .. } => {
                assert_eq!(packet.sequence, 1);
                assert_eq!(packet.channels, 2);
                assert_eq!(packet.frame_count(), 1_024);
                reply.send(Ok(())).unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::DrainAudio { output, reply } => {
                assert_eq!(output, native_output);
                reply
                    .send(Ok(AudioMeter {
                        sample_count: 2_048,
                        peak_dbfs: -6.0,
                        rms_dbfs: -9.0,
                    }))
                    .unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
        match backend.next_command().await.unwrap() {
            HostCommand::CloseAudio { output, reply } => {
                assert_eq!(output, native_output);
                reply.send(Ok(())).unwrap();
            }
            command => panic!("unexpected command: {}", command.operation()),
        }
    });
    let mut source = source();
    let mut executor = PlayerHostCommandExecutor::new(PlatformCommandSink::new(client));
    let mut host = NativeVnProductAudioHost::default();
    let request = NativeVnAudioRequest {
        command_id: "bgm.main".into(),
        command: "bgm".into(),
        attributes: BTreeMap::from([("loop".into(), "true".into())]),
        asset_id: "asset:/bgm/main".into(),
        codec: "wav".into(),
        encoded_bytes: vec![1],
        encoded_hash: Hash256::from_sha256(&[1]),
    };
    let audio = PlayerDecodedAudio {
        sample_rate: 44_100,
        channels: 1,
        samples: vec![0.25; 4_410],
    };
    let mut signals = BTreeSet::new();

    host.start(&mut source, &mut executor, &request, audio)
        .await
        .unwrap();
    host.pump(&mut source, &mut executor, &mut signals)
        .await
        .unwrap();
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.pause".into(),
            action: "pause".into(),
            target: "bgm.main".into(),
        },
        &mut signals,
    )
    .unwrap();
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.resume".into(),
            action: "resume".into(),
            target: "bgm.main".into(),
        },
        &mut signals,
    )
    .unwrap();
    host.control(
        &NativeVnAudioControlRequest {
            command_id: "audio.stop".into(),
            action: "stop".into(),
            target: "bgm.main".into(),
        },
        &mut signals,
    )
    .unwrap();
    assert!(signals.contains("bgm.main.end"));
    host.shutdown(&mut source, &mut executor).await.unwrap();
    backend_task.await.unwrap();
}
