#![cfg(target_os = "windows")]

use astra_platform::{
    PlatformErrorCode, PlatformHostFactory, PlatformHostProfile, RgbaFrame, SurfaceRequest,
    WindowRequest,
};

#[tokio::test]
async fn windows_host_owns_window_surface_present_capture_and_shutdown() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let mut session = astra_platform_windows::factory()
        .start(astra_platform::HostLaunchProfile::platform(profile))
        .await
        .expect("start Windows host");
    let window = session
        .client
        .create_window(WindowRequest {
            title: "Astra Platform Host Test".to_string(),
            width: 64,
            height: 64,
            visible: false,
        })
        .await
        .expect("create window");
    let surface = session
        .client
        .create_surface(SurfaceRequest {
            window,
            width: 64,
            height: 64,
        })
        .await
        .expect("create surface");
    let rgba8 = [32, 64, 128, 255].repeat(64 * 64);
    let out_of_order = session
        .client
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 2,
                width: 64,
                height: 64,
                rgba8: rgba8.clone(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(out_of_order.code, PlatformErrorCode::InvalidState);
    let malformed = session
        .client
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 1,
                width: 64,
                height: 64,
                rgba8: vec![0; 4],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(malformed.code, PlatformErrorCode::IntegrityMismatch);
    session
        .client
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 1,
                width: 64,
                height: 64,
                rgba8: rgba8.clone(),
            },
        )
        .await
        .expect("present frame");
    let capture = session
        .client
        .capture_surface(surface)
        .await
        .expect("capture GPU-uploaded frame");
    assert_eq!(capture.rgba8, rgba8);
    let resized = [8, 16, 32, 255].repeat(32 * 48);
    session
        .client
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 2,
                width: 32,
                height: 48,
                rgba8: resized.clone(),
            },
        )
        .await
        .expect("present resized frame");
    let resized_capture = session.client.capture_surface(surface).await.unwrap();
    assert_eq!((resized_capture.width, resized_capture.height), (32, 48));
    assert_eq!(resized_capture.rgba8, resized);
    session.client.destroy_surface(surface).await.unwrap();
    session.client.destroy_window(window).await.unwrap();
    session.client.shutdown().await.unwrap();
    assert!(session.events.recv().await.is_ok());
}
