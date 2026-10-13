use astra_byte_source::{
    audit_source, BoundedByteSource, ByteRange, ByteSourceStat, DEFAULT_MAX_RANGE_BYTES,
};

use astra_core::{Hash256, SchemaVersion};

use schemars::JsonSchema;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use std::{collections::BTreeSet, sync::Arc};

use thiserror::Error;

#[cfg(test)]
mod tests;

mod m_a_g_i_c;
pub use m_a_g_i_c::AstraContainerBuilder;
pub use m_a_g_i_c::AstraContainerReader;
pub use m_a_g_i_c::ContainerBlob;
pub use m_a_g_i_c::ContainerCryptoProvider;
pub use m_a_g_i_c::ContainerError;
pub use m_a_g_i_c::ContainerKind;
pub use m_a_g_i_c::EncryptionDescriptor;
pub use m_a_g_i_c::ExternalKeyRef;
pub use m_a_g_i_c::MigrationPolicy;
pub use m_a_g_i_c::NoCryptoProvider;
pub use m_a_g_i_c::SectionCodec;
pub use m_a_g_i_c::SectionEntry;
pub use m_a_g_i_c::SectionPayload;
use m_a_g_i_c::ALIGNMENT;
pub use m_a_g_i_c::CURRENT_CONTAINER_VERSION;
use m_a_g_i_c::FOOTER_LEN;
use m_a_g_i_c::FOOTER_MAGIC;
use m_a_g_i_c::HEADER_LEN;
use m_a_g_i_c::MAGIC;
use m_a_g_i_c::MAX_DECODED_SECTION_LEN;
use m_a_g_i_c::MAX_SECTION_COUNT;
use m_a_g_i_c::MAX_TABLE_LEN;
mod astra_container_reader;
use astra_container_reader::write_container;
mod read_container;
use read_container::read_container;
use read_container::read_container_source;
use read_container::read_source_range;
pub use read_container::section_aad_hash;
use read_container::section_aad_hash_from_section;
use read_container::section_merkle_root;
use read_container::SectionAad;
mod hash_section_aad;
use hash_section_aad::align;
use hash_section_aad::decode_payload;
use hash_section_aad::encode_payload;
use hash_section_aad::hash_section_aad;
use hash_section_aad::hex_prefix;
use hash_section_aad::validate_section_descriptor;
use hash_section_aad::validate_section_payloads;
