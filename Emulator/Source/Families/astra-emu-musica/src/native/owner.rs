use super::*;

/// The Host module instance owns this provider across game sessions. Creation
/// is lazy on the native worker; no Instance/device lives in a global cache.
pub(super) type GraphicsOwner = Arc<Mutex<crate::provider::MusicaProvider>>;

pub(super) static API: abi::ModuleOwnerApi = abi::ModuleOwnerApi {
    header: abi::ModuleOwnerApi::HEADER,
    create: Some(create),
    destroy: Some(destroy),
};
unsafe extern "C" fn create(output: *mut abi::ModuleOwner, error: *mut abi::Error) -> abi::Status {
    if !output.is_null() {
        unsafe { output.write(std::ptr::null_mut()) };
    }
    boundary(error, || {
        initialized()?;
        if output.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        let owner = Arc::new(Mutex::new(crate::provider::MusicaProvider::default()));
        unsafe { output.write(Box::into_raw(Box::new(owner)).cast()) };
        Ok(())
    })
}
unsafe extern "C" fn destroy(owner: abi::ModuleOwner, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if owner.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        drop(unsafe { Box::from_raw(owner.cast::<GraphicsOwner>()) });
        Ok(())
    })
}
pub(super) unsafe fn borrow(owner: abi::ModuleOwner) -> Result<GraphicsOwner, Failure> {
    unsafe { owner.cast::<GraphicsOwner>().as_ref() }
        .cloned()
        .ok_or_else(|| invalid(abi::INVALID_ARGUMENT))
}
