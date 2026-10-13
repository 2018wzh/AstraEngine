use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::Hash256;

use astra_media_core::TextureFrame;

use astra_ui_core::{
    UiBackend, UiBlueprintBundle, UiBlueprintFrameModel, UiBlueprintModalFrameModel, UiButtonState,
    UiCapability, UiEventBinding, UiFrameRequest, UiInputDispositionKind, UiInputEvent,
    UiInputEventKind, UiInputFrame, UiInsets, UiNodeBlueprint, UiRepeatBinding, UiSemanticAction,
    UiSemanticRole, UiThemeManifest, UiThemeValue, UiValue, UiValueExpr, UiViewBlueprint,
    UiViewport,
};

use astra_ui_yakui::{AstraYakuiBackend, BlueprintYakuiRenderer};

#[path = "blueprint_runtime/bindings.rs"]
mod bindings;
use bindings::node;
#[path = "blueprint_runtime/image_cache.rs"]
mod image_cache;
#[path = "blueprint_runtime/input_actions.rs"]
mod input_actions;
