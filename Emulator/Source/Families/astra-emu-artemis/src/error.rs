//! Error mapping from the vendored Artemis runtime to the family ABI.

use abi_stable::std_types::RString;
use astra_emu_family_api::FamilyError;

pub(crate) fn invalid(code: &'static str, message: impl Into<String>) -> FamilyError {
    FamilyError::invalid(code, RString::from(message.into()))
}

pub(crate) fn engine(error: String) -> FamilyError {
    // The engine error text may contain game-internal details; the code is
    // stable and the message is truncated to the top-level context.
    FamilyError::invalid(
        "ASTRA_EMU_ARTEMIS_ENGINE",
        error
            .lines()
            .next()
            .unwrap_or("the Artemis engine failed without a message")
            .to_string(),
    )
}
