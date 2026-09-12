use super::*;

type OpenFuture = Pin<Box<dyn Future<Output = Result<DecodeSessionHandle, PlatformError>> + Send>>;
type CloseFuture = Pin<Box<dyn Future<Output = Result<(), PlatformError>> + Send>>;

pub(super) enum PendingDecodeOpen {
    Opening(OpenFuture),
    Closing {
        handle: DecodeSessionHandle,
        response: CloseFuture,
    },
}

impl PendingDecodeOpen {
    fn closing(client: PlatformHostClient, handle: DecodeSessionHandle) -> Self {
        Self::Closing {
            handle,
            response: Box::pin(async move { client.close_decode(handle).await }),
        }
    }
}

impl PlatformCommandSink {
    const MAX_PENDING_DECODE_OPENS: usize = 64;

    pub(super) async fn open_owned_decoder(
        &mut self,
        logical: PlayerHostResourceId,
        kind: DecodeKind,
    ) -> Result<(), PlatformError> {
        if self.decoders.contains_key(&logical) || self.pending_decode_opens.contains_key(&logical)
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.open",
                "logical decoder already exists or is awaiting cleanup",
            ));
        }
        if self.pending_decode_opens.len() >= Self::MAX_PENDING_DECODE_OPENS {
            return Err(PlatformError::new(
                PlatformErrorCode::QueueOverflow,
                "decode.open",
                "pending decoder open budget exceeded; finish cleanup before opening more",
            ));
        }
        let client = self.client.clone();
        self.pending_decode_opens.insert(
            logical,
            PendingDecodeOpen::Opening(Box::pin(async move { client.open_decode(kind).await })),
        );
        let Some(PendingDecodeOpen::Opening(response)) =
            self.pending_decode_opens.get_mut(&logical)
        else {
            unreachable!("inserted an opening decoder")
        };
        let result = response.await;
        self.pending_decode_opens.remove(&logical);
        let handle = result?;
        self.decoders.insert(logical, handle);
        Ok(())
    }

    /// Finish abandoned decoder opens and close their resources before dropping the sink.
    /// Dropping this cleanup future preserves the in-flight response for the next call.
    /// Errors remain visible; a failed close is retained for an explicit retry.
    pub async fn cleanup_pending_decode_opens(&mut self) -> Result<(), PlatformError> {
        while let Some(logical) = self.pending_decode_opens.keys().next().copied() {
            let pending = self.pending_decode_opens.get_mut(&logical).unwrap();
            match pending {
                PendingDecodeOpen::Opening(response) => match response.await {
                    Ok(handle) => {
                        *pending = PendingDecodeOpen::closing(self.client.clone(), handle);
                    }
                    Err(error) => {
                        self.pending_decode_opens.remove(&logical);
                        return Err(error);
                    }
                },
                PendingDecodeOpen::Closing { handle, response } => match response.await {
                    Ok(()) => {
                        self.pending_decode_opens.remove(&logical);
                    }
                    Err(error) => {
                        *pending = PendingDecodeOpen::closing(self.client.clone(), *handle);
                        return Err(error);
                    }
                },
            }
        }
        Ok(())
    }
}
