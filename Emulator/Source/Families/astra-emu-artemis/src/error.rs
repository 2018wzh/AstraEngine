//! Native embedding errors; no Rust-owned value crosses the C boundary.
#[derive(Clone, Debug)]
pub(crate) struct NativeError {
    pub code: &'static str,
    pub message: String,
}
pub(crate) type NativeResult<T> = Result<T, NativeError>;
impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for NativeError {}
pub(crate) fn invalid(code: &'static str, message: impl Into<String>) -> NativeError {
    NativeError {
        code,
        message: message.into(),
    }
}
pub(crate) fn engine(message: String) -> NativeError {
    invalid("ASTRA_EMU_ARTEMIS_ENGINE", message)
}
