use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};

use tokio::sync::{mpsc, oneshot};

use astra_media_core::SceneCommand;

use crate::{
    AudioOutputHandle, DecodeSessionHandle, HeadlessHostProfile, HostLaunchProfile,
    MediaFrameHandle, PackageSourceHandle, PackageSourcePolicy, PlatformError, PlatformErrorCode,
    PlatformHostProfile, SaveTransactionHandle, SurfaceHandle, WindowHandle,
};

#[cfg(test)]
mod tests;

mod host_start_future;
pub use host_start_future::AudioCaptureReader;
pub use host_start_future::AudioDeviceFormat;
pub use host_start_future::AudioMeter;
pub use host_start_future::AudioOutputFormat;
pub use host_start_future::AudioOutputLane;
pub use host_start_future::AudioOutputRequest;
pub use host_start_future::AudioWakeRegistration;
pub use host_start_future::CapturedFrame;
pub use host_start_future::DecodeKind;
pub use host_start_future::DecodeOutput;
pub use host_start_future::DecodeStreamAction;
pub use host_start_future::HostStartFuture;
pub use host_start_future::OpenedAudioOutput;
pub use host_start_future::PackageSourceRequest;
pub use host_start_future::PlatformDecodeRequest;
pub use host_start_future::PlatformHostFactory;
pub use host_start_future::RgbaFrame;
pub use host_start_future::SceneFrame;
pub use host_start_future::ScenePresentReceipt;
pub use host_start_future::SurfaceRequest;
pub use host_start_future::UnavailablePlatformFactory;
pub use host_start_future::WindowRequest;
pub use host_start_future::DECODE_STREAM_EOS_DIAGNOSTIC;
mod host_command;
pub use host_command::AudioFocusState;
pub use host_command::GamepadControl;
pub use host_command::HostCommand;
pub use host_command::InputState;
pub use host_command::MemoryPressureLevel;
pub use host_command::PlatformEvent;
pub use host_command::PlatformEventKind;
pub use host_command::PlatformHostClient;
pub use host_command::PointerButton;
pub use host_command::TouchPhase;
pub use host_command::WindowInsets;
mod command_wake_registration;

mod shutdown;
pub use shutdown::host_channel;
pub use shutdown::host_channel_with_command_wake;
use shutdown::queue_closed;
use shutdown::validate_rgba_frame;
pub use shutdown::PlatformBackendChannels;
pub use shutdown::PlatformCommandWakeRegistration;
pub use shutdown::PlatformEventEmitter;
pub use shutdown::PlatformEventStream;
pub use shutdown::PlatformHostSession;
mod validate_scene_frame;
use validate_scene_frame::is_safe_slot;
use validate_scene_frame::validate_package_source;
use validate_scene_frame::validate_scene_frame;
mod https_origin;
use https_origin::https_origin;
