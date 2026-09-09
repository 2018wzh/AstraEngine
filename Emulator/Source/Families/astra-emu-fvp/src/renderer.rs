use rfvp::host_api::{
    AudioParams, AudioSampleFormat, AudioStreamDesc, AudioStreamId, ColorRgba, DrawSolidCommand,
    DrawSpriteCommand, EncodedAudioKind, PixelBuffer, RfvpAudio, RfvpError, RfvpRenderer,
    RfvpResult, TextureDesc, TextureId, TextureRect,
};

pub(crate) struct NullRenderer;
impl RfvpRenderer for NullRenderer {
    fn create_texture(
        &mut self,
        _id: TextureId,
        _desc: TextureDesc,
        _pixels: Option<PixelBuffer<'_>>,
    ) -> RfvpResult<()> {
        Ok(())
    }
    fn update_texture(
        &mut self,
        _id: TextureId,
        _rect: TextureRect,
        _pixels: PixelBuffer<'_>,
    ) -> RfvpResult<()> {
        Ok(())
    }
    fn destroy_texture(&mut self, _id: TextureId) {}
    fn begin_frame(
        &mut self,
        width: u32,
        height: u32,
        _clear: Option<ColorRgba>,
    ) -> RfvpResult<()> {
        if width == 0 || height == 0 {
            Err(RfvpError::InvalidArgument)
        } else {
            Ok(())
        }
    }
    fn draw_sprite(&mut self, command: &DrawSpriteCommand) -> RfvpResult<()> {
        if command
            .vertices
            .iter()
            .any(|vertex| vertex.position.iter().any(|value| !value.is_finite()))
        {
            Err(RfvpError::InvalidArgument)
        } else {
            Ok(())
        }
    }
    fn draw_solid(&mut self, command: &DrawSolidCommand) -> RfvpResult<()> {
        if [
            command.color.r,
            command.color.g,
            command.color.b,
            command.color.a,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            Ok(())
        } else {
            Err(RfvpError::InvalidArgument)
        }
    }
    fn end_frame(&mut self) -> RfvpResult<()> {
        Ok(())
    }
    fn present(&mut self) -> RfvpResult<()> {
        Ok(())
    }
}

pub(crate) struct NullAudio;
impl RfvpAudio for NullAudio {
    fn load_resource(
        &mut self,
        _id: AudioStreamId,
        _kind: EncodedAudioKind,
        _resource_uri: &str,
    ) -> RfvpResult<()> {
        Ok(())
    }
    fn load_encoded(
        &mut self,
        _id: AudioStreamId,
        _kind: EncodedAudioKind,
        _bytes: &[u8],
    ) -> RfvpResult<()> {
        Ok(())
    }
    fn create_stream(&mut self, _id: AudioStreamId, desc: AudioStreamDesc) -> RfvpResult<()> {
        if desc.sample_rate == 0 || desc.channels == 0 || desc.channels > 2 {
            return Err(RfvpError::InvalidArgument);
        }
        if !matches!(
            desc.sample_format,
            AudioSampleFormat::I16 | AudioSampleFormat::F32
        ) {
            return Err(RfvpError::Unsupported);
        }
        Ok(())
    }
    fn submit_i16(&mut self, _id: AudioStreamId, _samples: &[i16]) -> RfvpResult<()> {
        Ok(())
    }
    fn submit_f32(&mut self, _id: AudioStreamId, _samples: &[f32]) -> RfvpResult<()> {
        Ok(())
    }
    fn play(
        &mut self,
        _id: AudioStreamId,
        _params: AudioParams,
        _fade_in_ms: u32,
    ) -> RfvpResult<()> {
        Ok(())
    }
    fn stop(&mut self, _id: AudioStreamId, _fade_ms: u32) -> RfvpResult<()> {
        Ok(())
    }
    fn pause(&mut self, _id: AudioStreamId) -> RfvpResult<()> {
        Ok(())
    }
    fn resume(&mut self, _id: AudioStreamId) -> RfvpResult<()> {
        Ok(())
    }
    fn set_params(&mut self, _id: AudioStreamId, _params: AudioParams) -> RfvpResult<()> {
        Ok(())
    }
    fn set_master_volume(&mut self, volume: f32) -> RfvpResult<()> {
        if volume.is_finite() {
            Ok(())
        } else {
            Err(RfvpError::InvalidArgument)
        }
    }
    fn destroy_stream(&mut self, _id: AudioStreamId) {}
    fn tick(&mut self, _delta_us: u64) -> RfvpResult<()> {
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct SessionClock {
    micros: u64,
}
impl SessionClock {
    pub(crate) fn advance_ns(&mut self, elapsed_ns: u64) {
        self.micros = self.micros.saturating_add(elapsed_ns / 1_000);
    }
}
impl rfvp::host_api::RfvpClock for SessionClock {
    fn ticks_us(&mut self) -> u64 {
        self.micros
    }
}
