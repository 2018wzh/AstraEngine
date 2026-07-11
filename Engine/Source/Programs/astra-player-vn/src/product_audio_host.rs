use astra_player_core::PlayerHostCommandResult;

#[derive(Default)]
pub struct NativeVnProductAudioHost {
    mixer: Option<astra_player_core::PlayerPersistentAudioMixer>,
    output: Option<astra_player_core::PlayerHostResourceId>,
    next_packet_sequence: u64,
    queue: Option<astra_player_core::PlayerAudioQueueController>,
    voice_kinds: std::collections::BTreeMap<String, String>,
}

impl NativeVnProductAudioHost {
    const BUFFERED_FRAMES: u32 = 4_096;
    const TARGET_QUEUED_FRAMES: usize = 2_048;
    const MAX_RENDER_FRAMES: usize = 1_024;
    const MAX_VOICES: usize = 64;
    const MAX_CONVERTED_SAMPLES: usize = 20_000_000;

    pub fn is_active(&self) -> bool {
        self.mixer
            .as_ref()
            .is_some_and(|mixer| mixer.active_voice_count() > 0)
    }

    pub async fn start(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
        request: &crate::NativeVnAudioRequest,
        audio: astra_player_core::PlayerDecodedAudio,
    ) -> Result<(), astra_platform::PlatformError> {
        let looping = parse_audio_bool(request, "loop", request.command == "bgm")?;
        let gain = parse_audio_f32(request, "gain", 1.0)?;
        let bus = request
            .attributes
            .get("bus")
            .cloned()
            .unwrap_or_else(|| request.command.clone());
        let (output_sample_rate, output_channels) = if let Some(mixer) = &self.mixer {
            (mixer.sample_rate(), mixer.channels())
        } else {
            let query = source
                .prepare_audio_output_format_query()
                .map_err(|error| player_platform_error("player.audio.format.prepare", error))?;
            let result = executor
                .execute_batch(query)
                .await
                .map_err(|error| player_platform_error("player.audio.format", error))?;
            match result.as_slice() {
                [PlayerHostCommandResult::AudioFormat {
                    sample_rate,
                    channels,
                }] => (*sample_rate, *channels),
                _ => {
                    return Err(player_platform_error(
                        "player.audio.format",
                        "ASTRA_PLAYER_AUDIO_FORMAT_RESULT: platform returned an invalid preferred format",
                    ));
                }
            }
        };
        let audio = audio
            .convert_to(
                output_sample_rate,
                output_channels,
                Self::MAX_CONVERTED_SAMPLES,
            )
            .map_err(|error| player_platform_error("player.audio.convert", error))?;
        if self.mixer.is_none() {
            let (output, open) = source
                .prepare_persistent_audio_open(
                    audio.sample_rate,
                    audio.channels,
                    Self::BUFFERED_FRAMES,
                )
                .map_err(|error| player_platform_error("player.audio.mixer.open.prepare", error))?;
            let result = executor
                .execute_batch(open)
                .await
                .map_err(|error| player_platform_error("player.audio.mixer.open", error))?;
            if !matches!(
                result.as_slice(),
                [PlayerHostCommandResult::AudioOpened { output: opened }] if *opened == output
            ) {
                return Err(player_platform_error(
                    "player.audio.mixer.open",
                    "ASTRA_PLAYER_MIXER_OPEN_RESULT: platform returned an invalid output",
                ));
            }
            self.mixer = Some(
                astra_player_core::PlayerPersistentAudioMixer::new(
                    audio.sample_rate,
                    audio.channels,
                    Self::MAX_VOICES,
                    Self::MAX_RENDER_FRAMES,
                )
                .map_err(|error| player_platform_error("player.audio.mixer.create", error))?,
            );
            self.output = Some(output);
            self.next_packet_sequence = 1;
            self.queue = Some(
                astra_player_core::PlayerAudioQueueController::new(
                    Self::TARGET_QUEUED_FRAMES,
                    Self::MAX_RENDER_FRAMES,
                )
                .map_err(|error| player_platform_error("player.audio.mixer.queue", error))?,
            );
        }
        let mixer = self.mixer.as_mut().ok_or_else(|| {
            player_platform_error("player.audio.mixer.start", "ASTRA_PLAYER_MIXER_MISSING")
        })?;
        mixer
            .start_voice(astra_player_core::PlayerPersistentVoiceSpec {
                id: request.command_id.clone(),
                bus: bus.clone(),
                audio,
                looping,
                gain,
            })
            .map_err(|error| player_platform_error("player.audio.mixer.start", error))?;
        if let Some(fade_ms) = request.attributes.get("fade") {
            let fade_ms = fade_ms.parse::<u64>().map_err(|_| {
                player_platform_error(
                    "player.audio.mixer.fade",
                    "ASTRA_PLAYER_AUDIO_FADE_INVALID: fade must be an unsigned millisecond value",
                )
            })?;
            if fade_ms > 0 {
                let duration_frames = u64::from(mixer.sample_rate())
                    .checked_mul(fade_ms)
                    .and_then(|value| value.checked_add(999))
                    .map(|value| value / 1_000)
                    .ok_or_else(|| {
                        player_platform_error(
                            "player.audio.mixer.fade",
                            "ASTRA_PLAYER_AUDIO_FADE_OVERFLOW",
                        )
                    })?;
                mixer
                    .set_bus_gain(&bus, 0.0)
                    .and_then(|_| mixer.fade_bus(&bus, 1.0, duration_frames.max(1)))
                    .map_err(|error| player_platform_error("player.audio.mixer.fade", error))?;
            }
        }
        self.voice_kinds
            .insert(request.command_id.clone(), request.command.clone());
        Ok(())
    }

    pub async fn pump(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
        completed_signals: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), astra_platform::PlatformError> {
        if !self.is_active() {
            return Ok(());
        }
        let output = self.output.ok_or_else(|| {
            player_platform_error(
                "player.audio.mixer.pump",
                "ASTRA_PLAYER_MIXER_OUTPUT_MISSING",
            )
        })?;
        let query = source
            .prepare_persistent_audio_query(output)
            .map_err(|error| player_platform_error("player.audio.mixer.query.prepare", error))?;
        let result = executor
            .execute_batch(query)
            .await
            .map_err(|error| player_platform_error("player.audio.mixer.query", error))?;
        let (queued_frames, underflow_count) = match result.as_slice() {
            [PlayerHostCommandResult::AudioState {
                output: state_output,
                queued_frames,
                underflow_count,
                ..
            }] if *state_output == output => (
                usize::try_from(*queued_frames).map_err(|_| {
                    player_platform_error(
                        "player.audio.mixer.query",
                        "ASTRA_PLAYER_MIXER_QUEUE_RANGE",
                    )
                })?,
                *underflow_count,
            ),
            _ => {
                return Err(player_platform_error(
                    "player.audio.mixer.query",
                    "ASTRA_PLAYER_MIXER_QUERY_RESULT: platform returned invalid queue state",
                ));
            }
        };
        let frames = self
            .queue
            .as_mut()
            .ok_or_else(|| {
                player_platform_error(
                    "player.audio.mixer.query",
                    "ASTRA_PLAYER_AUDIO_QUEUE_MISSING",
                )
            })?
            .observe(queued_frames, underflow_count)
            .map_err(|error| player_platform_error("player.audio.mixer.query", error))?;
        if frames == 0 {
            return Ok(());
        }
        let mixed = self
            .mixer
            .as_mut()
            .ok_or_else(|| {
                player_platform_error("player.audio.mixer.pump", "ASTRA_PLAYER_MIXER_MISSING")
            })?
            .render(frames)
            .map_err(|error| player_platform_error("player.audio.mixer.render", error))?;
        let submit = source
            .prepare_persistent_audio_submit(output, self.next_packet_sequence, &mixed)
            .map_err(|error| player_platform_error("player.audio.mixer.submit.prepare", error))?;
        let submitted = executor
            .execute_batch(submit)
            .await
            .map_err(|error| player_platform_error("player.audio.mixer.submit", error))?;
        if !matches!(submitted.as_slice(), [PlayerHostCommandResult::Unit]) {
            return Err(player_platform_error(
                "player.audio.mixer.submit",
                "ASTRA_PLAYER_MIXER_SUBMIT_RESULT: platform returned an invalid result",
            ));
        }
        self.next_packet_sequence = self.next_packet_sequence.checked_add(1).ok_or_else(|| {
            player_platform_error(
                "player.audio.mixer.submit",
                "ASTRA_PLAYER_AUDIO_PACKET_SEQUENCE",
            )
        })?;
        self.queue
            .as_mut()
            .ok_or_else(|| {
                player_platform_error(
                    "player.audio.mixer.submit",
                    "ASTRA_PLAYER_AUDIO_QUEUE_MISSING",
                )
            })?
            .record_submit()
            .map_err(|error| player_platform_error("player.audio.mixer.submit", error))?;
        for completion in mixed.completed {
            let kind = self
                .voice_kinds
                .remove(&completion.voice_id)
                .ok_or_else(|| {
                    player_platform_error(
                        "player.audio.mixer.complete",
                        "ASTRA_PLAYER_AUDIO_COMPLETION_OWNER_MISSING",
                    )
                })?;
            completed_signals.insert(completion.voice_id.clone());
            completed_signals.insert(format!("{}.end", completion.voice_id));
            if kind == "voice" {
                completed_signals.insert("voice_end".into());
            }
            tracing::info!(
                event = "astra.player.audio.completed",
                command_id = %completion.voice_id,
                command = %kind,
                rendered_frames = completion.rendered_frames,
                "Persistent Player audio voice completed"
            );
        }
        Ok(())
    }

    pub fn control(
        &mut self,
        request: &crate::NativeVnAudioControlRequest,
        completed_signals: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), astra_platform::PlatformError> {
        let mixer = self.mixer.as_mut().ok_or_else(|| {
            player_platform_error("player.audio.mixer.control", "ASTRA_PLAYER_MIXER_MISSING")
        })?;
        match request.action.as_str() {
            "pause" => mixer
                .pause_voice(&request.target)
                .map_err(|error| player_platform_error("player.audio.mixer.pause", error))?,
            "resume" => mixer
                .resume_voice(&request.target)
                .map_err(|error| player_platform_error("player.audio.mixer.resume", error))?,
            "stop" => {
                let completion = mixer
                    .stop_voice(&request.target)
                    .map_err(|error| player_platform_error("player.audio.mixer.stop", error))?;
                let kind = self.voice_kinds.remove(&request.target).ok_or_else(|| {
                    player_platform_error(
                        "player.audio.mixer.stop",
                        "ASTRA_PLAYER_AUDIO_COMPLETION_OWNER_MISSING",
                    )
                })?;
                completed_signals.insert(completion.voice_id.clone());
                completed_signals.insert(format!("{}.end", completion.voice_id));
                if kind == "voice" {
                    completed_signals.insert("voice_end".into());
                }
            }
            _ => {
                return Err(player_platform_error(
                    "player.audio.mixer.control",
                    format!(
                        "ASTRA_PLAYER_AUDIO_CONTROL_UNSUPPORTED: {}",
                        request.command_id
                    ),
                ));
            }
        }
        tracing::info!(
            event = "astra.player.audio.controlled",
            command_id = %request.command_id,
            action = %request.action,
            target = %request.target,
            "Player applied a persistent audio control"
        );
        Ok(())
    }

    pub async fn shutdown(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
    ) -> Result<(), astra_platform::PlatformError> {
        let Some(output) = self.output.take() else {
            return Ok(());
        };
        let drain_result = async {
            let drain = source
                .prepare_persistent_audio_drain(output)
                .map_err(|error| player_platform_error("player.audio.mixer.drain.prepare", error))?;
            let drained = executor
                .execute_batch(drain)
                .await
                .map_err(|error| player_platform_error("player.audio.mixer.drain", error))?;
            if !matches!(
                drained.as_slice(),
                [PlayerHostCommandResult::AudioDrained { output: drained_output, .. }] if *drained_output == output
            ) {
                return Err(player_platform_error(
                    "player.audio.mixer.drain",
                    "ASTRA_PLAYER_MIXER_DRAIN_RESULT",
                ));
            }
            Ok::<(), astra_platform::PlatformError>(())
        }
        .await;
        let close_result = async {
            let close = source
                .prepare_persistent_audio_close(output)
                .map_err(|error| player_platform_error("player.audio.mixer.close.prepare", error))?;
            let closed = executor
                .execute_batch(close)
                .await
                .map_err(|error| player_platform_error("player.audio.mixer.close", error))?;
            if !matches!(
                closed.as_slice(),
                [PlayerHostCommandResult::AudioClosed { output: closed_output }] if *closed_output == output
            ) {
                return Err(player_platform_error(
                    "player.audio.mixer.close",
                    "ASTRA_PLAYER_MIXER_CLOSE_RESULT",
                ));
            }
            Ok::<(), astra_platform::PlatformError>(())
        }
        .await;
        self.mixer = None;
        self.queue = None;
        self.voice_kinds.clear();
        match (drain_result, close_result) {
            (Err(drain), Err(close)) => Err(player_platform_error(
                "player.audio.mixer.shutdown",
                format!("{drain}; close failed: {close}"),
            )),
            (Err(drain), Ok(())) => Err(drain),
            (Ok(()), Err(close)) => Err(close),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

fn parse_audio_bool(
    request: &crate::NativeVnAudioRequest,
    key: &str,
    default: bool,
) -> Result<bool, astra_platform::PlatformError> {
    match request.attributes.get(key).map(String::as_str) {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(player_platform_error(
            "player.audio.mixer.command",
            format!(
                "ASTRA_PLAYER_AUDIO_BOOL_INVALID: {}.{key}",
                request.command_id
            ),
        )),
    }
}

fn parse_audio_f32(
    request: &crate::NativeVnAudioRequest,
    key: &str,
    default: f32,
) -> Result<f32, astra_platform::PlatformError> {
    match request.attributes.get(key) {
        None => Ok(default),
        Some(value) => value.parse::<f32>().map_err(|_| {
            player_platform_error(
                "player.audio.mixer.command",
                format!(
                    "ASTRA_PLAYER_AUDIO_NUMBER_INVALID: {}.{key}",
                    request.command_id
                ),
            )
        }),
    }
}

fn player_platform_error(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> astra_platform::PlatformError {
    astra_platform::PlatformError::new(
        astra_platform::PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}
