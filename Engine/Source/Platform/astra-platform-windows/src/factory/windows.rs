use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::accessibility::WindowsAccessibilityBridge;

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
    platform::windows::EventLoopBuilderExtWindows,
    window::{Window, WindowAttributes, WindowId},
};

mod start;
pub use start::start;
pub use start::start_services;
use start::PackageHostConfig;
use start::WindowsHostApp;
mod new;

mod process_commands;

mod start_https_package_open;
use start_https_package_open::capture_surface;
use start_https_package_open::create_surface;
use start_https_package_open::windows_gamepad_worker;
use start_https_package_open::AudioCompletion;
use start_https_package_open::PackageCompletion;
use start_https_package_open::PackageSourceResource;
use start_https_package_open::SurfaceResource;
mod audio_stream;
use audio_stream::AudioResource;
use audio_stream::DecodeResource;
use audio_stream::DecodeWorkerState;
use audio_stream::WindowsVideoStreamState;
mod decode_worker_state;
use decode_worker_state::default_roots;
use decode_worker_state::host_error;
