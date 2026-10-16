use super::HostedLimits;
use rfvp::host_api::{
    AudioParams, AudioStreamDesc, AudioStreamId, EncodedAudioKind, RfvpAudio, RfvpError, RfvpResult,
};
/// Semantic audio operations.  Encoded and PCM payloads are bounded and are
/// copied only when the RFVP core actually emits a command.
#[derive(Debug, Clone, PartialEq)]
pub enum HostedAudioOperation {
    LoadResource {
        id: AudioStreamId,
        kind: EncodedAudioKind,
        resource_uri: String,
    },
    LoadEncoded {
        id: AudioStreamId,
        kind: EncodedAudioKind,
        bytes: Vec<u8>,
    },
    CreateStream {
        id: AudioStreamId,
        desc: AudioStreamDesc,
    },
    SubmitI16 {
        id: AudioStreamId,
        samples: Vec<i16>,
    },
    SubmitF32 {
        id: AudioStreamId,
        samples: Vec<f32>,
    },
    Play {
        id: AudioStreamId,
        params: AudioParams,
        fade_in_ms: u32,
    },
    Stop {
        id: AudioStreamId,
        fade_ms: u32,
    },
    Pause(AudioStreamId),
    Resume(AudioStreamId),
    SetParams {
        id: AudioStreamId,
        params: AudioParams,
    },
    SetMasterVolume(f32),
    DestroyStream(AudioStreamId),
}
pub(super) struct RecordingAudio<'a> {
    limits: HostedLimits,
    audio_bytes: usize,
    pub(super) operations: Vec<HostedAudioOperation>,
    failure: Option<RfvpError>,
    playback_states: &'a [(AudioStreamId, bool)],
}

impl<'a> RecordingAudio<'a> {
    pub(super) fn new(limits: HostedLimits, playback_states: &'a [(AudioStreamId, bool)]) -> Self {
        Self {
            limits,
            audio_bytes: 0,
            operations: Vec::new(),
            failure: None,
            playback_states,
        }
    }

    fn push(&mut self, operation: HostedAudioOperation) -> RfvpResult<()> {
        self.finish()?;
        if self.operations.len() >= self.limits.max_audio_operations {
            return Err(RfvpError::CapacityExceeded);
        }
        self.operations.push(operation);
        Ok(())
    }

    pub(super) fn finish(&self) -> RfvpResult<()> {
        self.failure.map_or(Ok(()), Err)
    }

    fn reserve_audio_bytes(&mut self, additional: usize) -> RfvpResult<()> {
        self.finish()?;
        self.audio_bytes = self
            .audio_bytes
            .checked_add(additional)
            .ok_or(RfvpError::CapacityExceeded)?;
        if self.audio_bytes > self.limits.max_audio_bytes {
            return Err(RfvpError::CapacityExceeded);
        }
        Ok(())
    }
}

impl RfvpAudio for RecordingAudio<'_> {
    fn load_resource(
        &mut self,
        id: AudioStreamId,
        kind: EncodedAudioKind,
        resource_uri: &str,
    ) -> RfvpResult<()> {
        if resource_uri.is_empty() {
            return Err(RfvpError::InvalidArgument);
        }
        self.push(HostedAudioOperation::LoadResource {
            id,
            kind,
            resource_uri: resource_uri.into(),
        })
    }

    fn load_encoded(
        &mut self,
        id: AudioStreamId,
        kind: EncodedAudioKind,
        bytes: &[u8],
    ) -> RfvpResult<()> {
        self.reserve_audio_bytes(bytes.len())?;
        self.push(HostedAudioOperation::LoadEncoded {
            id,
            kind,
            bytes: bytes.to_vec(),
        })
    }

    fn load_encoded_owned(
        &mut self,
        id: AudioStreamId,
        kind: EncodedAudioKind,
        bytes: Vec<u8>,
    ) -> RfvpResult<()> {
        self.reserve_audio_bytes(bytes.len())?;
        self.push(HostedAudioOperation::LoadEncoded { id, kind, bytes })
    }

    fn create_stream(&mut self, id: AudioStreamId, desc: AudioStreamDesc) -> RfvpResult<()> {
        self.push(HostedAudioOperation::CreateStream { id, desc })
    }

    fn submit_i16(&mut self, id: AudioStreamId, samples: &[i16]) -> RfvpResult<()> {
        let bytes = samples
            .len()
            .checked_mul(core::mem::size_of::<i16>())
            .ok_or(RfvpError::CapacityExceeded)?;
        self.reserve_audio_bytes(bytes)?;
        self.push(HostedAudioOperation::SubmitI16 {
            id,
            samples: samples.to_vec(),
        })
    }

    fn submit_i16_owned(&mut self, id: AudioStreamId, samples: Vec<i16>) -> RfvpResult<()> {
        let bytes = samples
            .len()
            .checked_mul(core::mem::size_of::<i16>())
            .ok_or(RfvpError::CapacityExceeded)?;
        self.reserve_audio_bytes(bytes)?;
        self.push(HostedAudioOperation::SubmitI16 { id, samples })
    }

    fn submit_f32(&mut self, id: AudioStreamId, samples: &[f32]) -> RfvpResult<()> {
        let bytes = samples
            .len()
            .checked_mul(core::mem::size_of::<f32>())
            .ok_or(RfvpError::CapacityExceeded)?;
        self.reserve_audio_bytes(bytes)?;
        self.push(HostedAudioOperation::SubmitF32 {
            id,
            samples: samples.to_vec(),
        })
    }

    fn submit_f32_owned(&mut self, id: AudioStreamId, samples: Vec<f32>) -> RfvpResult<()> {
        let bytes = samples
            .len()
            .checked_mul(core::mem::size_of::<f32>())
            .ok_or(RfvpError::CapacityExceeded)?;
        self.reserve_audio_bytes(bytes)?;
        self.push(HostedAudioOperation::SubmitF32 { id, samples })
    }

    fn play(&mut self, id: AudioStreamId, params: AudioParams, fade_in_ms: u32) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Play {
            id,
            params,
            fade_in_ms,
        })
    }

    fn stop(&mut self, id: AudioStreamId, fade_ms: u32) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Stop { id, fade_ms })
    }

    fn pause(&mut self, id: AudioStreamId) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Pause(id))
    }

    fn resume(&mut self, id: AudioStreamId) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Resume(id))
    }

    fn set_params(&mut self, id: AudioStreamId, params: AudioParams) -> RfvpResult<()> {
        self.push(HostedAudioOperation::SetParams { id, params })
    }

    fn set_master_volume(&mut self, volume: f32) -> RfvpResult<()> {
        if !volume.is_finite() {
            return Err(RfvpError::InvalidArgument);
        }
        self.push(HostedAudioOperation::SetMasterVolume(volume))
    }

    fn destroy_stream(&mut self, id: AudioStreamId) {
        if self.failure.is_some() {
            return;
        }
        if let Err(error) = self.push(HostedAudioOperation::DestroyStream(id)) {
            self.failure = Some(error);
        }
    }

    fn tick(&mut self, _elapsed_us: u64) -> RfvpResult<()> {
        // The Family mixer advances on bounded PCM consumption, not VM elapsed time.
        self.finish()
    }

    fn is_playing(&mut self, id: AudioStreamId) -> RfvpResult<bool> {
        Ok(self
            .playback_states
            .iter()
            .find(|(state_id, _)| *state_id == id)
            .map(|(_, playing)| *playing)
            .unwrap_or(false))
    }
}
