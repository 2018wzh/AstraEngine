use super::*;

use astra_platform::{
    HostLaunchProfile, InputState, PlatformErrorCode, PlatformEventKind, PlatformHostClient,
    PlatformHostFactory, PointerButton, SurfaceHandle, SurfaceRequest, WindowHandle, WindowRequest,
};

use astra_player_core::{
    PlatformCommandSink, PlayerHostCommandExecutor, PlayerHostCommandResult, PlayerHostResourceId,
};

use astra_player_vn::{NativeVnHostCommandSource, NativeVnProductMediaHost};

use astra_ui_core::{
    UiButtonState, UiInputEventKind, UiInsets, UiPoint, UiPointerButton as AstraUiPointerButton,
    UiViewport,
};

use astra_vn_core::VnRunConfig;

use futures_util::future::{select, Either};

use futures_util::FutureExt;

use js_sys::Promise;

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use wasm_bindgen_futures::{spawn_local, JsFuture};

use web_sys::Response;

mod contract;
use contract::WebEvidenceIdentity;
use contract::WebPlayerRuntime;
mod start;
pub use start::start;
mod execute_web_ui_host_request;
use execute_web_ui_host_request::emit_web_evidence;
use execute_web_ui_host_request::execute_web_load;
use execute_web_ui_host_request::execute_web_save;
use execute_web_ui_host_request::execute_web_ui_host_request;
use execute_web_ui_host_request::fetch_bytes;
use execute_web_ui_host_request::log_web_consumed_step;
use execute_web_ui_host_request::next_animation_frame;
use execute_web_ui_host_request::web_player_error;
