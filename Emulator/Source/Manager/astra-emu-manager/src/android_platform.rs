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
    jni_sig, jni_str,
    objects::{JByteArray, JObject, JString, JValue},
    strings::JNIStr,
    Env, EnvUnowned, JavaVM,
};

const BRIDGE_CLASS: &JNIStr = jni_str!("org/astraemu/manager/AstraPlatformBridge");
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
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
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
    if asset.length() as usize > MAX_ASSET_BYTES {
        return Err("ASTRA_EMU_ANDROID_ASSET_BOUNDS".into());
    }
    let mut bytes = Vec::with_capacity(asset.length() as usize);
    asset
        .read_to_end(&mut bytes)
        .map_err(|_| "ASTRA_EMU_ANDROID_ASSET_READ")?;
    if bytes.len() > MAX_ASSET_BYTES {
        return Err("ASTRA_EMU_ANDROID_ASSET_BOUNDS".into());
    }
    Ok(bytes)
}

pub fn package_identity() -> Result<AndroidPackageIdentity, String> {
    let bytes = call_bridge_bytes(jni_str!("packageIdentity"), jni_sig!("(Landroid/app/Activity;)[B"), &[])?;
    decode_identity(&bytes)
}

pub fn request_document_tree() -> Result<(), String> {
    let ctx = context()?;
    let activity = ctx.app.activity_as_ptr();
    ctx.vm
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            env.call_method(&activity, jni_str!("requestDocumentTree"), jni_sig!("()V"), &[])
                .map_err(|error| {
                    clear_jni_exception(env);
                    error
                })?;
            Ok(())
        })
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_REQUEST".to_owned())?;
    Ok(())
}

pub fn set_game_mode(enabled: bool) -> Result<(), String> {
    let ctx = context()?;
    let activity = ctx.app.activity_as_ptr();
    ctx.vm
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            env.call_method(&activity, jni_str!("setGameMode"), jni_sig!("(Z)V"), &[JValue::Bool(enabled)])
                .map_err(|error| {
                    clear_jni_exception(env);
                    error
                })?;
            Ok(())
        })
        .map_err(|_| "ASTRA_EMU_ANDROID_GAME_MODE".to_owned())?;
    Ok(())
}

pub fn store_secret(reference: &str, secret: &str) -> Result<(), String> {
    validate_secret(reference, secret)?;
    call_bridge_secret_method(jni_str!("storeSecret"), reference, Some(secret)).map(|_| ())
}

pub fn resolve_secret(reference: &str) -> Result<String, String> {
    validate_secret_reference(reference)?;
    call_bridge_secret_method(jni_str!("resolveSecret"), reference, None)
}

fn call_bridge_secret_method(
    method: &JNIStr,
    reference: &str,
    secret: Option<&str>,
) -> Result<String, String> {
    let ctx = context()?;
    let activity = ctx.app.activity_as_ptr();
    ctx.vm
        .attach_current_thread(|env| -> jni::errors::Result<String> {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            let reference = env.new_string(reference)?;
            let result = if let Some(secret) = secret {
                let secret_string = env.new_string(secret)?;
                env.call_static_method(
                    BRIDGE_CLASS,
                    method,
                    jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
                    &[
                        JValue::Object(&activity),
                        JValue::Object(&reference),
                        JValue::Object(&secret_string),
                    ],
                )
            } else {
                env.call_static_method(
                    BRIDGE_CLASS,
                    method,
                    jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;"),
                    &[JValue::Object(&activity), JValue::Object(&reference)],
                )
            };
            let object = result
                .map_err(|error| {
                    clear_jni_exception(env);
                    error
                })?
                .l()?;
            if object.is_null() {
                return Ok(String::new());
            }
            unsafe { JString::from_raw(env, object.as_raw()) }.try_to_string(env)
        })
        .map_err(|_| "ASTRA_EMU_ANDROID_SECRET_STORE".to_owned())
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
    let max_entries =
        i32::try_from(max_entries).map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS".to_owned())?;
    let max_encoded_bytes = i32::try_from(max_encoded_bytes)
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS".to_owned())?;
    let ctx = context()?;
    let activity = ctx.app.activity_as_ptr();
    let bytes = ctx
        .vm
        .attach_current_thread(|env| -> jni::errors::Result<Vec<u8>> {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            let uri = env.new_string(tree_uri)?;
            let result = env.call_static_method(
                BRIDGE_CLASS,
                jni_str!("enumerateTree"),
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;II)[B"),
                &[
                    JValue::Object(&activity),
                    JValue::Object(&uri),
                    JValue::Int(max_entries),
                    JValue::Int(max_encoded_bytes),
                ],
            );
            let object = result
                .map_err(|error| {
                    clear_jni_exception(env);
                    error
                })?
                .l()?;
            env.convert_byte_array(unsafe { JByteArray::from_raw(env, object.as_raw()) })
        })
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_ENUMERATE".to_owned())?;
    decode_document_entries(&bytes, max_entries as usize, max_encoded_bytes as usize)
}

pub fn read_document(document_uri: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    validate_content_uri(document_uri)?;
    let max_bytes = i32::try_from(max_bytes).map_err(|_| "ASTRA_EMU_ANDROID_SAF_BOUNDS")?;
    let ctx = context()?;
    let activity = ctx.app.activity_as_ptr();
    let bytes = ctx
        .vm
        .attach_current_thread(|env| -> jni::errors::Result<Vec<u8>> {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            let uri = env.new_string(document_uri)?;
            let result = env.call_static_method(
                BRIDGE_CLASS,
                jni_str!("readDocument"),
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;I)[B"),
                &[
                    JValue::Object(&activity),
                    JValue::Object(&uri),
                    JValue::Int(max_bytes),
                ],
            );
            let object = result
                .map_err(|error| {
                    clear_jni_exception(env);
                    error
                })?
                .l()?;
            env.convert_byte_array(unsafe { JByteArray::from_raw(env, object.as_raw()) })
        })
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_READ".to_owned())?;
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
    let activity = ctx.app.activity_as_ptr();
    let bytes = ctx
        .vm
        .attach_current_thread(|env| -> jni::errors::Result<Vec<u8>> {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            let uri = env.new_string(document_uri)?;
            let result = env.call_static_method(
                BRIDGE_CLASS,
                jni_str!("readDocumentRange"),
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;JJJI)[B"),
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
                .map_err(|error| {
                    clear_jni_exception(env);
                    error
                })?
                .l()?;
            env.convert_byte_array(unsafe { JByteArray::from_raw(env, object.as_raw()) })
        })
        .map_err(|_| "ASTRA_EMU_ANDROID_SAF_RANGE_READ".to_owned())?;
    if bytes.len() != length as usize {
        return Err("ASTRA_EMU_ANDROID_SAF_SHORT_READ".into());
    }
    Ok(bytes)
}

fn call_bridge_bytes(
    name: &JNIStr,
    signature: jni::signature::MethodSignature<'_, '_>,
    tail: &[JValue<'_>],
) -> Result<Vec<u8>, String> {
    let ctx = context()?;
    let activity = ctx.app.activity_as_ptr();
    ctx.vm
        .attach_current_thread(|env| -> jni::errors::Result<Vec<u8>> {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            let mut arguments = Vec::with_capacity(1 + tail.len());
            arguments.push(JValue::Object(&activity));
            arguments.extend_from_slice(tail);
            let result = env.call_static_method(BRIDGE_CLASS, name, signature, &arguments);
            let object = result
                .map_err(|error| {
                    clear_jni_exception(env);
                    error
                })?
                .l()?;
            env.convert_byte_array(unsafe { JByteArray::from_raw(env, object.as_raw()) })
        })
        .map_err(|_| "ASTRA_EMU_ANDROID_BRIDGE".to_owned())
}

fn clear_jni_exception(env: &mut Env) {
    if env.exception_check() {
        env.exception_clear();
    }
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
    mut env: EnvUnowned<'_>,
    _activity: JObject<'_>,
    uri: JString<'_>,
) {
    let result = env
        .with_env(|env| uri.try_to_string(env))
        .into_outcome();
    let uri = match result {
        jni::Outcome::Ok(uri) => uri,
        jni::Outcome::Err(_) | jni::Outcome::Panic(_) => {
            tracing::error!(
                event = "astra.emu.android.saf_grant_rejected",
                diagnostic_code = "ASTRA_EMU_ANDROID_SAF_URI"
            );
            return;
        }
    };
    let result = validate_content_uri(&uri).and_then(|()| {
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
    _env: EnvUnowned<'_>,
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
    mut env: EnvUnowned<'_>,
    _activity: JObject<'_>,
    control: JString<'_>,
    pressed: bool,
    value: f32,
) {
    let outcome = env.with_env(|env| control.try_to_string(env)).into_outcome();
    let control_name = match outcome {
        jni::Outcome::Ok(name) => name,
        jni::Outcome::Err(_) | jni::Outcome::Panic(_) => {
            tracing::error!(
                event = "astra.emu.android.gamepad_input_rejected",
                diagnostic_code = "ASTRA_EMU_ANDROID_GAMEPAD_CONTROL"
            );
            return;
        }
    };
    let control = match control_name.as_str() {
        // Canonical ABI key names.
        "enter" => Ok("enter"),
        "escape" => Ok("escape"),
        "arrow_up" => Ok("arrow_up"),
        "arrow_down" => Ok("arrow_down"),
        "arrow_left" => Ok("arrow_left"),
        "arrow_right" => Ok("arrow_right"),
        "space" => Ok("space"),
        _ => Err("ASTRA_EMU_ANDROID_GAMEPAD_CONTROL".to_owned()),
    };
    let result = control.and_then(|control| {
        if !value.is_finite() {
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
            pressed,
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
