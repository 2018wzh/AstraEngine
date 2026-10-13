use super::package_completion::{deliver_package_result, PackageCompletion, PackageSourceResource};

use super::package_worker::{cancellable_fetch, PackageWorker};

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc, Arc,
    },
    thread,
    time::Duration,
};

use crate::decode::DecodeSession;

use astra_platform::{
    host_channel_with_command_wake, AudioDeviceFormat, AudioOutputHandle, AudioOutputRequest,
    AudioWakeRegistration, CapturedFrame, DecodeSessionHandle, HostCommand, HostLaunchProfile,
    InputState, OpenedAudioOutput, PackageSourceHandle, PackageSourceRequest,
    PlatformBackendChannels, PlatformCommandWakeRegistration, PlatformError, PlatformErrorCode,
    PlatformEvent, PlatformEventKind, PlatformHostProfile, PlatformHostSession, PointerButton,
    SaveTransactionHandle, SurfaceHandle, TouchPhase, WindowHandle,
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
    platform::wayland::EventLoopBuilderExtWayland,
    window::{Window, WindowAttributes, WindowId},
};

mod start;
use start::run_backend;
pub use start::start;
use start::LinuxHostApp;
use start::PackageHostConfig;
use start::SingleInstanceGuard;
mod new;

mod open_user_authorized_package;
use open_user_authorized_package::capture_surface;
use open_user_authorized_package::create_surface;
use open_user_authorized_package::input_state;
use open_user_authorized_package::linux_gamepad_worker;
use open_user_authorized_package::pointer_button;
use open_user_authorized_package::raw_gamepad_event;
use open_user_authorized_package::touch_phase;
use open_user_authorized_package::AudioResource;
use open_user_authorized_package::SurfaceResource;
mod audio_resource;
use audio_resource::default_roots;
use audio_resource::host_error;
use audio_resource::sample_format_rank;
use audio_resource::set_stream_error;
