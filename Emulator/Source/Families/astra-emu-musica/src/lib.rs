mod configuration;
pub use configuration::*;
mod audio;
mod movie;
mod provider;
mod scene;
mod session;
mod storage;
mod voice_preferences;

mod archive;
mod image_container;
mod localization;
mod message;
mod subtitles;
pub use localization::{
    musica_display_cstr_hash, musica_display_hash, MusicaTranslationTable, TranslationMode,
};
mod paz;
mod profile;
mod resources;
pub use resources::MusicaResourcePaths;
mod runtime;
pub use message::*;
mod script;
mod script_loader;
pub use script_loader::{read_expanded_script, validate_musica_script, ScriptValidation};
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

mod native;
pub use native::musica_query;

#[cfg(test)]
pub(crate) use provider::{musica_descriptor, MusicaProvider};

#[cfg(test)]
mod test_fixture;

pub mod eden_save;
