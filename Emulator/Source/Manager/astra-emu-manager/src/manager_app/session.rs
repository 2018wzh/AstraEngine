use super::*;

pub(super) struct ActiveFamilySession {
    pub(super) family_id: String,
    session: Option<Box<dyn FamilySession>>,
    pub(super) audio: Option<audio_executor::HostAudioExecutor>,
    text: Option<TextReplacementBridge>,
    mailbox: FrameMailbox,
    pub(super) status: FamilyStatus,
    pub(super) last_tick: Instant,
    pub(super) next_deadline: Instant,
}

impl ActiveFamilySession {
    pub(super) fn open(
        game: &GameRecord,
        candidate: &FamilyProbeCandidate,
        registry: &mut FamilyProviderRegistry,
        mailbox: FrameMailbox,
        initial_window: WindowState,
        text: Option<TextReplacementBridge>,
        audio: Option<audio_executor::HostAudioExecutor>,
    ) -> Result<Self, String> {
        if candidate.descriptor.family_id != candidate.report.family_id
            || game.family_id.as_deref() != Some(candidate.report.family_id.as_str())
        {
            return Err("ASTRA_EMU_FAMILY_BINDING_MISMATCH".into());
        }
        let host = FamilyHostServices {
            audio_sink: audio
                .as_ref()
                .map(|value| ROption::RSome(value.sink()))
                .unwrap_or(ROption::RNone),
            text_replacement: text
                .as_ref()
                .map(|bridge| ROption::RSome(bridge.sink()))
                .unwrap_or(ROption::RNone),
        };
        let opened = registry
            .open_selected(
                candidate,
                OpenRequest {
                    game_path: game.location.clone().into(),
                    initial_window,
                    host,
                },
            )
            .map_err(|error| error.to_string())?;
        let mut active = Self {
            family_id: candidate.descriptor.family_id.clone(),
            session: Some(opened.session),
            audio,
            text,
            mailbox,
            status: FamilyStatus::Running,
            last_tick: Instant::now(),
            next_deadline: Instant::now() + Duration::from_nanos(FIXED_FRAME_NS),
        };
        if let Err(error) = active.capture_frame() {
            return match active.close() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}; {cleanup}")),
            };
        }
        Ok(active)
    }

    pub(super) fn capture_frame(&self) -> Result<(), String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_FAMILY_SESSION_CLOSED".to_owned())?;
        let mut collector = FrameCollector::new(self.mailbox.clone());
        session
            .visit_frame(&mut collector)
            .map_err(|error| error.to_string())
    }

    pub(super) fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> Result<(), String> {
        let started = Instant::now();
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_FAMILY_SESSION_CLOSED".to_owned())?;
        self.status = session
            .advance(elapsed_ns, events)
            .map_err(|error| error.to_string())?
            .status;
        self.capture_frame()?;
        self.last_tick = started;
        self.next_deadline = self.last_tick + Duration::from_nanos(FIXED_FRAME_NS);
        Ok(())
    }

    pub(super) fn close(&mut self) -> Result<(), String> {
        let text_error = self
            .text
            .take()
            .and_then(|text| text.close().err())
            .map(|error| error.to_string());
        let audio_error = self.audio.take().and_then(|audio| audio.close().err());
        let session_error = self
            .session
            .take()
            .and_then(|session| session.close().err())
            .map(|error| error.to_string());
        let errors: Vec<_> = [
            text_error,
            audio_error,
            session_error,
            self.mailbox.clear().err(),
        ]
        .into_iter()
        .flatten()
        .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

impl Drop for ActiveFamilySession {
    fn drop(&mut self) {
        if self.session.is_some() || self.audio.is_some() {
            if let Err(error) = self.close() {
                tracing::error!(
                    event = "astra.emu.family.session_drop_cleanup_failed",
                    family_id = %self.family_id,
                    diagnostic_code = %error
                );
            }
        }
    }
}
