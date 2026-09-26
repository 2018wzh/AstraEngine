use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(with = "u32")]
pub(super) struct VmVersion;
impl Serialize for VmVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(7)
    }
}
impl<'de> Deserialize<'de> for VmVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if u32::deserialize(deserializer)? == 7 {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom(
                "unsupported CMVS VM snapshot version",
            ))
        }
    }
}
