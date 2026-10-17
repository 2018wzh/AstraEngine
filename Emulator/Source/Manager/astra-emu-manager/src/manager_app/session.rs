use super::*;

pub(super) struct SessionServices {
    pub(super) text: Option<TextReplacementBridge>,
    pub(super) audio: Option<audio_executor::HostAudioExecutor>,
    pub(super) modal: Option<ModalBridge>,
}

pub(super) struct ActiveFamilySession {
    pub(super) family_id: String,
    session_epoch: u64,
    session: Option<Box<dyn FamilySession>>,
    pub(super) audio: Option<audio_executor::HostAudioExecutor>,
    text: Option<TextReplacementBridge>,
    modal: Option<ModalBridge>,
    mailbox: FrameMailbox,
    pub(super) status: FamilyStatus,
    pub(super) fullscreen: bool,
    pub(super) title: Option<String>,
    pub(super) pending_icon: Option<astra_emu_family_api::FamilyWindowIcon>,
    pub(super) pending_pointer: Option<astra_emu_family_api::FamilyPointerRequest>,
    pub(super) last_tick: Instant,
    pub(super) next_deadline: Instant,
    pub(super) frame_poll_deadline: Option<Instant>,
    pub(super) clock_reset_pending: bool,
}

impl ActiveFamilySession {
    pub(super) fn open(
        game_config: (&GameRecord, Vec<astra_emu_family_api::ConfigEntry>, u64),
        candidate: &FamilyProbeCandidate,
        registry: &mut FamilyProviderRegistry,
        mailbox: FrameMailbox,
        initial_window: WindowState,
        services: SessionServices,
    ) -> Result<Self, String> {
        let (game, configuration, session_epoch) = game_config;
        let SessionServices { text, audio, modal } = services;
        if candidate.descriptor.family_id != candidate.report.family_id
            || game.family_id.as_deref() != Some(candidate.report.family_id.as_str())
        {
            return Err("ASTRA_EMU_FAMILY_BINDING_MISMATCH".into());
        }
        mailbox
            .activate(session_epoch)
            .map_err(|error| error.to_string())?;
        let host = FamilyHostServices {
            audio_sink: audio
                .as_ref()
                .map(|value| ROption::RSome(value.sink()))
                .unwrap_or(ROption::RNone),
            text_replacement: text
                .as_ref()
                .map(|bridge| ROption::RSome(bridge.sink()))
                .unwrap_or(ROption::RNone),
            modal: modal
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
                    session_epoch,
                    initial_window,
                    host,
                },
            )
            .map_err(|error| {
                if let Some(modal) = modal.as_ref() {
                    modal.cancel_pending();
                }
                error.to_string()
            })?;
        let mut active = Self {
            family_id: candidate.descriptor.family_id.clone(),
            session_epoch,
            session: Some(opened.session),
            audio,
            text,
            modal,
            mailbox,
            status: FamilyStatus::Running,
            fullscreen: false,
            title: None,
            pending_icon: None,
            pending_pointer: None,
            last_tick: Instant::now(),
            next_deadline: Instant::now() + Duration::from_nanos(FIXED_FRAME_NS),
            frame_poll_deadline: None,
            clock_reset_pending: false,
        };
        if let Err(error) = active.capture_initial_frame() {
            return match active.close() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}; {cleanup}")),
            };
        }
        Ok(active)
    }

    pub(super) fn capture_frame(&mut self) -> Result<(), String> {
        let after = self.mailbox.identity()?;
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_FAMILY_SESSION_CLOSED".to_owned())?;
        let frame = session
            .acquire_frame(after)
            .map_err(|error| error.to_string())?;
        frame
            .validate(self.session_epoch)
            .map_err(|error| error.to_string())?;
        self.frame_poll_deadline = None;
        match frame {
            astra_emu_family_api::FramePoll::Ready(frame) => self
                .mailbox
                .publish(frame)
                .map_err(|error| error.to_string()),
            astra_emu_family_api::FramePoll::Unchanged(identity) if after == Some(identity) => {
                Ok(())
            }
            astra_emu_family_api::FramePoll::Pending(_) => {
                self.frame_poll_deadline = Some(Instant::now() + Duration::from_millis(1));
                Ok(())
            }
            astra_emu_family_api::FramePoll::Unchanged(_) => {
                Err("ASTRA_EMU_FAMILY_FRAME_BASELINE".into())
            }
        }
    }

    fn capture_initial_frame(&mut self) -> Result<(), String> {
        self.clock_reset_pending = true;
        self.capture_frame()?;
        self.settle_clock("initial_frame");
        Ok(())
    }

    pub(super) fn wake_deadline(&self) -> Instant {
        match self.frame_poll_deadline {
            Some(poll) if self.clock_reset_pending => poll,
            Some(poll) => poll.min(self.next_deadline),
            None => self.next_deadline,
        }
    }

    pub(super) fn poll_pending_frame(&mut self) -> Result<bool, String> {
        self.capture_frame()?;
        self.settle_clock("family_frame");
        Ok(self.frame_poll_deadline.is_none())
    }

    fn settle_clock(&mut self, reason: &'static str) {
        if self.clock_reset_pending && self.frame_poll_deadline.is_none() {
            self.clock_reset_pending = false;
            self.reset_clock_after_host_work(reason);
        }
    }

    pub(super) fn epoch(&self) -> u64 {
        self.session_epoch
    }

    fn reset_clock_after_host_work(&mut self, reason: &'static str) {
        let now = Instant::now();
        self.last_tick = now;
        self.next_deadline = now + Duration::from_nanos(FIXED_FRAME_NS);
        tracing::debug!(
            event = "astra.emu.host.clock_anchor",
            family_id = %self.family_id,
            reason
        );
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
        let response = match session.advance(elapsed_ns, events) {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(
                    event = "astra.emu.host.advance_failed",
                    family_id = %self.family_id,
                    diagnostic_code = %error.code(),
                    elapsed_ns
                );
                return Err(error.to_string());
            }
        };
        let core_finished = tracing::enabled!(tracing::Level::DEBUG).then(Instant::now);
        response.validate().map_err(|error| error.to_string())?;
        self.status = response.status;
        if let ROption::RSome(command) = response.window_command {
            match command {
                astra_emu_family_api::FamilyWindowCommand::SetFullscreen(value) => {
                    self.fullscreen = value
                }
                astra_emu_family_api::FamilyWindowCommand::SetIcon(icon) => {
                    self.pending_icon = Some(icon)
                }
                astra_emu_family_api::FamilyWindowCommand::SetPointer(request) => {
                    self.pending_pointer
                        .get_or_insert_with(Default::default)
                        .merge(request);
                }
                astra_emu_family_api::FamilyWindowCommand::SetTitle(title) => {
                    self.title = Some(title.into_string())
                }
            }
        }
        let copy_started = core_finished.map(|_| Instant::now());
        self.clock_reset_pending |= response.reset_clock;
        self.capture_frame()?;
        if let (Some(core_finished), Some(copy_started)) = (core_finished, copy_started) {
            tracing::debug!(
                event = "astra.emu.host.frame_timing",
                family_id = %self.family_id,
                core_advance_us = core_finished.duration_since(started).as_micros() as u64,
                frame_capture_us = copy_started.elapsed().as_micros() as u64
            );
        }
        self.last_tick = started;
        self.next_deadline = started + Duration::from_nanos(FIXED_FRAME_NS);
        self.settle_clock("family_frame");
        Ok(())
    }

    pub(super) fn advance_host_events(&mut self, events: &[FamilyEvent]) -> Result<(), String> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_FAMILY_SESSION_CLOSED".to_owned())?;
        let response = match session.advance(0, events) {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(
                    event = "astra.emu.host.advance_failed",
                    family_id = %self.family_id,
                    diagnostic_code = %error.code(),
                    elapsed_ns = 0
                );
                return Err(error.to_string());
            }
        };
        response.validate().map_err(|error| error.to_string())?;
        self.status = response.status;
        if let ROption::RSome(command) = response.window_command {
            match command {
                astra_emu_family_api::FamilyWindowCommand::SetFullscreen(value) => {
                    self.fullscreen = value
                }
                astra_emu_family_api::FamilyWindowCommand::SetIcon(icon) => {
                    self.pending_icon = Some(icon)
                }
                astra_emu_family_api::FamilyWindowCommand::SetPointer(request) => {
                    self.pending_pointer
                        .get_or_insert_with(Default::default)
                        .merge(request);
                }
                astra_emu_family_api::FamilyWindowCommand::SetTitle(title) => {
                    self.title = Some(title.into_string())
                }
            }
        }
        self.clock_reset_pending |= response.reset_clock;
        self.capture_frame()?;
        self.next_deadline = self.last_tick + Duration::from_nanos(FIXED_FRAME_NS);
        self.settle_clock("family_event");
        Ok(())
    }

    pub(super) fn close(&mut self) -> Result<(), String> {
        if let Some(modal) = self.modal.take() {
            modal.cancel_pending();
        }
        // Let the family cancel and join its workers while their host sinks
        // are still alive. Releasing the bounded PCM queue first races a
        // family worker into ASTRA_EMU_AUDIO_CLOSED before it can observe its
        // own stop flag.
        let session_error = self
            .session
            .take()
            .and_then(|session| session.close().err())
            .map(|error| error.to_string());
        let text_error = self
            .text
            .take()
            .and_then(|text| text.close().err())
            .map(|error| error.to_string());
        let audio_error = self.audio.take().and_then(|audio| audio.close().err());
        let errors: Vec<_> = [
            text_error,
            audio_error,
            session_error,
            self.mailbox.clear(self.session_epoch).err(),
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

    mod clock;
    mod window;

    struct AudioSession {
        sink: AudioSinkBox,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<Result<(), String>>>,
    }

    impl FamilySession for AudioSession {
        fn advance(&mut self, _: u64, _: &[FamilyEvent]) -> FamilyResult<AdvanceResponse> {
            Ok(AdvanceResponse::running())
        }

        fn acquire_frame(&mut self, _: Option<FrameIdentity>) -> FamilyResult<FramePoll> {
            Ok(FramePoll::Pending(FrameIdentity {
                session_epoch: 1,
                generation: 0,
            }))
        }

        fn close(mut self: Box<Self>) -> FamilyResult<()> {
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            self.sink.cancel().into_result()?;
            self.worker
                .take()
                .expect("audio worker is present")
                .join()
                .map_err(|_| FamilyError::new("ASTRA_TEST_AUDIO_WORKER", "worker panicked"))?
                .map_err(|error| FamilyError::new("ASTRA_TEST_AUDIO_WORKER", error))
        }
    }

    fn active_with_audio() -> ActiveFamilySession {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;

        let audio =
            audio_executor::HostAudioExecutor::new(audio_executor::AudioDeviceKind::Null, None);
        let sink = audio.sink();
        sink.configure(PcmFormatSpec {
            sample_rate: 48_000,
            channels: 2,
            format: PcmFormat::F32,
        })
        .into_result()
        .unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let started = std::sync::Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_started = started.clone();
        let worker_sink = audio.sink();
        let worker = std::thread::spawn(move || {
            worker_started.store(true, Ordering::Release);
            loop {
                let status = worker_sink
                    .write(PcmChunk::F32(abi_stable::std_types::RVec::from(vec![
                        0.0;
                        960
                    ])))
                    .into_result()
                    .map_err(|error| error.to_string())?;
                match status {
                    AudioWriteStatus::Accepted if !worker_stop.load(Ordering::Acquire) => {}
                    AudioWriteStatus::Cancelled | AudioWriteStatus::Closed
                        if worker_stop.load(Ordering::Acquire) =>
                    {
                        return Ok(());
                    }
                    status => return Err(format!("unexpected audio status: {status:?}")),
                }
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !started.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < deadline,
                "audio worker did not start"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        ActiveFamilySession {
            session_epoch: 1,
            family_id: "audio-lifecycle-test".into(),
            session: Some(Box::new(AudioSession {
                sink,
                stop,
                worker: Some(worker),
            })),
            audio: Some(audio),
            text: None,
            modal: None,
            mailbox: FrameMailbox::new(),
            status: FamilyStatus::Running,
            fullscreen: false,
            title: None,
            pending_icon: None,
            pending_pointer: None,
            last_tick: Instant::now(),
            next_deadline: Instant::now(),
            frame_poll_deadline: None,
            clock_reset_pending: false,
        }
    }

    #[test]
    fn close_joins_family_pcm_worker_before_releasing_null_audio_and_reopens() {
        for _ in 0..2 {
            let mut active = active_with_audio();
            active.close().unwrap();
        }
    }
}
