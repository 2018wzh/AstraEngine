use std::fmt;

enum OwnedWritableStorage {
    Vec(Vec<u8>),
    #[cfg(feature = "ffi")]
    Ffi(FfiOwnedWritableByteBuffer),
}

/// An exclusively owned byte allocation that may cross the family ABI while
/// retaining write access. Unlike `OwnedByteBuffer`, this type is deliberately
/// not cloneable: a writable surface lease has exactly one current owner.
pub struct OwnedWritableByteBuffer {
    storage: OwnedWritableStorage,
}

impl OwnedWritableByteBuffer {
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self {
            storage: OwnedWritableStorage::Vec(bytes),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.storage {
            OwnedWritableStorage::Vec(bytes) => bytes,
            #[cfg(feature = "ffi")]
            OwnedWritableStorage::Ffi(bytes) => bytes.as_slice(),
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        match &mut self.storage {
            OwnedWritableStorage::Vec(bytes) => bytes,
            #[cfg(feature = "ffi")]
            OwnedWritableStorage::Ffi(bytes) => bytes.as_mut_slice(),
        }
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    #[cfg(feature = "ffi")]
    pub fn into_ffi(self) -> FfiOwnedWritableByteBuffer {
        match self.storage {
            OwnedWritableStorage::Vec(bytes) => FfiOwnedWritableByteBuffer::from_vec(bytes),
            OwnedWritableStorage::Ffi(bytes) => bytes,
        }
    }

    #[cfg(feature = "ffi")]
    pub fn from_ffi(bytes: FfiOwnedWritableByteBuffer) -> Self {
        Self {
            storage: OwnedWritableStorage::Ffi(bytes),
        }
    }
}

impl From<Vec<u8>> for OwnedWritableByteBuffer {
    fn from(value: Vec<u8>) -> Self {
        Self::from_vec(value)
    }
}

impl fmt::Debug for OwnedWritableByteBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnedWritableByteBuffer")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl PartialEq for OwnedWritableByteBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for OwnedWritableByteBuffer {}

#[cfg(feature = "ffi")]
use std::{ffi::c_void, slice};

#[cfg(feature = "ffi")]
use abi_stable::StableAbi;

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(StableAbi)]
pub struct FfiOwnedWritableByteBuffer {
    data: *mut u8,
    len: usize,
    owner: *mut c_void,
    drop_owner: unsafe extern "C" fn(*mut c_void),
}

#[cfg(feature = "ffi")]
unsafe impl Send for FfiOwnedWritableByteBuffer {}

#[cfg(feature = "ffi")]
impl fmt::Debug for FfiOwnedWritableByteBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FfiOwnedWritableByteBuffer")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "ffi")]
impl FfiOwnedWritableByteBuffer {
    fn from_vec(bytes: Vec<u8>) -> Self {
        let mut owner = Box::new(bytes);
        let data = owner.as_mut_ptr();
        let len = owner.len();
        Self {
            data,
            len,
            owner: Box::into_raw(owner).cast(),
            drop_owner: drop_vec_owner,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: construction binds the pointer and length to the boxed owner.
        unsafe { slice::from_raw_parts(self.data.cast_const(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        if self.len == 0 {
            return &mut [];
        }
        // SAFETY: the ABI type is non-Clone and is transferred by value, so the
        // current owner has exclusive access to this allocation.
        unsafe { slice::from_raw_parts_mut(self.data, self.len) }
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

#[cfg(feature = "ffi")]
impl Drop for FfiOwnedWritableByteBuffer {
    fn drop(&mut self) {
        if !self.owner.is_null() {
            // SAFETY: owner and destructor are created as one private pair.
            unsafe { (self.drop_owner)(self.owner) };
            self.owner = std::ptr::null_mut();
            self.data = std::ptr::null_mut();
            self.len = 0;
        }
    }
}

#[cfg(feature = "ffi")]
unsafe extern "C" fn drop_vec_owner(owner: *mut c_void) {
    // SAFETY: `from_vec` stores exactly a `Box<Vec<u8>>` in this pointer.
    unsafe { drop(Box::from_raw(owner.cast::<Vec<u8>>())) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_buffer_mutates_in_place() {
        let mut bytes = OwnedWritableByteBuffer::from_vec(vec![0; 4]);
        let address = bytes.as_slice().as_ptr();
        bytes.as_mut_slice().copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(bytes.as_slice(), [1, 2, 3, 4]);
        assert_eq!(bytes.as_slice().as_ptr(), address);
    }

    #[cfg(feature = "ffi")]
    #[test]
    fn writable_buffer_round_trips_ffi_without_copy() {
        let bytes = OwnedWritableByteBuffer::from_vec(vec![0; 4]);
        let address = bytes.as_slice().as_ptr();
        let mut bytes = OwnedWritableByteBuffer::from_ffi(bytes.into_ffi());
        bytes.as_mut_slice()[2] = 9;
        assert_eq!(bytes.as_slice().as_ptr(), address);
        let bytes = OwnedWritableByteBuffer::from_ffi(bytes.into_ffi());
        assert_eq!(bytes.as_slice(), [0, 0, 9, 0]);
        assert_eq!(bytes.as_slice().as_ptr(), address);
    }
}
