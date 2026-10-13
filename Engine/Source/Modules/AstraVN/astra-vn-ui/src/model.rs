use std::collections::BTreeMap;

use astra_ui_core::{UiValidationError, UiValue, ValidateUi};

use astra_vn_script::{
    CompiledStory, ReadingMode, SkipMode, SystemPageKind, VnRuntimeState, VnTextRevealState,
    VnWaitKind,
};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

mod message_view_model;
pub use message_view_model::BacklogEntryViewModel;
pub use message_view_model::ChoiceOptionViewModel;
pub use message_view_model::ChoiceViewModel;
pub use message_view_model::ConfigViewModel;
pub use message_view_model::LocalizationEntryViewModel;
pub use message_view_model::MessageViewModel;
pub use message_view_model::RouteNodeViewModel;
pub use message_view_model::SaveSlotViewModel;
pub use message_view_model::SystemPageUnderlayViewModel;
pub use message_view_model::SystemPageViewModel;
pub use message_view_model::TextInputViewModel;
pub use message_view_model::UnlockItemViewModel;
pub use message_view_model::VnUiModelContext;
pub use message_view_model::VnUiPageModel;
mod unlock_item_view_model;
mod vn_ui_model_context;
use unlock_item_view_model::optional_string;
use unlock_item_view_model::skip_mode_name;
use unlock_item_view_model::ui_map;
