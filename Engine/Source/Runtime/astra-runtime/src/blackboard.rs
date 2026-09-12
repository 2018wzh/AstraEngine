use std::collections::BTreeMap;

use astra_core::StableId;
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
}

impl Clone for Blackboard {
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
        }
    }
}

impl PartialEq for Blackboard {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl Blackboard {
    pub fn set(&mut self, key: impl Into<String>, value: BlackboardValue) {
        let key = key.into();

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
}
