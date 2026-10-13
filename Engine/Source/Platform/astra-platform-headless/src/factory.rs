mod host_thread;

mod thread_owner;

pub use thread_owner::HeadlessThreadOwner;

use host_thread::spawn_host;

use std::{
    collections::VecDeque,
    fmt::Debug,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use crate::artifact::{ArtifactRecorder, AudioArtifactStream};

use astra_headless_protocol::RendererExecutionIdentity;

#[cfg(feature = "ffmpeg-vcpkg")]
use astra_media::FfmpegDecodedPacket;

use astra_media::{
    DecodeKind as MediaDecodeKind, DecodeOutput as MediaDecodeOutput, DecodeProvider,
    DecodeRequest, ImageDecodeProvider, SymphoniaAudioDecodeProvider,
};

use astra_media_core::{
    CpuRendererProvider, HeadlessRenderer, MediaError, RenderTargetFormat, Renderer2DProvider,
    RendererCreateRequest, SceneCommand,
};

use astra_platform::{
    host_channel, AudioDeviceFormat, AudioOutputHandle, AudioOutputLane, AudioWakeRegistration,
    CapturedFrame, DecodeKind, DecodeOutput, DecodeSessionHandle, HeadlessHostProfile,
    HeadlessReadbackPolicy, HeadlessRenderPolicy, HostCommand, HostLaunchProfile,
    OpenedAudioOutput, PackageSourceHandle, PackageSourceRequest, PlatformError, PlatformErrorCode,
    PlatformHostFactory, PlatformHostSession, RgbaFrame, SaveTransactionHandle, SurfaceHandle,
    WindowHandle,
};

use astra_platform_common::WGPU_TIMESTAMP_RING_SIZE;

use astra_platform_common::{
    AtomicSaveStore, FilePackageSource, ResourceTable, SaveTransaction, WgpuOffscreenRenderer,
    WgpuPendingProfile, WgpuProfiledSubmission,
};

use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};

use sha2::{Digest, Sha256};

#[cfg(test)]
mod tests;

mod headless_platform_factory;
#[cfg(feature = "ffmpeg-vcpkg")]
use headless_platform_factory::validate_ffmpeg_binding;
use headless_platform_factory::AudioState;
use headless_platform_factory::DecodeState;
use headless_platform_factory::HeadlessAudioLane;
pub use headless_platform_factory::HeadlessGpuFrameSample;
pub use headless_platform_factory::HeadlessPerformanceObserver;
pub use headless_platform_factory::HeadlessPlatformFactory;
#[cfg(feature = "ffmpeg-vcpkg")]
use headless_platform_factory::HeadlessVideoStream;
use headless_platform_factory::HostState;
use headless_platform_factory::PackageState;
use headless_platform_factory::PendingGpuProfile;
use headless_platform_factory::PendingScene;
use headless_platform_factory::SurfaceState;
use headless_platform_factory::WindowState;
mod gpu_instance;

mod handle;

mod open_package;
use open_package::decode_session;
use open_package::ensure_increasing;
#[cfg(feature = "ffmpeg-vcpkg")]
use open_package::next_headless_video_output;
#[cfg(feature = "ffmpeg-vcpkg")]
use open_package::open_headless_video_stream;
use open_package::present_rgba;
mod package_range;
use package_range::canonical_json_digest;
use package_range::complete_gpu_sample;
use package_range::elapsed_ns;
use package_range::invalid;
use package_range::media_error;
use package_range::open_https_range;
use package_range::package_range;
use package_range::safe_join;
use package_range::scene_submission_identity;
