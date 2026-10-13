use super::*;

impl PlatformHostClient {
    pub fn command_wake_registration(&self) -> Option<PlatformCommandWakeRegistration> {
        self.command_wake.clone()
    }

    pub fn audio_wake(&self) -> AudioWakeRegistration {
        self.audio_wake.clone()
    }

    pub async fn create_window(
        &self,
        request: WindowRequest,
    ) -> Result<WindowHandle, PlatformError> {
        if request.width == 0 || request.height == 0 || request.title.trim().is_empty() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "window.create",
                "window title and dimensions must be valid",
            ));
        }
        self.ensure_running("window.create")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::CreateWindow { request, reply })?;
        response.await.map_err(|_| queue_closed("window.create"))?
    }

    pub async fn create_surface(
        &self,
        request: SurfaceRequest,
    ) -> Result<SurfaceHandle, PlatformError> {
        if request.width == 0 || request.height == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "surface.create",
                "surface dimensions must be non-zero",
            ));
        }
        self.ensure_running("surface.create")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::CreateSurface { request, reply })?;
        response.await.map_err(|_| queue_closed("surface.create"))?
    }

    pub async fn capture_surface(
        &self,
        surface: SurfaceHandle,
    ) -> Result<CapturedFrame, PlatformError> {
        self.ensure_running("surface.capture")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::CaptureSurface { surface, reply })?;
        let frame = response
            .await
            .map_err(|_| queue_closed("surface.capture"))??;
        validate_rgba_frame(
            frame.width,
            frame.height,
            &frame.rgba8,
            self.profile.limits().max_frame_bytes,
        )?;
        Ok(frame)
    }

    pub async fn present_rgba(
        &self,
        surface: SurfaceHandle,
        frame: RgbaFrame,
    ) -> Result<(), PlatformError> {
        if frame.sequence == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "surface.present_rgba",
                "presented frame sequence must be non-zero",
            ));
        }
        validate_rgba_frame(
            frame.width,
            frame.height,
            &frame.rgba8,
            self.profile.limits().max_frame_bytes,
        )?;
        self.ensure_running("surface.present_rgba")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::PresentRgba {
            surface,
            frame,
            reply,
        })?;
        response
            .await
            .map_err(|_| queue_closed("surface.present_rgba"))?
    }

    pub async fn present_scene(
        &self,
        surface: SurfaceHandle,
        frame: SceneFrame,
    ) -> Result<(), PlatformError> {
        self.submit_scene(surface, frame)?.complete().await
    }

    pub fn submit_scene(
        &self,
        surface: SurfaceHandle,
        frame: SceneFrame,
    ) -> Result<ScenePresentReceipt, PlatformError> {
        validate_scene_frame(&frame, self.profile.limits().max_frame_bytes)?;
        self.ensure_running("surface.present_scene")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::PresentScene {
            surface,
            frame,
            reply,
        })?;
        Ok(ScenePresentReceipt { response })
    }

    #[cfg(feature = "platform-test-driver")]
    pub async fn inject_surface_device_loss(
        &self,
        surface: SurfaceHandle,
    ) -> Result<(), PlatformError> {
        self.ensure_running("surface.test.inject_device_loss")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::InjectSurfaceDeviceLoss { surface, reply })?;
        response
            .await
            .map_err(|_| queue_closed("surface.test.inject_device_loss"))?
    }

    pub async fn destroy_surface(&self, surface: SurfaceHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::DestroySurface { surface, reply },
            response,
            "surface.destroy",
        )
        .await
    }

    pub async fn destroy_window(&self, window: WindowHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::DestroyWindow { window, reply },
            response,
            "window.destroy",
        )
        .await
    }

    pub async fn open_audio_output(
        &self,
        request: AudioOutputRequest,
    ) -> Result<OpenedAudioOutput, PlatformError> {
        if request.sample_rate == 0
            || request.channels == 0
            || request.chunk_frames == 0
            || request.chunk_frames > request.max_buffered_frames
            || request.max_buffered_frames == 0
            || request.max_buffered_frames > self.profile.limits().max_audio_frames
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.open",
                "audio output descriptor is invalid or exceeds profile limits",
            ));
        }
        self.ensure_running("audio.open")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::OpenAudioOutput { request, reply })?;
        response.await.map_err(|_| queue_closed("audio.open"))?
    }

    pub async fn pause_audio(&self, output: AudioOutputHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::PauseAudio { output, reply },
            response,
            "audio.pause",
        )
        .await
    }

    pub async fn resume_audio(&self, output: AudioOutputHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::ResumeAudio { output, reply },
            response,
            "audio.resume",
        )
        .await
    }

    pub async fn abort_audio(&self, output: AudioOutputHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::AbortAudio { output, reply },
            response,
            "audio.abort",
        )
        .await
    }

    #[cfg(feature = "platform-test-driver")]
    pub async fn inject_audio_device_loss(
        &self,
        output: AudioOutputHandle,
    ) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::InjectAudioDeviceLoss { output, reply },
            response,
            "audio.test.inject_device_loss",
        )
        .await
    }

    pub async fn close_audio(&self, output: AudioOutputHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::CloseAudio { output, reply },
            response,
            "audio.close",
        )
        .await
    }

    pub fn launch_profile(&self) -> &HostLaunchProfile {
        &self.profile
    }

    pub fn platform_profile(&self) -> Result<&PlatformHostProfile, PlatformError> {
        self.profile.require_platform()
    }

    pub fn headless_profile(&self) -> Result<&HeadlessHostProfile, PlatformError> {
        self.profile.require_headless()
    }

    pub async fn open_decode(
        &self,
        kind: DecodeKind,
    ) -> Result<DecodeSessionHandle, PlatformError> {
        self.ensure_running("decode.open")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::OpenDecode { kind, reply })?;
        response.await.map_err(|_| queue_closed("decode.open"))?
    }

    pub async fn decode(
        &self,
        session: DecodeSessionHandle,
        request: PlatformDecodeRequest,
    ) -> Result<DecodeOutput, PlatformError> {
        let payload_invalid = match request.stream_action {
            DecodeStreamAction::OneShot | DecodeStreamAction::Start => {
                request.codec.is_empty()
                    || request.bytes.is_empty()
                    || request.bytes.len() > self.profile.limits().max_frame_bytes
            }
            DecodeStreamAction::Next => {
                !matches!(request.kind, DecodeKind::Video | DecodeKind::Audio)
                    || !request.bytes.is_empty()
            }
        };
        if request.sequence == 0 || payload_invalid {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request is invalid or exceeds profile limits",
            ));
        }
        self.ensure_running("decode.submit")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::Decode {
            session,
            request,
            reply,
        })?;
        response.await.map_err(|_| queue_closed("decode.submit"))?
    }

    pub async fn close_decode(&self, session: DecodeSessionHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::CloseDecode { session, reply },
            response,
            "decode.close",
        )
        .await
    }

    pub async fn begin_save(
        &self,
        slot: impl Into<String>,
    ) -> Result<SaveTransactionHandle, PlatformError> {
        let slot = slot.into();
        if !is_safe_slot(&slot) {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "save.begin",
                "save slot must be a safe relative symbol",
            ));
        }
        self.ensure_running("save.begin")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::BeginSave { slot, reply })?;
        response.await.map_err(|_| queue_closed("save.begin"))?
    }

    pub async fn write_save(
        &self,
        transaction: SaveTransactionHandle,
        bytes: Vec<u8>,
    ) -> Result<(), PlatformError> {
        if bytes.is_empty() || bytes.len() > self.profile.limits().max_frame_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "save.write",
                "save payload is empty or exceeds profile limits",
            ));
        }
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::WriteSave {
                transaction,
                bytes,
                reply,
            },
            response,
            "save.write",
        )
        .await
    }

    pub async fn commit_save(
        &self,
        transaction: SaveTransactionHandle,
    ) -> Result<String, PlatformError> {
        self.ensure_running("save.commit")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::CommitSave { transaction, reply })?;
        let hash = response.await.map_err(|_| queue_closed("save.commit"))??;
        if !hash.starts_with("sha256:") {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "save.commit",
                "save commit did not return a sha256 identity",
            ));
        }
        Ok(hash)
    }

    pub async fn abort_save(
        &self,
        transaction: SaveTransactionHandle,
    ) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::AbortSave { transaction, reply },
            response,
            "save.abort",
        )
        .await
    }

    pub async fn read_save(&self, slot: impl Into<String>) -> Result<Vec<u8>, PlatformError> {
        let slot = slot.into();
        if !is_safe_slot(&slot) {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "save.read",
                "save slot must be a safe relative symbol",
            ));
        }
        self.ensure_running("save.read")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::ReadSave { slot, reply })?;
        let bytes = response.await.map_err(|_| queue_closed("save.read"))??;
        if bytes.is_empty() || bytes.len() > self.profile.limits().max_frame_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "save.read",
                "save backend returned invalid payload length",
            ));
        }
        Ok(bytes)
    }

    pub async fn delete_save(&self, slot: impl Into<String>) -> Result<(), PlatformError> {
        let slot = slot.into();
        if !is_safe_slot(&slot) {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "save.delete",
                "save slot must be a safe relative symbol",
            ));
        }
        self.ensure_running("save.delete")?;
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::DeleteSave { slot, reply },
            response,
            "save.delete",
        )
        .await
    }

    pub async fn list_saves(&self) -> Result<Vec<String>, PlatformError> {
        self.ensure_running("save.list")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::ListSaves { reply })?;
        let slots = response.await.map_err(|_| queue_closed("save.list"))??;
        if slots.len() > 256
            || slots.windows(2).any(|pair| pair[0] >= pair[1])
            || slots.iter().any(|slot| !is_safe_slot(slot))
        {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "save.list",
                "save backend returned an invalid, duplicate, unsorted, or oversized slot catalog",
            ));
        }
        Ok(slots)
    }

    pub async fn open_package(
        &self,
        source: PackageSourceRequest,
    ) -> Result<PackageSourceHandle, PlatformError> {
        validate_package_source(&self.profile, &source)?;
        self.ensure_running("package.open")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::OpenPackage { source, reply })?;
        response.await.map_err(|_| queue_closed("package.open"))?
    }

    pub async fn read_package_range(
        &self,
        source: PackageSourceHandle,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, PlatformError> {
        if length == 0 || length > self.profile.limits().max_package_read_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.read_range",
                "package range length is invalid or exceeds profile limits",
            ));
        }
        self.ensure_running("package.read_range")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::ReadPackageRange {
            source,
            offset,
            length,
            reply,
        })?;
        let bytes = response
            .await
            .map_err(|_| queue_closed("package.read_range"))??;
        if bytes.len() > length {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.read_range",
                "package backend returned more bytes than requested",
            ));
        }
        Ok(bytes)
    }

    pub async fn close_package(&self, source: PackageSourceHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::ClosePackage { source, reply },
            response,
            "package.close",
        )
        .await
    }
}
