#[derive(Debug, Clone)]
pub struct VnHostLimits {
    max_outputs: usize,
    max_output_bytes: usize,
}

impl Default for VnHostLimits {
    fn default() -> Self {
        Self {
            max_outputs: 256,
            max_output_bytes: 8 * 1024 * 1024,
        }
    }
}

impl VnHostLimits {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_bounds(mut self, max_outputs: usize, max_output_bytes: usize) -> Self {
        self.max_outputs = max_outputs;
        self.max_output_bytes = max_output_bytes;
        self
    }

    /// Apply the same budget to native typed output without constructing an ABI envelope.
    pub fn validate_output_count(&self, count: usize) -> Result<(), VnHostError> {
        if count > self.max_outputs {
            return Err(VnHostError::new(
                "ASTRA_RUNTIME_HOST_OUTPUT_COUNT",
                "VN live output count exceeds the configured bound",
            ));
        }
        Ok(())
    }

    /// Bound a native Runtime save without constructing ABI section metadata.
    pub fn validate_save_bytes(&self, bytes: usize) -> Result<(), VnHostError> {
        if bytes > self.max_output_bytes {
            return Err(VnHostError::new(
                "ASTRA_RUNTIME_HOST_SECTION_BYTES",
                "runtime save section bytes exceed the configured bound",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VnHostError {
    code: &'static str,
    message: String,
}

impl VnHostError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl std::fmt::Display for VnHostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for VnHostError {}
