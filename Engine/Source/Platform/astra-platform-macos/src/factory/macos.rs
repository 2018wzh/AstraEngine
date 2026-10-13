use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::accessibility::MacosAccessibilityBridge;

use astra_media::{DecodeOutput as MediaDecodeOutput, DecodeProvider};

use astra_platform::{
    host_channel_with_command_wake, AudioDeviceFormat, AudioOutputHandle, AudioOutputRequest,
    AudioWakeRegistration, CapturedFrame, DecodeKind, DecodeOutput, DecodeSessionHandle,
    HostCommand, HostLaunchProfile, InputState, OpenedAudioOutput, PackageSourceHandle,
    PackageSourceRequest, PlatformBackendChannels, PlatformCommandWakeRegistration,
    PlatformDecodeRequest, PlatformError, PlatformErrorCode, PlatformEvent, PlatformEventKind,
    PlatformHostProfile, PlatformHostSession, PointerButton, SaveTransactionHandle, SurfaceHandle,
    TouchPhase, WindowHandle,
};

use astra_platform_common::{
    AtomicSaveStore, CachedPackageSource, FilePackageSource, ResourceTable, SaveTransaction,
    VerifiedPackageCache,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use tokio::sync::oneshot;

use winit::{
    application::ApplicationHandler,
    event::{
        ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase as WinitTouchPhase,
        WindowEvent,
    },
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    platform::pump_events::EventLoopExtPumpEvents,
    window::{Window, WindowAttributes, WindowId},
};

mod backend_setup;
pub use backend_setup::start;
pub(super) use backend_setup::BackendSetup;
use backend_setup::MacosHostApp;
pub use backend_setup::MacosHostRunner;
use backend_setup::MacosRunnerApp;
use backend_setup::PackageHostConfig;
use backend_setup::SingleInstanceGuard;
mod new;

mod process_commands;

mod macos_host_app;
use macos_host_app::capture_surface;
use macos_host_app::create_surface;
use macos_host_app::input_state;
use macos_host_app::pointer_button;
use macos_host_app::raw_gamepad_event;
use macos_host_app::touch_phase;
use macos_host_app::AudioResource;
use macos_host_app::PackageCompletion;
use macos_host_app::PackageSourceResource;
use macos_host_app::SurfaceResource;
mod audio_resource;
use audio_resource::decode_image;
use audio_resource::sample_format_rank;
use audio_resource::set_stream_error;
use audio_resource::supported_codec;
use audio_resource::DecodeResource;
mod decode_avfoundation;
use decode_avfoundation::decode_avfoundation;
use decode_avfoundation::default_roots;
use decode_avfoundation::host_error;
use decode_avfoundation::media_decode_error;
