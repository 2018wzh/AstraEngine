mod audio;
mod movie;
mod provider;
mod scene;
mod session;
mod storage;
mod voice_preferences;

mod archive;
mod image_container;
mod message;
mod paz;
mod profile;
mod runtime;
pub use message::*;
mod script;
mod text_renderer;

pub use archive::*;
pub use astra_emu_sdk::CoreError;
pub use image_container::*;
pub use paz::*;
pub use profile::*;
pub use runtime::*;
pub use script::*;
pub use text_renderer::MusicaTextRenderer;

pub const MUSICA_READER_ID: &str = "astra.emu.musica.paz.v1";
pub const MUSICA_DECRYPT_PROVIDER_ID: &str = "astra.emu.musica.paz.decrypt.v1";
pub const MUSICA_DECRYPT_DESCRIPTOR_SCHEMA: &str = "astra.emu.musica.paz.decrypt_descriptor.v1";
pub const MUSICA_SCRIPT_IR_SCHEMA: &str = "astra.emu.musica.script_ir.v3";

pub use provider::{create_musica_provider, musica_descriptor, MusicaProvider};

#[cfg(feature = "dynamic-plugin-export")]
mod ffi;
#[cfg(feature = "dynamic-plugin-export")]
pub use ffi::astra_musica_family_root_module;

#[cfg(test)]
mod test_fixture;
