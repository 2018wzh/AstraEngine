#![cfg(target_arch = "wasm32")]

use astra_platform::{
    AudioOutputRequest, DecodeKind, DecodeOutput, PlatformDecodeRequest, PlatformErrorCode,
    PlatformHostFactory, PlatformHostProfile, RgbaFrame, SurfaceRequest, WindowRequest,
};
use js_sys::{Function, Promise, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test(async)]
async fn same_game_profile_rejects_a_second_browser_host() {
    let profile = PlatformHostProfile::web_release("nativevn-web", "com.example.web-single");
    let first = astra_platform_web::factory()
        .start(profile.clone())
        .await
        .unwrap();
    let error = match astra_platform_web::factory().start(profile).await {
        Ok(_) => panic!("second browser host unexpectedly started"),
        Err(error) => error,
    };
    assert_eq!(error.code, PlatformErrorCode::AlreadyInUse);
    first.client.shutdown().await.unwrap();
}

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

#[wasm_bindgen_test(async)]
async fn browser_host_webaudio_fails_fast_without_user_activation() {
    let profile = PlatformHostProfile::web_release("nativevn-web", "com.example.audio-test");
    let session = astra_platform_web::factory().start(profile).await.unwrap();
    let error = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 1,
            max_buffered_frames: 9_600,
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.code,
        astra_platform::PlatformErrorCode::PermissionDenied
    );
    session.client.shutdown().await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn browser_host_opfs_commit_reload_and_abort_are_transactional() {
    let profile = PlatformHostProfile::web_release("nativevn-web", "com.example.opfs-test");
    let session = astra_platform_web::factory().start(profile).await.unwrap();
    let transaction = session.client.begin_save("slot_main").await.unwrap();
    session
        .client
        .write_save(transaction, b"committed".to_vec())
        .await
        .unwrap();
    let hash = session.client.commit_save(transaction).await.unwrap();
    assert!(hash.starts_with("sha256:"));
    assert_eq!(
        session.client.read_save("slot_main").await.unwrap(),
        b"committed"
    );

    let aborted = session.client.begin_save("slot_main").await.unwrap();
    session
        .client
        .write_save(aborted, b"discarded".to_vec())
        .await
        .unwrap();
    session.client.abort_save(aborted).await.unwrap();
    assert_eq!(
        session.client.read_save("slot_main").await.unwrap(),
        b"committed"
    );
    session.client.shutdown().await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn browser_host_webcodecs_decodes_encoded_video_chunk() {
    let encoded = encoded_vp8_frame().await;
    let profile = PlatformHostProfile::web_release("nativevn-web", "com.example.codec-test");
    let session = astra_platform_web::factory().start(profile).await.unwrap();
    let decoder = session.client.open_decode(DecodeKind::Video).await.unwrap();
    let output = session
        .client
        .decode(
            decoder,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Video,
                codec: "vp8".to_string(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: Some(2),
                coded_height: Some(2),
                keyframe: true,
                bytes: encoded,
            },
        )
        .await
        .unwrap();
    match output {
        DecodeOutput::CpuBuffer {
            format,
            bytes,
            hash,
        } => {
            assert_eq!(format, "rgba8:2x2");
            assert_eq!(bytes.len(), 16);
            assert!(hash.starts_with("sha256:"));
        }
        DecodeOutput::MediaFrame(_) => panic!("WebCodecs test expects copied RGBA output"),
    }
    session.client.close_decode(decoder).await.unwrap();
    session.client.shutdown().await.unwrap();
}

async fn encoded_vp8_frame() -> Vec<u8> {
    let function = Function::new_no_args(
        "return (async () => { let resolveChunk, rejectChunk; const chunkPromise = new Promise((resolve, reject) => { resolveChunk = resolve; rejectChunk = reject; }); const encoder = new VideoEncoder({output: chunk => { const bytes = new Uint8Array(chunk.byteLength); chunk.copyTo(bytes); resolveChunk(bytes); }, error: rejectChunk}); encoder.configure({codec: 'vp8', width: 2, height: 2, framerate: 1}); const rgba = new Uint8Array([255,0,0,255, 0,255,0,255, 0,0,255,255, 255,255,255,255]); const frame = new VideoFrame(rgba, {format: 'RGBA', codedWidth: 2, codedHeight: 2, timestamp: 0}); encoder.encode(frame, {keyFrame: true}); frame.close(); await encoder.flush(); const bytes = await chunkPromise; encoder.close(); return bytes; })();",
    );
    let promise: Promise = function.call0(&JsValue::NULL).unwrap().dyn_into().unwrap();
    let value = JsFuture::from(promise).await.unwrap();
    Uint8Array::new(&value).to_vec()
}
