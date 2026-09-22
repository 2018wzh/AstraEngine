use crate::preview_transport::PreviewTransport;
use astra_platform::{PlatformError, PlatformErrorCode};
use astra_player_core::{PlatformCommandSink, PlayerHostCommandExecutor};
use astra_player_vn::{NativeVnHostCommandSource, NativeVnPreview, NativeVnProductMediaHost};
use astra_vn_core::{
    PreviewCommand, PreviewRejectCode as Reject, PreviewRequest, PreviewResponse, PREVIEW_PROTOCOL,
};

pub(crate) struct PreviewControl {
    transport: PreviewTransport,
    preview: NativeVnPreview,
}
impl PreviewControl {
    pub(crate) async fn attach(
        source: &mut NativeVnHostCommandSource,
        media: &NativeVnProductMediaHost,
    ) -> Result<Self, PlatformError> {
        let mut transport = PreviewTransport::open().map_err(|_| error(Reject::Disconnected))?;
        let request = tokio::time::timeout(std::time::Duration::from_secs(10), transport.recv())
            .await
            .map_err(|_| error(Reject::NotAttached))?
            .map_err(error)?;
        if request.protocol != PREVIEW_PROTOCOL
            || !matches!(request.command, PreviewCommand::Attach)
        {
            let _ = transport.send(&PreviewResponse::Failure {
                code: Reject::InvalidRequest,
            });
            return Err(error(Reject::InvalidRequest));
        }
        let mut preview =
            NativeVnPreview::attach(source, request.identity.clone()).map_err(|code| {
                let _ = transport.send(&PreviewResponse::Failure { code });
                error(code)
            })?;
        preview
            .accept(&request.identity, request.sequence)
            .map_err(error)?;
        preview.record(source, media).map_err(error)?;
        transport
            .send(&PreviewResponse::Ready {
                protocol: PREVIEW_PROTOCOL.into(),
                sequence: request.sequence,
                status: preview.status(),
            })
            .map_err(error)?;
        Ok(Self { transport, preview })
    }
    pub(crate) fn is_paused(&self) -> bool {
        self.preview.is_paused()
    }
    pub(crate) fn record(
        &mut self,
        source: &mut NativeVnHostCommandSource,
        media: &NativeVnProductMediaHost,
    ) -> Result<(), PlatformError> {
        if self.preview.record(source, media).map_err(error)? {
            self.transport
                .send(&PreviewResponse::State {
                    sequence: 0,
                    status: self.preview.status(),
                })
                .map_err(error)?;
        }
        Ok(())
    }
    pub(crate) async fn recv(&mut self) -> Result<PreviewRequest, PlatformError> {
        self.transport.recv().await.map_err(error)
    }
    /// False means the parent requested Stop. Invalid/stale commands cannot mutate Player.
    pub(crate) async fn request(
        &mut self,
        request: PreviewRequest,
        source: &mut NativeVnHostCommandSource,
        media: &mut NativeVnProductMediaHost,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<bool, PlatformError> {
        let accepted = if request.protocol != PREVIEW_PROTOCOL {
            Err(Reject::InvalidRequest)
        } else {
            self.preview.accept(&request.identity, request.sequence)
        };
        let result = match accepted {
            Err(code) => Err(code),
            Ok(()) => match request.command {
                PreviewCommand::Attach => Err(Reject::InvalidRequest),
                PreviewCommand::Pause => self.preview.set_paused(true, media, executor).await,
                PreviewCommand::Resume => self.preview.set_paused(false, media, executor).await,
                PreviewCommand::SeekWithinFragment {
                    source_id,
                    checkpoint,
                } => {
                    match self
                        .preview
                        .seek(&source_id, checkpoint, source, media, executor)
                        .await
                    {
                        Ok(batch) => {
                            executor
                                .execute_batch(batch)
                                .await
                                .map_err(|_| error(Reject::RestoreFailed))?;
                            Ok(())
                        }
                        Err(code) => Err(code),
                    }
                }
                PreviewCommand::Stop => return Ok(false),
            },
        };
        if result.is_ok() {
            self.preview.record(source, media).map_err(error)?;
        }
        if self.preview.is_cancelled() {
            let _ = self.transport.send(&PreviewResponse::Failure {
                code: Reject::RestoreFailed,
            });
            return Err(error(Reject::RestoreFailed));
        }
        match result {
            Ok(()) => self.transport.send(&PreviewResponse::State {
                sequence: request.sequence,
                status: self.preview.status(),
            }),
            Err(code) => self.transport.send(&PreviewResponse::Rejected {
                sequence: request.sequence,
                code,
            }),
        }
        .map_err(error)?;
        Ok(true)
    }
    pub(crate) fn finished(&self, success: bool) {
        let response = if success {
            PreviewResponse::Stopped
        } else {
            PreviewResponse::Failure {
                code: Reject::NotRecoverable,
            }
        };
        let _ = self.transport.send(&response);
    }
}
fn error(code: Reject) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "player.preview",
        code.to_string(),
    )
}
