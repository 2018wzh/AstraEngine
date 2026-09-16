//! CMVS format readers; Family session integration is in progress.
mod archive;
mod audio;
mod profile;
pub use profile::{mount_cmvs, CmvsProfile, CMVS_PROFILE_FILE, CMVS_PROFILE_SCHEMA};
mod cmvs_md5;
pub use archive::CmvsArchive;
mod cpz;
mod jbp;
mod mgv;
mod pb;
mod ps2a;
mod scene;
mod scheme;
mod script;
pub use scene::{CmvsScene, CmvsTextureSlot};
mod system_save;
pub use cpz::*;
pub use mgv::*;
pub use pb::*;
pub use ps2a::*;
pub use scheme::*;
pub use system_save::*;

pub const CMVS_FAMILY_ID: &str = "cmvs";
pub const CMVS_READER_ID: &str = "astra.emu.cmvs.reader.v1";
pub const CMVS_DECRYPT_PROVIDER_ID: &str = "astra.emu.cmvs.cpz.decrypt.v1";

mod command;
mod vm;
pub use command::*;
pub use vm::*;

pub const CMVS_DECRYPT_DESCRIPTOR_SCHEMA: &str = "astra.emu.cmvs.cpz5_descriptor.v1";
