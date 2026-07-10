#![cfg(target_arch = "wasm32")]

use astra_platform::{
    PlatformHostFactory, PlatformHostProfile, RgbaFrame, SurfaceRequest, WindowRequest,
};
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test(async)]
async fn browser_host_owns_canvas_webgpu_present_and_readback() {
    let profile = PlatformHostProfile::web_release("nativevn-web", "com.example.game");
    let session = astra_platform_web::factory().start(profile).await.unwrap();
    let window = session
        .client
        .create_window(WindowRequest {
            title: "Astra Web Host Test".to_string(),
            width: 64,
            height: 64,
            visible: true,
        })
        .await
        .unwrap();
    let surface = session
        .client
        .create_surface(SurfaceRequest {
            window,
            width: 64,
            height: 64,
        })
        .await
        .unwrap();
    let rgba8 = [12, 34, 56, 255].repeat(64 * 64);
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
        .unwrap();
    assert_eq!(
        session.client.capture_surface(surface).await.unwrap().rgba8,
        rgba8
    );
    session.client.destroy_surface(surface).await.unwrap();
    session.client.destroy_window(window).await.unwrap();
    session.client.shutdown().await.unwrap();
}
