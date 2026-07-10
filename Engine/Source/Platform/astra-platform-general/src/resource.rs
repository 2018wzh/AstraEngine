use std::marker::PhantomData;

use astra_platform::{PlatformError, PlatformErrorCode, ResourceHandle, TypedHandle};

struct ResourceSlot<T> {
    generation: u32,
    value: Option<T>,
}

pub struct ResourceTable<T, H> {
    kind: &'static str,
    slots: Vec<ResourceSlot<T>>,
    free: Vec<usize>,
    live: usize,
    marker: PhantomData<H>,
}

impl<T, H: TypedHandle> ResourceTable<T, H> {
    pub fn new(kind: &'static str) -> Self {
        Self {
            kind,
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
            marker: PhantomData,
        }
    }

    pub fn insert(&mut self, value: T) -> Result<H, PlatformError> {
        let index = if let Some(index) = self.free.pop() {
            self.slots[index].value = Some(value);
            index
        } else {
            self.slots.push(ResourceSlot {
                generation: 1,
                value: Some(value),
            });
            self.slots.len() - 1
        };
        self.live += 1;
        let slot = u32::try_from(index + 1).map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "resource.insert",
                "resource table exhausted its handle space",
            )
            .with_field("resource_kind", self.kind)
        })?;
        let resource = ResourceHandle::from_parts(slot, self.slots[index].generation)?;
        Ok(H::from_resource(resource))
    }

    pub fn get(&self, handle: H) -> Result<&T, PlatformError> {
        let index = self.validate(handle)?;
        self.slots[index].value.as_ref().ok_or_else(|| self.stale())
    }

    pub fn get_mut(&mut self, handle: H) -> Result<&mut T, PlatformError> {
        let index = self.validate(handle)?;
        if self.slots[index].value.is_none() {
            return Err(self.stale());
        }
        Ok(self.slots[index].value.as_mut().expect("checked above"))
    }

    pub fn remove(&mut self, handle: H) -> Result<T, PlatformError> {
        let index = self.validate(handle)?;
        let value = self.slots[index].value.take().ok_or_else(|| self.stale())?;
        self.live -= 1;
        self.slots[index].generation = self.slots[index]
            .generation
            .checked_add(1)
            .filter(|generation| *generation != 0)
            .ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "resource.remove",
                    "resource handle generation overflowed",
                )
                .with_field("resource_kind", self.kind)
            })?;
        self.free.push(index);
        Ok(value)
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    pub fn ensure_empty(&self) -> Result<(), PlatformError> {
        if self.live == 0 {
            return Ok(());
        }
        Err(PlatformError::new(
            PlatformErrorCode::ResourceLeak,
            "resource.shutdown",
            "platform resource table still contains live resources",
        )
        .with_field("resource_kind", self.kind)
        .with_field("resource_count", self.live.to_string()))
    }

    fn validate(&self, handle: H) -> Result<usize, PlatformError> {
        let (slot, generation) = handle.resource().parts();
        let index = usize::try_from(slot - 1).map_err(|_| self.stale())?;
        let entry = self.slots.get(index).ok_or_else(|| self.stale())?;
        if entry.generation != generation || entry.value.is_none() {
            return Err(self.stale());
        }
        Ok(index)
    }

    fn stale(&self) -> PlatformError {
        PlatformError::new(
            PlatformErrorCode::StaleHandle,
            "resource.resolve",
            "resource handle is stale or belongs to another table",
        )
        .with_field("resource_kind", self.kind)
    }
}
