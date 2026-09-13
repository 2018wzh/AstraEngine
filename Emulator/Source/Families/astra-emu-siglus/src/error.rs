//! Error mapping from the vendored Siglus engine to the family ABI.

use abi_stable::std_types::RString;
use astra_emu_family_api::FamilyError;

pub(crate) fn invalid(code: &'static str, message: impl Into<String>) -> FamilyError {
    FamilyError::invalid(code, RString::from(message.into()))
}

pub(crate) fn engine(error: anyhow::Error) -> FamilyError {
    // The engine error chain may contain game-internal details; the code is
    // stable and the message is truncated to the top-level context.
    FamilyError::invalid(
        "ASTRA_EMU_SIGLUS_ENGINE",
        error
            .root_cause()
            .to_string()
            .lines()
            .next()
            .unwrap_or("the Siglus engine failed without a message")
            .to_string(),
    )
}
