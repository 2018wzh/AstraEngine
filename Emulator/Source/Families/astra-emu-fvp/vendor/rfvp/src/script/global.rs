use alloc::vec::Vec;
#[cfg(feature = "no_std")]
use core::cell::UnsafeCell;
#[cfg(feature = "no_std")]
use core::ops::{Deref, DerefMut};
#[cfg(not(feature = "no_std"))]
use std::sync::Mutex;

use crate::script::Variant;
#[cfg(feature = "no_std")]
use crate::utils::stable_hash::StableHashMap;
#[cfg(not(feature = "no_std"))]
use crate::utils::stable_hash::StableHashMap;
use serde::{Deserialize, Serialize};

/// Global variables
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Global {
    global_table: StableHashMap<u16, Variant>,
    none_volatile_count: u16,
    volatile_count: u16,
}

/// Complete per-session global state used by hosted snapshot/restore.  Unlike
/// legacy save files this includes volatile globals because a hosted restore is
/// an exact runtime checkpoint, not a user-facing save migration.
#[cfg(feature = "hosted")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedGlobalSnapshot {
    pub non_volatile_count: u16,
    pub volatile_count: u16,
    pub values: Vec<Variant>,
}

impl Global {
    pub fn new() -> Self {
        Global {
            global_table: StableHashMap::default(),
            none_volatile_count: 0,
            volatile_count: 0,
        }
    }

    pub fn get(&self, key: u16) -> Option<&Variant> {
        self.global_table.get(&key)
    }

    pub fn get_mut(&mut self, key: u16) -> Option<&mut Variant> {
        self.global_table.get_mut(&key)
    }

    pub fn set(&mut self, key: u16, value: Variant) {
        self.global_table.insert(key, value);
    }

    pub fn init_with(&mut self, none_volatile: u16, volatile: u16) {
        self.none_volatile_count = none_volatile;
        self.volatile_count = volatile;

        for i in 0..none_volatile + volatile {
            self.global_table.insert(i, Variant::Nil);
        }
    }

    pub fn get_int_var(&self, key: u16) -> i32 {
        let key = key + self.none_volatile_count;
        if let Some(Variant::Int(val)) = self.global_table.get(&key) {
            return *val;
        }
        0
    }

    pub fn non_volatile_count(&self) -> u16 {
        self.none_volatile_count
    }

    pub fn volatile_count(&self) -> u16 {
        self.volatile_count
    }

    pub fn snapshot_non_volatile(&self) -> Vec<Variant> {
        let mut out: Vec<Variant> = Vec::with_capacity(self.none_volatile_count as usize);
        for i in 0..self.none_volatile_count {
            out.push(self.global_table.get(&i).cloned().unwrap_or(Variant::Nil));
        }
        out
    }

    pub fn restore_non_volatile(&mut self, vals: &[Variant]) {
        let n = self.none_volatile_count as usize;
        let take = vals.len().min(n);
        for i in 0..take {
            self.global_table.insert(i as u16, vals[i].clone());
        }
        // Missing entries remain unchanged.
    }

    pub fn snapshot_volatile_globals(&self) -> Vec<Variant> {
        let mut out: Vec<Variant> = Vec::with_capacity(self.volatile_count as usize);
        let base = self.none_volatile_count;
        for i in 0..self.volatile_count {
            let key = base.saturating_add(i);
            out.push(self.global_table.get(&key).cloned().unwrap_or(Variant::Nil));
        }
        out
    }

    pub fn restore_volatile_globals(
        &mut self,
        expected_non_volatile: u16,
        expected_volatile: u16,
        vars: &[Variant],
    ) {
        let base = self.none_volatile_count;

        if expected_non_volatile != self.none_volatile_count
            || expected_volatile != self.volatile_count
        {
            log::warn!(
            "GlobalSaveData: global counts mismatch: saved non_volatile={} volatile={} but current non_volatile={} volatile={}",
            expected_non_volatile,
            expected_volatile,
            self.none_volatile_count,
            self.volatile_count
        );
        }

        let n = vars.len().min(self.volatile_count as usize);
        for i in 0..n {
            let key = base.saturating_add(i as u16);
            self.global_table.insert(key, vars[i].clone());
        }
    }

    #[cfg(feature = "hosted")]
    pub fn capture_hosted_snapshot(&self) -> HostedGlobalSnapshot {
        let total = self.none_volatile_count as usize + self.volatile_count as usize;
        let mut values = Vec::with_capacity(total);
        for key in 0..total {
            values.push(
                self.global_table
                    .get(&(key as u16))
                    .cloned()
                    .unwrap_or(Variant::Nil),
            );
        }
        HostedGlobalSnapshot {
            non_volatile_count: self.none_volatile_count,
            volatile_count: self.volatile_count,
            values,
        }
    }

    #[cfg(feature = "hosted")]
    pub fn restore_hosted_snapshot(&mut self, snapshot: &HostedGlobalSnapshot) -> bool {
        let total = snapshot.non_volatile_count as usize + snapshot.volatile_count as usize;
        if snapshot.non_volatile_count != self.none_volatile_count
            || snapshot.volatile_count != self.volatile_count
            || snapshot.values.len() != total
        {
            return false;
        }
        for (key, value) in snapshot.values.iter().enumerate() {
            self.global_table.insert(key as u16, value.clone());
        }
        true
    }
}

#[cfg(not(feature = "no_std"))]
lazy_static::lazy_static! {
    pub static ref GLOBAL: Mutex<Global> = Mutex::new(Global::new());
}

#[cfg(feature = "no_std")]
pub static GLOBAL: NoStdGlobal = NoStdGlobal::new();

#[cfg(feature = "no_std")]
pub struct NoStdGlobal {
    inner: UnsafeCell<Global>,
}

#[cfg(feature = "no_std")]
pub struct NoStdGlobalGuard<'a> {
    inner: &'a mut Global,
}

#[cfg(feature = "no_std")]
unsafe impl Sync for NoStdGlobal {}

#[cfg(feature = "no_std")]
impl NoStdGlobal {
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(Global {
                global_table: StableHashMap::new(),
                none_volatile_count: 0,
                volatile_count: 0,
            }),
        }
    }

    pub fn lock(&self) -> Result<NoStdGlobalGuard<'_>, ()> {
        Ok(NoStdGlobalGuard {
            inner: unsafe { &mut *self.inner.get() },
        })
    }
}

#[cfg(feature = "no_std")]
impl Deref for NoStdGlobalGuard<'_> {
    type Target = Global;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

#[cfg(feature = "no_std")]
impl DerefMut for NoStdGlobalGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}

pub fn get_int_var(key: u16) -> i32 {
    GLOBAL.lock().unwrap().get_int_var(key)
}
