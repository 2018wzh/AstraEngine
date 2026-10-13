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

mod web_audio_output;
pub(crate) use web_audio_output::commit_save;
pub(crate) use web_audio_output::read_save;
pub(crate) use web_audio_output::PackageBytes;
pub(crate) use web_audio_output::SaveTransaction;
pub(crate) use web_audio_output::WebAudioOutput;
pub(crate) use web_audio_output::WebDecodeSession;
mod list_saves;
use list_saves::audio_error;
use list_saves::await_decode;
use list_saves::await_promise;
use list_saves::cache_error;
use list_saves::decode_error;
pub(crate) use list_saves::delete_save;
use list_saves::fetch_bytes;
use list_saves::fetch_https_verified;
use list_saves::invalid_origin;
use list_saves::js_error;
use list_saves::linear_to_db;
pub(crate) use list_saves::list_saves;
use list_saves::pick_file;
use list_saves::range_error;
use list_saves::read_verified_cache;
use list_saves::require_policy;
use list_saves::write_verified_cache;
