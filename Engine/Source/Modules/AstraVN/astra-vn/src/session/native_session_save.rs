use super::*;

impl VnSession {
    /// Validate a saved candidate without changing the live world or cancelling work.
    pub fn validate_save(&self, blob: &SaveBlob) -> Result<(), CoreVnError> {
        restore::validate_session_save(self, blob)
    }
    pub fn save(&self) -> Result<SaveBlob, CoreVnError> {
        if self.failed {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_FAILED",
                "failed session cannot produce a save",
            ));
        }
        astra_runtime::write_runtime_save(materialized_save_snapshot(self)?, SaveRequest::default())
            .map_err(|error| CoreVnError::message(error.to_string()))
    }

    pub fn restore(&mut self, blob: SaveBlob) -> Result<astra_runtime::LoadReport, CoreVnError> {
        let (step, seed) = restore::restore_session(self, blob)?;
        self.failed = false;
        Ok(astra_runtime::LoadReport { step, seed })
    }
}
