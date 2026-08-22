use std::{collections::BTreeMap, sync::Mutex};

use astra_byte_source::OwnedWritableByteBuffer;
use astra_emu_family_api::{
    LegacyProviderError, LegacySurfaceCommitV9, LegacySurfaceFormatV9, LegacySurfaceHostV9,
    LegacySurfaceLeaseV9,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SurfaceKey {
    session_id: String,
    surface_id: String,
}

struct CommittedSurface {
    generation: u64,
    width: u32,
    height: u32,
    stride: u32,
    format: LegacySurfaceFormatV9,
    pixels: OwnedWritableByteBuffer,
}

struct OutstandingLease {
    key: SurfaceKey,
    fixed_step: u64,
    generation: u64,
    width: u32,
    height: u32,
    stride: u32,
    format: LegacySurfaceFormatV9,
    byte_len: u64,
}

#[derive(Default)]
struct State {
    next_lease: u64,
    committed: BTreeMap<SurfaceKey, CommittedSurface>,
    outstanding: BTreeMap<String, OutstandingLease>,
    committed_bytes: u64,
    reserved_bytes: u64,
}

/// Retains Host-owned ABI v9 surfaces. The writable allocation is removed
/// from this store while a family owns its lease, then returned at commit.
pub struct LegacySurfaceStoreV9 {
    max_surface_bytes: u64,
    max_total_bytes: u64,
    state: Mutex<State>,
}

impl LegacySurfaceStoreV9 {
    pub fn new(max_surface_bytes: u64, max_total_bytes: u64) -> Result<Self, LegacyProviderError> {
        if max_surface_bytes == 0
            || max_total_bytes < max_surface_bytes
            || max_total_bytes > usize::MAX as u64
        {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_LIMITS",
                "invalid surface limits",
            ));
        }
        Ok(Self {
            max_surface_bytes,
            max_total_bytes,
            state: Mutex::new(State::default()),
        })
    }

    pub fn with_committed_surface<T>(
        &self,
        session_id: &str,
        surface_id: &str,
        generation: u64,
        read: impl FnOnce(&[u8], u32, u32, u32, LegacySurfaceFormatV9) -> T,
    ) -> Result<T, LegacyProviderError> {
        let state = self.lock()?;
        let surface = state
            .committed
            .get(&SurfaceKey {
                session_id: session_id.into(),
                surface_id: surface_id.into(),
            })
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_MISSING", "surface is not committed"))?;
        if surface.generation != generation {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_GENERATION",
                "surface generation does not match the layer reference",
            ));
        }
        Ok(read(
            surface.pixels.as_slice(),
            surface.width,
            surface.height,
            surface.stride,
            surface.format,
        ))
    }

    pub fn remove_session(&self, session_id: &str) -> Result<(), LegacyProviderError> {
        let mut state = self.lock()?;
        if state
            .outstanding
            .values()
            .any(|lease| lease.key.session_id == session_id)
        {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_LEASE_OUTSTANDING",
                "session still owns a writable surface lease",
            ));
        }
        let keys = state
            .committed
            .keys()
            .filter(|key| key.session_id == session_id)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            let surface = state
                .committed
                .remove(&key)
                .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_ACCOUNTING", "surface disappeared"))?;
            state.committed_bytes = state
                .committed_bytes
                .checked_sub(surface.pixels.len() as u64)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_SURFACE_ACCOUNTING",
                        "surface byte accounting underflowed",
                    )
                })?;
        }
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, LegacyProviderError> {
        self.state.lock().map_err(|_| {
            invalid(
                "ASTRA_EMU_SURFACE_LOCK_POISONED",
                "surface store lock is poisoned",
            )
        })
    }
}

impl LegacySurfaceHostV9 for LegacySurfaceStoreV9 {
    fn acquire(
        &self,
        session_id: &str,
        fixed_step: u64,
        surface_id: &str,
        width: u32,
        height: u32,
        format: LegacySurfaceFormatV9,
    ) -> Result<LegacySurfaceLeaseV9, LegacyProviderError> {
        if session_id.is_empty() || surface_id.is_empty() || width == 0 || height == 0 {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_REQUEST",
                "surface identity and dimensions must be non-empty",
            ));
        }
        let stride = width
            .checked_mul(4)
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_BOUNDS", "surface stride overflowed"))?;
        let byte_len = u64::from(stride)
            .checked_mul(u64::from(height))
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_BOUNDS", "surface size overflowed"))?;
        if byte_len > self.max_surface_bytes {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_BOUNDS",
                "surface exceeds its byte limit",
            ));
        }
        let key = SurfaceKey {
            session_id: session_id.into(),
            surface_id: surface_id.into(),
        };
        let mut state = self.lock()?;
        if state.outstanding.values().any(|lease| lease.key == key) {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_LEASE_CONFLICT",
                "surface already has an outstanding lease",
            ));
        }
        let retained_len = state
            .committed
            .get(&key)
            .map_or(0, |surface| surface.pixels.len() as u64);
        let projected_total = state
            .committed_bytes
            .checked_sub(retained_len)
            .and_then(|bytes| bytes.checked_add(state.reserved_bytes))
            .and_then(|bytes| bytes.checked_add(byte_len))
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_ACCOUNTING", "byte total overflowed"))?;
        if projected_total > self.max_total_bytes {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_TOTAL_BOUNDS",
                "leased and committed surfaces exceed their total byte limit",
            ));
        }
        let retained = state.committed.remove(&key);
        if let Some(surface) = &retained {
            state.committed_bytes = state
                .committed_bytes
                .checked_sub(surface.pixels.len() as u64)
                .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_ACCOUNTING", "byte underflow"))?;
        }
        let generation = retained
            .as_ref()
            .map_or(1, |surface| surface.generation.saturating_add(1));
        if generation == u64::MAX {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_GENERATION",
                "surface generation overflowed",
            ));
        }
        let pixels = match retained {
            Some(surface)
                if surface.width == width
                    && surface.height == height
                    && surface.stride == stride
                    && surface.format == format =>
            {
                surface.pixels
            }
            _ => OwnedWritableByteBuffer::from_vec(vec![0; byte_len as usize]),
        };
        state.next_lease = state
            .next_lease
            .checked_add(1)
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_LEASE_ID", "lease sequence overflowed"))?;
        let lease_id = format!("astra.surface.lease.{}", state.next_lease);
        state.reserved_bytes = state
            .reserved_bytes
            .checked_add(byte_len)
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_ACCOUNTING", "reservation overflowed"))?;
        state.outstanding.insert(
            lease_id.clone(),
            OutstandingLease {
                key,
                fixed_step,
                generation,
                width,
                height,
                stride,
                format,
                byte_len,
            },
        );
        Ok(LegacySurfaceLeaseV9 {
            lease_id,
            surface_id: surface_id.into(),
            generation,
            width,
            height,
            stride,
            format,
            pixels,
        })
    }

    fn commit(
        &self,
        session_id: &str,
        fixed_step: u64,
        commit: LegacySurfaceCommitV9,
    ) -> Result<(), LegacyProviderError> {
        commit.validate()?;
        let mut state = self.lock()?;
        let expected = state
            .outstanding
            .get(&commit.lease.lease_id)
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_LEASE_MISSING", "lease is not active"))?;
        if expected.key.session_id != session_id
            || expected.key.surface_id != commit.lease.surface_id
            || expected.fixed_step != fixed_step
            || expected.generation != commit.lease.generation
            || expected.width != commit.lease.width
            || expected.height != commit.lease.height
            || expected.stride != commit.lease.stride
            || expected.format != commit.lease.format
        {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_LEASE_BINDING",
                "commit does not match its lease",
            ));
        }
        let next_total = state
            .committed_bytes
            .checked_add(commit.lease.pixels.len() as u64)
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_ACCOUNTING", "byte total overflowed"))?;
        if next_total > self.max_total_bytes {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_TOTAL_BOUNDS",
                "committed surfaces exceed their total byte limit",
            ));
        }
        let expected = state
            .outstanding
            .remove(&commit.lease.lease_id)
            .expect("validated lease remains active");
        state.reserved_bytes = state
            .reserved_bytes
            .checked_sub(expected.byte_len)
            .ok_or_else(|| invalid("ASTRA_EMU_SURFACE_ACCOUNTING", "reservation underflowed"))?;
        if state.committed.contains_key(&expected.key) {
            return Err(invalid(
                "ASTRA_EMU_SURFACE_COMMIT_CONFLICT",
                "surface generation is already committed",
            ));
        }
        state.committed_bytes = next_total;
        state.committed.insert(
            expected.key,
            CommittedSurface {
                generation: commit.lease.generation,
                width: commit.lease.width,
                height: commit.lease.height,
                stride: commit.lease.stride,
                format: commit.lease.format,
                pixels: commit.lease.pixels,
            },
        );
        Ok(())
    }
}

fn invalid(code: &'static str, message: impl Into<String>) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::LegacySurfaceDamageV9;

    #[test]
    fn writable_allocation_round_trips_without_copy() {
        let store = LegacySurfaceStoreV9::new(4096, 8192).unwrap();
        let mut lease = store
            .acquire(
                "session.one",
                1,
                "surface.stage",
                4,
                4,
                LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            )
            .unwrap();
        let address = lease.pixels.as_slice().as_ptr();
        lease.pixels.as_mut_slice()[3] = 0xff;
        store
            .commit(
                "session.one",
                1,
                LegacySurfaceCommitV9 {
                    lease,
                    damage: LegacySurfaceDamageV9::Full,
                },
            )
            .unwrap();
        store
            .with_committed_surface(
                "session.one",
                "surface.stage",
                1,
                |pixels, width, height, stride, _| {
                    assert_eq!((width, height, stride), (4, 4, 16));
                    assert_eq!(pixels.as_ptr(), address);
                    assert_eq!(pixels[3], 0xff);
                },
            )
            .unwrap();
        let lease = store
            .acquire(
                "session.one",
                2,
                "surface.stage",
                4,
                4,
                LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            )
            .unwrap();
        assert_eq!(lease.generation, 2);
        assert_eq!(lease.pixels.as_slice().as_ptr(), address);
    }

    #[test]
    fn rejects_duplicate_and_mismatched_leases() {
        let store = LegacySurfaceStoreV9::new(4096, 8192).unwrap();
        let lease = store
            .acquire(
                "session.one",
                1,
                "surface.stage",
                2,
                2,
                LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            )
            .unwrap();
        assert!(store
            .acquire(
                "session.one",
                1,
                "surface.stage",
                2,
                2,
                LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            )
            .is_err());
        assert!(store
            .commit(
                "session.one",
                2,
                LegacySurfaceCommitV9 {
                    lease,
                    damage: LegacySurfaceDamageV9::Full,
                },
            )
            .is_err());
    }
}
