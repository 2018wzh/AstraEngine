use astra_ui_core::UiValidationError;

#[derive(Debug, Clone, PartialEq)]
pub struct AstraTextMeasureRequest {
    pub semantic_id: String,
    pub text: String,
    pub max_width: f32,
    pub font_size: f32,
    pub max_lines: u32,
    pub direction: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AstraTextMeasureResult {
    pub width: f32,
    pub height: f32,
}

pub trait AstraTextMeasurer: Send + Sync {
    fn measure(
        &self,
        request: &AstraTextMeasureRequest,
    ) -> Result<AstraTextMeasureResult, UiValidationError>;
}
