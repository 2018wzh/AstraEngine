use crate::{mgv1_embedded_ogg, CmvsArchive, MGV1_MAGIC};
use astra_emu_sdk::{decode_audio, CoreError};
use kira::sound::static_sound::StaticSoundData;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_ENCODED_AUDIO_BYTES: u64 = 64 * 1024 * 1024;

impl CmvsArchive {
    /// Decode ordinary audio or a validated MGV1 embedded Ogg stream.
    /// This does not interpret MGV video timing or create a playback worker.
    pub fn load_audio(
        &self,
        uri: &str,
        max_frames: usize,
        cancelled: &AtomicBool,
    ) -> Result<StaticSoundData, CoreError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(CoreError::invalid(
                "ASTRA_EMU_AUDIO_CANCELLED",
                "audio decode cancelled",
            ));
        }
        let size = self.stat(uri)?.size;
        if size == 0 || size > MAX_ENCODED_AUDIO_BYTES {
            return Err(CoreError::invalid(
                "ASTRA_EMU_CMVS_AUDIO_BOUND",
                "encoded audio exceeds the byte limit",
            ));
        }
        let source = self.read_range(uri, 0, size)?.bytes;
        decode_source(source.as_slice(), max_frames, cancelled)
    }
}

fn decode_source(
    source: &[u8],
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<StaticSoundData, CoreError> {
    let audio = if source.starts_with(MGV1_MAGIC) {
        mgv1_embedded_ogg(source)?
    } else {
        source
    };
    decode_audio(audio.to_vec(), max_frames, cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_mgv_is_not_retried_as_another_audio_format() {
        assert_eq!(
            decode_source(b"MGV1invalid", 32, &AtomicBool::new(false))
                .err()
                .unwrap()
                .code(),
            "ASTRA_EMU_CMVS_MGV_HEADER"
        );
    }
}
