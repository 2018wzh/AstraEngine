//! Native eden save container. Encoding is an explicit caller choice; no detection.
//! This codec preserves fields and does not establish that a VM can restore them.
mod body;
mod checkpoint;
mod position;
mod slot;
mod state;
pub use slot::{export_slot, EdenSlotExport};
mod write_checkpoint;
pub use checkpoint::{EdenCheckpoint, EdenHistoryMessage};
pub use position::EdenMessagePosition;
pub use state::{EdenExportRejection, EdenExportState};
#[cfg(test)]
mod checkpoint_tests;
#[cfg(test)]
mod tests;
use crate::CoreError;
use encoding_rs::{Encoding, GBK, SHIFT_JIS, WINDOWS_1252};
use flate2::{write::ZlibEncoder, Compression, Decompress, FlushDecompress, Status};
use std::io::Write;

const MAX_CONTAINER: usize = 16 * 1024 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub enum EdenEdition {
    Japanese,
    English,
}
impl EdenEdition {
    fn magic(self) -> &'static [u8; 4] {
        match self {
            Self::Japanese => b";\n!\xaa",
            Self::English => &[0; 4],
        }
    }
    fn signature(self) -> &'static [u8] {
        match self {
            Self::Japanese => b"eden 1.00",
            Self::English => b"eden_en 1.00",
        }
    }
}
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub enum EdenSaveEncoding {
    ShiftJis,
    Gbk,
    Windows1252,
}
impl EdenSaveEncoding {
    fn encoding(self) -> &'static Encoding {
        match self {
            Self::ShiftJis => SHIFT_JIS,
            Self::Gbk => GBK,
            Self::Windows1252 => WINDOWS_1252,
        }
    }
    fn decode(self, value: &[u8]) -> Result<String, CoreError> {
        let text = self
            .encoding()
            .decode_without_bom_handling_and_without_replacement(value)
            .ok_or_else(|| invalid("ENCODING", "save text is invalid for the selected encoding"))?;
        // Refuse non-roundtrippable byte aliases as well as replacement decoding.
        if self.encode(&text)? != value {
            return Err(invalid(
                "ENCODING",
                "save text cannot be represented exactly",
            ));
        }
        Ok(text.into_owned())
    }
    fn encode(self, text: &str) -> Result<Vec<u8>, CoreError> {
        let (bytes, _, replaced) = self.encoding().encode(text);
        if replaced {
            return Err(invalid(
                "ENCODING",
                "save text is not representable in the selected encoding",
            ));
        }
        Ok(bytes.into_owned())
    }
}
/// Ordered native fields. Values may contain commercial text and must not be logged.
#[derive(Clone, PartialEq, Eq)]
pub struct EdenSaveField {
    pub name: String,
    pub value: String,
}
#[derive(Clone, PartialEq, Eq)]
pub struct EdenSave {
    pub edition: EdenEdition,
    pub encoding: EdenSaveEncoding,
    pub comment: String,
    pub route: [u8; 4],
    pub variables: Vec<EdenSaveField>,
    pub backlog: Vec<Vec<EdenSaveField>>,
}
impl EdenSave {
    /// Parse the exact edition signature and a single bounded zlib stream.
    pub fn decode(
        bytes: &[u8],
        edition: EdenEdition,
        encoding: EdenSaveEncoding,
    ) -> Result<Self, CoreError> {
        if bytes.len() > MAX_CONTAINER {
            return Err(invalid("BOUND", "save container exceeds the byte limit"));
        }
        let suffix = bytes
            .strip_prefix(edition.magic())
            .and_then(|b| b.strip_prefix(edition.signature()))
            .and_then(|b| b.strip_prefix(&[0]))
            .ok_or_else(|| invalid("SIGNATURE", "save edition signature does not match"))?;
        let end = suffix
            .iter()
            .position(|b| *b == 0)
            .filter(|end| *end <= 4096)
            .ok_or_else(|| invalid("HEADER", "save comment is not bounded and terminated"))?;
        let comment = encoding.decode(&suffix[..end])?;
        let suffix = &suffix[end + 1..];
        let route: [u8; 4] = suffix
            .get(..4)
            .ok_or_else(|| invalid("HEADER", "save route is missing"))?
            .try_into()
            .unwrap();
        let compressed = &suffix[4..];
        let mut decoder = Decompress::new(true);
        let mut raw = vec![0; MAX_BODY + 1];
        let status = decoder
            .decompress(compressed, &mut raw, FlushDecompress::Finish)
            .map_err(|_| invalid("ZLIB", "save compressed body is corrupt"))?;
        if decoder.total_out() > MAX_BODY as u64 {
            return Err(invalid("BOUND", "save body exceeds the byte limit"));
        }
        if status != Status::StreamEnd || decoder.total_in() as usize != compressed.len() {
            return Err(invalid(
                "ZLIB",
                "save stream is incomplete or contains trailing data",
            ));
        }
        raw.truncate(decoder.total_out() as usize);
        let text = encoding.decode(&raw)?;
        let (variables, backlog) = body::parse(&text)?;
        Ok(Self {
            edition,
            encoding,
            comment,
            route,
            variables,
            backlog,
        })
    }
    /// Container serialization only. Callers must validate VM representability separately.
    pub fn encode(&self) -> Result<Vec<u8>, CoreError> {
        let comment = self.encoding.encode(&self.comment)?;
        if comment.len() > 4096 || comment.contains(&0) {
            return Err(invalid("HEADER", "save comment is invalid"));
        }
        let text = body::format(self)?;
        let raw = self.encoding.encode(&text)?;
        if raw.len() > MAX_BODY {
            return Err(invalid("BOUND", "save body exceeds the byte limit"));
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.edition.magic());
        bytes.extend_from_slice(self.edition.signature());
        bytes.push(0);
        bytes.extend_from_slice(&comment);
        bytes.push(0);
        bytes.extend_from_slice(&self.route);
        let mut encoder = ZlibEncoder::new(bytes, Compression::default());
        encoder
            .write_all(&raw)
            .map_err(|_| invalid("ZLIB", "save compression failed"))?;
        let bytes = encoder
            .finish()
            .map_err(|_| invalid("ZLIB", "save compression failed"))?;
        if bytes.len() > MAX_CONTAINER {
            return Err(invalid("BOUND", "save container exceeds the byte limit"));
        }
        Ok(bytes)
    }
    pub fn variable(&self, name: &str) -> Option<&str> {
        self.variables
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.value.as_str())
    }
}
fn invalid(suffix: &str, message: &str) -> CoreError {
    let code = match suffix {
        "BOUND" => "ASTRA_EMU_EDEN_SAVE_BOUND",
        "ENCODING" => "ASTRA_EMU_EDEN_SAVE_ENCODING",
        "SIGNATURE" => "ASTRA_EMU_EDEN_SAVE_SIGNATURE",
        "HEADER" => "ASTRA_EMU_EDEN_SAVE_HEADER",
        "POSITION" => "ASTRA_EMU_EDEN_SAVE_POSITION",
        "STATE" => "ASTRA_EMU_EDEN_SAVE_STATE",
        "ZLIB" => "ASTRA_EMU_EDEN_SAVE_ZLIB",
        _ => "ASTRA_EMU_EDEN_SAVE_BODY",
    };
    CoreError::invalid(code, message)
}
