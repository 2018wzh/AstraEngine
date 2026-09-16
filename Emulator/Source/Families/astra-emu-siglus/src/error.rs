//! Error mapping from the vendored Siglus engine to the family ABI.

use abi_stable::std_types::RString;
use astra_emu_family_api::FamilyError;

pub(crate) fn invalid(code: &'static str, message: impl Into<String>) -> FamilyError {
    FamilyError::invalid(code, RString::from(message.into()))
}

pub(crate) fn engine(operation: &'static str, _error: anyhow::Error) -> FamilyError {
    // Engine chains can contain game text and private paths. Expose only the
    // audited operation at this boundary; never forward arbitrary Debug text.
    tracing::error!(event = "astra.emu.siglus.engine.failed", operation);
    FamilyError::invalid(
        "ASTRA_EMU_SIGLUS_ENGINE",
        format!("Siglus engine failed during {operation}"),
    )
}
