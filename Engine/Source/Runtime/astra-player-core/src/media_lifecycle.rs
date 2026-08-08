use crate::{
    PlayerHostCommandBatch, PlayerHostCommandExecutor, PlayerHostCommandResult,
    PlayerHostCommandSink, PlayerHostResourceId,
};

pub struct PlayerDecodeLifecyclePlan {
    pub session: PlayerHostResourceId,
    pub open: PlayerHostCommandBatch,
    pub decode: PlayerHostCommandBatch,
    pub close: PlayerHostCommandBatch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerDecodedBuffer {
    pub output: astra_platform::DecodeOutput,
}

impl<S> PlayerHostCommandExecutor<S>
where
    S: PlayerHostCommandSink,
    S::Error: std::fmt::Display,
{
    pub async fn execute_decode_open(
        &mut self,
        session: PlayerHostResourceId,
        open: PlayerHostCommandBatch,
    ) -> Result<(), PlayerMediaLifecycleError> {
        let results = self.execute_batch(open).await.map_err(|error| {
            PlayerMediaLifecycleError::new("ASTRA_PLAYER_DECODE_OPEN", error.to_string())
        })?;
        if !matches!(
            results.as_slice(),
            [PlayerHostCommandResult::DecodeOpened { session: opened }] if *opened == session
        ) {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_OPEN_RESULT",
                "decode open returned an invalid logical resource",
            ));
        }
        Ok(())
    }

    pub async fn execute_decode_submit(
        &mut self,
        session: PlayerHostResourceId,
        decode: PlayerHostCommandBatch,
    ) -> Result<PlayerDecodedBuffer, PlayerMediaLifecycleError> {
        let results = self.execute_batch(decode).await.map_err(|error| {
            PlayerMediaLifecycleError::new("ASTRA_PLAYER_DECODE_SUBMIT", error.to_string())
        })?;
        let Some(PlayerHostCommandResult::Decoded {
            session: decoded_session,
            output,
        }) = results.into_iter().next()
        else {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_RESULT",
                "decode submit returned an invalid result",
            ));
        };
        if decoded_session != session {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_RESULT",
                "decode submit returned the wrong logical resource",
            ));
        }
        Ok(PlayerDecodedBuffer { output })
    }

    pub async fn execute_decode_close(
        &mut self,
        session: PlayerHostResourceId,
        close: PlayerHostCommandBatch,
    ) -> Result<(), PlayerMediaLifecycleError> {
        let results = self.execute_batch(close).await.map_err(|error| {
            PlayerMediaLifecycleError::new("ASTRA_PLAYER_DECODE_CLOSE", error.to_string())
        })?;
        if !matches!(
            results.as_slice(),
            [PlayerHostCommandResult::DecodeClosed { session: closed }] if *closed == session
        ) {
            return Err(PlayerMediaLifecycleError::new(
                "ASTRA_PLAYER_DECODE_CLOSE_RESULT",
                "decode close returned an invalid logical resource",
            ));
        }
        Ok(())
    }

    pub async fn execute_decode_lifecycle(
        &mut self,
        plan: PlayerDecodeLifecyclePlan,
    ) -> Result<PlayerDecodedBuffer, PlayerMediaLifecycleError> {
        if let Err(error) = self.execute_decode_open(plan.session, plan.open).await {
            return Err(self.cleanup_decode(plan.close, plan.session, error).await);
        }
        let decoded = match self.execute_decode_submit(plan.session, plan.decode).await {
            Ok(decoded) => decoded,
            Err(error) => {
                return Err(self.cleanup_decode(plan.close, plan.session, error).await);
            }
        };
        self.execute_decode_close(plan.session, plan.close).await?;
        Ok(decoded)
    }

    async fn cleanup_decode(
        &mut self,
        close: PlayerHostCommandBatch,
        expected_session: PlayerHostResourceId,
        mut error: PlayerMediaLifecycleError,
    ) -> PlayerMediaLifecycleError {
        match self.execute_batch(close).await {
            Ok(results) if matches!(results.as_slice(), [PlayerHostCommandResult::DecodeClosed { session }] if *session == expected_session) =>
                {}
            Ok(_) => {
                error.cleanup_error =
                    Some("decode close returned an invalid logical resource".into())
            }
            Err(cleanup) => error.cleanup_error = Some(cleanup.to_string()),
        }
        error
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerMediaLifecycleError {
    code: &'static str,
    message: String,
    cleanup_error: Option<String>,
}

impl PlayerMediaLifecycleError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            cleanup_error: None,
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl std::fmt::Display for PlayerMediaLifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)?;
        if let Some(cleanup) = &self.cleanup_error {
            write!(formatter, "; cleanup failed: {cleanup}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PlayerMediaLifecycleError {}
