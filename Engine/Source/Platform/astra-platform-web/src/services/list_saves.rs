use super::*;

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

pub(super) async fn fetch_bytes(path: &str) -> Result<Vec<u8>, PlatformError> {
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

pub(super) async fn fetch_https_verified(
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

pub(super) async fn read_verified_cache(
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

pub(super) async fn write_verified_cache(
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

pub(super) async fn pick_file() -> Result<Vec<u8>, PlatformError> {
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

pub(super) async fn await_promise(
    value: Result<JsValue, JsValue>,
) -> Result<JsValue, PlatformError> {
    let promise: Promise = value
        .map_err(|_| js_error("browser.storage"))?
        .dyn_into()
        .map_err(|_| js_error("browser.storage"))?;
    JsFuture::from(promise)
        .await
        .map_err(|_| js_error("browser.storage"))
}

pub(super) async fn await_decode(
    value: Result<JsValue, JsValue>,
) -> Result<JsValue, PlatformError> {
    let promise: Promise = value
        .map_err(|_| decode_error())?
        .dyn_into()
        .map_err(|_| decode_error())?;
    JsFuture::from(promise).await.map_err(|_| decode_error())
}

pub(super) fn require_policy(
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

pub(super) fn invalid_origin() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::PermissionDenied,
        "package.open",
        "HTTPS package origin is not allowlisted",
    )
}

pub(super) fn range_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "package.read_range",
        "package range is outside the validated source",
    )
}

pub(super) fn cache_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::IntegrityMismatch,
        "package.https.cache",
        "verified package cache identity is invalid",
    )
}

pub(super) fn js_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Io,
        operation,
        "browser storage operation failed",
    )
}

pub(super) fn linear_to_db(value: f32) -> f32 {
    if value <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * value.log10()
    }
}

pub(super) fn audio_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::DeviceLost,
        operation,
        "WebAudio operation failed",
    )
}

pub(super) fn decode_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::ProviderUnavailable,
        "decode.submit",
        "WebCodecs decode failed",
    )
}
