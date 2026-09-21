use astra_emu_family_api::FamilyError;
use rfvp::host_api::RfvpError;

pub(crate) fn invalid(code: &'static str, message: &'static str) -> FamilyError {
    FamilyError::invalid(code, message)
}

pub(crate) fn rfvp(error: RfvpError) -> FamilyError {
    rfvp_operation(error, "family operation")
}

pub(crate) fn rfvp_operation(error: RfvpError, operation: &'static str) -> FamilyError {
    let code = match error {
        RfvpError::Io => "ASTRA_EMU_FVP_RFVP_IO",
        RfvpError::NotFound => "ASTRA_EMU_FVP_RFVP_NOT_FOUND",
        RfvpError::InvalidData => "ASTRA_EMU_FVP_RFVP_INVALID_DATA",
        RfvpError::InvalidArgument => "ASTRA_EMU_FVP_RFVP_INVALID_ARGUMENT",
        RfvpError::Unsupported => "ASTRA_EMU_FVP_RFVP_UNSUPPORTED",
        RfvpError::OutOfMemory => "ASTRA_EMU_FVP_RFVP_OUT_OF_MEMORY",
        RfvpError::CapacityExceeded => "ASTRA_EMU_FVP_RFVP_CAPACITY",
        RfvpError::EndOfFile => "ASTRA_EMU_FVP_RFVP_EOF",
        RfvpError::Backend => "ASTRA_EMU_FVP_RFVP_BACKEND",
        RfvpError::UnsupportedSnapshotVersion => "ASTRA_EMU_FVP_RFVP_UNSUPPORTED_SNAPSHOT_VERSION",
    };
    let message = if matches!(error, RfvpError::UnsupportedSnapshotVersion) {
        format!("RFVP rejected {operation}: snapshot version is unsupported")
    } else {
        format!("RFVP rejected {operation}")
    };
    FamilyError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::rfvp_operation;
    use rfvp::host_api::RfvpError;

    #[test]
    fn unsupported_snapshot_version_has_stable_diagnostic() {
        let error = rfvp_operation(RfvpError::UnsupportedSnapshotVersion, "snapshot restore");

        assert_eq!(
            error.code(),
            "ASTRA_EMU_FVP_RFVP_UNSUPPORTED_SNAPSHOT_VERSION"
        );
        assert_eq!(
            error.message.as_str(),
            "RFVP rejected snapshot restore: snapshot version is unsupported"
        );
    }
}
