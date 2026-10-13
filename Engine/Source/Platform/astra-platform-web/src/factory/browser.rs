use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use astra_platform::{
    host_channel, AudioOutputHandle, CapturedFrame, DecodeSessionHandle, HostCommand,
    HostLaunchProfile, OpenedAudioOutput, PackageSourceHandle, PlatformBackendChannels,
    PlatformError, PlatformErrorCode, PlatformEventKind, PlatformHostProfile, PlatformHostSession,
    SaveTransactionHandle, SurfaceHandle, WindowHandle,
};

use astra_platform_common::ResourceTable;

use astra_ui_core::{UiSemanticRole, UiSemanticSnapshot, ValidateUi};

use js_sys::{Function, Promise, Reflect};

use wasm_bindgen::{closure::Closure, JsCast, JsValue};

use wasm_bindgen_futures::{spawn_local, JsFuture};

use web_sys::{
    CompositionEvent, Element, Event, EventTarget, HtmlCanvasElement, HtmlElement, KeyboardEvent,
    PointerEvent, TouchEvent, WheelEvent,
};

use crate::services::{
    commit_save, delete_save, list_saves, read_save, PackageBytes, SaveTransaction, WebAudioOutput,
    WebDecodeSession,
};

mod start;
use start::run_backend;
pub use start::start;
use start::AnimationFrameCallback;
use start::BrowserLifecycle;
use start::WebInstanceLock;
mod browser_lifecycle;
use browser_lifecycle::raw_gamepad_event;
use browser_lifecycle::AriaListener;
use browser_lifecycle::CanvasResource;
use browser_lifecycle::GlobalListener;
mod new;

mod bind_events;
use bind_events::aria_role;
use bind_events::capture_surface;
#[cfg(target_arch = "wasm32")]
use bind_events::create_surface;
#[cfg(all(feature = "web-code-check", not(target_arch = "wasm32")))]
use bind_events::create_surface;
use bind_events::pointer_button;
use bind_events::web_error;
use bind_events::SurfaceResource;
