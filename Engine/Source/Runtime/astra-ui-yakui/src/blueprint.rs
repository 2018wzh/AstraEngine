use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;

use astra_media_core::TextureFrame;

use astra_ui_core::{
    UiActionEnvelope, UiBindingRoot, UiBlueprintBundle, UiBlueprintFrameModel, UiEventBinding,
    UiFrameRequest, UiInputEventKind, UiPoint, UiRect, UiSemanticAction, UiSemanticNode,
    UiSemanticRole, UiSemanticSnapshot, UiThemeValue, UiValidationError, UiValue, UiValueExpr,
    UiViewBlueprint, ValidateUi, MAX_TEXTURE_BYTES,
};

use yakui_core::geometry::{Color, UVec2, Vec2};

use yakui_core::paint::{Texture, TextureFormat};

use yakui_core::{Alignment, CrossAxisAlignment, MainAxisSize, ManagedTextureId, WidgetId};

use yakui_widgets::widgets::{
    Align, CountGrid, Image, List, NineSlice, Pad, Scrollable, Slider, Stack,
};

use crate::{
    AstraNodeProps, AstraNodeWidget, AstraTextMeasureRequest, AstraTextMeasurer, BoundedLru,
    TextCharacterPolicy, TextInputState, VirtualGridState, VirtualListState, YakuiViewOutput,
    YakuiViewRenderer,
};

mod image_cache;
pub use image_cache::BlueprintYakuiRenderer;
use image_cache::PendingSemantic;
pub use image_cache::UiImageResourceProvider;
use image_cache::MAX_MANAGED_IMAGE_TEXTURES;
mod render_node;

mod render_children;
use render_children::virtual_state_key;
use render_children::virtual_viewport_extent;
mod build;

mod semantics;
use semantics::bounded_u32_property;
use semantics::default_ui_font_size;
use semantics::non_negative_layout_property;
use semantics::parse_alignment;
use semantics::parse_cross_axis_alignment;
use semantics::property_bool;
use semantics::property_color;
use semantics::property_number;
use semantics::property_number_f64;
use semantics::property_string;
use semantics::property_text;
use semantics::required_nine_slice;
use semantics::required_visual_asset;
use semantics::semantic_number;
use semantics::semantic_role;
use semantics::validate_image_resource;
use semantics::viewport_height_points;
use semantics::viewport_width_points;
mod bounded_usize_property;
use bounded_usize_property::action_from_event;
use bounded_usize_property::action_from_event_sequence;
use bounded_usize_property::bounded_usize_property;
use bounded_usize_property::evaluate;
use bounded_usize_property::evaluate_collection;
use bounded_usize_property::next_select_value;
use bounded_usize_property::repeat_key;
use bounded_usize_property::semantic_text_properties;
