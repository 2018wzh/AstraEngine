use std::{fmt, sync::Arc};

#[cfg(feature = "ffi")]
use std::slice;

trait ByteOwner: Send + Sync {
    fn bytes(&self) -> &[u8];
}

struct TypedByteOwner<T> {
    value: T,
    bytes: fn(&T) -> &[u8],
}

impl<T: Send + Sync> ByteOwner for TypedByteOwner<T> {
    fn bytes(&self) -> &[u8] {
        (self.bytes)(&self.value)
    }
}

#[derive(Clone)]
enum OwnedByteStorage {
    Vec(Arc<Vec<u8>>),
    Owner(Arc<dyn ByteOwner>),
    #[cfg(feature = "ffi")]
    Ffi(Arc<FfiOwnedByteBuffer>),
}

/// A process-local byte allocation whose concrete owner can remain in the
/// producer crate or dynamic library. The live scene path only borrows its
/// slice and moves this owner; it never rebuilds the payload.
#[derive(Clone)]
pub struct OwnedByteBuffer {
    storage: OwnedByteStorage,
}

impl OwnedByteBuffer {
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self {
            storage: OwnedByteStorage::Vec(Arc::new(bytes)),
        }
    }

    pub fn from_owner<T: Send + Sync + 'static>(value: T, bytes: fn(&T) -> &[u8]) -> Self {
        Self {
            storage: OwnedByteStorage::Owner(Arc::new(TypedByteOwner { value, bytes })),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.storage {
            OwnedByteStorage::Vec(bytes) => bytes,
            OwnedByteStorage::Owner(owner) => owner.bytes(),
            #[cfg(feature = "ffi")]
            OwnedByteStorage::Ffi(bytes) => bytes.as_slice(),
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.as_slice().as_ptr()
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    /// Recovers the original Vec without copying when this buffer is the sole
    /// owner of a Vec allocation. Foreign and shared owners are returned to
    /// the caller unchanged so live paths can fail instead of hiding a copy.
    pub fn try_into_vec(self) -> Result<Vec<u8>, Self> {
        match self.storage {
            OwnedByteStorage::Vec(bytes) => match Arc::try_unwrap(bytes) {
                Ok(bytes) => Ok(bytes),
                Err(bytes) => Err(Self {
                    storage: OwnedByteStorage::Vec(bytes),
                }),
            },
            storage => Err(Self { storage }),
        }
    }

    /// Returns mutable Vec storage, copying only when a foreign/shared owner
    /// must be isolated for a real mutation (generation COW or partial update).
    pub fn make_mut_vec(&mut self) -> &mut Vec<u8> {
        if !matches!(self.storage, OwnedByteStorage::Vec(_)) {
            self.storage = OwnedByteStorage::Vec(Arc::new(self.as_slice().to_vec()));
        }
        match &mut self.storage {
            OwnedByteStorage::Vec(bytes) => Arc::make_mut(bytes),
            _ => unreachable!("storage was converted to Vec"),
        }
    }

    #[cfg(feature = "ffi")]
    pub fn into_ffi(self) -> FfiOwnedByteBuffer {
        FfiOwnedByteBuffer::new(self)
    }

    #[cfg(feature = "ffi")]
    pub fn from_ffi(bytes: FfiOwnedByteBuffer) -> Self {
        Self {
            storage: OwnedByteStorage::Ffi(Arc::new(bytes)),
        }
    }
}

impl From<Vec<u8>> for OwnedByteBuffer {
    fn from(value: Vec<u8>) -> Self {
        Self::from_vec(value)
    }
}

impl Default for OwnedByteBuffer {
    fn default() -> Self {
        Self::from_vec(Vec::new())
    }
}

impl fmt::Debug for OwnedByteBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnedByteBuffer")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl PartialEq for OwnedByteBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for OwnedByteBuffer {}

impl AsRef<[u8]> for OwnedByteBuffer {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl std::ops::Deref for OwnedByteBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[cfg(feature = "ffi")]
use std::ffi::c_void;

#[cfg(feature = "ffi")]
use abi_stable::StableAbi;

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(StableAbi)]
pub struct FfiOwnedByteBuffer {
    data: *const u8,
    len: usize,
    owner: *mut c_void,
    drop_owner: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: construction is private and accepts only `OwnedByteBuffer`, whose
// external owners are `Send + Sync`. The data pointer is immutable and remains
// tied to that owner until the drop callback runs exactly once.
#[cfg(feature = "ffi")]
unsafe impl Send for FfiOwnedByteBuffer {}

// SAFETY: the same construction invariant as `Send` applies. Only immutable
// byte access is exposed while the `Send + Sync` owner remains alive.
#[cfg(feature = "ffi")]
unsafe impl Sync for FfiOwnedByteBuffer {}

#[cfg(feature = "ffi")]
impl FfiOwnedByteBuffer {
    fn new(bytes: OwnedByteBuffer) -> Self {
        let owner = Box::new(bytes);
        let data = owner.as_ptr();
        let len = owner.len();
        Self {
            data,
            len,
            owner: Box::into_raw(owner).cast(),
            drop_owner: drop_owned_byte_buffer,
        }
    }

    pub fn into_owned(self) -> OwnedByteBuffer {
        OwnedByteBuffer::from_ffi(self)
    }

    pub fn as_slice(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: `new` obtains this pointer from the boxed owner and the
        // owner remains alive until this value is dropped. ABI producers are
        // internal and construct the value only through `OwnedByteBuffer`.
        unsafe { slice::from_raw_parts(self.data, self.len) }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.data
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(feature = "ffi")]
impl fmt::Debug for FfiOwnedByteBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FfiOwnedByteBuffer")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "ffi")]
impl Drop for FfiOwnedByteBuffer {
    fn drop(&mut self) {
        if !self.owner.is_null() {
            // SAFETY: the callback and owner pointer are created together by
            // `new` and ownership reaches this drop exactly once.
            unsafe { (self.drop_owner)(self.owner) };
            self.owner = std::ptr::null_mut();
        }
    }
}

#[cfg(feature = "ffi")]
unsafe extern "C" fn drop_owned_byte_buffer(owner: *mut c_void) {
    if !owner.is_null() {
        // SAFETY: `owner` was produced by `Box::into_raw` in `new` and this
        // callback is invoked once by `Drop`.
        unsafe { drop(Box::from_raw(owner.cast::<OwnedByteBuffer>())) };
    }
}
