use super::*;

#[derive(Debug, thiserror::Error)]
pub enum NativeVnHostError {
    #[error("compiled story has no playable entry state")]
    EmptyStory,
    #[error("Player host command sequence overflowed")]
    SequenceOverflow,
    #[error(transparent)]
    RuntimeHost(#[from] VnHostError),
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Command(#[from] PlayerHostCommandError),
    #[error(transparent)]
    Ui(#[from] UiValidationError),
    #[error(transparent)]
    UiBinding(#[from] VnUiBindingError),
    #[error("presentation serialization failed: {0}")]
    Serialize(String),
    #[error("package validation failed: {0}")]
    Package(String),
    #[error("presentation asset failed: {0}")]
    Asset(String),
    #[error("localization failed: {0}")]
    Localization(String),
    #[error("player input failed: {0}")]
    Input(String),
    #[error("runtime evidence failed: {0}")]
    RuntimeEvidence(String),
    #[error("provider binding failed: {0}")]
    ProviderBinding(String),
    #[error("player save failed: {0}")]
    Save(String),
}
