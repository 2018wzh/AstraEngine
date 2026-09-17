use super::*;
use crate::MusicaConfigAudioBus;
const TEST_STREAMS: [u32; 3] = [0xffff_ff00, 0xffff_ff01, 0xffff_ff02];
impl MusicaVm {
    pub(crate) fn config_test_audio(
        &mut self,
        bus: MusicaConfigAudioBus,
    ) -> Result<Vec<MusicaAudioCommand>, MusicaRuntimeError> {
        self.config_for_presentation()?;
        let (index, bus, uri) = match bus {
            MusicaConfigAudioBus::Bgm => (0, "bgm", "musica:/sys/BGMTest.wav"),
            MusicaConfigAudioBus::Voice => (1, "voice", "musica:/sys/VOICEtest.wav"),
            MusicaConfigAudioBus::Se => (2, "se", "musica:/sys/SEtest.wav"),
        };
        let id = TEST_STREAMS[index];
        let mut commands = match audio_commands::stop_audio_stream(&mut self.state, id, 0)? {
            Some(MusicaVmEvent::Audio { commands }) => commands,
            _ => return Err(MusicaRuntimeError::State),
        };
        audio_commands::append_audio_load_and_play(
            &mut self.state,
            &mut commands,
            id,
            uri,
            1000,
            0,
            false,
            0,
        )?;
        self.state.audio.insert(
            id,
            MusicaAudioState {
                bus: bus.into(),
                resource_uri: uri.into(),
                looped: false,
                volume_milli: 1000,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        Ok(commands)
    }
    pub(crate) fn stop_config_tests(
        &mut self,
    ) -> Result<Vec<MusicaAudioCommand>, MusicaRuntimeError> {
        let mut result = Vec::new();
        for id in TEST_STREAMS {
            if let Some(MusicaVmEvent::Audio { commands }) =
                audio_commands::stop_audio_stream(&mut self.state, id, 0)?
            {
                result.extend(commands);
            }
        }
        Ok(result)
    }
}
