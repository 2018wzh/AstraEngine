use crate::{Error, Header, Status, UNSUPPORTED};
use std::ffi::c_void;

pub type ModuleOwner = *mut c_void;
pub const MODULE_OWNER_EXTENSION: &[u8] = b"astra.module_owner";
pub const MODULE_OWNER_VERSION: u32 = 1;

/// Optional ownership extension for real cross-session module resources, e.g.
/// Musica's explicit graphics Instance owner. A Host core instance creates one
/// owner after diagnostic initialization and retains it through all its sessions.
/// Other cores report UNSUPPORTED rather than constructing a dummy owner.
///
/// create does not start a game. On error it returns null after cleanup; Host
/// also destroys a non-null failed result defensively. destroy consumes the owner
/// on every outcome, only after sessions close and callbacks finish. Both calls
/// run on the module owner thread. Libraries remain resident after destruction.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModuleOwnerApi {
    pub header: Header,
    pub create: Option<unsafe extern "C" fn(*mut ModuleOwner, *mut Error) -> Status>,
    pub destroy: Option<unsafe extern "C" fn(ModuleOwner, *mut Error) -> Status>,
}
impl ModuleOwnerApi {
    pub const HEADER: Header = Header {
        size: std::mem::size_of::<Self>() as u32,
        version: MODULE_OWNER_VERSION,
    };
    pub fn validate(&self) -> Result<(), Status> {
        if self.header.version != MODULE_OWNER_VERSION
            || self.header.size < Self::HEADER.size
            || self.create.is_none()
            || self.destroy.is_none()
        {
            return Err(UNSUPPORTED);
        }
        Ok(())
    }
}
