use std::{fmt, sync::Arc};

#[cfg(feature = "ffi")]
use std::{ffi::c_void, slice};

#[cfg(feature = "ffi")]
use abi_stable::StableAbi;

trait SampleOwner<T>: Send + Sync {
    fn samples(&self) -> &[T];
}

struct TypedSampleOwner<T, O> {
    value: O,
    samples: fn(&O) -> &[T],
}

impl<T, O: Send + Sync> SampleOwner<T> for TypedSampleOwner<T, O> {
    fn samples(&self) -> &[T] {
        (self.samples)(&self.value)
    }
}

macro_rules! owned_pcm_buffer {
    ($storage:ident, $owned:ident, $ffi:ident, $sample:ty, $drop_fn:ident) => {
        #[derive(Clone)]
        enum $storage {
            Vec(Arc<Vec<$sample>>),
            Owner(Arc<dyn SampleOwner<$sample>>),
            #[cfg(feature = "ffi")]
            Ffi(Arc<$ffi>),
        }

        /// Process-local owner for one typed PCM allocation. Cloning shares the
        /// allocation; converting to or from its FFI form never copies samples.
        #[derive(Clone)]
        pub struct $owned {
            storage: $storage,
        }

        impl $owned {
            pub fn from_vec(samples: Vec<$sample>) -> Self {
                Self {
                    storage: $storage::Vec(Arc::new(samples)),
                }
            }

            pub fn from_owner<O: Send + Sync + 'static>(
                value: O,
                samples: fn(&O) -> &[$sample],
            ) -> Self {
                Self {
                    storage: $storage::Owner(Arc::new(TypedSampleOwner { value, samples })),
                }
            }

            pub fn as_slice(&self) -> &[$sample] {
                match &self.storage {
                    $storage::Vec(samples) => samples,
                    $storage::Owner(owner) => owner.samples(),
                    #[cfg(feature = "ffi")]
                    $storage::Ffi(samples) => samples.as_slice(),
                }
            }

            pub fn as_ptr(&self) -> *const $sample {
                self.as_slice().as_ptr()
            }

            pub fn len(&self) -> usize {
                self.as_slice().len()
            }

            pub fn is_empty(&self) -> bool {
                self.len() == 0
            }

            /// Recovers local Vec storage only when it is uniquely owned. A
            /// foreign allocation is returned unchanged so callers cannot hide
            /// a cross-ABI copy behind a convenience conversion.
            pub fn try_into_vec(self) -> Result<Vec<$sample>, Self> {
                match self.storage {
                    $storage::Vec(samples) => match Arc::try_unwrap(samples) {
                        Ok(samples) => Ok(samples),
                        Err(samples) => Err(Self {
                            storage: $storage::Vec(samples),
                        }),
                    },
                    $storage::Owner(owner) => Err(Self {
                        storage: $storage::Owner(owner),
                    }),
                    #[cfg(feature = "ffi")]
                    $storage::Ffi(samples) => Err(Self {
                        storage: $storage::Ffi(samples),
                    }),
                }
            }

            #[cfg(feature = "ffi")]
            pub fn into_ffi(self) -> $ffi {
                $ffi::new(self)
            }

            #[cfg(feature = "ffi")]
            pub fn from_ffi(samples: $ffi) -> Self {
                Self {
                    storage: $storage::Ffi(Arc::new(samples)),
                }
            }
        }

        impl From<Vec<$sample>> for $owned {
            fn from(samples: Vec<$sample>) -> Self {
                Self::from_vec(samples)
            }
        }

        impl Default for $owned {
            fn default() -> Self {
                Self::from_vec(Vec::new())
            }
        }

        impl fmt::Debug for $owned {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($owned))
                    .field("len", &self.len())
                    .finish_non_exhaustive()
            }
        }

        impl PartialEq for $owned {
            fn eq(&self, other: &Self) -> bool {
                self.as_slice() == other.as_slice()
            }
        }

        impl AsRef<[$sample]> for $owned {
            fn as_ref(&self) -> &[$sample] {
                self.as_slice()
            }
        }

        impl std::ops::Deref for $owned {
            type Target = [$sample];

            fn deref(&self) -> &Self::Target {
                self.as_slice()
            }
        }

        /// ABI-owned immutable PCM allocation. The producer supplies the
        /// pointer and destruction authority as one move-only value.
        #[cfg(feature = "ffi")]
        #[repr(C)]
        #[derive(StableAbi)]
        pub struct $ffi {
            data: *const $sample,
            len: usize,
            owner: *mut c_void,
            drop_owner: unsafe extern "C" fn(*mut c_void),
        }

        #[cfg(feature = "ffi")]
        unsafe impl Send for $ffi {}

        #[cfg(feature = "ffi")]
        unsafe impl Sync for $ffi {}

        #[cfg(feature = "ffi")]
        impl $ffi {
            fn new(samples: $owned) -> Self {
                let owner = Box::new(samples);
                let data = owner.as_ptr();
                let len = owner.len();
                Self {
                    data,
                    len,
                    owner: Box::into_raw(owner).cast(),
                    drop_owner: $drop_fn,
                }
            }

            pub fn into_owned(self) -> $owned {
                $owned::from_ffi(self)
            }

            pub fn as_slice(&self) -> &[$sample] {
                if self.len == 0 {
                    return &[];
                }
                // SAFETY: `new` obtains the pointer from the boxed owner, and
                // the owner remains alive until the paired callback runs.
                unsafe { slice::from_raw_parts(self.data, self.len) }
            }

            pub fn as_ptr(&self) -> *const $sample {
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
        impl fmt::Debug for $ffi {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($ffi))
                    .field("len", &self.len)
                    .finish_non_exhaustive()
            }
        }

        #[cfg(feature = "ffi")]
        impl Drop for $ffi {
            fn drop(&mut self) {
                if !self.owner.is_null() {
                    // SAFETY: callback and owner originate from `new`, and
                    // ownership reaches this Drop exactly once.
                    unsafe { (self.drop_owner)(self.owner) };
                    self.owner = std::ptr::null_mut();
                }
            }
        }

        #[cfg(feature = "ffi")]
        unsafe extern "C" fn $drop_fn(owner: *mut c_void) {
            if !owner.is_null() {
                // SAFETY: owner was produced by Box::into_raw in `new`.
                unsafe { drop(Box::from_raw(owner.cast::<$owned>())) };
            }
        }
    };
}

owned_pcm_buffer!(
    I16Storage,
    OwnedI16Buffer,
    FfiOwnedI16Buffer,
    i16,
    drop_i16_owner
);
owned_pcm_buffer!(
    F32Storage,
    OwnedF32Buffer,
    FfiOwnedF32Buffer,
    f32,
    drop_f32_owner
);

#[cfg(all(test, feature = "ffi"))]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use super::{OwnedF32Buffer, OwnedI16Buffer};

    struct I16Owner {
        samples: Vec<i16>,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for I16Owner {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn i16_pointer_survives_ffi_and_foreign_owner_drops_once() {
        let drops = Arc::new(AtomicUsize::new(0));
        let owner = I16Owner {
            samples: vec![-7, 0, 23, i16::MAX],
            drops: Arc::clone(&drops),
        };
        let source_ptr = owner.samples.as_ptr();
        let owned = OwnedI16Buffer::from_owner(owner, |owner| &owner.samples);
        let ffi = owned.into_ffi();
        assert_eq!(ffi.as_ptr(), source_ptr);
        let received = ffi.into_owned();
        assert_eq!(received.as_ptr(), source_ptr);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(received);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn f32_vec_allocation_survives_ffi_round_trip() {
        let samples = vec![-1.0_f32, 0.0, 0.5, 1.0];
        let source_ptr = samples.as_ptr();
        let ffi = OwnedF32Buffer::from_vec(samples).into_ffi();
        assert_eq!(ffi.as_ptr(), source_ptr);
        let received = ffi.into_owned();
        assert_eq!(received.as_ptr(), source_ptr);
    }
}
