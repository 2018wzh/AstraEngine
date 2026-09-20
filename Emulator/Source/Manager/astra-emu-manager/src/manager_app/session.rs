use super::*;

pub(super) struct ActiveFamilySession {
    pub(super) family_id: String,
    session: Option<Box<dyn FamilySession>>,
    pub(super) audio: Option<audio_executor::HostAudioExecutor>,
    text: Option<TextReplacementBridge>,
    mailbox: FrameMailbox,
    pub(super) status: FamilyStatus,
    pub(super) fullscreen: bool,
    pub(super) last_tick: Instant,
    pub(super) next_deadline: Instant,
}

impl ActiveFamilySession {
    pub(super) fn open(
        game_config: (&GameRecord, Vec<astra_emu_family_api::ConfigEntry>),
        candidate: &FamilyProbeCandidate,
        registry: &mut FamilyProviderRegistry,
        mailbox: FrameMailbox,
        initial_window: WindowState,
        text: Option<TextReplacementBridge>,
        audio: Option<audio_executor::HostAudioExecutor>,
    ) -> Result<Self, String> {
        let (game, configuration) = game_config;
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
                    configuration: configuration.into(),
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
            fullscreen: false,
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
        let response = session
            .advance(elapsed_ns, events)
            .map_err(|error| error.to_string())?;
        self.status = response.status;
        if let ROption::RSome(astra_emu_family_api::FamilyWindowCommand::SetFullscreen(value)) = response.window_command {
            self.fullscreen = value;
        }
        self.capture_frame()?;
        self.last_tick = if response.reset_clock {
            tracing::debug!(event = "astra.emu.host.clock_reset", family_id = %self.family_id);
            Instant::now()
        } else {
            started
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::*;

    struct WindowSession(std::collections::VecDeque<Option<bool>>);
    impl FamilySession for WindowSession {
        fn advance(&mut self, _: u64, _: &[FamilyEvent]) -> FamilyResult<AdvanceResponse> {
            Ok(AdvanceResponse {
                window_command: self.0.pop_front().flatten().map(FamilyWindowCommand::SetFullscreen).into(),
                ..AdvanceResponse::running()
            })
        }
        fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
            visitor.accept(FrameView::from_slice(&[0, 0, 0, 255], FrameInfo {
                width: 1, height: 1, stride: 4,
                logical_width: 1, logical_height: 1,
                format: FrameFormat::Rgba8Srgb { alpha: FrameAlpha::Opaque },
            })?)
        }
        fn close(self: Box<Self>) -> FamilyResult<()> { Ok(()) }
    }

    #[test]
    fn restored_clock_starts_after_frame_capture_only_when_requested() {
        struct ClockSession {
            reset: bool,
            captured: std::sync::Arc<std::sync::Mutex<Option<Instant>>>,
        }
        impl FamilySession for ClockSession {
            fn advance(&mut self, _: u64, _: &[FamilyEvent]) -> FamilyResult<AdvanceResponse> {
                Ok(AdvanceResponse { reset_clock: self.reset, ..AdvanceResponse::running() })
            }
            fn visit_frame(&self, _: &mut dyn FrameVisitor) -> FamilyResult<()> {
                *self.captured.lock().unwrap() = Some(Instant::now());
                Ok(())
            }
            fn close(self: Box<Self>) -> FamilyResult<()> { Ok(()) }
        }
        for reset in [false, true] {
            let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
            let mut active = ActiveFamilySession {
                family_id: "clock-test".into(),
                session: Some(Box::new(ClockSession { reset, captured: captured.clone() })),
                audio: None, text: None, mailbox: FrameMailbox::new(),
                status: FamilyStatus::Running, fullscreen: false,
                last_tick: Instant::now(), next_deadline: Instant::now(),
            };
            active.advance(1, &[]).unwrap();
            let completion = captured.lock().unwrap().unwrap();
            if reset {
                assert!(active.last_tick >= completion);
            } else {
                assert!(active.last_tick <= completion);
            }
            assert_eq!(active.next_deadline, active.last_tick + Duration::from_nanos(FIXED_FRAME_NS));
            active.close().unwrap();
        }
    }

    #[test]
    fn window_request_persists_until_explicitly_replaced() {
        let mut active = ActiveFamilySession {
            family_id: "test".into(),
            session: Some(Box::new(WindowSession([Some(true), None, Some(false)].into()))),
            audio: None, text: None, mailbox: FrameMailbox::new(),
            status: FamilyStatus::Running, fullscreen: false,
            last_tick: Instant::now(), next_deadline: Instant::now(),
        };
        active.advance(1, &[]).unwrap();
        assert!(active.fullscreen);
        active.advance(1, &[]).unwrap();
        assert!(active.fullscreen);
        active.advance(1, &[]).unwrap();
        assert!(!active.fullscreen);
        active.close().unwrap();
    }
}
