//! Script loading owned by the core, without a Host VFS or runtime provider.
use crate::{
    parse_ps2a, CmvsArchive, CmvsPs2aVmState, CmvsScript, CmvsScriptFrameIdentity,
    CMVS_MAX_SCRIPT_CALL_INDEX,
};
use astra_core::Hash256;
use astra_emu_sdk::{validate_archive_uri, CoreError};
use std::{collections::BTreeMap, io::Read};

const MAX_SCRIPT_BYTES: usize = 64 * 1024 * 1024;

impl CmvsArchive {
    /// Resolve a VM script-call operand and load it through the same archive.
    pub fn load_called_script(
        &self,
        name: &str,
        frame: u16,
        vm: &mut CmvsPs2aVmState,
    ) -> Result<CmvsScript, CoreError> {
        let uri = self.resolve_script_uri(name)?;
        self.load_script(&uri, frame, vm)
    }

    /// Decode the complete script before replacing its frame's data segment.
    /// A failed read or parse leaves the interpreter untouched.
    pub fn load_script(
        &self,
        uri: &str,
        frame: u16,
        vm: &mut CmvsPs2aVmState,
    ) -> Result<CmvsScript, CoreError> {
        validate_frame(frame)?;
        validate_archive_uri(&self.manifest().prefix, uri)?;
        if self.stat(uri)?.size > MAX_SCRIPT_BYTES as u64 {
            return Err(script_bound());
        }
        let mut bytes = Vec::new();
        self.open_stream(uri)?
            .take(MAX_SCRIPT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                CoreError::invalid("ASTRA_EMU_CMVS_SCRIPT_READ", "script could not be read")
            })?;
        if bytes.len() > MAX_SCRIPT_BYTES {
            return Err(script_bound());
        }
        let script = parse_ps2a(&bytes)?;
        vm.install_script_frame(frame, uri, Hash256::from_sha256(&bytes), &script)?;
        tracing::debug!(
            event = "cmvs.script.loaded",
            frame,
            decoded_bytes = script.decoded_size,
            "CMVS script loaded"
        );
        Ok(script)
    }
}

impl CmvsPs2aVmState {
    /// Publish a fully decoded frame and its lookup tables as one load operation.
    pub fn install_script_frame(
        &mut self,
        frame: u16,
        uri: &str,
        hash: Hash256,
        script: &CmvsScript,
    ) -> Result<(), CoreError> {
        validate_frame(frame)?;
        validate_archive_uri("cmvs:/", uri)?;
        let name_index = script.name_index.clone();
        let string_lengths = script.private_string_length_table();
        self.install_script_data(frame, &script.data_segment)?;
        self.script_frames.insert(
            frame,
            CmvsScriptFrameIdentity {
                script_uri: uri.to_owned(),
                script_hash: hash,
            },
        );
        self.script_name_indices.insert(frame, name_index);
        self.frame_string_lengths.insert(frame, string_lengths);
        self.program_counter = script.header.initial_pc;
        Ok(())
    }

    /// Replace initial script memory, including removal of stale sparse words.
    /// Partial trailing words remain outside the VM's dword access range.
    pub fn install_script_data(&mut self, frame: u16, bytes: &[u8]) -> Result<(), CoreError> {
        validate_frame(frame)?;
        if bytes.len() > MAX_SCRIPT_BYTES {
            return Err(script_bound());
        }
        let words: BTreeMap<_, _> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, word)| **word != [0; 4])
            .map(|(index, word)| ((index * 4) as u32, u32::from_le_bytes(*word)))
            .collect();
        self.script_data_segment_sizes
            .insert(frame, bytes.len() as u32);
        if words.is_empty() {
            self.script_data_segment_words.remove(&frame);
        } else {
            self.script_data_segment_words.insert(frame, words);
        }
        Ok(())
    }
}

fn validate_frame(frame: u16) -> Result<(), CoreError> {
    if u32::from(frame) > CMVS_MAX_SCRIPT_CALL_INDEX {
        return Err(CoreError::invalid(
            "ASTRA_EMU_CMVS_SCRIPT_FRAME",
            "script frame index exceeds the engine limit",
        ));
    }
    Ok(())
}

fn script_bound() -> CoreError {
    CoreError::invalid(
        "ASTRA_EMU_CMVS_SCRIPT_BOUND",
        "script exceeds the byte limit",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_script_data_clears_old_words_and_preserves_other_frames() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.install_script_data(0, &[1, 0, 0, 0, 2, 0, 0, 0])
            .unwrap();
        vm.install_script_data(1, &[3, 0, 0, 0]).unwrap();
        vm.install_script_data(0, &[0; 4]).unwrap();
        assert_eq!(vm.script_data_segment_sizes[&0], 4);
        assert!(!vm.script_data_segment_words.contains_key(&0));
        assert_eq!(vm.script_data_segment_words[&1][&0], 3);
        vm.install_script_data(0, &[]).unwrap();
        assert_eq!(vm.script_data_segment_sizes[&0], 0);
        assert!(!vm.script_data_segment_words.contains_key(&0));
    }

    #[test]
    fn script_memory_keeps_little_endian_words_and_exact_byte_size() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.install_script_data(2, &[1, 2, 3, 4, 0, 0, 0, 0, 255])
            .unwrap();
        assert_eq!(vm.script_data_segment_sizes[&2], 9);
        assert_eq!(
            vm.script_data_segment_words[&2],
            BTreeMap::from([(0, 0x04030201)])
        );
    }
}
