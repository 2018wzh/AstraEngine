use astra_core::Hash256;
use astra_platform::{
    AudioDeviceFormat, AudioOutputRequest, AudioWakeRegistration, DecodeKind, DecodeOutput,
    PackageCachePolicy, PackageSourcePolicy, PackageSourceRequest, PlatformDecodeRequest,
    PlatformError, PlatformErrorCode,
};
use astra_platform_common::{NativeAudioProducer, NativeAudioQueue};
use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Response, Url};

pub(crate) struct WebAudioOutput {
    context: web_sys::AudioContext,
    node: web_sys::AudioWorkletNode,
    port: web_sys::MessagePort,
    _on_message: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::MessageEvent)>,
}

impl WebAudioOutput {
    pub async fn open(
        request: AudioOutputRequest,
        audio_wake: AudioWakeRegistration,
    ) -> Result<(Self, NativeAudioProducer, AudioDeviceFormat), PlatformError> {
        if request.sample_rate == 0
            || request.channels == 0
            || request.chunk_frames == 0
            || request.max_buffered_frames == 0
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.open",
                "WebAudio requires a non-zero format and bounded chunk queue",
            ));
        }
        let context = web_sys::AudioContext::new().map_err(|_| audio_error("audio.open"))?;
        let format = AudioDeviceFormat {
            sample_rate: context.sample_rate() as u32,
            channels: context.destination().max_channel_count().clamp(1, 2) as u16,
        };
        if format.sample_rate != request.sample_rate || format.channels < request.channels {
            let _ = JsFuture::from(context.close().map_err(|_| audio_error("audio.open"))?).await;
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.open",
                "WebAudio output format changed after capability negotiation",
            ));
        }
        if context.state() != web_sys::AudioContextState::Running {
            let resumed = match context.resume() {
                Ok(promise) => JsFuture::from(promise).await.is_ok(),
                Err(_) => false,
            };
            if !resumed || context.state() != web_sys::AudioContextState::Running {
                if let Ok(promise) = context.close() {
                    let _ = JsFuture::from(promise).await;
                }
                return Err(PlatformError::new(
                    PlatformErrorCode::PermissionDenied,
                    "audio.open",
                    "WebAudio requires a completed user activation handshake",
                ));
            }
        }

        let create = Function::new_with_args(
            "context, channels, capacity",
            "return (async () => { await context.audioWorklet.addModule('astra-audio-worklet.js'); const node = new AudioWorkletNode(context, 'astra-audio-output', {numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [channels], processorOptions: {channels, capacityFrames: capacity}}); node.connect(context.destination); return node; })();",
        );
        let value = match await_promise(create.call3(
            &JsValue::NULL,
            context.as_ref(),
            &JsValue::from_f64(f64::from(request.channels)),
            &JsValue::from_f64(request.max_buffered_frames as f64),
        ))
        .await
        {
            Ok(value) => value,
            Err(error) => {
                if let Ok(promise) = context.close() {
                    let _ = JsFuture::from(promise).await;
                }
                return Err(error);
            }
        };
        let node: web_sys::AudioWorkletNode = match value.dyn_into() {
            Ok(node) => node,
            Err(_) => {
                if let Ok(promise) = context.close() {
                    let _ = JsFuture::from(promise).await;
                }
                return Err(audio_error("audio.open"));
            }
        };
        let port = match node.port() {
            Ok(port) => port,
            Err(_) => {
                let _ = node.disconnect();
                if let Ok(promise) = context.close() {
                    let _ = JsFuture::from(promise).await;
                }
                return Err(audio_error("audio.open"));
            }
        };

        let chunk_samples = request
            .chunk_frames
            .checked_mul(usize::from(request.channels))
            .ok_or_else(|| audio_error("audio.open"))?;
        let chunk_capacity = request.max_buffered_frames.div_ceil(request.chunk_frames);
        let (producer, mut consumer, _telemetry) =
            NativeAudioQueue::create(chunk_capacity, chunk_samples, audio_wake.clone())?;
        let mut scratch = vec![0.0_f32; chunk_samples];
        let refill_port = port.clone();
        let refill_wake = audio_wake;
        let mut sequence = 0_u64;
        let on_message =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                let data = event.data();
                let message_type = Reflect::get(&data, &JsValue::from_str("type"))
                    .ok()
                    .and_then(|value| value.as_string());
                if message_type.as_deref() != Some("refill") {
                    return;
                }
                let filled = consumer.pop_samples(&mut scratch);
                if filled == 0 {
                    consumer.record_underflow();
                    let message = js_sys::Object::new();
                    if Reflect::set(
                        &message,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("empty"),
                    )
                    .is_ok()
                    {
                        let _ = refill_port.post_message(&message);
                    }
                    refill_wake.notify();
                    return;
                }
                let message = js_sys::Object::new();
                if Reflect::set(
                    &message,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("packet"),
                )
                .is_err()
                    || Reflect::set(
                        &message,
                        &JsValue::from_str("sequence"),
                        &JsValue::from_f64(sequence as f64),
                    )
                    .is_err()
                {
                    return;
                }
                let samples = js_sys::Float32Array::from(&scratch[..filled]);
                if Reflect::set(&message, &JsValue::from_str("samples"), samples.as_ref()).is_ok()
                    && refill_port.post_message(&message).is_ok()
                {
                    sequence = sequence.saturating_add(1);
                }
                refill_wake.notify();
            })
                as Box<dyn FnMut(web_sys::MessageEvent)>);
        port.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        if request.start_paused {
            let suspended = match context.suspend() {
                Ok(promise) => JsFuture::from(promise).await.is_ok(),
                Err(_) => false,
            };
            if !suspended || context.state() != web_sys::AudioContextState::Suspended {
                port.set_onmessage(None);
                let _ = node.disconnect();
                if let Ok(promise) = context.close() {
                    let _ = JsFuture::from(promise).await;
                }
                return Err(audio_error("audio.open"));
            }
        }

        Ok((
            Self {
                context,
                node,
                port,
                _on_message: on_message,
            },
            producer,
            AudioDeviceFormat {
                sample_rate: request.sample_rate,
                channels: request.channels,
            },
        ))
    }

    pub async fn pause(&self) -> Result<(), PlatformError> {
        if self.context.state() != web_sys::AudioContextState::Running {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.pause",
                "WebAudio output is not running",
            ));
        }
        JsFuture::from(
            self.context
                .suspend()
                .map_err(|_| audio_error("audio.pause"))?,
        )
        .await
        .map_err(|_| audio_error("audio.pause"))?;
        Ok(())
    }

    pub async fn resume(&self) -> Result<(), PlatformError> {
        if self.context.state() != web_sys::AudioContextState::Suspended {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.resume",
                "WebAudio output is not suspended",
            ));
        }
        JsFuture::from(
            self.context
                .resume()
                .map_err(|_| audio_error("audio.resume"))?,
        )
        .await
        .map_err(|_| audio_error("audio.resume"))?;
        Ok(())
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
        if request.kind != self.kind
            || request.sequence != self.next_sequence
            || request.stream_action != astra_platform::DecodeStreamAction::OneShot
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request kind or sequence is invalid",
            ));
        }
        if request.kind == DecodeKind::Image {
            return Err(PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "decode.submit",
                "Web platform image decode is not owned by the WebCodecs audio/video session",
            ));
        }
        let configuration = serde_json::json!({
            "kind": match request.kind {
                DecodeKind::Audio => "audio",
                DecodeKind::Video => "video",
                DecodeKind::Image => unreachable!("image decode was rejected above"),
            },
            "codec": request.codec,
            "sampleRate": request.sample_rate,
            "numberOfChannels": request.channels,
            "codedWidth": request.coded_width,
            "codedHeight": request.coded_height,
            "keyframe": request.keyframe,
        });
        let function = Function::new_with_args(
            "configuration, description, data",
            "return (async () => { const c = JSON.parse(configuration); const descriptionBytes = new Uint8Array(description); const config = {codec: c.codec}; if (descriptionBytes.length) config.description = descriptionBytes; let resolveOutput, rejectOutput; const output = new Promise((resolve, reject) => { resolveOutput = resolve; rejectOutput = reject; }); let decoder; if (c.kind === 'video') { config.codedWidth = c.codedWidth; config.codedHeight = c.codedHeight; decoder = new VideoDecoder({ output: async frame => { try { const bytes = new Uint8Array(frame.allocationSize({format: 'RGBA'})); await frame.copyTo(bytes, {format: 'RGBA'}); resolveOutput({format: `rgba8:${frame.displayWidth}x${frame.displayHeight}`, bytes}); } catch (error) { rejectOutput(error); } finally { frame.close(); } }, error: rejectOutput }); decoder.configure(config); decoder.decode(new EncodedVideoChunk({type: c.keyframe ? 'key' : 'delta', timestamp: 0, data: new Uint8Array(data)})); } else { config.sampleRate = c.sampleRate; config.numberOfChannels = c.numberOfChannels; decoder = new AudioDecoder({ output: async audio => { try { const channels = audio.numberOfChannels; const frames = audio.numberOfFrames; const planes = []; for (let channel = 0; channel < channels; channel++) { const plane = new Float32Array(frames); await audio.copyTo(plane, {planeIndex: channel, format: 'f32-planar'}); planes.push(plane); } const samples = new Float32Array(frames * channels); for (let frame = 0; frame < frames; frame++) for (let channel = 0; channel < channels; channel++) samples[frame * channels + channel] = planes[channel][frame]; resolveOutput({format: `f32-interleaved:${audio.sampleRate}:${channels}`, bytes: new Uint8Array(samples.buffer)}); } catch (error) { rejectOutput(error); } finally { audio.close(); } }, error: rejectOutput }); decoder.configure(config); decoder.decode(new EncodedAudioChunk({type: 'key', timestamp: 0, data: new Uint8Array(data)})); } try { await decoder.flush(); return await output; } finally { decoder.close(); } })();",
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
        if request.kind == DecodeKind::Audio {
            let mut parts = format.split(':');
            if parts.next() != Some("f32-interleaved") {
                return Err(decode_error());
            }
            let sample_rate = parts
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or_else(decode_error)?;
            let channels = parts
                .next()
                .and_then(|value| value.parse::<u16>().ok())
                .ok_or_else(decode_error)?;
            if parts.next().is_some() || !bytes.len().is_multiple_of(4) {
                return Err(decode_error());
            }
            let samples = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|sample| f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]))
                .collect();
            Ok(DecodeOutput::AudioPcmF32 {
                sample_rate,
                channels,
                samples,
            })
        } else {
            Ok(DecodeOutput::CpuBuffer {
                format,
                bytes: bytes.into(),
            })
        }
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

pub(crate) async fn list_saves(package_id: &str) -> Result<Vec<String>, PlatformError> {
    let function = Function::new_with_args(
        "packageId",
        "return (async () => { const root = await navigator.storage.getDirectory(); let dir; try { dir = await root.getDirectoryHandle(packageId); } catch (error) { if (error && error.name === 'NotFoundError') return []; throw error; } const slots = []; for await (const [name, handle] of dir.entries()) { if (handle.kind === 'file' && name.endsWith('.save')) slots.push(name.slice(0, -5)); } slots.sort(); return slots; })();",
    );
    let value =
        await_promise(function.call1(&JsValue::NULL, &JsValue::from_str(package_id))).await?;
    let values = Array::from(&value);
    let mut slots = Vec::with_capacity(values.length() as usize);
    for value in values.iter() {
        slots.push(value.as_string().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "save.list",
                "Web save store returned a non-string slot identity",
            )
        })?);
    }
    Ok(slots)
}

pub(crate) async fn delete_save(package_id: &str, slot: &str) -> Result<(), PlatformError> {
    let function = Function::new_with_args(
        "packageId, slot",
        "return (async () => { const root = await navigator.storage.getDirectory(); const dir = await root.getDirectoryHandle(packageId); await dir.removeEntry(slot + '.save'); })();",
    );
    await_promise(function.call2(
        &JsValue::NULL,
        &JsValue::from_str(package_id),
        &JsValue::from_str(slot),
    ))
    .await?;
    Ok(())
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
