use astra_platform::{PlatformCapabilityReport, PlatformId, ReportBackedPlatformHost, SdkStatus};

pub fn host(target: Option<&str>) -> ReportBackedPlatformHost {
    ReportBackedPlatformHost::new(probe(target))
}

pub fn browser_smoke_probe(target: Option<&str>) -> Option<PlatformCapabilityReport> {
    #[cfg(target_arch = "wasm32")]
    {
        web_probe::browser_present().then(|| probe(target))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = target;
        None
    }
}

pub async fn browser_smoke_probe_async(target: Option<&str>) -> Option<PlatformCapabilityReport> {
    #[cfg(target_arch = "wasm32")]
    {
        if web_probe::browser_present() {
            return Some(probe(target).with_smoke(web_probe::smoke_checks_async().await));
        }
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = target;
        None
    }
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    PlatformCapabilityReport::new(
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
    )
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
    use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    use astra_platform::{PlatformSmokeCheck, PlatformSmokeEvidence, PlatformSmokeStatus};

    const FLOWER_MP4: &[u8] = include_bytes!("../../../../Fixtures/PublicDomainMedia/flower.mp4");
    const FLOWER_WEBM: &[u8] = include_bytes!("../../../../Fixtures/PublicDomainMedia/flower.webm");
    const TREX_MP3: &[u8] = include_bytes!("../../../../Fixtures/PublicDomainMedia/t-rex-roar.mp3");

    pub fn browser_present() -> bool {
        web_sys::window()
            .and_then(|window| window.document())
            .is_some()
    }

    pub async fn smoke_checks_async() -> Vec<PlatformSmokeCheck> {
        match run_browser_probe().await {
            Ok(checks) => checks,
            Err(err) => vec![smoke(
                "browser_smoke",
                PlatformSmokeStatus::Blocked,
                format!("browser smoke probe failed: {}", js_error_message(&err)),
                Vec::new(),
            )],
        }
    }

    async fn run_browser_probe() -> Result<Vec<PlatformSmokeCheck>, JsValue> {
        let probe = Function::new_with_args("mp4Bytes, webmBytes, mp3Bytes", BROWSER_PROBE_JS);
        let mp4 = Uint8Array::from(FLOWER_MP4);
        let webm = Uint8Array::from(FLOWER_WEBM);
        let mp3 = Uint8Array::from(TREX_MP3);
        let promise = probe
            .call3(&JsValue::NULL, mp4.as_ref(), webm.as_ref(), mp3.as_ref())?
            .dyn_into::<Promise>()?;
        let value = JsFuture::from(promise).await?;
        let checks = Reflect::get(&value, &JsValue::from_str("checks"))?;
        Ok(Array::from(&checks).iter().map(parse_check).collect())
    }

    fn parse_check(value: JsValue) -> PlatformSmokeCheck {
        let id = string_property(&value, "id");
        let status = match string_property(&value, "status").as_str() {
            "pass" => PlatformSmokeStatus::Pass,
            "warning" => PlatformSmokeStatus::Warning,
            _ => PlatformSmokeStatus::Blocked,
        };
        let summary = string_property(&value, "summary");
        let evidence = Reflect::get(&value, &JsValue::from_str("evidence"))
            .ok()
            .map(|entries| {
                Array::from(&entries)
                    .iter()
                    .map(|entry| PlatformSmokeEvidence {
                        key: string_property(&entry, "key"),
                        value: string_property(&entry, "value"),
                    })
                    .filter(|entry| !entry.key.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        PlatformSmokeCheck {
            id,
            status,
            summary,
            evidence,
        }
    }

    fn smoke(
        id: impl Into<String>,
        status: PlatformSmokeStatus,
        summary: impl Into<String>,
        evidence: Vec<PlatformSmokeEvidence>,
    ) -> PlatformSmokeCheck {
        PlatformSmokeCheck {
            id: id.into(),
            status,
            summary: summary.into(),
            evidence,
        }
    }

    fn property(value: &JsValue, name: &str) -> Option<JsValue> {
        let property = Reflect::get(value, &JsValue::from_str(name)).ok()?;
        if property.is_null() || property.is_undefined() {
            None
        } else {
            Some(property)
        }
    }

    fn string_property(value: &JsValue, name: &str) -> String {
        property(value, name)
            .and_then(|value| value.as_string())
            .unwrap_or_default()
    }

    fn js_error_message(value: &JsValue) -> String {
        value
            .as_string()
            .or_else(|| {
                let message = string_property(value, "message");
                (!message.is_empty()).then_some(message)
            })
            .unwrap_or_else(|| "unknown JavaScript error".to_string())
    }

    const BROWSER_PROBE_JS: &str = r#"
return (async () => {
  const checks = [];
  const toArrayBuffer = (bytes) => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const check = (id, status, summary, evidence = {}) => {
    checks.push({
      id,
      status,
      summary,
      evidence: Object.entries(evidence).map(([key, value]) => ({ key, value: String(value) })),
    });
  };
  const run = async (id, fn) => {
    try {
      const result = await fn();
      check(id, "pass", result.summary, result.evidence || {});
    } catch (error) {
      check(id, "blocked", error && error.message ? error.message : String(error), {});
    }
  };
  const hex = (bytes) => Array.from(new Uint8Array(bytes)).map((b) => b.toString(16).padStart(2, "0")).join("");
  const sha256 = async (bytes) => {
    const source = bytes instanceof ArrayBuffer ? bytes : toArrayBuffer(bytes);
    const digest = await crypto.subtle.digest("SHA-256", source);
    return `sha256:${hex(digest)}`;
  };
  const loadMedia = (bytes, type, tag) => new Promise((resolve, reject) => {
    const blob = new Blob([bytes], { type });
    const url = URL.createObjectURL(blob);
    const element = document.createElement(tag);
    element.preload = "metadata";
    element.muted = true;
    const timer = setTimeout(() => {
      URL.revokeObjectURL(url);
      reject(new Error(`${tag} metadata load timed out`));
    }, 8000);
    element.onloadedmetadata = () => {
      clearTimeout(timer);
      const result = {
        duration: Number.isFinite(element.duration) ? element.duration : 0,
        width: element.videoWidth || 0,
        height: element.videoHeight || 0,
      };
      URL.revokeObjectURL(url);
      resolve(result);
    };
    element.onerror = () => {
      clearTimeout(timer);
      URL.revokeObjectURL(url);
      reject(new Error(`${tag} metadata decode failed`));
    };
    element.src = url;
  });
  const indexedDbRoundTrip = (bytes) => new Promise((resolve, reject) => {
    if (!("indexedDB" in globalThis)) {
      reject(new Error("IndexedDB is unavailable"));
      return;
    }
    const name = `astra-stage2-${Date.now()}-${Math.random()}`;
    const open = indexedDB.open(name, 1);
    open.onupgradeneeded = () => open.result.createObjectStore("fixtures");
    open.onerror = () => reject(open.error || new Error("IndexedDB open failed"));
    open.onsuccess = () => {
      const db = open.result;
      const tx = db.transaction("fixtures", "readwrite");
      const store = tx.objectStore("fixtures");
      store.put(toArrayBuffer(bytes), "mp3");
      tx.oncomplete = () => {
        const readTx = db.transaction("fixtures", "readonly");
        const read = readTx.objectStore("fixtures").get("mp3");
        read.onerror = () => {
          db.close();
          indexedDB.deleteDatabase(name);
          reject(read.error || new Error("IndexedDB read failed"));
        };
        read.onsuccess = () => {
          const value = new Uint8Array(read.result || new ArrayBuffer(0));
          db.close();
          const deleteRequest = indexedDB.deleteDatabase(name);
          deleteRequest.onsuccess = () => resolve(value);
          deleteRequest.onerror = () => reject(deleteRequest.error || new Error("IndexedDB delete failed"));
        };
      };
      tx.onerror = () => {
        db.close();
        indexedDB.deleteDatabase(name);
        reject(tx.error || new Error("IndexedDB write failed"));
      };
    };
  });

  await run("browser_smoke", async () => {
    if (typeof window === "undefined" || !window.document || !window.navigator) {
      throw new Error("window, document, or navigator is unavailable");
    }
    return { summary: "browser window, document and navigator objects are live", evidence: { document: "present", secure_context: window.isSecureContext } };
  });

  await run("renderer.browser_context", async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 64;
    canvas.height = 64;
    const gl = canvas.getContext("webgl2", { antialias: false }) || canvas.getContext("webgl", { antialias: false });
    if (!gl) {
      throw new Error("WebGL renderer context could not be created");
    }
    gl.clearColor(0.125, 0.25, 0.5, 1.0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    const pixel = new Uint8Array(4);
    gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
    if (pixel[3] === 0) {
      throw new Error("WebGL renderer context produced an empty pixel");
    }
    return { summary: "browser renderer context rendered and read back a pixel", evidence: { context: gl.constructor.name, pixel_alpha: pixel[3], canvas: "64x64" } };
  });

  await run("decode.browser_media", async () => {
    const mp4 = await loadMedia(mp4Bytes, "video/mp4", "video");
    const webm = await loadMedia(webmBytes, "video/webm", "video");
    const mp3 = await loadMedia(mp3Bytes, "audio/mpeg", "audio");
    if (mp4.width <= 0 || mp4.height <= 0 || webm.width <= 0 || webm.height <= 0 || mp3.duration <= 0) {
      throw new Error("browser media decode did not produce metadata for all fixtures");
    }
    return { summary: "browser media path loaded public MP4, WebM and MP3 fixtures", evidence: { mp4: `${mp4.width}x${mp4.height}`, webm: `${webm.width}x${webm.height}`, mp3_duration: mp3.duration.toFixed(3) } };
  });

  await run("decode.webcodecs_config", async () => {
    if (!("VideoDecoder" in globalThis) || !("AudioDecoder" in globalThis)) {
      throw new Error("WebCodecs VideoDecoder or AudioDecoder API is unavailable");
    }
    const video = await VideoDecoder.isConfigSupported({ codec: "vp8", codedWidth: 960, codedHeight: 540 });
    const audio = await AudioDecoder.isConfigSupported({ codec: "mp3", sampleRate: 44100, numberOfChannels: 2 });
    if (!video.supported && !audio.supported) {
      throw new Error("WebCodecs did not report support for the fixture decode configs");
    }
    return { summary: "WebCodecs config support probe completed", evidence: { video_vp8: video.supported, audio_mp3: audio.supported } };
  });

  await run("audio.webaudio_render", async () => {
    const Ctor = globalThis.OfflineAudioContext || globalThis.webkitOfflineAudioContext;
    if (!Ctor) {
      throw new Error("OfflineAudioContext is unavailable");
    }
    const context = new Ctor(1, 512, 44100);
    const oscillator = context.createOscillator();
    oscillator.frequency.value = 220;
    oscillator.connect(context.destination);
    oscillator.start(0);
    oscillator.stop(0.01);
    const buffer = await context.startRendering();
    if (!buffer || buffer.length === 0 || buffer.sampleRate !== 44100) {
      throw new Error("OfflineAudioContext produced no rendered buffer");
    }
    return { summary: "WebAudio OfflineAudioContext rendered a bounded buffer", evidence: { sample_rate: buffer.sampleRate, frames: buffer.length, channels: buffer.numberOfChannels } };
  });

  await run("save.web_storage_rw", async () => {
    const written = new Uint8Array(mp3Bytes);
    const read = await indexedDbRoundTrip(written);
    if (read.byteLength !== written.byteLength || read[0] !== written[0] || read[read.byteLength - 1] !== written[written.byteLength - 1]) {
      throw new Error("IndexedDB readback did not match written fixture bytes");
    }
    return { summary: "IndexedDB storage passed write/read/delete", evidence: { indexeddb: "rw", bytes: read.byteLength, opfs_available: !!(navigator.storage && navigator.storage.getDirectory) } };
  });

  await run("package.web_source_read", async () => {
    if (!("Blob" in globalThis) || !("File" in globalThis) || !("fetch" in globalThis)) {
      throw new Error("Blob, File, or fetch is unavailable");
    }
    const sourceHash = await sha256(mp4Bytes);
    const file = new File([mp4Bytes], "fixture.astrapkg", { type: "application/octet-stream" });
    const url = URL.createObjectURL(file);
    try {
      const response = await fetch(url);
      const bytes = await response.arrayBuffer();
      const readHash = await sha256(bytes);
      if (sourceHash !== readHash) {
        throw new Error("Blob/File/fetch package source hash mismatch");
      }
      return { summary: "Blob/File/fetch package source was read back by hash", evidence: { bytes: bytes.byteLength, hash: readHash } };
    } finally {
      URL.revokeObjectURL(url);
    }
  });

  const optional = (id, ok, passSummary, warningSummary, evidence = {}) => {
    check(id, ok ? "pass" : "warning", ok ? passSummary : warningSummary, evidence);
  };
  optional("input.browser", "KeyboardEvent" in globalThis && ("PointerEvent" in globalThis || "MouseEvent" in globalThis || "TouchEvent" in globalThis), "browser input event APIs are available", "browser input event APIs are incomplete");
  optional("lifecycle.worker_visibility", "Worker" in globalThis && document.visibilityState !== undefined, "document visibility and Worker APIs are available", "document visibility or Worker API is unavailable");

  return { checks };
})();
"#;
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use astra_platform::SdkStatus;

    use super::{browser_smoke_probe, probe};

    #[test]
    fn native_probe_does_not_treat_web_sdk_env_as_browser_evidence() {
        std::env::set_var("ASTRA_WEB_SDK", "1");
        let report = probe(Some("nativevn-web"));
        std::env::remove_var("ASTRA_WEB_SDK");

        assert_eq!(report.sdk_status, SdkStatus::Missing);
        assert!(report.smoke.is_empty());
        assert!(browser_smoke_probe(Some("nativevn-web")).is_none());
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod browser_tests {
    use astra_platform::{PlatformSmokeStatus, SdkStatus};
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::{browser_smoke_probe_async, probe};

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test(async)]
    async fn browser_probe_reports_required_product_evidence() {
        let report = browser_smoke_probe_async(Some("nativevn-web"))
            .await
            .unwrap_or_else(|| probe(None));

        assert_eq!(report.sdk_status, SdkStatus::Present);
        for required in [
            "browser_smoke",
            "renderer.browser_context",
            "decode.browser_media",
            "decode.webcodecs_config",
            "audio.webaudio_render",
            "save.web_storage_rw",
            "package.web_source_read",
        ] {
            assert!(
                report.smoke.iter().any(|check| check.id == required
                    && check.status == PlatformSmokeStatus::Pass
                    && !check.evidence.is_empty()),
                "missing required browser product evidence {required}: {:?}",
                report.smoke
            );
        }
    }
}
