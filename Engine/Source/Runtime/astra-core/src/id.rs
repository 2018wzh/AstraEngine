use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, JsonSchema)]
pub struct StableId(Uuid);

impl StableId {
    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(value).map(Self)
    }

    pub fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }

    pub fn nil() -> Self {
        Self(Uuid::nil())
    }

    pub fn deterministic_v7(unix_ms: u64, sequence: u64, entropy: u64) -> Self {
        let mut bytes = [0u8; 16];
        let ts = unix_ms & 0x0000_ffff_ffff_ffff;
        bytes[0] = (ts >> 40) as u8;
        bytes[1] = (ts >> 32) as u8;
        bytes[2] = (ts >> 24) as u8;
        bytes[3] = (ts >> 16) as u8;
        bytes[4] = (ts >> 8) as u8;
        bytes[5] = ts as u8;
        bytes[6] = 0x70 | (((sequence >> 8) as u8) & 0x0f);
        bytes[7] = sequence as u8;
        let mixed = entropy ^ sequence.rotate_left(17) ^ unix_ms.rotate_left(29);
        bytes[8] = 0x80 | ((mixed >> 56) as u8 & 0x3f);
        bytes[9] = (mixed >> 48) as u8;
        bytes[10] = (mixed >> 40) as u8;
        bytes[11] = (mixed >> 32) as u8;
        bytes[12] = (mixed >> 24) as u8;
        bytes[13] = (mixed >> 16) as u8;
        bytes[14] = (mixed >> 8) as u8;
        bytes[15] = mixed as u8;
        Self(Uuid::from_bytes(bytes))
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for StableId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for StableId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for StableId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StableIdGenerator {
    seed: u64,
    step: u64,
    sequence: u64,
}

impl StableIdGenerator {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            step: 0,
            sequence: 0,
        }
    }

    pub fn set_step(&mut self, step: u64) {
        self.step = step;
    }

    pub fn next_id(&mut self) -> StableId {
        let id = StableId::deterministic_v7(self.step, self.sequence, self.seed);
        self.sequence = self.sequence.wrapping_add(1);
        id
    }
}
