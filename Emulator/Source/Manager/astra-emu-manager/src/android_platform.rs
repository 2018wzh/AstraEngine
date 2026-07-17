use std::{
    ffi::CString,
    io::{Cursor, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};

use android_activity::AndroidApp;
use astra_core::Hash256;
use jni::{
    objects::{JByteArray, JObject, JString, JValue},
    JNIEnv, JavaVM,
};

const BRIDGE_CLASS: &str = "org/astraemu/manager/AstraPlatformBridge";
const MAX_ASSET_BYTES: usize = 1024 * 1024;

struct AndroidContext {
    app: AndroidApp,
    vm: JavaVM,
}

static CONTEXT: OnceLock<AndroidContext> = OnceLock::new();
static PENDING_TREE_GRANTS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static PENDING_LIFECYCLE: OnceLock<Mutex<Vec<AndroidLifecycleState>>> = OnceLock::new();
static PENDING_GAMEPAD_INPUTS: OnceLock<Mutex<Vec<AndroidGamepadInput>>> = OnceLock::new();
static GAMEPAD_QUEUE_OVERFLOWED: AtomicBool = AtomicBool::new(false);
const MAX_PENDING_GAMEPAD_INPUTS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidLifecycleState {
    Resumed,
    Paused,
    AudioFocusLost,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AndroidGamepadInput {
    pub control: &'static str,
    pub pressed: bool,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidPackageIdentity {
    pub package_name: String,
    pub version_code: u64,
    pub apk_signer_digest: Hash256,
    pub native_library_dir: String,
    pub data_directory: String,
    pub sdk_int: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidDocumentEntry {
    pub relative_path: String,
    pub document_uri: String,
    pub modified_ms: i64,
    pub byte_size: u64,
}

pub fn initialize(app: AndroidApp) -> Result<(), String> {
    let vm =
        unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|_| "ASTRA_EMU_ANDROID_JVM")?;
    CONTEXT
        .set(AndroidContext { app, vm })
        .map_err(|_| "ASTRA_EMU_ANDROID_CONTEXT_DUPLICATE".to_owned())?;
    Ok(())
}

fn context() -> Result<&'static AndroidContext, String> {
    CONTEXT
        .get()
        .ok_or("ASTRA_EMU_ANDROID_CONTEXT_MISSING".into())
}

pub fn read_asset(path: &str) -> Result<Vec<u8>, String> {
    if path.is_empty() || path.len() > 512 || path.contains("..") || path.starts_with('/') {
        return Err("ASTRA_EMU_ANDROID_ASSET_PATH".into());
    }
    let path = CString::new(path).map_err(|_| "ASTRA_EMU_ANDROID_ASSET_PATH")?;
    let mut asset = context()?
        .app
        .asset_manager()
        .open(&path)
        .ok_or("ASTRA_EMU_ANDROID_ASSET_MISSING")?;
    if asset.get_length() as usize > MAX_ASSET_BYTES {
        return Err("ASTRA_EMU_ANDROID_ASSET_BOUNDS".into());
    }
    let mut bytes = Vec::with_capacity(asset.get_length() as usize);
    asset
        .read_to_end(&mut bytes)
        .map_err(|_| "ASTRA_EMU_ANDROID_ASSET_READ")?;
    if bytes.len() > MAX_ASSET_BYTES {
        return Err("ASTRA_EMU_ANDROID_ASSET_BOUNDS".into());
    }
    Ok(bytes)
}

pub fn package_identity() -> Result<AndroidPackageIdentity, String> {
    let bytes = call_bridge_bytes("packageIdentity", "(Landroid/app/Activity;)[B", &[])?;
    decode_identity(&bytes)
}

pub fn request_document_tree() -> Result<(), String> {
    let ctx = context()?;
    let mut env = ctx
        .vm
        .attach_current_thread()
        .map_err(|_| "ASTRA_EMU_ANDROID_JNI_ATTACH")?;
    let activity = unsafe { JObject::from_raw(ctx.app.activity_as_ptr().cast()) };
    env.call_method(&activity, "requestDocumentTree", "()V", &[])
        .map_err(|_| clear_jni_error(&mut env, "ASTRA_EMU_ANDROID_SAF_REQUEST"))?;
    Ok(())
}

pub fn set_game_mode(enabled: bool) -> Result<(), String> {
    let ctx = context()?;
    let mut env = ctx
        .vm
        .attach_current_thread()
        .map_err(|_| "ASTRA_EMU_ANDROID_JNI_ATTACH")?;
    let activity = unsafe { JObject::from_raw(ctx.app.activity_as_ptr().cast()) };
    env.call_method(
        &activity,
        "setGameMode",
        "(Z)V",
        &[JValue::Bool(u8::from(enabled))],
    )
    .map_err(|_| clear_jni_error(&mut env, "ASTRA_EMU_ANDROID_GAME_MODE"))?;
    Ok(())
}

pub fn store_secret(reference: &str, secret: &str) -> Result<(), String> {
    validate_secret(reference, secret)?;
    call_bridge_secret_method("storeSecret", reference, Some(secret)).map(|_| ())
}

pub fn resolve_secret(reference: &str) -> Result<String, String> {
    validate_secret_reference(reference)?;
    call_bridge_secret_method("resolveSecret", reference, None)
}

fn call_bridge_secret_method(
    method: &str,
    reference: &str,
    secret: Option<&str>,
) -> Result<String, String> {
    let ctx = context()?;
    let mut env = ctx
        .vm
        .attach_current_thread()
        .map_err(|_| "ASTRA_EMU_ANDROID_JNI_ATTACH")?;
    let activity = unsafe { JObject::from_raw(ctx.app.activity_as_ptr().cast()) };
    let reference = env
        .new_string(reference)
        .map_err(|_| "ASTRA_EMU_ANDROID_SECRET_REFERENCE")?;
    let secret_string = secret
        .map(|secret| env.new_string(secret))
        .transpose()
        .map_err(|_| "ASTRA_EMU_ANDROID_SECRET_VALUE")?;
    let result = if let Some(secret) = secret_string.as_ref() {
        env.call_static_method(
            BRIDGE_CLASS,
            method,
            "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[
                JValue::Object(&activity),
                JValue::Object(&reference),
                JValue::Object(secret),
            ],
        )
    } else {
        env.call_static_method(
            BRIDGE_CLASS,
            method,
            "(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&activity), JValue::Object(&reference)],
        )
    };
    let object = result
        .map_err(|_| clear_jni_error(&mut env, "ASTRA_EMU_ANDROID_SECRET_STORE"))?
        .l()
        .map_err(|_| "ASTRA_EMU_ANDROID_SECRET_STORE")?;
    if object.is_null() {
        return Ok(String::new());
    }
    env.get_string(&JString::from(object))
        .map(|value| value.into())
        .map_err(|_| "ASTRA_EMU_ANDROID_SECRET_STORE".into())
}

pub fn take_pending_tree_grants() -> Result<Vec<String>, String> {
    let mut pending = PENDING_TREE_GRANTS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_QUEUE_LOCK")?;
    Ok(std::mem::take(&mut *pending))
}

pub fn take_pending_lifecycle() -> Result<Vec<AndroidLifecycleState>, String> {
    let mut pending = PENDING_LIFECYCLE
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|_| "ASTRA_EMU_ANDROID_LIFECYCLE_QUEUE_LOCK")?;
    Ok(std::mem::take(&mut *pending))
}

pub fn take_pending_gamepad_inputs() -> Result<Vec<AndroidGamepadInput>, String> {
    if GAMEPAD_QUEUE_OVERFLOWED.swap(false, Ordering::AcqRel) {
        PENDING_GAMEPAD_INPUTS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .map_err(|_| "ASTRA_EMU_ANDROID_GAMEPAD_QUEUE_LOCK")?
            .clear();
        return Err("ASTRA_EMU_ANDROID_GAMEPAD_QUEUE_BOUNDS".into());
    }
    let mut pending = PENDING_GAMEPAD_INPUTS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|_| "ASTRA_EMU_ANDROID_GAMEPAD_QUEUE_LOCK")?;
    Ok(std::mem::take(&mut *pending))
}

pub fn enumerate_tree(
    tree_uri: &str,
    max_entries: usize,
    max_encoded_bytes: usize,
) -> Result<Vec<AndroidDocumentEntry>, String> {
    validate_content_uri(tree_uri)?;
    let ctx = context()?;
    let mut env = ctx
        .vm
        .attach_current_thread()
        .map_err(|_| "ASTRA_EMU_ANDROID_JNI_ATTACH")?;
    let activity = unsafe { JObject::from_raw(ctx.app.activity_as_ptr().cast()) };
    let uri = env
        .new_string(tree_uri)
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_URI")?;
    let result = env.call_static_method(
        BRIDGE_CLASS,
        "enumerateTree",
        "(Landroid/app/Activity;Ljava/lang/String;II)[B",
        &[
            JValue::Object(&activity),
            JValue::Object(&uri),
            JValue::Int(i32::try_from(max_entries).map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS")?),
            JValue::Int(
                i32::try_from(max_encoded_bytes).map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS")?,
            ),
        ],
    );
    let object = result
        .map_err(|_| clear_jni_error(&mut env, "ASTRA_EMU_ANDROID_SAF_ENUMERATE"))?
        .l()
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_ENUMERATE")?;
    let bytes = env
        .convert_byte_array(JByteArray::from(object))
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_ENUMERATE")?;
    decode_document_entries(&bytes, max_entries, max_encoded_bytes)
}

pub fn read_document(document_uri: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    validate_content_uri(document_uri)?;
    let max_bytes = i32::try_from(max_bytes).map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS")?;
    let ctx = context()?;
    let mut env = ctx
        .vm
        .attach_current_thread()
        .map_err(|_| "ASTRA_EMU_ANDROID_JNI_ATTACH")?;
    let activity = unsafe { JObject::from_raw(ctx.app.activity_as_ptr().cast()) };
    let uri = env
        .new_string(document_uri)
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_URI")?;
    let result = env.call_static_method(
        BRIDGE_CLASS,
        "readDocument",
        "(Landroid/app/Activity;Ljava/lang/String;I)[B",
        &[
            JValue::Object(&activity),
            JValue::Object(&uri),
            JValue::Int(max_bytes),
        ],
    );
    let object = result
        .map_err(|_| clear_jni_error(&mut env, "ASTRA_EMU_ANDROID_SAF_READ"))?
        .l()
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_READ")?;
    let bytes = env
        .convert_byte_array(JByteArray::from(object))
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_READ")?;
    if bytes.len() > max_bytes as usize {
        return Err("ASTRA_EMU_ANDROID_SAF_BOUNDS".into());
    }
    Ok(bytes)
}

pub fn read_document_range(
    document_uri: &str,
    expected_size: u64,
    expected_modified_ms: i64,
    offset: u64,
    length: u32,
) -> Result<Vec<u8>, String> {
    validate_content_uri(document_uri)?;
    let expected_size = i64::try_from(expected_size).map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS")?;
    let offset = i64::try_from(offset).map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS")?;
    let length = i32::try_from(length).map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS")?;
    let ctx = context()?;
    let mut env = ctx
        .vm
        .attach_current_thread()
        .map_err(|_| "ASTRA_EMU_ANDROID_JNI_ATTACH")?;
    let activity = unsafe { JObject::from_raw(ctx.app.activity_as_ptr().cast()) };
    let uri = env
        .new_string(document_uri)
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_URI")?;
    let result = env.call_static_method(
        BRIDGE_CLASS,
        "readDocumentRange",
        "(Landroid/app/Activity;Ljava/lang/String;JJJI)[B",
        &[
            JValue::Object(&activity),
            JValue::Object(&uri),
            JValue::Long(expected_size),
            JValue::Long(expected_modified_ms),
            JValue::Long(offset),
            JValue::Int(length),
        ],
    );
    let object = result
        .map_err(|_| clear_jni_error(&mut env, "ASTRA_EMU_ANDROID_SAF_RANGE_READ"))?
        .l()
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_RANGE_READ")?;
    let bytes = env
        .convert_byte_array(JByteArray::from(object))
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_RANGE_READ")?;
    if bytes.len() != length as usize {
        return Err("ASTRA_EMU_ANDROID_SAF_SHORT_READ".into());
    }
    Ok(bytes)
}

fn call_bridge_bytes(
    name: &str,
    signature: &str,
    tail: &[JValue<'_, '_>],
) -> Result<Vec<u8>, String> {
    let ctx = context()?;
    let mut env = ctx
        .vm
        .attach_current_thread()
        .map_err(|_| "ASTRA_EMU_ANDROID_JNI_ATTACH")?;
    let activity = unsafe { JObject::from_raw(ctx.app.activity_as_ptr().cast()) };
    let mut arguments = Vec::with_capacity(1 + tail.len());
    arguments.push(JValue::Object(&activity));
    arguments.extend_from_slice(tail);
    let result = env.call_static_method(BRIDGE_CLASS, name, signature, &arguments);
    let object = result
        .map_err(|_| clear_jni_error(&mut env, "ASTRA_EMU_ANDROID_BRIDGE"))?
        .l()
        .map_err(|_| "ASTRA_EMU_ANDROID_BRIDGE")?;
    env.convert_byte_array(JByteArray::from(object))
        .map_err(|_| "ASTRA_EMU_ANDROID_BRIDGE".into())
}

fn clear_jni_error(env: &mut JNIEnv<'_>, code: &'static str) -> String {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    code.into()
}

fn decode_identity(bytes: &[u8]) -> Result<AndroidPackageIdentity, String> {
    let mut input = Cursor::new(bytes);
    expect_magic(&mut input, b"ASTI1")?;
    let package_name = read_string(&mut input, 255)?;
    let version_code = read_u64(&mut input)?;
    let signer = read_bytes(&mut input, 64 * 1024)?;
    let native_library_dir = read_string(&mut input, 4096)?;
    let data_directory = read_string(&mut input, 4096)?;
    let sdk_int = read_u32(&mut input)?;
    ensure_eof(&mut input)?;
    if signer.is_empty()
        || package_name.is_empty()
        || native_library_dir.is_empty()
        || data_directory.is_empty()
    {
        return Err("ASTRA_EMU_ANDROID_IDENTITY_INVALID".into());
    }
    Ok(AndroidPackageIdentity {
        package_name,
        version_code,
        apk_signer_digest: Hash256::from_sha256(&signer),
        native_library_dir,
        data_directory,
        sdk_int,
    })
}

fn decode_document_entries(
    bytes: &[u8],
    max_entries: usize,
    max_encoded_bytes: usize,
) -> Result<Vec<AndroidDocumentEntry>, String> {
    if bytes.len() > max_encoded_bytes {
        return Err("ASTRA_EMU_ANDROID_SAF_BOUNDS".into());
    }
    let mut input = Cursor::new(bytes);
    expect_magic(&mut input, b"ASTS1")?;
    let count = read_u32(&mut input)? as usize;
    if count > max_entries {
        return Err("ASTRA_EMU_ANDROID_SAF_ENTRY_BOUNDS".into());
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let relative_path = read_string(&mut input, 4096)?;
        validate_relative_path(&relative_path)?;
        let document_uri = read_string(&mut input, 8192)?;
        validate_content_uri(&document_uri)?;
        let modified_ms = read_i64(&mut input)?;
        let byte_size = read_u64(&mut input)?;
        entries.push(AndroidDocumentEntry {
            relative_path,
            document_uri,
            modified_ms,
            byte_size,
        });
    }
    ensure_eof(&mut input)?;
    Ok(entries)
}

fn expect_magic(input: &mut Cursor<&[u8]>, expected: &[u8]) -> Result<(), String> {
    let mut observed = vec![0; expected.len()];
    input
        .read_exact(&mut observed)
        .map_err(|_| "ASTRA_EMU_ANDROID_BRIDGE_PAYLOAD")?;
    if observed != expected {
        return Err("ASTRA_EMU_ANDROID_BRIDGE_PAYLOAD".into());
    }
    Ok(())
}

fn read_u32(input: &mut Cursor<&[u8]>) -> Result<u32, String> {
    let mut bytes = [0; 4];
    input
        .read_exact(&mut bytes)
        .map_err(|_| "ASTRA_EMU_ANDROID_BRIDGE_PAYLOAD")?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_u64(input: &mut Cursor<&[u8]>) -> Result<u64, String> {
    let mut bytes = [0; 8];
    input
        .read_exact(&mut bytes)
        .map_err(|_| "ASTRA_EMU_ANDROID_BRIDGE_PAYLOAD")?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_i64(input: &mut Cursor<&[u8]>) -> Result<i64, String> {
    read_u64(input).map(|value| i64::from_be_bytes(value.to_be_bytes()))
}

fn read_bytes(input: &mut Cursor<&[u8]>, max_len: usize) -> Result<Vec<u8>, String> {
    let len = read_u32(input)? as usize;
    if len > max_len
        || len
            > input
                .get_ref()
                .len()
                .saturating_sub(input.position() as usize)
    {
        return Err("ASTRA_EMU_ANDROID_BRIDGE_PAYLOAD".into());
    }
    let mut bytes = vec![0; len];
    input
        .read_exact(&mut bytes)
        .map_err(|_| "ASTRA_EMU_ANDROID_BRIDGE_PAYLOAD")?;
    Ok(bytes)
}

fn read_string(input: &mut Cursor<&[u8]>, max_len: usize) -> Result<String, String> {
    String::from_utf8(read_bytes(input, max_len)?)
        .map_err(|_| "ASTRA_EMU_ANDROID_BRIDGE_PAYLOAD".into())
}

fn ensure_eof(input: &mut Cursor<&[u8]>) -> Result<(), String> {
    if input.position() as usize != input.get_ref().len() {
        return Err("ASTRA_EMU_ANDROID_BRIDGE_PAYLOAD".into());
    }
    Ok(())
}

fn validate_content_uri(uri: &str) -> Result<(), String> {
    if uri.len() < 11 || uri.len() > 8192 || !uri.starts_with("content://") || uri.contains('\0') {
        return Err("ASTRA_EMU_ANDROID_SAF_URI".into());
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains(':')
        || path
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("ASTRA_EMU_ANDROID_SAF_PATH".into());
    }
    Ok(())
}

fn validate_secret_reference(reference: &str) -> Result<(), String> {
    if reference.is_empty()
        || reference.len() > 128
        || !reference
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err("ASTRA_EMU_ANDROID_SECRET_REFERENCE".into());
    }
    Ok(())
}

fn validate_secret(reference: &str, secret: &str) -> Result<(), String> {
    validate_secret_reference(reference)?;
    if secret.is_empty() || secret.len() > 16 * 1024 {
        return Err("ASTRA_EMU_ANDROID_SECRET_VALUE".into());
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_astraemu_manager_MainActivity_nativeOnDocumentTreeGranted(
    mut env: JNIEnv<'_>,
    _activity: JObject<'_>,
    uri: JString<'_>,
) {
    let result = env
        .get_string(&uri)
        .map(|value| value.into())
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_URI".to_owned())
        .and_then(|uri: String| {
            validate_content_uri(&uri)?;
            PENDING_TREE_GRANTS
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .map_err(|_| "ASTRA_EMU_ANDROID_SAF_QUEUE_LOCK".to_owned())?
                .push(uri);
            Ok(())
        });
    if let Err(code) = result {
        tracing::error!(
            event = "astra.emu.android.saf_grant_rejected",
            diagnostic_code = %code
        );
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_astraemu_manager_MainActivity_nativeOnLifecycleChanged(
    _env: JNIEnv<'_>,
    _activity: JObject<'_>,
    state: i32,
) {
    let state = match state {
        0 => AndroidLifecycleState::Paused,
        1 => AndroidLifecycleState::Resumed,
        2 => AndroidLifecycleState::AudioFocusLost,
        _ => {
            tracing::error!(
                event = "astra.emu.android.lifecycle_rejected",
                diagnostic_code = "ASTRA_EMU_ANDROID_LIFECYCLE_STATE"
            );
            return;
        }
    };
    match PENDING_LIFECYCLE
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
    {
        Ok(mut pending) => {
            if pending.last().copied() != Some(state) {
                pending.push(state);
            }
        }
        Err(_) => tracing::error!(
            event = "astra.emu.android.lifecycle_queue_failed",
            diagnostic_code = "ASTRA_EMU_ANDROID_LIFECYCLE_QUEUE_LOCK"
        ),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_astraemu_manager_MainActivity_nativeOnGamepadInput(
    mut env: JNIEnv<'_>,
    _activity: JObject<'_>,
    control: JString<'_>,
    pressed: u8,
    value: f32,
) {
    let control = env
        .get_string(&control)
        .map(|value| value.into())
        .map_err(|_| "ASTRA_EMU_ANDROID_GAMEPAD_CONTROL".to_owned())
        .and_then(|control: String| match control.as_str() {
            "confirm" => Ok("confirm"),
            "cancel" => Ok("cancel"),
            "up" => Ok("up"),
            "down" => Ok("down"),
            "left" => Ok("left"),
            "right" => Ok("right"),
            _ => Err("ASTRA_EMU_ANDROID_GAMEPAD_CONTROL".to_owned()),
        });
    let result = control.and_then(|control| {
        if !value.is_finite() || pressed > 1 {
            return Err("ASTRA_EMU_ANDROID_GAMEPAD_VALUE".into());
        }
        let mut pending = PENDING_GAMEPAD_INPUTS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .map_err(|_| "ASTRA_EMU_ANDROID_GAMEPAD_QUEUE_LOCK".to_owned())?;
        if pending.len() >= MAX_PENDING_GAMEPAD_INPUTS {
            GAMEPAD_QUEUE_OVERFLOWED.store(true, Ordering::Release);
            return Err("ASTRA_EMU_ANDROID_GAMEPAD_QUEUE_BOUNDS".into());
        }
        pending.push(AndroidGamepadInput {
            control,
            pressed: pressed != 0,
            value,
        });
        Ok(())
    });
    if let Err(code) = result {
        tracing::error!(
            event = "astra.emu.android.gamepad_input_rejected",
            diagnostic_code = %code
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
        output.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        output.extend_from_slice(bytes);
    }

    #[test]
    fn bridge_payload_decoder_rejects_trailing_and_unsafe_paths() {
        let mut payload = b"ASTS1".to_vec();
        payload.extend_from_slice(&1_u32.to_be_bytes());
        write_bytes(&mut payload, b"../escape.bin");
        write_bytes(&mut payload, b"content://provider/document/1");
        payload.extend_from_slice(&0_i64.to_be_bytes());
        payload.extend_from_slice(&1_u64.to_be_bytes());
        assert!(decode_document_entries(&payload, 10, 4096).is_err());

        let mut identity = b"ASTI1".to_vec();
        write_bytes(&mut identity, b"org.astraemu.manager");
        identity.extend_from_slice(&1_u64.to_be_bytes());
        write_bytes(&mut identity, b"certificate");
        write_bytes(&mut identity, b"/native/lib");
        write_bytes(&mut identity, b"/data/files");
        identity.extend_from_slice(&36_u32.to_be_bytes());
        identity.push(0);
        assert!(decode_identity(&identity).is_err());
    }
}
