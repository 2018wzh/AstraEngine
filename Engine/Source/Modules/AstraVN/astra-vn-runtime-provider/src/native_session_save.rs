use super::*;

impl NativeVnSession {
    pub fn save(&self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, CoreVnError> {
        self.validate_id(&request.session_id)?;
        let session = self;
        let save = astra_runtime::write_runtime_save(
            materialized_save_snapshot(session)?,
            SaveRequest::default(),
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?;
        Ok(RuntimeSaveSections {
            session_id: request.session_id,
            sections: vec![RuntimeSectionPayload {
                section_id: "runtime.world".to_string(),
                schema: "astra.runtime.save_blob.v5".to_string(),
                version: SchemaVersion::new(5, 0, 0),
                codec: RuntimeSectionCodec::Raw,
                hash: astra_core::Hash256::from_sha256(&save.0),
                bytes: save.0,
            }],
            diagnostics: Vec::new(),
        })
    }

    pub fn restore(
        &mut self,
        request: RuntimeRestoreRequest,
    ) -> Result<RuntimeRestoreReport, CoreVnError> {
        self.validate_id(&request.session_id)?;
        if request.sections.len() != 1 {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_RESTORE_SECTION_SET",
                "restore requires exactly one authoritative runtime.world section",
            ));
        }
        let runtime_section = required_restore_section_with_codec(
            &request.sections,
            "runtime.world",
            "astra.runtime.save_blob.v5",
            RuntimeSectionCodec::Raw,
        )?;
        if runtime_section.version != SchemaVersion::new(5, 0, 0)
            || runtime_section.hash != astra_core::Hash256::from_sha256(&runtime_section.bytes)
        {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_RESTORE_INTEGRITY",
                "runtime.world section version or hash is invalid",
            ));
        }
        let session = self;
        let (step, seed) = restore::restore_session(session, runtime_section)?;
        Ok(RuntimeRestoreReport {
            session_id: request.session_id,
            restored_fixed_step: step,
            session_seed: seed,
            status: "restored".to_string(),
            diagnostics: Vec::new(),
        })
    }
}
