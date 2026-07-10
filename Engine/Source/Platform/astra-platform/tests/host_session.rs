use astra_platform::{
    host_channel, HostCommand, PlatformErrorCode, PlatformEvent, PlatformEventKind,
    PlatformHostProfile, WindowHandle, WindowRequest,
};

#[tokio::test]
async fn async_client_roundtrips_typed_commands_and_ordered_events() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, mut events) = host_channel(profile, 4, 4).unwrap();
    assert!(backend.try_next_command().unwrap().is_none());

    let create = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .create_window(WindowRequest {
                    title: "AstraPlayer".to_string(),
                    width: 800,
                    height: 600,
                    visible: true,
                })
                .await
        }
    });

    let command = backend.next_command().await.expect("create command");
    match command {
        HostCommand::CreateWindow { request, reply } => {
            assert_eq!(request.width, 800);
            reply
                .send(Ok(WindowHandle::from_parts(1, 1).unwrap()))
                .expect("client still waiting");
        }
        other => panic!("unexpected command: {}", other.operation()),
    }
    assert_eq!(create.await.unwrap().unwrap().parts(), (1, 1));

    backend
        .emit_event(PlatformEvent::new(1, PlatformEventKind::Resumed))
        .unwrap();
    backend
        .emit_event(PlatformEvent::new(
            2,
            PlatformEventKind::WindowFocused {
                window: WindowHandle::from_parts(1, 1).unwrap(),
                focused: true,
            },
        ))
        .unwrap();
    assert_eq!(events.recv().await.unwrap().sequence, 1);
    assert_eq!(events.recv().await.unwrap().sequence, 2);
}

#[tokio::test]
async fn shutdown_is_explicit_and_repeated_shutdown_fails_fast() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let (client, mut backend, _events) = host_channel(profile, 2, 2).unwrap();
    let shutdown = tokio::spawn({
        let client = client.clone();
        async move { client.shutdown().await }
    });
    match backend.next_command().await.unwrap() {
        HostCommand::Shutdown { reply } => reply.send(Ok(())).unwrap(),
        other => panic!("unexpected command: {}", other.operation()),
    }
    shutdown.await.unwrap().unwrap();
    assert_eq!(
        client.shutdown().await.unwrap_err().code,
        PlatformErrorCode::InvalidState
    );
}

#[tokio::test]
async fn cloned_event_emitter_assigns_one_global_monotonic_sequence() {
    let profile = PlatformHostProfile::web_release("nativevn-web", "com.example.game");
    let (_client, backend, mut events) = host_channel(profile, 2, 4).unwrap();
    let first = backend.event_emitter();
    let second = first.clone();
    first.emit(PlatformEventKind::Resumed).unwrap();
    second.emit(PlatformEventKind::Suspended).unwrap();
    assert_eq!(events.recv().await.unwrap().sequence, 1);
    assert_eq!(events.recv().await.unwrap().sequence, 2);
}
