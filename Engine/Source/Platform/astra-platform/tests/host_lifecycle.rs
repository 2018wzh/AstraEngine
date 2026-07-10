use astra_platform::{
    host_channel, AudioMeter, AudioOutputHandle, DecodeKind, DecodeOutput, DecodeSessionHandle,
    HostCommand, PackageSourceHandle, PlatformDecodeRequest, PlatformHostProfile, RgbaFrame,
    SaveTransactionHandle, SurfaceHandle, WindowHandle,
};

#[tokio::test]
async fn client_exposes_explicit_present_close_and_storage_lifecycle() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 32, 8).unwrap();
    let window = WindowHandle::from_parts(1, 1).unwrap();
    let surface = SurfaceHandle::from_parts(1, 1).unwrap();
    let audio = AudioOutputHandle::from_parts(1, 1).unwrap();
    let save = SaveTransactionHandle::from_parts(1, 1).unwrap();
    let package = PackageSourceHandle::from_parts(1, 1).unwrap();

    let present = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .present_rgba(
                    surface,
                    RgbaFrame {
                        sequence: 1,
                        width: 1,
                        height: 1,
                        rgba8: vec![10, 20, 30, 255],
                    },
                )
                .await
        }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::PresentRgba { frame, reply, .. } => {
            assert_eq!(frame.sequence, 1);
            reply.send(Ok(())).unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    present.await.unwrap().unwrap();

    let write = tokio::spawn({
        let client = client.clone();
        async move { client.write_save(save, vec![1, 2, 3]).await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::WriteSave { bytes, reply, .. } => {
            assert_eq!(bytes, [1, 2, 3]);
            reply.send(Ok(())).unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    write.await.unwrap().unwrap();

    let commit = tokio::spawn({
        let client = client.clone();
        async move { client.commit_save(save).await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::CommitSave { reply, .. } => reply.send(Ok("sha256:save".into())).unwrap(),
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(commit.await.unwrap().unwrap(), "sha256:save");

    let read_save = tokio::spawn({
        let client = client.clone();
        async move { client.read_save("slot-1").await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::ReadSave { slot, reply } => {
            assert_eq!(slot, "slot-1");
            reply.send(Ok(vec![1, 2, 3])).unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(read_save.await.unwrap().unwrap(), [1, 2, 3]);

    let read_package = tokio::spawn({
        let client = client.clone();
        async move { client.read_package_range(package, 8, 4).await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::ReadPackageRange {
            offset,
            length,
            reply,
            ..
        } => {
            assert_eq!((offset, length), (8, 4));
            reply.send(Ok(vec![4, 3, 2, 1])).unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(read_package.await.unwrap().unwrap(), [4, 3, 2, 1]);

    let drain = tokio::spawn({
        let client = client.clone();
        async move { client.drain_audio(audio).await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::DrainAudio { reply, .. } => reply
            .send(Ok(AudioMeter {
                sample_count: 2,
                peak_dbfs: -6.0,
                rms_dbfs: -9.0,
            }))
            .unwrap(),
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(drain.await.unwrap().unwrap().sample_count, 2);

    let open_decode = tokio::spawn({
        let client = client.clone();
        async move { client.open_decode(DecodeKind::Video).await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::OpenDecode { kind, reply } => {
            assert_eq!(kind, DecodeKind::Video);
            reply
                .send(Ok(DecodeSessionHandle::from_parts(1, 1).unwrap()))
                .unwrap();
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    let decode = open_decode.await.unwrap().unwrap();

    let decode_request = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .decode(
                    decode,
                    PlatformDecodeRequest {
                        sequence: 1,
                        kind: DecodeKind::Video,
                        codec: "mp4".to_string(),
                        description: Vec::new(),
                        sample_rate: None,
                        channels: None,
                        coded_width: None,
                        coded_height: None,
                        keyframe: true,
                        bytes: vec![1, 2, 3],
                    },
                )
                .await
        }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::Decode { reply, .. } => reply
            .send(Ok(DecodeOutput::CpuBuffer {
                format: "bgra8:1x1".to_string(),
                bytes: vec![1, 2, 3, 255],
                hash: "sha256:frame".to_string(),
            }))
            .unwrap(),
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert!(matches!(
        decode_request.await.unwrap().unwrap(),
        DecodeOutput::CpuBuffer { .. }
    ));

    for (operation, task) in [
        (
            "decode.close",
            tokio::spawn({
                let client = client.clone();
                async move { client.close_decode(decode).await }
            }),
        ),
        (
            "package.close",
            tokio::spawn({
                let client = client.clone();
                async move { client.close_package(package).await }
            }),
        ),
        (
            "audio.close",
            tokio::spawn({
                let client = client.clone();
                async move { client.close_audio(audio).await }
            }),
        ),
        (
            "surface.destroy",
            tokio::spawn({
                let client = client.clone();
                async move { client.destroy_surface(surface).await }
            }),
        ),
        (
            "window.destroy",
            tokio::spawn({
                let client = client.clone();
                async move { client.destroy_window(window).await }
            }),
        ),
    ] {
        let command = backend.next_command().await.unwrap();
        assert_eq!(command.operation(), operation);
        command.reply_unit(Ok(())).unwrap();
        task.await.unwrap().unwrap();
    }
}
