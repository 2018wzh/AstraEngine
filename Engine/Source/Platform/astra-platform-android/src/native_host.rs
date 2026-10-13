use std::{
    collections::BTreeMap,
    sync::{mpsc as std_mpsc, Arc},
    thread,
};

use android_activity::AndroidApp;

use astra_core::Hash256;

use astra_platform::{
    host_channel_with_command_wake, AudioOutputHandle, CapturedFrame, DecodeSessionHandle,
    HostCommand, HostLaunchProfile, InputState, MemoryPressureLevel, OpenedAudioOutput,
    PackageCachePolicy, PackageSourceHandle, PackageSourcePolicy, PackageSourceRequest,
    PlatformBackendChannels, PlatformCommandWakeRegistration, PlatformError, PlatformErrorCode,
    PlatformEvent, PlatformEventKind, PlatformHostSession, PointerButton, SaveTransactionHandle,
    SurfaceHandle, TouchPhase, WindowHandle,
};

use astra_platform_common::{
    AtomicSaveStore, CachedPackageSource, ResourceTable, SaveTransaction, VerifiedPackageCache,
};

use winit::{
    application::ApplicationHandler,
    event::{
        ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase as WinitTouchPhase,
        WindowEvent,
    },
    event_loop::{ActiveEventLoop, ControlFlow},
    window::{Window, WindowAttributes, WindowId},
};

use crate::accessibility::AndroidAccessibilityBridge;

use crate::audio::AndroidAudioResource;

use crate::decode::AndroidDecodeWorker;

#[path = "native_host/surface_core.rs"]
mod surface_core;
pub(super) use surface_core::create_host;
pub(super) use surface_core::AndroidHostApp;
use surface_core::MemoryPackageSource;
use surface_core::PackageCompletion;
use surface_core::PackageSourceResource;
use surface_core::PlayerStart;
use surface_core::SurfaceCore;
use surface_core::SurfaceSlot;
#[path = "native_host/process_command.rs"]
mod process_command;

#[path = "native_host/android_host_app.rs"]
mod android_host_app;
use android_host_app::capture_surface;
use android_host_app::host_error;
use android_host_app::input_state;
use android_host_app::pointer_button;
use android_host_app::touch_phase;
