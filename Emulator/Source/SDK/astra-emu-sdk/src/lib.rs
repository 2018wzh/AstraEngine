//! Optional shared facilities for ported emulator cores, independent of engine sessions.
#[cfg(feature = "audio")]
mod audio;
#[cfg(feature = "audio")]
pub use audio::decode_audio;
#[cfg(feature = "archive")]
mod archive;
#[cfg(feature = "archive")]
mod profile;
#[cfg(feature = "archive")]
pub use profile::{read_game_profile, resolve_game_file};
#[cfg(feature = "cache")]
mod cache;
mod error;
#[cfg(feature = "archive")]
pub use archive::*;
#[cfg(feature = "cache")]
pub use cache::*;
#[cfg(feature = "text")]
mod text;
#[cfg(feature = "image")]
mod texture;

pub use error::CoreError;
#[cfg(feature = "text")]
pub use text::{TextOutline, TextScene, TextSceneLayout};
#[cfg(feature = "image")]
pub use texture::TextureCache;

#[cfg(feature = "video-ffmpeg")]
pub mod video;
