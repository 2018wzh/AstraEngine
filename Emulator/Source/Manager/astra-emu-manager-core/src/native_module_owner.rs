use crate::{
    family_loader::checked,
    model::{FamilyError, FamilyResult},
};
use astra_emu_abi as abi;

/// Resources shared by sessions of one loaded core instance, never a process
/// cache. This value stays on the same owner thread as its containing module.
pub(super) struct ModuleResources {
    api: abi::ModuleOwnerApi,
    handle: abi::ModuleOwner,
}
impl ModuleResources {
    pub(super) fn discover(core: &abi::CoreApi) -> FamilyResult<Option<Self>> {
        let mut table = std::ptr::null();
        let status = unsafe {
            core.extension.expect("validated CoreApi")(
                abi::Bytes::new(abi::MODULE_OWNER_EXTENSION),
                abi::MODULE_OWNER_VERSION,
                &mut table,
            )
        };
        if status == abi::UNSUPPORTED && table.is_null() {
            return Ok(None);
        }
        if status != abi::OK || table.is_null() {
            return Err(invalid("invalid module-owner extension response"));
        }
        let table = table.cast::<abi::ModuleOwnerApi>();
        if !table.is_aligned() {
            return Err(invalid("misaligned module-owner extension table"));
        }
        let header = unsafe { (*table).header };
        if header.version != abi::MODULE_OWNER_VERSION
            || header.size < abi::ModuleOwnerApi::HEADER.size
        {
            return Err(invalid("unsupported module-owner extension layout"));
        }
        let api = unsafe { *table };
        api.validate()
            .map_err(|_| invalid("invalid module-owner extension callbacks"))?;
        let mut owner = Self {
            api,
            handle: std::ptr::null_mut(),
        };
        let mut error = abi::Error::default();
        checked(
            unsafe { api.create.expect("validated ModuleOwnerApi")(&mut owner.handle, &mut error) },
            error,
        )?;
        if owner.handle.is_null() {
            return Err(invalid("module-owner creation returned a null handle"));
        }
        Ok(Some(owner))
    }
    pub(super) fn handle(&self) -> abi::ModuleOwner {
        self.handle
    }
    pub(super) fn close(&mut self) -> FamilyResult<()> {
        let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
        if handle.is_null() {
            return Ok(());
        }
        let mut error = abi::Error::default();
        checked(
            unsafe { self.api.destroy.expect("validated ModuleOwnerApi")(handle, &mut error) },
            error,
        )
    }
}
impl Drop for ModuleResources {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(event = "astra.emu.module_owner.cleanup_failed", diagnostic_code = %error.code());
        }
    }
}
fn invalid(message: &str) -> FamilyError {
    FamilyError::invalid("ASTRA_EMU_MODULE_OWNER_ABI", message)
}

#[cfg(test)]
#[path = "native_module_owner_tests.rs"]
mod tests;
