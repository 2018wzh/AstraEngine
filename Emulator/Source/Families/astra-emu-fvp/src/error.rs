use astra_emu_family_api::FamilyError;
use rfvp::host_api::RfvpError;

pub(crate) fn invalid(code: &'static str, message: &'static str) -> FamilyError {
    FamilyError::invalid(code, message)
}

pub(crate) fn rfvp(error: RfvpError) -> FamilyError {
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
    };
    FamilyError::invalid(code, "RFVP rejected the family operation")
}
