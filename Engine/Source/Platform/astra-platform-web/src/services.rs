use astra_core::Hash256;
use astra_platform::{
    AudioMeter, AudioOutputRequest, AudioOutputState, AudioPacket, DecodeKind, DecodeOutput,
    PackageCachePolicy, PackageSourcePolicy, PackageSourceRequest, PlatformDecodeRequest,
    PlatformError, PlatformErrorCode,
};
use js_sys::{Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Response, Url};

pub(crate) struct WebAudioOutput {
    context: web_sys::AudioContext,
    node: web_sys::AudioWorkletNode,
    port: web_sys::MessagePort,
    _on_message: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::MessageEvent)>,
    request: AudioOutputRequest,
    next_sequence: u64,
    pending: std::rc::Rc<std::cell::RefCell<std::collections::BTreeMap<u64, usize>>>,
    queued_frames: std::rc::Rc<std::cell::Cell<usize>>,
    meter: std::rc::Rc<std::cell::RefCell<AudioMeter>>,
    underflow_count: std::rc::Rc<std::cell::Cell<u64>>,
    submitted_samples: u64,
}

impl WebAudioOutput {
    pub async fn open(request: AudioOutputRequest) -> Result<Self, PlatformError> {
        let context = web_sys::AudioContext::new().map_err(|_| audio_error("audio.open"))?;
        if context.state() != web_sys::AudioContextState::Running {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "audio.open",
                "WebAudio requires a completed user activation handshake",
            ));
        }
        let create = Function::new_with_args(
            "context, channels, capacity",
            "return (async () => { await context.audioWorklet.addModule('astra-audio-worklet.js'); const node = new AudioWorkletNode(context, 'astra-audio-output', {numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [channels], processorOptions: {channels, capacityFrames: capacity}}); node.connect(context.destination); return node; })();",
        );
        let value = await_promise(create.call3(
            &JsValue::NULL,
            context.as_ref(),
            &JsValue::from_f64(f64::from(request.channels)),
            &JsValue::from_f64(request.max_buffered_frames as f64),
        ))
        .await?;
        let node: web_sys::AudioWorkletNode =
            value.dyn_into().map_err(|_| audio_error("audio.open"))?;
        let port = node.port().map_err(|_| audio_error("audio.open"))?;
        let pending = std::rc::Rc::new(std::cell::RefCell::new(std::collections::BTreeMap::<
            u64,
            usize,
        >::new()));
        let queued_frames = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let meter = std::rc::Rc::new(std::cell::RefCell::new(AudioMeter {
            sample_count: 0,
            peak_dbfs: -120.0,
            rms_dbfs: -120.0,
        }));
        let underflow_count = std::rc::Rc::new(std::cell::Cell::new(0));
        let on_message = {
            let pending = pending.clone();
            let queued_frames = queued_frames.clone();
            let meter = meter.clone();
            let underflow_count = underflow_count.clone();
            wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                let data = event.data();
                let message_type = Reflect::get(&data, &JsValue::from_str("type"))
                    .ok()
                    .and_then(|value| value.as_string());
                match message_type.as_deref() {
                    Some("consumed") => {
                        if let Some(sequence) = Reflect::get(&data, &JsValue::from_str("sequence"))
                            .ok()
                            .and_then(|value| value.as_f64())
                            .map(|value| value as u64)
                        {
                            if let Some(frames) = pending.borrow_mut().remove(&sequence) {
                                queued_frames.set(queued_frames.get().saturating_sub(frames));
                            }
                        }
                    }
                    Some("meter") => {
                        let sample_count = Reflect::get(&data, &JsValue::from_str("sampleCount"))
                            .ok()
                            .and_then(|value| value.as_f64())
                            .unwrap_or_default() as u64;
                        let peak = Reflect::get(&data, &JsValue::from_str("peak"))
                            .ok()
                            .and_then(|value| value.as_f64())
                            .unwrap_or_default() as f32;
                        let rms = Reflect::get(&data, &JsValue::from_str("rms"))
                            .ok()
                            .and_then(|value| value.as_f64())
                            .unwrap_or_default() as f32;
                        *meter.borrow_mut() = AudioMeter {
                            sample_count,
                            peak_dbfs: linear_to_db(peak),
                            rms_dbfs: linear_to_db(rms),
                        };
                        if let Some(value) =
                            Reflect::get(&data, &JsValue::from_str("underflowCount"))
                                .ok()
                                .and_then(|value| value.as_f64())
                        {
                            underflow_count.set(value as u64);
                        }
                    }
                    _ => {}
                }
            })
                as Box<dyn FnMut(web_sys::MessageEvent)>)
        };
        port.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        Ok(Self {
            context,
            node,
            port,
            _on_message: on_message,
            request,
            next_sequence: 0,
            pending,
            queued_frames,
            meter,
            underflow_count,
            submitted_samples: 0,
        })
    }

    pub fn submit(&mut self, packet: AudioPacket) -> Result<(), PlatformError> {
        if packet.sequence != self.next_sequence || packet.channels != self.request.channels {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.submit",
                "audio packet sequence or channel count is invalid",
            ));
        }
        let frames = packet.frame_count();
        if self.queued_frames.get().saturating_add(frames) > self.request.max_buffered_frames {
            return Err(PlatformError::new(
                PlatformErrorCode::QueueOverflow,
                "audio.submit",
                "WebAudio bounded queue is full",
            ));
        }
        let message = js_sys::Object::new();
        Reflect::set(
            &message,
            &JsValue::from_str("type"),
            &JsValue::from_str("packet"),
        )
        .map_err(|_| audio_error("audio.submit"))?;
        Reflect::set(
            &message,
            &JsValue::from_str("sequence"),
            &JsValue::from_f64(packet.sequence as f64),
        )
        .map_err(|_| audio_error("audio.submit"))?;
        let samples = js_sys::Float32Array::from(packet.samples.as_slice());
        Reflect::set(&message, &JsValue::from_str("samples"), samples.as_ref())
            .map_err(|_| audio_error("audio.submit"))?;
        self.pending.borrow_mut().insert(packet.sequence, frames);
        self.queued_frames
            .set(self.queued_frames.get().saturating_add(frames));
        if self.port.post_message(&message).is_err() {
            self.pending.borrow_mut().remove(&packet.sequence);
            self.queued_frames
                .set(self.queued_frames.get().saturating_sub(frames));
            return Err(audio_error("audio.submit"));
        }
        self.submitted_samples = self
            .submitted_samples
            .saturating_add(packet.samples.len() as u64);
        self.next_sequence += 1;
        Ok(())
    }

    pub async fn drain(&self) -> Result<AudioMeter, PlatformError> {
        let poll_count = self
            .request
            .drain_timeout(self.submitted_samples)
            .as_millis()
            .div_ceil(5)
            .max(1);
        for _ in 0..poll_count {
            if self.queued_frames.get() == 0 {
                let message = js_sys::Object::new();
                Reflect::set(
                    &message,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("meter"),
                )
                .map_err(|_| audio_error("audio.drain"))?;
                self.port
                    .post_message(&message)
                    .map_err(|_| audio_error("audio.drain"))?;
                sleep(5).await?;
                let meter = self.meter.borrow().clone();
                if meter.sample_count >= self.submitted_samples {
                    return Ok(meter);
                }
            } else {
                sleep(5).await?;
            }
        }
        Err(PlatformError::new(
            PlatformErrorCode::DeviceLost,
            "audio.drain",
            "AudioWorklet did not drain before the deadline",
        ))
    }

    pub async fn state(&self) -> Result<AudioOutputState, PlatformError> {
        let message = js_sys::Object::new();
        Reflect::set(
            &message,
            &JsValue::from_str("type"),
            &JsValue::from_str("meter"),
        )
        .map_err(|_| audio_error("audio.query"))?;
        self.port
            .post_message(&message)
            .map_err(|_| audio_error("audio.query"))?;
        sleep(5).await?;
        let meter = self.meter.borrow().clone();
        Ok(AudioOutputState {
            queued_frames: self.queued_frames.get(),
            submitted_samples: self.submitted_samples,
            consumed_samples: meter.sample_count.min(self.submitted_samples),
            underflow_count: self.underflow_count.get(),
            meter,
        })
    }

    pub async fn close(self) -> Result<(), PlatformError> {
        self.port.set_onmessage(None);
        self.node
            .disconnect()
            .map_err(|_| audio_error("audio.close"))?;
        JsFuture::from(
            self.context
                .close()
                .map_err(|_| audio_error("audio.close"))?,
        )
        .await
        .map(|_| ())
        .map_err(|_| audio_error("audio.close"))
    }
}

pub(crate) struct WebDecodeSession {
    kind: DecodeKind,
    next_sequence: u64,
}

impl WebDecodeSession {
    pub fn new(kind: DecodeKind) -> Self {
        Self {
            kind,
            next_sequence: 1,
        }
    }

    pub async fn decode(
        &mut self,
        request: PlatformDecodeRequest,
    ) -> Result<DecodeOutput, PlatformError> {
        if request.kind != self.kind || request.sequence != self.next_sequence {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request kind or sequence is invalid",
            ));
        }
        let configuration = serde_json::json!({
            "kind": match request.kind { DecodeKind::Audio => "audio", DecodeKind::Video => "video" },
            "codec": request.codec,
            "sampleRate": request.sample_rate,
            "numberOfChannels": request.channels,
            "codedWidth": request.coded_width,
            "codedHeight": request.coded_height,
            "keyframe": request.keyframe,
        });
        let function = Function::new_with_args(
            "configuration, description, data",
            "return (async () => { const c = JSON.parse(configuration); const descriptionBytes = new Uint8Array(description); const config = {codec: c.codec}; if (descriptionBytes.length) config.description = descriptionBytes; let resolveOutput, rejectOutput; const output = new Promise((resolve, reject) => { resolveOutput = resolve; rejectOutput = reject; }); let decoder; if (c.kind === 'video') { config.codedWidth = c.codedWidth; config.codedHeight = c.codedHeight; decoder = new VideoDecoder({ output: async frame => { try { const bytes = new Uint8Array(frame.allocationSize({format: 'RGBA'})); await frame.copyTo(bytes, {format: 'RGBA'}); resolveOutput({format: `rgba8:${frame.displayWidth}x${frame.displayHeight}`, bytes}); } catch (error) { rejectOutput(error); } finally { frame.close(); } }, error: rejectOutput }); decoder.configure(config); decoder.decode(new EncodedVideoChunk({type: c.keyframe ? 'key' : 'delta', timestamp: 0, data: new Uint8Array(data)})); } else { config.sampleRate = c.sampleRate; config.numberOfChannels = c.numberOfChannels; decoder = new AudioDecoder({ output: async audio => { try { const planes = []; let total = 0; for (let planeIndex = 0; planeIndex < audio.numberOfChannels; planeIndex++) { const size = audio.allocationSize({planeIndex, format: 'f32-planar'}); const plane = new Uint8Array(size); await audio.copyTo(plane, {planeIndex, format: 'f32-planar'}); planes.push(plane); total += size; } const bytes = new Uint8Array(total); let offset = 0; for (const plane of planes) { bytes.set(plane, offset); offset += plane.length; } resolveOutput({format: `f32-planar:${audio.sampleRate}:${audio.numberOfChannels}`, bytes}); } catch (error) { rejectOutput(error); } finally { audio.close(); } }, error: rejectOutput }); decoder.configure(config); decoder.decode(new EncodedAudioChunk({type: 'key', timestamp: 0, data: new Uint8Array(data)})); } try { await decoder.flush(); return await output; } finally { decoder.close(); } })();",
        );
        let description = Uint8Array::from(request.description.as_slice());
        let bytes = Uint8Array::from(request.bytes.as_slice());
        let result = await_decode(function.call3(
            &JsValue::NULL,
            &JsValue::from_str(&configuration.to_string()),
            description.as_ref(),
            bytes.as_ref(),
        ))
        .await?;
        let format = Reflect::get(&result, &JsValue::from_str("format"))
            .ok()
            .and_then(|value| value.as_string())
            .ok_or_else(decode_error)?;
        let bytes = Uint8Array::new(
            &Reflect::get(&result, &JsValue::from_str("bytes")).map_err(|_| decode_error())?,
        )
        .to_vec();
        self.next_sequence += 1;
        Ok(DecodeOutput::CpuBuffer {
            format,
            hash: Hash256::from_sha256(&bytes).to_string(),
            bytes,
        })
    }
}

pub(crate) struct SaveTransaction {
    pub slot: String,
    pub bytes: Vec<u8>,
}

pub(crate) struct PackageBytes {
    bytes: Vec<u8>,
}

impl PackageBytes {
    pub async fn open(
        source: PackageSourceRequest,
        policies: &[PackageSourcePolicy],
        package_id: &str,
        cache_policy: &PackageCachePolicy,
    ) -> Result<Self, PlatformError> {
        let (bytes, expected_hash) = match source {
            PackageSourceRequest::Bundled {
                relative_path,
                expected_hash,
            } => {
                require_policy(policies, |policy| {
                    matches!(policy, PackageSourcePolicy::Bundled)
                })?;
                (fetch_bytes(&relative_path).await?, expected_hash)
            }
            PackageSourceRequest::UserAuthorized { expected_hash } => {
                require_policy(policies, |policy| {
                    matches!(policy, PackageSourcePolicy::UserAuthorized)
                })?;
                (pick_file().await?, expected_hash)
            }
            PackageSourceRequest::HttpsRange { url, expected_hash } => {
                let parsed = Url::new(&url).map_err(|_| invalid_origin())?;
                if parsed.protocol() != "https:"
                    || !parsed.username().is_empty()
                    || !parsed.password().is_empty()
                {
                    return Err(invalid_origin());
                }
                let origin = parsed.origin();
                let allowed = policies.iter().any(|policy| match policy {
                    PackageSourcePolicy::HttpsRange { allowed_origins } => {
                        allowed_origins.iter().any(|allowed| allowed == &origin)
                    }
                    _ => false,
                });
                if !allowed {
                    return Err(invalid_origin());
                }
                (
                    fetch_https_verified(&url, &origin, &expected_hash, package_id, cache_policy)
                        .await?,
                    expected_hash,
                )
            }
        };
        if Hash256::from_sha256(&bytes).to_string() != expected_hash {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open",
                "package source hash does not match the request",
            ));
        }
        Ok(Self { bytes })
    }

    pub fn read_range(&self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
        let start = usize::try_from(offset).map_err(|_| range_error())?;
        let end = start.checked_add(length).ok_or_else(range_error)?;
        self.bytes
            .get(start..end)
            .map(ToOwned::to_owned)
            .ok_or_else(range_error)
    }
}

pub(crate) async fn commit_save(
    package_id: &str,
    transaction: &SaveTransaction,
) -> Result<String, PlatformError> {
    let bytes = Uint8Array::from(transaction.bytes.as_slice());
    let function = Function::new_with_args(
        "packageId, slot, bytes",
        "return (async () => { const root = await navigator.storage.getDirectory(); const dir = await root.getDirectoryHandle(packageId, {create: true}); const file = await dir.getFileHandle(slot + '.save', {create: true}); const writer = await file.createWritable({keepExistingData: false}); try { await writer.write(bytes); await writer.close(); } catch (error) { try { await writer.abort(); } catch (_) {} throw error; } })();",
    );
    await_promise(function.call3(
        &JsValue::NULL,
        &JsValue::from_str(package_id),
        &JsValue::from_str(&transaction.slot),
        bytes.as_ref(),
    ))
    .await?;
    Ok(Hash256::from_sha256(&transaction.bytes).to_string())
}

pub(crate) async fn read_save(package_id: &str, slot: &str) -> Result<Vec<u8>, PlatformError> {
    let function = Function::new_with_args(
        "packageId, slot",
        "return (async () => { const root = await navigator.storage.getDirectory(); const dir = await root.getDirectoryHandle(packageId); const handle = await dir.getFileHandle(slot + '.save'); return new Uint8Array(await (await handle.getFile()).arrayBuffer()); })();",
    );
    let value = await_promise(function.call2(
        &JsValue::NULL,
        &JsValue::from_str(package_id),
        &JsValue::from_str(slot),
    ))
    .await?;
    Ok(Uint8Array::new(&value).to_vec())
}

async fn fetch_bytes(path: &str) -> Result<Vec<u8>, PlatformError> {
    let window = web_sys::window().ok_or_else(|| js_error("package.open"))?;
    let response = JsFuture::from(window.fetch_with_str(path))
        .await
        .map_err(|_| js_error("package.open"))?;
    let response: Response = response.dyn_into().map_err(|_| js_error("package.open"))?;
    if !response.ok() {
        return Err(PlatformError::new(
            PlatformErrorCode::Io,
            "package.open",
            "package fetch returned a non-success status",
        ));
    }
    let buffer = JsFuture::from(
        response
            .array_buffer()
            .map_err(|_| js_error("package.open"))?,
    )
    .await
    .map_err(|_| js_error("package.open"))?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

async fn fetch_https_verified(
    url: &str,
    origin: &str,
    expected_hash: &str,
    package_id: &str,
    policy: &PackageCachePolicy,
) -> Result<Vec<u8>, PlatformError> {
    if let Some(bytes) = read_verified_cache(package_id, expected_hash).await? {
        if Hash256::from_sha256(&bytes).to_string() == expected_hash {
            return Ok(bytes);
        }
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.https.open",
            "verified OPFS cache entry hash does not match its identity",
        ));
    }
    let window = web_sys::window().ok_or_else(|| js_error("package.https.open"))?;
    let response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|_| js_error("package.https.open"))?
        .dyn_into::<Response>()
        .map_err(|_| js_error("package.https.open"))?;
    if response.redirected() || response.status() != 200 {
        return Err(PlatformError::new(
            PlatformErrorCode::Io,
            "package.https.open",
            "HTTPS package response must be an unredirected complete response",
        ));
    }
    let final_url = Url::new(&response.url()).map_err(|_| invalid_origin())?;
    if final_url.protocol() != "https:" || final_url.origin() != origin {
        return Err(invalid_origin());
    }
    if response
        .headers()
        .get("content-encoding")
        .map_err(|_| js_error("package.https.open"))?
        .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
    {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.https.open",
            "HTTPS package response uses unsupported content encoding",
        ));
    }
    let declared_length = response
        .headers()
        .get("content-length")
        .map_err(|_| js_error("package.https.open"))?
        .ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.https.open",
                "HTTPS package response must declare content length",
            )
        })?
        .parse::<u64>()
        .map_err(|_| js_error("package.https.open"))?;
    if declared_length > policy.max_entry_bytes {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidState,
            "package.https.open",
            "HTTPS package exceeds cache entry limit",
        ));
    }
    let buffer = JsFuture::from(
        response
            .array_buffer()
            .map_err(|_| js_error("package.https.open"))?,
    )
    .await
    .map_err(|_| js_error("package.https.open"))?;
    let bytes = Uint8Array::new(&buffer).to_vec();
    if u64::try_from(bytes.len()).map_err(|_| js_error("package.https.open"))? != declared_length {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.https.open",
            "HTTPS package response is truncated",
        ));
    }
    if Hash256::from_sha256(&bytes).to_string() != expected_hash {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.https.open",
            "HTTPS package hash does not match its declared identity",
        ));
    }
    write_verified_cache(package_id, expected_hash, &bytes).await?;
    Ok(bytes)
}

async fn read_verified_cache(
    package_id: &str,
    expected_hash: &str,
) -> Result<Option<Vec<u8>>, PlatformError> {
    let key = expected_hash
        .strip_prefix("sha256:")
        .ok_or_else(cache_error)?;
    let function = Function::new_with_args(
        "packageId, key",
        "return (async () => { const root = await navigator.storage.getDirectory(); try { const app = await root.getDirectoryHandle(packageId); const cache = await app.getDirectoryHandle('packages'); const file = await cache.getFileHandle(key); return new Uint8Array(await (await file.getFile()).arrayBuffer()); } catch (error) { if (error && error.name === 'NotFoundError') return null; throw error; } })();",
    );
    let value = await_promise(function.call2(
        &JsValue::NULL,
        &JsValue::from_str(package_id),
        &JsValue::from_str(key),
    ))
    .await?;
    if value.is_null() {
        Ok(None)
    } else {
        Ok(Some(Uint8Array::new(&value).to_vec()))
    }
}

async fn write_verified_cache(
    package_id: &str,
    expected_hash: &str,
    bytes: &[u8],
) -> Result<(), PlatformError> {
    let key = expected_hash
        .strip_prefix("sha256:")
        .ok_or_else(cache_error)?;
    let data = Uint8Array::from(bytes);
    let function = Function::new_with_args(
        "packageId, key, bytes",
        "return (async () => { const root = await navigator.storage.getDirectory(); const app = await root.getDirectoryHandle(packageId, {create: true}); const cache = await app.getDirectoryHandle('packages', {create: true}); const file = await cache.getFileHandle(key, {create: true}); const writer = await file.createWritable({keepExistingData: false}); try { await writer.write(bytes); await writer.close(); } catch (error) { try { await writer.abort(); } catch (_) {} throw error; } })();",
    );
    await_promise(function.call3(
        &JsValue::NULL,
        &JsValue::from_str(package_id),
        &JsValue::from_str(key),
        data.as_ref(),
    ))
    .await?;
    Ok(())
}

async fn pick_file() -> Result<Vec<u8>, PlatformError> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Astra package", &["astrapkg"])
        .pick_file()
        .await
        .ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::Cancelled,
                "package.open_user_authorized",
                "user cancelled package selection",
            )
        })?;
    Ok(file.read().await)
}

async fn await_promise(value: Result<JsValue, JsValue>) -> Result<JsValue, PlatformError> {
    let promise: Promise = value
        .map_err(|_| js_error("browser.storage"))?
        .dyn_into()
        .map_err(|_| js_error("browser.storage"))?;
    JsFuture::from(promise)
        .await
        .map_err(|_| js_error("browser.storage"))
}

async fn await_decode(value: Result<JsValue, JsValue>) -> Result<JsValue, PlatformError> {
    let promise: Promise = value
        .map_err(|_| decode_error())?
        .dyn_into()
        .map_err(|_| decode_error())?;
    JsFuture::from(promise).await.map_err(|_| decode_error())
}

fn require_policy(
    policies: &[PackageSourcePolicy],
    predicate: impl Fn(&PackageSourcePolicy) -> bool,
) -> Result<(), PlatformError> {
    if policies.iter().any(predicate) {
        Ok(())
    } else {
        Err(PlatformError::new(
            PlatformErrorCode::PermissionDenied,
            "package.open",
            "package source is not declared by the platform profile",
        ))
    }
}

fn invalid_origin() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::PermissionDenied,
        "package.open",
        "HTTPS package origin is not allowlisted",
    )
}

fn range_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "package.read_range",
        "package range is outside the validated source",
    )
}

fn cache_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::IntegrityMismatch,
        "package.https.cache",
        "verified package cache identity is invalid",
    )
}

fn js_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Io,
        operation,
        "browser storage operation failed",
    )
}

async fn sleep(milliseconds: u32) -> Result<(), PlatformError> {
    let function = Function::new_with_args(
        "milliseconds",
        "return new Promise(resolve => setTimeout(resolve, milliseconds));",
    );
    await_promise(function.call1(&JsValue::NULL, &JsValue::from_f64(f64::from(milliseconds))))
        .await
        .map(|_| ())
}

fn linear_to_db(value: f32) -> f32 {
    if value <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * value.log10()
    }
}

fn audio_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::DeviceLost,
        operation,
        "WebAudio operation failed",
    )
}

fn decode_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::ProviderUnavailable,
        "decode.submit",
        "WebCodecs decode failed",
    )
}
