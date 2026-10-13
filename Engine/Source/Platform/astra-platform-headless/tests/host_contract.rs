use std::fs;

use std::sync::{atomic::AtomicBool, Arc};

use astra_headless_protocol::ArtifactManifest;

use astra_media_core::{
    BlendMode, MeshDraw2D, MeshMaterial2D, MeshVertex2D, RectI, SceneCommand, SceneCompositing2D,
    TextureFilter2D, TextureFrame,
};

#[cfg(feature = "ffmpeg-vcpkg")]
use astra_platform::DecodeOutput;

use astra_platform::{
    AudioOutputRequest, DecodeKind, HeadlessArtifactRetention, HeadlessHostProfile,
    HeadlessReadbackPolicy, HeadlessRenderPolicy, PackageSourcePolicy, PackageSourceRequest,
    PlatformDecodeRequest, PlatformErrorCode, PlatformHostFactory, RgbaFrame, SceneFrame,
    SurfaceRequest, WindowRequest,
};

use astra_platform_headless::HeadlessPlatformFactory;

use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "host_contract/fixtures.rs"]
mod fixtures;
use fixtures::hash;
use fixtures::spawn_tls_range_server;
#[path = "host_contract/session_contract.rs"]
mod session_contract;
use session_contract::expected_gpu_backend;
#[path = "host_contract/render_audio_contract.rs"]
mod render_audio_contract;
#[cfg(feature = "ffmpeg-vcpkg")]
use render_audio_contract::ffmpeg_video_rejects_one_shot_decode;
#[cfg(feature = "ffmpeg-vcpkg")]
use render_audio_contract::ffmpeg_video_start_enforces_frame_budget_independently_of_stream_budget;
#[path = "host_contract/shutdown_contract.rs"]
mod shutdown_contract;
#[cfg(feature = "ffmpeg-vcpkg")]
use shutdown_contract::ffmpeg_video_stream_returns_complete_ordered_frames_incrementally;
#[path = "host_contract/https_package_source_uses_strict_verified_byte_ranges.rs"]
mod https_package_source_uses_strict_verified_byte_ranges;
