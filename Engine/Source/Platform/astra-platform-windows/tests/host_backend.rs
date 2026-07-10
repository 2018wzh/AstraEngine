#![cfg(target_os = "windows")]

use astra_platform::{
    PlatformHostFactory, PlatformHostProfile, RgbaFrame, SurfaceRequest, WindowRequest,
};

#[tokio::test]
async fn windows_host_owns_window_surface_present_capture_and_shutdown() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let mut session = astra_platform_windows::factory()
        .start(profile)
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
    session.client.destroy_surface(surface).await.unwrap();
    session.client.destroy_window(window).await.unwrap();
    session.client.shutdown().await.unwrap();
    assert!(session.events.recv().await.is_ok());
}
