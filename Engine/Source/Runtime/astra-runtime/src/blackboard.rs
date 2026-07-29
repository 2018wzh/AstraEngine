use std::{collections::BTreeMap, sync::Mutex};

use astra_core::{Hash128, StableId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum BlackboardValue {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<BlackboardValue>),
    Map(BTreeMap<String, BlackboardValue>),
    StableId(StableId),
}

impl From<&str> for BlackboardValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for BlackboardValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for BlackboardValue {
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}

impl From<bool> for BlackboardValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Blackboard {
    values: BTreeMap<String, BlackboardValue>,
    #[serde(skip)]
    #[schemars(skip)]
    transaction: Option<BTreeMap<String, Option<BlackboardValue>>>,
    #[serde(skip)]
    #[schemars(skip)]
    fingerprint: Mutex<Option<Hash128>>,
}

impl Clone for Blackboard {
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
            transaction: self.transaction.clone(),
            fingerprint: Mutex::new(
                *self
                    .fingerprint
                    .lock()
                    .expect("blackboard fingerprint cache lock must not be poisoned"),
            ),
        }
    }
}

impl PartialEq for Blackboard {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl Blackboard {
    pub(crate) fn begin_transaction(&mut self) -> Result<(), &'static str> {
        if self.transaction.is_some() {
            return Err(
                "ASTRA_RUNTIME_BLACKBOARD_TRANSACTION_NESTED: blackboard transaction is already active",
            );
        }
        self.transaction = Some(BTreeMap::new());
        Ok(())
    }

    pub(crate) fn commit_transaction(&mut self) {
        self.transaction = None;
    }

    pub(crate) fn rollback_transaction(&mut self) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        for (key, value) in transaction {
            match value {
                Some(value) => {
                    self.values.insert(key, value);
                }
                None => {
                    self.values.remove(&key);
                }
            }
        }
        *self
            .fingerprint
            .get_mut()
            .expect("blackboard fingerprint cache lock must not be poisoned") = None;
    }

    fn record_before(&mut self, key: &str) {
        let Some(transaction) = self.transaction.as_ref() else {
            return;
        };
        if transaction.contains_key(key) {
            return;
        }
        let previous = self.values.get(key).cloned();
        self.transaction
            .as_mut()
            .expect("transaction presence was checked")
            .insert(key.to_string(), previous);
    }

    pub fn set(&mut self, key: impl Into<String>, value: BlackboardValue) {
        let key = key.into();
        self.record_before(&key);
        *self
            .fingerprint
            .get_mut()
            .expect("blackboard fingerprint cache lock must not be poisoned") = None;
        self.values.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<&BlackboardValue> {
        self.values.get(key)
    }

    pub fn values(&self) -> &BTreeMap<String, BlackboardValue> {
        &self.values
    }
}

pub(crate) trait BlackboardAccess {
    fn set(&mut self, key: String, value: BlackboardValue);
    fn get(&self, key: &str) -> Option<&BlackboardValue>;
    fn values(&self) -> BTreeMap<String, BlackboardValue>;
    fn deterministic_fingerprint(&self) -> Hash128;
}

impl BlackboardAccess for Blackboard {
    fn set(&mut self, key: String, value: BlackboardValue) {
        Blackboard::set(self, key, value);
    }

    fn get(&self, key: &str) -> Option<&BlackboardValue> {
        Blackboard::get(self, key)
    }

    fn values(&self) -> BTreeMap<String, BlackboardValue> {
        self.values.clone()
    }

    fn deterministic_fingerprint(&self) -> Hash128 {
        if let Some(fingerprint) = *self
            .fingerprint
            .lock()
            .expect("blackboard fingerprint cache lock must not be poisoned")
        {
            return fingerprint;
        }
        let fingerprint = Hash128::from_blake3(
            &postcard::to_allocvec(&self.values)
                .expect("blackboard must serialize for deterministic fingerprinting"),
        );
        *self
            .fingerprint
            .lock()
            .expect("blackboard fingerprint cache lock must not be poisoned") = Some(fingerprint);
        fingerprint
    }
}

pub(crate) struct BlackboardOverlay<'a> {
    base: &'a Blackboard,
    updates: BTreeMap<String, BlackboardValue>,
}

pub(crate) struct BlackboardDelta {
    updates: BTreeMap<String, BlackboardValue>,
}

impl<'a> BlackboardOverlay<'a> {
    pub(crate) fn new(base: &'a Blackboard) -> Self {
        Self {
            base,
            updates: BTreeMap::new(),
        }
    }

    pub(crate) fn into_delta(self) -> BlackboardDelta {
        BlackboardDelta {
            updates: self.updates,
        }
    }
}

impl BlackboardDelta {
    pub(crate) fn commit(self, target: &mut Blackboard) {
        for (key, value) in self.updates {
            target.set(key, value);
        }
    }
}

impl BlackboardAccess for BlackboardOverlay<'_> {
    fn set(&mut self, key: String, value: BlackboardValue) {
        self.updates.insert(key, value);
    }

    fn get(&self, key: &str) -> Option<&BlackboardValue> {
        self.updates.get(key).or_else(|| self.base.get(key))
    }

    fn values(&self) -> BTreeMap<String, BlackboardValue> {
        let mut values = self.base.values.clone();
        values.extend(self.updates.clone());
        values
    }

    fn deterministic_fingerprint(&self) -> Hash128 {
        Hash128::from_blake3(
            &postcard::to_allocvec(&(
                BlackboardAccess::deterministic_fingerprint(self.base),
                &self.updates,
            ))
            .expect("blackboard overlay must serialize for deterministic fingerprinting"),
        )
    }
}
