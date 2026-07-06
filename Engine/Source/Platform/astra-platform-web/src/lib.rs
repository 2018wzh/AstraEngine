use astra_platform::{PlatformCapabilityReport, PlatformId, ReportBackedPlatformHost, SdkStatus};

pub fn host(target: Option<&str>) -> ReportBackedPlatformHost {
    ReportBackedPlatformHost::new(probe(target))
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    let report = PlatformCapabilityReport::new(
        PlatformId::Web,
        target.map(str::to_string),
        sdk_status(),
        vec!["webgpu".to_string(), "webgl_fallback".to_string()],
        vec!["webcodecs".to_string(), "software_profile".to_string()],
        vec!["webaudio".to_string()],
        vec![
            "opfs".to_string(),
            "indexeddb".to_string(),
            "file_api".to_string(),
            "http_range".to_string(),
        ],
        vec![
            "keyboard".to_string(),
            "mouse".to_string(),
            "touch".to_string(),
            "gamepad".to_string(),
        ],
        vec![
            "browser_launch".to_string(),
            "visibility_resume".to_string(),
            "worker".to_string(),
        ],
        vec![
            "browser_sandbox".to_string(),
            "network_runtime_ai_profile_gated".to_string(),
        ],
    );

    #[cfg(target_arch = "wasm32")]
    {
        if web_probe::browser_present() {
            return report.with_smoke(web_probe::smoke_checks());
        }
    }

    report
}

fn sdk_status() -> SdkStatus {
    #[cfg(target_arch = "wasm32")]
    {
        if web_probe::browser_present() {
            SdkStatus::Present
        } else {
            SdkStatus::Missing
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        SdkStatus::Missing
    }
}

#[cfg(target_arch = "wasm32")]
mod web_probe {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;

    use astra_platform::{PlatformSmokeCheck, PlatformSmokeStatus};

    pub fn browser_present() -> bool {
        web_sys::window()
            .and_then(|window| window.document())
            .is_some()
    }

    pub fn smoke_checks() -> Vec<PlatformSmokeCheck> {
        vec![
            pass("browser_smoke", "window and document objects are available"),
            required(
                "renderer.webgpu_or_webgl",
                renderer_available(),
                "WebGPU or WebGL API is available",
                "neither WebGPU nor WebGL API is available",
            ),
            required(
                "decode.webcodecs",
                webcodecs_available(),
                "WebCodecs audio/video decoder APIs are available",
                "WebCodecs audio/video decoder APIs are unavailable",
            ),
            required(
                "audio.webaudio_unlock",
                webaudio_available(),
                "WebAudio context API is available; user gesture unlock is required at runtime",
                "WebAudio context API is unavailable",
            ),
            required(
                "save.web_storage",
                web_storage_available(),
                "IndexedDB storage is available; OPFS is reported when browser exposes it",
                "neither IndexedDB nor OPFS storage API is available",
            ),
            required(
                "package.web_source",
                package_source_available(),
                "File API and fetch are available for local file and HTTP range package sources",
                "File API or fetch is unavailable for web package sources",
            ),
            optional(
                "input.browser",
                input_available(),
                "keyboard, pointer or touch event APIs are available",
                "browser input event APIs are incomplete",
            ),
            optional(
                "lifecycle.worker_visibility",
                lifecycle_available(),
                "document visibility and Worker APIs are available",
                "document visibility or Worker API is unavailable",
            ),
        ]
    }

    fn required(
        id: &'static str,
        ok: bool,
        pass_summary: &'static str,
        blocked_summary: &'static str,
    ) -> PlatformSmokeCheck {
        smoke(
            id,
            if ok {
                PlatformSmokeStatus::Pass
            } else {
                PlatformSmokeStatus::Blocked
            },
            if ok { pass_summary } else { blocked_summary },
        )
    }

    fn optional(
        id: &'static str,
        ok: bool,
        pass_summary: &'static str,
        warning_summary: &'static str,
    ) -> PlatformSmokeCheck {
        smoke(
            id,
            if ok {
                PlatformSmokeStatus::Pass
            } else {
                PlatformSmokeStatus::Warning
            },
            if ok { pass_summary } else { warning_summary },
        )
    }

    fn pass(id: &'static str, summary: &'static str) -> PlatformSmokeCheck {
        smoke(id, PlatformSmokeStatus::Pass, summary)
    }

    fn smoke(
        id: impl Into<String>,
        status: PlatformSmokeStatus,
        summary: impl Into<String>,
    ) -> PlatformSmokeCheck {
        PlatformSmokeCheck {
            id: id.into(),
            status,
            summary: summary.into(),
        }
    }

    fn renderer_available() -> bool {
        navigator_property("gpu").is_some()
            || has_global("WebGL2RenderingContext")
            || has_global("WebGLRenderingContext")
    }

    pub fn webcodecs_available() -> bool {
        has_global("VideoDecoder") && has_global("AudioDecoder")
    }

    fn webaudio_available() -> bool {
        has_global("AudioContext") || has_global("webkitAudioContext")
    }

    fn web_storage_available() -> bool {
        has_global("indexedDB") || storage_property("getDirectory").is_some()
    }

    fn package_source_available() -> bool {
        has_global("fetch") && has_global("File") && has_global("Blob")
    }

    fn input_available() -> bool {
        has_global("KeyboardEvent")
            && (has_global("PointerEvent") || has_global("MouseEvent") || has_global("TouchEvent"))
    }

    fn lifecycle_available() -> bool {
        has_global("Worker")
            && web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| property(document.as_ref(), "visibilityState"))
                .is_some()
    }

    fn has_global(name: &str) -> bool {
        Reflect::has(&js_sys::global(), &JsValue::from_str(name)).unwrap_or(false)
    }

    fn navigator_property(name: &str) -> Option<JsValue> {
        property(&js_sys::global(), "navigator").and_then(|navigator| property(&navigator, name))
    }

    fn storage_property(name: &str) -> Option<JsValue> {
        navigator_property("storage").and_then(|storage| property(&storage, name))
    }

    fn property(value: &JsValue, name: &str) -> Option<JsValue> {
        let property = Reflect::get(value, &JsValue::from_str(name)).ok()?;
        if property.is_null() || property.is_undefined() {
            None
        } else {
            Some(property)
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use astra_platform::SdkStatus;

    use super::probe;

    #[test]
    fn native_probe_does_not_treat_web_sdk_env_as_browser_evidence() {
        std::env::set_var("ASTRA_WEB_SDK", "1");
        let report = probe(Some("nativevn-web"));
        std::env::remove_var("ASTRA_WEB_SDK");

        assert_eq!(report.sdk_status, SdkStatus::Missing);
        assert!(report.smoke.is_empty());
    }
}
