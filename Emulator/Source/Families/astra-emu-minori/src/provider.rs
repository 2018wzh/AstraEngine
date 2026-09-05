use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    sync::Arc,
};

use astra_byte_source::{BoundedByteSourceReader, ByteRange, OwnedByteBuffer};
use astra_core::Hash256;
#[cfg(test)]
use astra_core::SchemaVersion;
use astra_emu_family_api::{
    validate_symbol, FamilyId, LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioPacketV7,
    LegacyBlackboardMutation, LegacyBlendMode, LegacyConfirmationChoiceV1,
    LegacyConfirmationResultV1, LegacyConfirmationTransactionV1, LegacyControlTransaction,
    LegacyCoverageDelta, LegacyDrawV1, LegacyEvent, LegacyFamilyHostServicesV9,
    LegacyFamilyPluginDescriptor, LegacyLayerBlendV9, LegacyLayerFilterV9, LegacyLayerOperationV9,
    LegacyLayerStateV9, LegacyLayerTransactionV9, LegacyLayerTransformV9,
    LegacyLiveOutput as LegacyLiveOutputV9, LegacyOpenRequest, LegacyProbeReport,
    LegacyProbeRequest, LegacyProviderError, LegacyRenderResourceFrameV1, LegacyResourceRead,
    LegacyRuntimeHostCtx, LegacyRuntimeProvider, LegacyRuntimeSessionId, LegacyRuntimeStatus,
    LegacyScissorV1, LegacySequenced, LegacyShutdownReport, LegacyStepInput,
    LegacyStepOutput as LegacyStepOutputV9, LegacySurfaceCommitV9, LegacySurfaceDamageV9,
    LegacySurfaceFormatV9, LegacySystemCommandKindV1, LegacySystemCommandStatusV1,
    LegacySystemCommandTransactionV1, LegacySystemMenuActionV1, LegacySystemMenuItemKindV1,
    LegacySystemMenuItemV1, LegacySystemMenuTransactionV1, LegacyTextInputChoiceV1,
    LegacyTextInputTransactionV1, LegacyTextureFilter, LegacyTextureFormat,
    LegacyTextureResourceV1, LegacyTraceEntry, LegacyVertexV1, LegacyVfsReader,
    LegacyVideoCommandV1, LegacyVideoMode, LegacyVmTraceRecord, LegacyWaitRequest,
    LEGACY_FAMILY_ABI_FINGERPRINT, LEGACY_SYSTEM_UI_ACTIVE_BLACKBOARD_KEY,
};
#[cfg(test)]
use astra_emu_family_api::{LegacySystemCommandResultV1, LegacyTextInputResultV1};
use astra_emu_family_core::LegacyCoreError;
use astra_emu_family_support::LegacyRuntimeVfsByteSource;
use astra_media::{
    probe_symphonia_audio_metadata_reader, DecodeBindingContext, DecodeKind, DecodeOutput,
    DecodeProviderRegistry, DecodeRequest, ImageDecodeProvider,
};
use astra_media_core::{
    BlendMode, CpuRendererProvider, MeshMaterial2D, MeshVertex2D, RectI, RenderTargetFormat,
    Renderer2DProvider, RendererCreateRequest, SceneCommand, TextureFilter2D, TextureFrame,
};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder, ImageFormat};
use serde::{Deserialize, Serialize};

use crate::save::{
    decode as decode_save, decode_config, encode as encode_save, encode_config,
    quick_save_file_number, slot_path, slot_temporary_path, MinoriConfigEnvelope,
    MinoriSaveEnvelope, MINORI_CONFIG_MAX_BYTES, MINORI_CONFIG_PATH, MINORI_CONFIG_ROOT,
    MINORI_CONFIG_SCHEMA, MINORI_CONFIG_TEMPORARY_PATH, MINORI_QUICK_SAVE_SLOT_COUNT,
    MINORI_SAVE_COMMENT_MAX_BYTES, MINORI_SAVE_MAX_BYTES, MINORI_SAVE_MAX_SLOTS,
    MINORI_SAVE_ROOT, MINORI_SAVE_SCHEMA, MINORI_SAVE_THUMBNAIL_HEIGHT,
    MINORI_SAVE_THUMBNAIL_MAX_BYTES, MINORI_SAVE_THUMBNAIL_WIDTH, MINORI_SAVE_TIMESTAMP_MAX_BYTES,
};
use crate::text_surface::{
    MinoriTextSurfaceRenderer, TextAlignment, TextOutline, TextRegion, TextSurfaceRequest,
};
#[cfg(test)]
use crate::MinoriCharacterReplacementState;
use crate::{
    collect_resource_references, message_voice_wait_resources, parse_sc, MinoriAudioCommand,
    MinoriAudioEncoding, MinoriAxisScrollFrame, MinoriCharacterFrame, MinoriCharacterState,
    MinoriChoicePresentation, MinoriConfigAudioBus, MinoriConfigChange, MinoriConfigControl,
    MinoriConfigState, MinoriEffectFrame, MinoriExecutedCommand, MinoriImageDecodeProvider,
    MinoriLinearScrollFrame, MinoriLocaleHook, MinoriMessageMarkupError, MinoriMovieState,
    MinoriPlayMode, MinoriRuntimeError, MinoriRuntimeState, MinoriScreenShakeFrame,
    MinoriScrollXfFrame, MinoriSecondaryEffectFrame, MinoriStageCommand, MinoriStageLayer,
    MinoriStandLayer, MinoriSystemPage, MinoriVm, MinoriVmEvent, MinoriWScroll2Frame,
    MinoriWaitState, ScOpcodeCatalog, MINORI_CHOICE_PRESENTATION_SCHEMA,
    MINORI_IMAGE_DECODE_PROVIDER_ID, MINORI_MAX_RESOURCE_AUDIT_SCRIPTS,
};
use crate::{MinoriAniArchive, MinoriSqzArchive};

pub const MINORI_FAMILY_ID: &str = "minori";
pub const MINORI_RUNTIME_PROVIDER_ID: &str = "astra.emu.family.minori";
const MAX_SCRIPT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INSTRUCTIONS_PER_STEP: u32 = 100_000;
const MAX_RESOURCE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_DECODED_TEXTURE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_LAYER_SURFACE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_WSCROLL2_SYNC_BYTES: u64 = 64 * 1024;
const MAX_WSCROLL2_SYNC_VALUES: usize = 4096;
const MAX_EPHEMERAL_TEXT_BYTES: usize = 64 * 1024;
const MINORI_TEXT_SURFACE_ID: &str = "minori.surface.text";
const MINORI_TEXT_LAYER_ID: &str = "minori.layer.text";
const MINORI_CONTROL_KEY: &str = "control";
const MINORI_POINTER_X: &str = "pointer.x";
const MINORI_POINTER_Y: &str = "pointer.y";
const MINORI_POINTER_PRIMARY: &str = "pointer.primary";
// The title artwork is authored at the fixed 1280x720 stage.  Its menu hit
// regions are deliberately kept in stage space so the host can scale and
// letterbox the window without changing family semantics.  The left edge is
// aligned with the transparent/hover artwork rather than the visible glyphs;
// this matches the original's generous mouse target without accepting clicks
// from the gameplay area.
const MINORI_TITLE_MENU_LEFT: i32 = 1024;
const MINORI_TITLE_MENU_RIGHT: i32 = 1280;
const MINORI_TITLE_MENU_ROW_HEIGHT: i32 = 48;
const MINORI_TITLE_MENU_ROW_TOPS_BASE: [i32; 4] = [24, 72, 120, 216];
const MINORI_TITLE_MENU_ROW_TOPS_MEMORIES: [i32; 5] = [24, 72, 120, 168, 216];
const MINORI_TITLE_MENU_HOVER_SCISSOR_X: i32 = 1024;
const MINORI_GAME_MENU_PLAY_MODE_LEFT: i32 = 1125;
const MINORI_GAME_MENU_PLAY_MODE_TOP: i32 = 577;
const MINORI_GAME_MENU_PLAY_MODE_RIGHT: i32 = 1177;
const MINORI_GAME_MENU_PLAY_MODE_BOTTOM: i32 = 616;
const MINORI_CHOICE_CONFIRM_CONTROLS: [&str; 2] = ["enter", "space"];
const MINORI_CHOICE_NAVIGATION_CONTROLS: [&str; 2] = ["arrow_up", "arrow_down"];
const MINORI_CHOICE_RESOURCE_URIS: [&str; 3] = [
    "minori:/sys/SelectBLur.png",
    "minori:/sys/SelectFocus.png",
    "minori:/sys/SelectActive.png",
];
const MINORI_CHOICE_TEXTURE_BASE: u32 = 500;
const MINORI_CHARACTER_TEXTURE_BASE: u32 = 10_000;
const MINORI_CHARACTER_REPLACEMENT_TEXTURE_BASE: u32 = 15_000;
const MINORI_SYSTEM_TEXTURE_ID: u32 = 20_000;
const MINORI_BACKLOG_GAUGE_TEXTURE_ID: u32 = 20_001;
const MINORI_BACKLOG_BALL_TEXTURE_ID: u32 = 20_002;
// Save/load headings are authored against the fixed 1280x720 Minori stage.
// Keep these positions in stage coordinates so host scaling and letterboxing
// do not change the family-owned system-page layout.
const MINORI_SAVE_LOAD_TITLE_X: i32 = 64;
const MINORI_SAVE_LOAD_HEADER_Y: i32 = 16;
const MINORI_SAVE_LOAD_PAGE_X: i32 = 608;
const MINORI_CONFIG_KNOB_TEXTURE_ID: u32 = 20_010;
const MINORI_CONFIG_CHECKMARK_TEXTURE_ID: u32 = 20_011;
const MINORI_CONFIG_CIRCLE_TEXTURE_ID: u32 = 20_012;
const MINORI_GALLERY_CG_THUMB_TEXTURE_BASE: u32 = 21_000;
const MINORI_TITLE_BASE_ITEM_COUNT: u32 = 4;
const MINORI_TITLE_MEMORIES_ITEM_COUNT: u32 = 5;
const MINORI_MEMORIES_ITEM_COUNT: u32 = 5;
const MINORI_GALLERY_CG_PAGE_COUNT: u32 = 12;
const MINORI_GALLERY_BGM_TRACK_COUNT: u32 = 47;
const MINORI_GALLERY_REPLAY_PAGE_COUNT: u32 = 4;
const MINORI_GALLERY_MOVIE_COUNT: u32 = 4;
const MINORI_GALLERY_CG_PAGE_URIS: [&str; 12] = [
    "minori:/sys/cgpage001.png",
    "minori:/sys/cgpage002.png",
    "minori:/sys/cgpage003.png",
    "minori:/sys/cgpage004.png",
    "minori:/sys/cgpage005.png",
    "minori:/sys/cgpage006.png",
    "minori:/sys/cgpage007.png",
    "minori:/sys/cgpage008.png",
    "minori:/sys/cgpage009.png",
    "minori:/sys/cgpage010.png",
    "minori:/sys/cgpage011.png",
    "minori:/sys/cgpage012.png",
];
const MINORI_GALLERY_BGM_PAGE_URIS: [&str; 3] = [
    "minori:/sys/musicPage1.png",
    "minori:/sys/musicPage2.png",
    "minori:/sys/musicPage3.png",
];
const MINORI_GALLERY_BGM_TRACK_URIS: [&str; 47] = [
    "minori:/bgm/BGM001.ogg",
    "minori:/bgm/BGM002.ogg",
    "minori:/bgm/BGM003.ogg",
    "minori:/bgm/BGM004.ogg",
    "minori:/bgm/BGM005.ogg",
    "minori:/bgm/BGM006.ogg",
    "minori:/bgm/BGM007.ogg",
    "minori:/bgm/BGM008.ogg",
    "minori:/bgm/BGM009.ogg",
    "minori:/bgm/BGM010.ogg",
    "minori:/bgm/BGM011.ogg",
    "minori:/bgm/BGM012.ogg",
    "minori:/bgm/BGM013.ogg",
    "minori:/bgm/BGM014.ogg",
    "minori:/bgm/BGM015.ogg",
    "minori:/bgm/BGM016.ogg",
    "minori:/bgm/BGM017.ogg",
    "minori:/bgm/BGM018.ogg",
    "minori:/bgm/BGM019.ogg",
    "minori:/bgm/BGM020.ogg",
    "minori:/bgm/BGM021.ogg",
    "minori:/bgm/BGM022.ogg",
    "minori:/bgm/BGM023.ogg",
    "minori:/bgm/BGM024.ogg",
    "minori:/bgm/BGM031.ogg",
    "minori:/bgm/BGM032.ogg",
    "minori:/bgm/BGM033.ogg",
    "minori:/bgm/BGM034.ogg",
    "minori:/bgm/BGM035.ogg",
    "minori:/bgm/BGM036.ogg",
    "minori:/bgm/BGM037.ogg",
    "minori:/bgm/BGM038.ogg",
    "minori:/bgm/BGM041.ogg",
    "minori:/bgm/BGM042.ogg",
    "minori:/bgm/BGM043.ogg",
    "minori:/bgm/BGM051.ogg",
    "minori:/bgm/BGM052.ogg",
    "minori:/bgm/BGM061.ogg",
    "minori:/bgm/BGM062.ogg",
    "minori:/bgm/BGM071.ogg",
    "minori:/bgm/BGM072.ogg",
    "minori:/bgm/BGM073.ogg",
    "minori:/bgm/BGM074.ogg",
    "minori:/bgm/BGM081.ogg",
    "minori:/bgm/BGM082.ogg",
    "minori:/bgm/BGM083.ogg",
    "minori:/bgm/BGM084.ogg",
];
const MINORI_GALLERY_REPLAY_PAGE_URIS: [&str; 4] = [
    "minori:/sys/flash0.png",
    "minori:/sys/flash1.png",
    "minori:/sys/flash2.png",
    "minori:/sys/flash3.png",
];
const MINORI_GALLERY_REPLAY_SCRIPT_TARGETS: [&str; 4] = [
    "fb_ren_04.sc",
    "fb_aya_04.sc",
    "fb_sui_04.sc",
    "fb_tou_04.sc",
];
const MINORI_GALLERY_MOVIE_SCRIPT_TARGETS: [&str; 4] = [
    "fb_aya_12.sc",
    "fb_ren_16.sc",
    "fb_sui_12.sc",
    "fb_tou_12.sc",
];
const MINORI_GALLERY_MOVIE_LABELS: [&str; 4] =
    ["ed_ayame.avi", "ed_ren.avi", "ed_sui.avi", "ed_tohka.avi"];
const MINORI_GLOBAL_PROGRESS_OPTION: &str = "astra.provider.storage";
const MINORI_WRITABLE_FILE_BINDING_ID: &str = "astra.writable_file.v1";
const MINORI_GLOBAL_PROGRESS_DIRECTORY: &str = MINORI_CONFIG_ROOT;
const MINORI_GLOBAL_PROGRESS_PATH: &str = "minori/global-progress-v1.bin";
const MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH: &str = "minori/global-progress-v1.tmp";
const MAX_GLOBAL_PROGRESS_BYTES: u64 = 1024 * 1024;
const MINORI_GLOBAL_PROGRESS_SCHEMA: &str = "astra.emu.minori.global_progress.v1";
#[allow(dead_code)]
const MINORI_GLOBAL_PROGRESS_SNAPSHOT_SCHEMA: &str = "astra.emu.minori.global_progress_snapshot.v1";

// Family-owned text layout staging. The current ABI never exports these values.
// The v10 publisher rasterizes the original Japanese text into Host-owned
// layer surfaces before publishing the retained transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyTextHorizontalAlignmentV1 {
    Start,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LegacyTextOutlineV1 {
    radius: u32,
    rgba: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LegacyTextRegionV1 {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    font_size: f32,
    line_height: f32,
    max_lines: u32,
    horizontal_alignment: LegacyTextHorizontalAlignmentV1,
}

#[derive(Debug, Clone, PartialEq)]
struct LegacyTextPresentationV1 {
    layout_id: String,
    language: String,
    font_families: Vec<String>,
    body: LegacyTextRegionV1,
    speaker: Option<LegacyTextRegionV1>,
    rgba: [u8; 4],
    outline: Option<LegacyTextOutlineV1>,
}

impl LegacyTextPresentationV1 {
    fn validate(&self) -> Result<(), LegacyProviderError> {
        validate_symbol("text_layout_id", &self.layout_id)?;
        if self.language != "ja-JP"
            || self.font_families.as_slice() != ["Noto Sans JP"]
            || self.rgba[3] == 0
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TEXT_LAYOUT_BINDING",
                "Minori text layout requires its explicit Japanese font and visible color",
            ));
        }
        for region in self.speaker.iter().chain(std::iter::once(&self.body)) {
            if region.x < 0
                || region.y < 0
                || region.width == 0
                || region.height == 0
                || !region.font_size.is_finite()
                || !region.line_height.is_finite()
                || region.font_size <= 0.0
                || region.line_height < region.font_size
                || region.max_lines == 0
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TEXT_LAYOUT_REGION",
                    "Minori text layout region is invalid",
                ));
            }
        }
        if self
            .outline
            .is_some_and(|outline| outline.radius == 0 || outline.rgba[3] == 0)
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TEXT_OUTLINE",
                "Minori text outline is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
struct LegacyTextPresentationLeaseV1 {
    lease_id: String,
    presentation: LegacyTextPresentationV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StagedEphemeralText {
    lease_id: String,
    text: String,
    speaker: Option<String>,
    /// The original message panel owns a separate progress marker.  Keep the
    /// flag beside the ephemeral lease so it cannot leak into the backlog or
    /// runtime state.
    show_advance_indicator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StagedTextLease {
    sequence: u64,
    lease_id: String,
    byte_len: u32,
    source_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MinoriSaveSlotMetadata {
    timestamp: String,
    comment: String,
    thumbnail_rgba: Arc<[u8]>,
}

#[derive(Debug, Clone, Copy)]
enum SaveSlotMetadataContext {
    List,
    Load,
}

impl SaveSlotMetadataContext {
    const fn diagnostic(self, kind: SaveSlotMetadataDiagnostic) -> &'static str {
        match (self, kind) {
            (Self::List, SaveSlotMetadataDiagnostic::Format) => "ASTRA_EMU_MINORI_SAVE_LIST_FORMAT",
            (Self::Load, SaveSlotMetadataDiagnostic::Format) => "ASTRA_EMU_MINORI_LOAD_SLOT_FORMAT",
            (Self::List, SaveSlotMetadataDiagnostic::Identity) => {
                "ASTRA_EMU_MINORI_SAVE_LIST_IDENTITY"
            }
            (Self::Load, SaveSlotMetadataDiagnostic::Identity) => {
                "ASTRA_EMU_MINORI_LOAD_SLOT_IDENTITY"
            }
            (Self::List, SaveSlotMetadataDiagnostic::Timestamp) => {
                "ASTRA_EMU_MINORI_SAVE_LIST_TIMESTAMP"
            }
            (Self::Load, SaveSlotMetadataDiagnostic::Timestamp) => {
                "ASTRA_EMU_MINORI_LOAD_SLOT_TIMESTAMP"
            }
            (Self::List, SaveSlotMetadataDiagnostic::Comment) => {
                "ASTRA_EMU_MINORI_SAVE_LIST_COMMENT"
            }
            (Self::Load, SaveSlotMetadataDiagnostic::Comment) => {
                "ASTRA_EMU_MINORI_LOAD_SLOT_COMMENT"
            }
            (Self::List, SaveSlotMetadataDiagnostic::Thumbnail) => {
                "ASTRA_EMU_MINORI_SAVE_LIST_THUMBNAIL"
            }
            (Self::Load, SaveSlotMetadataDiagnostic::Thumbnail) => {
                "ASTRA_EMU_MINORI_LOAD_SLOT_THUMBNAIL"
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SaveSlotMetadataDiagnostic {
    Format,
    Identity,
    Timestamp,
    Comment,
    Thumbnail,
}

impl LegacyTextPresentationLeaseV1 {
    fn validate(&self) -> Result<(), LegacyProviderError> {
        validate_symbol("text_presentation_lease_id", &self.lease_id)?;
        self.presentation.validate()
    }
}

#[derive(Debug, Default, PartialEq)]
struct LegacyLiveOutput {
    clear_text: bool,
    resource_scenes: Vec<LegacySequenced<LegacyRenderResourceFrameV1>>,
    text_presentations: Vec<LegacySequenced<LegacyTextPresentationLeaseV1>>,
    text: Vec<StagedTextLease>,
    audio: Vec<LegacyAudioPacketV7>,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
    video: Vec<LegacySequenced<LegacyVideoCommandV1>>,
}

#[derive(Debug, PartialEq)]
struct LegacyStepOutput {
    status: LegacyRuntimeStatus,
    live: LegacyLiveOutput,
    control: LegacyControlTransaction,
    trace: Vec<LegacyTraceEntry>,
    diagnostics: Vec<astra_emu_family_api::LegacyDiagnostic>,
    coverage: LegacyCoverageDelta,
    state_revision: u64,
}

impl LegacyStepOutput {
    fn validate(&self) -> Result<(), LegacyProviderError> {
        for frame in &self.live.resource_scenes {
            frame.value.validate()?;
        }
        for presentation in &self.live.text_presentations {
            presentation.value.validate()?;
        }
        for command in &self.live.audio_commands {
            command.value.validate()?;
        }
        for command in &self.live.video {
            command.value.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MinoriGlobalProgressV1 {
    schema: String,
    gallery_unlocks: Vec<Hash256>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct MinoriGlobalProgressSnapshotV1 {
    schema: String,
    enabled: bool,
    loaded: bool,
    persisted_unlocks: Vec<Hash256>,
}

#[cfg(test)]
struct TestCheckpointSection {
    version: SchemaVersion,
    bytes: Vec<u8>,
}

#[cfg(test)]
struct TestProviderCheckpoint {
    family_sections: Vec<TestCheckpointSection>,
    global_progress: MinoriGlobalProgressSnapshotV1,
}

#[derive(Debug, Clone)]
struct MinoriGlobalProgressSession {
    enabled: bool,
    loaded: bool,
    persisted_unlocks: Vec<Hash256>,
}
fn message_input_keys(control_fast_forward_enabled: bool) -> Vec<String> {
    MINORI_MESSAGE_HOST_AWAIT_CONTROLS
        .iter()
        .copied()
        .chain(control_fast_forward_enabled.then_some(MINORI_CONTROL_KEY))
        .map(str::to_owned)
        .collect()
}

fn choice_input_keys() -> Vec<String> {
    MINORI_CHOICE_CONFIRM_CONTROLS
        .iter()
        .map(|key| key.to_string())
        .collect()
}

const MINORI_MESSAGE_INPUT_CONTROLS: [&str; 3] = ["enter", "space", "pointer.primary"];
const MINORI_WINDOW_CLOSE_CONTROL: &str = "window.close";
// The host wait contract must cover every canonical message activation edge
// that the family accepts directly.  Escape is also a host-owned system-menu
// shortcut, but it remains in the contract so the host removes the active
// message wait in the same tick that the family opens the save page.  The
// escape edge remains visible to the family (see the CLI wait router), where
// it is consumed as the menu action.
const MINORI_MESSAGE_HOST_AWAIT_CONTROLS: [&str; 4] =
    ["enter", "space", "escape", "pointer.primary"];

fn minori_message_presentation(
    stage_size: Option<(u32, u32)>,
    text_shadow: bool,
) -> Result<LegacyTextPresentationV1, LegacyProviderError> {
    if stage_size != Some((1280, 720)) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY",
            "the verified Minori message layout requires the 1280x720 reference stage",
        ));
    }
    let presentation = LegacyTextPresentationV1 {
        layout_id: "minori.message".into(),
        language: "ja-JP".into(),
        font_families: vec!["Noto Sans JP".into()],
        body: LegacyTextRegionV1 {
            x: 160,
            y: 568,
            width: 960,
            height: 112,
            font_size: 26.0,
            line_height: 32.0,
            max_lines: 3,
            horizontal_alignment: LegacyTextHorizontalAlignmentV1::Start,
        },
        speaker: Some(LegacyTextRegionV1 {
            x: 160,
            y: 528,
            width: 960,
            height: 32,
            font_size: 26.0,
            line_height: 32.0,
            max_lines: 1,
            horizontal_alignment: LegacyTextHorizontalAlignmentV1::Start,
        }),
        rgba: [255, 255, 255, 255],
        outline: text_shadow.then_some(LegacyTextOutlineV1 {
            radius: 2,
            rgba: [0, 0, 0, 192],
        }),
    };
    presentation.validate()?;
    Ok(presentation)
}

fn minori_gallery_movie_presentation(
    stage_size: Option<(u32, u32)>,
) -> Result<LegacyTextPresentationV1, LegacyProviderError> {
    if stage_size != Some((1280, 720)) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GALLERY_MOVIE_STAGE_IDENTITY",
            "the verified movie gallery layout requires the 1280x720 stage",
        ));
    }
    let presentation = LegacyTextPresentationV1 {
        layout_id: "minori.gallery.movie".into(),
        language: "ja-JP".into(),
        font_families: vec!["Noto Sans JP".into()],
        body: LegacyTextRegionV1 {
            x: 160,
            y: 112,
            width: 520,
            height: 280,
            font_size: 32.0,
            line_height: 52.0,
            max_lines: 4,
            horizontal_alignment: LegacyTextHorizontalAlignmentV1::Start,
        },
        speaker: None,
        rgba: [255, 255, 255, 255],
        outline: Some(LegacyTextOutlineV1 {
            radius: 2,
            rgba: [0, 0, 0, 192],
        }),
    };
    presentation.validate()?;
    Ok(presentation)
}

struct MinoriSession {
    #[allow(dead_code)]
    case_fingerprint: Hash256,
    package_hash: Hash256,
    profile_fingerprint: Hash256,
    mount_set_id: String,
    /// The title's configured gameplay entry.  Chain and gallery scripts may
    /// replace the active VM program, but starting a new game from the title
    /// must always reload this verified root instead of continuing at the
    /// previous script's end.
    entry_script_uri: String,
    fixed_delta_ns: u64,
    session_seed: u64,
    stage_size: Option<(u32, u32)>,
    vm: MinoriVm,
    ephemeral_text: BTreeMap<String, StagedEphemeralText>,
    collect_evidence_vm_trace: bool,
    evidence_contexts: BTreeMap<u32, Hash256>,
    evidence_vm_trace: BTreeSet<(u32, u32, u8)>,
    restore_audio_pending: bool,
    restore_presentation_pending: bool,
    reported_system_page: Option<MinoriSystemPage>,
    reported_play_mode: Option<MinoriPlayMode>,
    reported_gallery_unlock_count: Option<usize>,
    reported_choice_active: Option<bool>,
    reported_progress_in_background: Option<bool>,
    global_progress: MinoriGlobalProgressSession,
    config_storage_enabled: bool,
    config_persisted: MinoriConfigState,
    /// Persisted Quick Save rotation cursor mirrored from the config envelope.
    /// The original keeps `quickSaveFileNumber` in the installation-scoped
    /// system parameters, so rotation survives restarts.
    quick_save_cursor: u32,
    config_persisted_cursor: u32,
    /// Script line of the last successful quick save.  The original skips a
    /// quick save whose script line cursor is unchanged since the previous
    /// one, so a repeated request at the same line neither writes a file nor
    /// advances the rotation.
    last_quick_save_pc_line: Option<u32>,
    /// Host-owned window sampling preference mirrored only for rebuilding the
    /// next native menu transaction. It is not part of the game save/config
    /// payload; the platform host remains the source of the actual sampler.
    resize_antialias: bool,
    /// Current title-menu pointer target.  This is host input presentation
    /// state, not gameplay/save state, so it is intentionally rebuilt after a
    /// restore rather than serialized into the VM snapshot.
    title_pointer_focus: Option<u32>,
    save_slots: BTreeSet<u32>,
    save_slot_comments: BTreeMap<u32, String>,
    save_slot_lengths: BTreeMap<u32, u64>,
    save_slot_metadata: BTreeMap<u32, MinoriSaveSlotMetadata>,
    text_renderer: Option<MinoriTextSurfaceRenderer>,
    last_text_surface: Option<Arc<[u8]>>,
    last_gameplay_frame: Option<Arc<[u8]>>,
    published_layers: BTreeSet<String>,
    /// Last resource-backed presentation descriptor committed to the host.
    ///
    /// The descriptor contains only bounded URI/geometry metadata; retaining
    /// it lets the family keep the ABI v11 Layer2D scene retained across fixed
    /// ticks instead of re-decoding every unchanged frame. It is deliberately
    /// session-local and is cleared on restore, so it can never stand in for
    /// a restored host surface.
    last_resource_frame: Option<LegacyRenderResourceFrameV1>,
    /// Per-role raster cache for resource-backed scenes.  Minori animation
    /// changes draw geometry for one role at a time; retaining the other
    /// role surfaces avoids re-decoding the same PAZ image and re-running the
    /// full CPU renderer on every fixed tick.  The cache is session-local and
    /// invalidated on restore or when the role's bounded descriptors change.
    presentation_layers: BTreeMap<MinoriLayerRole, CachedMinoriLayer>,
    last_layer_sequence: u64,
    /// The complete transaction is retained until the Host returns a typed
    /// selection.  Keeping the transaction (instead of only its id) lets the
    /// family enforce the same item/parent/enabled boundary as the native
    /// Host, even when a caller bypasses a particular Host adapter.
    active_system_menu: Option<LegacySystemMenuTransactionV1>,
    active_confirmation: Option<ActiveMinoriConfirmation>,
    active_system_command: Option<ActiveMinoriSystemCommand>,
    active_text_input: Option<ActiveMinoriTextInput>,
    poisoned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MinoriConfirmationAction {
    Exit,
    ReturnTitle,
}

const MINORI_CONFIRMATION_TITLE: &str = "確認";
const MINORI_CONFIRMATION_ACCEPT_LABEL: &str = "是(Y)";
const MINORI_CONFIRMATION_CANCEL_LABEL: &str = "否(N)";

fn minori_confirmation_message(action: MinoriConfirmationAction) -> &'static str {
    match action {
        MinoriConfirmationAction::Exit => "終了してもよろしいですか?",
        MinoriConfirmationAction::ReturnTitle => {
            "ゲームを中断してメニューに戻ります。よろしいですか?"
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveMinoriConfirmation {
    confirmation_id: String,
    action: MinoriConfirmationAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveMinoriSystemCommand {
    command_id: String,
    command: LegacySystemCommandKindV1,
    /// Config closes directly back to gameplay, so the family must rebuild
    /// the retained gameplay presentation while the Host applies fullscreen.
    /// Native-menu window commands keep the existing retained scene.
    resume_gameplay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveMinoriTextInput {
    prompt_id: String,
    slot: u32,
    max_bytes: u32,
}

#[derive(Default)]
pub struct MinoriRuntimeProvider {
    vfs: Option<Arc<dyn LegacyVfsReader>>,
    host_services: Option<LegacyFamilyHostServicesV9>,
    sessions: BTreeMap<String, MinoriSession>,
}

impl MinoriRuntimeProvider {
    pub fn with_vfs(vfs: Arc<dyn LegacyVfsReader>) -> Self {
        Self {
            vfs: Some(vfs),
            host_services: None,
            sessions: BTreeMap::new(),
        }
    }

    pub fn with_host_services(host_services: LegacyFamilyHostServicesV9) -> Self {
        Self {
            vfs: Some(Arc::clone(&host_services.vfs)),
            host_services: Some(host_services),
            sessions: BTreeMap::new(),
        }
    }

    pub fn has_active_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    fn vfs(&self) -> Result<&Arc<dyn LegacyVfsReader>, LegacyProviderError> {
        self.vfs.as_ref().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_RUNTIME_VFS",
                "Minori runtime has no explicitly bound VFS reader",
            )
        })
    }

    fn host_services(&self) -> Result<&LegacyFamilyHostServicesV9, LegacyProviderError> {
        self.host_services.as_ref().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                "Minori runtime has no explicitly bound current Family ABI host services",
            )
        })
    }
}

pub fn create_static_minori_provider(
    host_services: LegacyFamilyHostServicesV9,
) -> Result<Box<dyn LegacyRuntimeProvider>, LegacyProviderError> {
    let provider = MinoriRuntimeProvider::with_host_services(host_services);
    provider.descriptor().validate()?;
    Ok(Box::new(provider))
}

impl LegacyRuntimeProvider for MinoriRuntimeProvider {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor {
        LegacyFamilyPluginDescriptor {
            family_id: FamilyId(MINORI_FAMILY_ID.into()),
            plugin_id: "astra.emu.minori".into(),
            provider_id: MINORI_RUNTIME_PROVIDER_ID.into(),
            core_kind: astra_emu_family_api::LegacyFamilyCoreKind::Native,
            presentation_mode: astra_emu_family_api::LegacyFamilyPresentationMode::MultiLayer,
            engine_version: env!("CARGO_PKG_VERSION").into(),
            rustc_fingerprint: env!("ASTRA_MINORI_RUSTC_FINGERPRINT").into(),
            feature_fingerprint: env!("ASTRA_MINORI_FEATURE_FINGERPRINT").into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            supported_formats: vec![
                "minori.sc".into(),
                "minori.paz".into(),
                "minori.ani".into(),
                "minori.sqz".into(),
            ],
            permissions: vec![
                "vfs.read".into(),
                "surface.write".into(),
                "hook.invoke".into(),
                "media.submit".into(),
                "writable_file".into(),
            ],
            report_redaction: "astra.emu.redaction.v1".into(),
            license: "MPL-2.0".into(),
        }
    }

    fn probe(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, LegacyProviderError> {
        ctx.validate()?;
        let candidates = request
            .candidate_uris
            .iter()
            .filter(|uri| uri.starts_with("minori:/scr/") && uri.ends_with(".sc"))
            .collect::<Vec<_>>();
        let candidate = candidates
            .iter()
            .find(|uri| uri.eq_ignore_ascii_case("minori:/scr/test.sc"))
            .copied()
            .or_else(|| (candidates.len() == 1).then(|| candidates[0]))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_PROBE_ENTRY",
                    "probe requires one unambiguous Minori entry script",
                )
            })?;
        let (_, identity, _) = load_script_uri(self.vfs()?, &request.root_mount_id, candidate)?;
        let marker_match =
            request.marker_hashes.is_empty() || request.marker_hashes.contains(&identity);
        Ok(LegacyProbeReport {
            family_id: FamilyId(MINORI_FAMILY_ID.into()),
            confidence_permyriad: if marker_match { 10_000 } else { 0 },
            markers: if marker_match {
                vec!["minori.sc.cp932".into(), "minori.sc.command_stream".into()]
            } else {
                Vec::new()
            },
            blockers: Vec::new(),
            content_identity: identity,
        })
    }

    fn open(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyOpenRequest,
    ) -> Result<LegacyRuntimeSessionId, LegacyProviderError> {
        ctx.validate()?;
        validate_symbol("session_id", &request.requested_session_id.0)?;
        validate_symbol("compatibility_profile", &request.compatibility_profile)?;
        if request.fixed_delta_ns == 0 || request.fixed_delta_ns > 1_000_000_000 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FIXED_DELTA",
                "fixed delta is outside 1ns..=1s",
            ));
        }
        if self.sessions.contains_key(&request.requested_session_id.0) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_DUPLICATE",
                "session id is already active",
            ));
        }
        validate_script_uri(&request.script_uri)?;
        let profile_fingerprint = profile_fingerprint(ctx, &request)?;
        let (script_uri, script_hash, script) =
            load_script_uri(self.vfs()?, &ctx.mount_set_id, &request.script_uri)?;
        let entry_script_uri = script_uri.clone();
        match request
            .family_options
            .get("astra.resource_audit")
            .map(String::as_str)
        {
            None => {}
            Some("full") => {
                let (resource_count, audit_hash) =
                    audit_script_resources(self.vfs()?, &ctx.mount_set_id)?;
                tracing::info!(
                    target: "astra_emu_minori::resource",
                    event = "astra_emu_minori_script_resource_audit_completed",
                    resource_count,
                    audit_hash = %audit_hash,
                    "validated every bounded script resource reference"
                );
            }
            Some(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_RESOURCE_AUDIT_POLICY",
                    "resource audit policy must be full when present",
                ));
            }
        }
        let title_launch = match request
            .family_options
            .get("astra.launch_entry_explicit")
            .map(String::as_str)
        {
            Some("false") => true,
            Some("true") | None => false,
            Some(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_LAUNCH_MODE",
                    "launch entry explicitness must be true or false",
                ));
            }
        };
        tracing::debug!(
            target: "astra_emu_minori::runtime",
            event = "astra_emu_minori_launch_profile_received",
            launch_marker = request
                .family_options
                .get("astra.launch_entry_explicit")
                .map(String::as_str)
                .unwrap_or("missing"),
            title_launch,
            "received the explicit Minori launch profile"
        );
        let message_voice_durations =
            decode_message_voice_durations(self.vfs()?, &ctx.mount_set_id, &script)?;
        let mut vm = MinoriVm::new(script_uri, script_hash, script, request.session_seed)
            .map_err(runtime_error)?;
        vm.set_message_voice_durations(message_voice_durations)
            .map_err(runtime_error)?;
        if title_launch {
            vm.begin_title_launch().map_err(runtime_error)?;
            tracing::debug!(
                target: "astra_emu_minori::system_ui",
                event = "astra_emu_minori_title_session_initialized",
                page = system_page_name(vm.state().system_ui.page),
                terminal = vm.state().terminal,
                "initialized the title session without executing the entry script"
            );
        }
        let stage_size = match (
            request.family_options.get("astra.stage_width"),
            request.family_options.get("astra.stage_height"),
        ) {
            (None, None) => None,
            (Some(width), Some(height)) => {
                let width = width.parse::<u32>().map_err(|_| {
                    invalid("ASTRA_EMU_MINORI_STAGE_SIZE", "stage width is invalid")
                })?;
                let height = height.parse::<u32>().map_err(|_| {
                    invalid("ASTRA_EMU_MINORI_STAGE_SIZE", "stage height is invalid")
                })?;
                if !(320..=8192).contains(&width) || !(240..=8192).contains(&height) {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_STAGE_SIZE",
                        "stage dimensions are outside the supported bound",
                    ));
                }
                Some((width, height))
            }
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_STAGE_SIZE",
                    "stage dimensions must be supplied together",
                ));
            }
        };
        let collect_evidence_vm_trace = match request
            .family_options
            .get("astra.hosted_trace_profile")
            .map(String::as_str)
        {
            Some("evidence") => true,
            Some("shipping") | None => false,
            Some(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TRACE_PROFILE",
                    "hosted trace profile must be evidence or shipping",
                ));
            }
        };
        let global_progress_enabled = match request
            .family_options
            .get(MINORI_GLOBAL_PROGRESS_OPTION)
            .map(String::as_str)
        {
            None => false,
            Some(MINORI_WRITABLE_FILE_BINDING_ID) => true,
            Some(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_PROVIDER",
                    "global progress requires the explicitly bound v9 writable-file port",
                ));
            }
        };
        let config_storage_enabled = global_progress_enabled;
        let (persisted_config, quick_save_cursor) = if config_storage_enabled {
            let services = self.host_services()?.clone();
            load_persistent_config(
                services.writable_files.as_ref(),
                &request.requested_session_id,
                request.case_fingerprint,
                ctx.package_hash,
                profile_fingerprint,
            )?
        } else {
            (MinoriConfigState::default(), 0)
        };
        vm.set_persistent_config(persisted_config.clone())
            .map_err(runtime_error)?;
        let id = request.requested_session_id;
        self.sessions.insert(
            id.0.clone(),
            MinoriSession {
                case_fingerprint: request.case_fingerprint,
                package_hash: ctx.package_hash,
                profile_fingerprint,
                mount_set_id: ctx.mount_set_id.clone(),
                entry_script_uri,
                fixed_delta_ns: request.fixed_delta_ns,
                session_seed: request.session_seed,
                stage_size,
                vm,
                ephemeral_text: BTreeMap::new(),
                collect_evidence_vm_trace,
                evidence_contexts: BTreeMap::new(),
                evidence_vm_trace: BTreeSet::new(),
                restore_audio_pending: false,
                restore_presentation_pending: false,
                reported_system_page: None,
                reported_play_mode: None,
                reported_gallery_unlock_count: None,
                reported_choice_active: None,
                // `false` is the stable default and is intentionally not
                // emitted on the first tick. A persisted `true` setting still
                // produces an edge immediately, while a later true -> false
                // transition remains observable.
                reported_progress_in_background: Some(false),
                global_progress: MinoriGlobalProgressSession {
                    enabled: global_progress_enabled,
                    loaded: !global_progress_enabled,
                    persisted_unlocks: Vec::new(),
                },
                config_storage_enabled,
                config_persisted: persisted_config.clone(),
                quick_save_cursor,
                config_persisted_cursor: quick_save_cursor,
                last_quick_save_pc_line: None,
                resize_antialias: true,
                title_pointer_focus: None,
                save_slots: BTreeSet::new(),
                save_slot_comments: BTreeMap::new(),
                save_slot_lengths: BTreeMap::new(),
                save_slot_metadata: BTreeMap::new(),
                text_renderer: match stage_size {
                    Some((width, height)) => Some(
                        MinoriTextSurfaceRenderer::new(width, height)
                            .map_err(|code| invalid(code, "Minori text renderer setup failed"))?,
                    ),
                    None => None,
                },
                last_text_surface: None,
                last_gameplay_frame: None,
                published_layers: BTreeSet::new(),
                last_resource_frame: None,
                presentation_layers: BTreeMap::new(),
                last_layer_sequence: 0,
                active_system_menu: None,
                active_confirmation: None,
                active_system_command: None,
                active_text_input: None,
                poisoned: false,
            },
        );
        Ok(id)
    }

    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> Result<LegacyStepOutputV9, LegacyProviderError> {
        let fixed_step = input.tick_index;
        let services = self.host_services()?.clone();
        let vfs = Arc::clone(self.vfs()?);
        let staged = self.step_staged(ctx, session_id, input)?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        publish_v9_output(&services, &vfs, session_id, fixed_step, session, staged)
            .inspect_err(|_| session.poisoned = true)
    }

    fn shutdown(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
    ) -> Result<LegacyShutdownReport, LegacyProviderError> {
        self.shutdown_session_impl(ctx, session_id)
    }
}

fn profile_fingerprint(
    ctx: &LegacyRuntimeHostCtx,
    request: &LegacyOpenRequest,
) -> Result<Hash256, LegacyProviderError> {
    // The launch-mode marker selects whether the family opens its title or
    // gameplay entry; it is not part of the save-compatible installation
    // profile. A save made from the direct gameplay launch must remain
    // loadable after the host starts the same case through the title screen.
    let stable_family_options = request
        .family_options
        .iter()
        .filter(|(key, _)| key.as_str() != "astra.launch_entry_explicit")
        .collect::<Vec<_>>();
    let identity = (
        &ctx.profile,
        &request.compatibility_profile,
        stable_family_options,
    );
    let bytes = postcard::to_allocvec(&identity).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_PROFILE_IDENTITY",
            "Minori profile identity could not be encoded",
        )
    })?;
    Ok(Hash256::from_sha256(&bytes))
}

impl MinoriRuntimeProvider {
    #[cfg(test)]
    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError> {
        self.step_staged(ctx, session_id, input)
    }

    fn step_staged(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        mut input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError> {
        ctx.validate()?;
        input.validate()?;
        let vfs = Arc::clone(self.vfs()?);
        let host_services = self.host_services.clone();
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_POISONED",
                "poisoned session cannot continue",
            ));
        }
        if input.delta_ns != session.fixed_delta_ns || input.session_seed != session.session_seed {
            return Err(invalid(
                "ASTRA_EMU_MINORI_STEP_IDENTITY",
                "step timing or seed does not match the open session",
            ));
        }
        if session
            .vm
            .state()
            .fixed_tick
            .checked_add(1)
            .is_none_or(|expected| input.tick_index != expected)
        {
            session.poisoned = true;
            return Err(runtime_error(MinoriRuntimeError::State));
        }
        if session.active_system_command.is_some() || input.system_command.is_some() {
            let audio_commands = take_restore_audio_commands(session, &vfs)?;
            let services = host_services.as_ref().ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                    "system command requires current Family ABI Host services",
                )
            })?;
            return handle_system_command_step(
                services,
                session_id,
                session,
                &vfs,
                &input,
                audio_commands,
            );
        }
        if session.active_confirmation.is_some() || input.confirmation.is_some() {
            let audio_commands = take_restore_audio_commands(session, &vfs)?;
            return handle_confirmation_step(session, &vfs, &input, audio_commands);
        }
        if session.active_text_input.is_some() || input.text_input.is_some() {
            let audio_commands = take_restore_audio_commands(session, &vfs)?;
            let services = host_services.as_ref().ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                    "text input requires current Family ABI Host services",
                )
            })?;
            return handle_text_input_step(
                services,
                session_id,
                session,
                &vfs,
                &input,
                audio_commands,
            );
        }
        for edge in &input.input_edges {
            if edge.control == MINORI_CONTROL_KEY {
                session.vm.set_control_pressed(edge.pressed);
            } else if edge.control == MINORI_POINTER_X {
                session
                    .vm
                    .set_pointer_axis('x', edge.value)
                    .map_err(runtime_error)?;
            } else if edge.control == MINORI_POINTER_Y {
                session
                    .vm
                    .set_pointer_axis('y', edge.value)
                    .map_err(runtime_error)?;
            } else if edge.control == MINORI_POINTER_PRIMARY {
                session.vm.set_pointer_primary_pressed(edge.pressed);
            }
        }
        if session.vm.state().system_ui.page == MinoriSystemPage::Title {
            let pointer_axis = input
                .input_edges
                .iter()
                .any(|edge| matches!(edge.control.as_str(), MINORI_POINTER_X | MINORI_POINTER_Y));
            let pointer_primary = input
                .input_edges
                .iter()
                .any(|edge| edge.control == MINORI_POINTER_PRIMARY && edge.pressed);
            if pointer_axis || pointer_primary {
                session.title_pointer_focus = title_menu_focus_at(
                    session.vm.title_variant(),
                    session.vm.state().system_ui.pointer_x,
                    session.vm.state().system_ui.pointer_y,
                );
            } else if input.input_edges.iter().any(|edge| edge.pressed) {
                session.title_pointer_focus = None;
            }
        } else {
            session.title_pointer_focus = None;
        }
        let system_menu_request = input.system_menu.clone();
        if let Some(request) = system_menu_request.as_ref() {
            if request.action == LegacySystemMenuActionV1::Open {
                if let Some(pointer_x) = request.pointer_x {
                    session
                        .vm
                        .set_pointer_axis('x', pointer_x as f32)
                        .map_err(runtime_error)?;
                }
                if let Some(pointer_y) = request.pointer_y {
                    session
                        .vm
                        .set_pointer_axis('y', pointer_y as f32)
                        .map_err(runtime_error)?;
                }
            }
        }
        if !input.provider_results.is_empty() {
            session.poisoned = true;
            return Err(invalid(
                "ASTRA_EMU_MINORI_PROVIDER_RESULT_REMOVED",
                "current Family ABI Minori does not accept provider-result payloads",
            ));
        }
        if session.global_progress.enabled && !session.global_progress.loaded {
            let writable_files = host_services
                .as_ref()
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                        "global progress requires current Family ABI Host services",
                    )
                })?
                .writable_files
                .as_ref();
            load_global_progress(writable_files, session_id, session)?;
        }
        let mut restore_audio = match take_restore_audio_commands(session, &vfs) {
            Ok(commands) => commands,
            Err(error) => {
                session.poisoned = true;
                return Err(error);
            }
        };
        let window_close_requested = input
            .input_edges
            .iter()
            .any(|edge| edge.pressed && edge.control == MINORI_WINDOW_CLOSE_CONTROL);
        if window_close_requested {
            if input.input_edges.len() != 1
                || input.system_menu.is_some()
                || !input.await_results.is_empty()
                || !input.provider_results.is_empty()
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_WINDOW_CLOSE_INPUT_AMBIGUOUS",
                    "window close must be the only input and completion in its tick",
                ));
            }
            let services = host_services.as_ref().ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                    "window close requires current Family ABI Host services",
                )
            })?;
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            let confirmation_id = format!("minori.confirmation.window_close.{sequence}");
            services.confirmations.publish(
                &session_id.0,
                LegacyConfirmationTransactionV1 {
                    sequence,
                    confirmation_id: confirmation_id.clone(),
                    // These strings are the native Minori close-dialog
                    // contract observed in the original Windows build.  The
                    // host owns the modal presentation, but the family owns
                    // the Japanese wording and button order.
                    title: MINORI_CONFIRMATION_TITLE.into(),
                    message: minori_confirmation_message(MinoriConfirmationAction::Exit).into(),
                    accept_label: MINORI_CONFIRMATION_ACCEPT_LABEL.into(),
                    cancel_label: MINORI_CONFIRMATION_CANCEL_LABEL.into(),
                },
            )?;
            session.active_confirmation = Some(ActiveMinoriConfirmation {
                confirmation_id,
                action: MinoriConfirmationAction::Exit,
            });
            session
                .vm
                .advance_provider_tick(input.tick_index)
                .map_err(runtime_error)?;
            return if session.vm.state().system_ui.page == MinoriSystemPage::None {
                idle_system_menu_output(session, &vfs, &input, restore_audio, None)
            } else {
                system_ui_output(session, &vfs, &input, restore_audio)
            };
        }
        if let Some(request) = system_menu_request.as_ref() {
            return handle_system_menu_request(
                host_services.as_ref().ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                        "system menu requires current Family ABI Host services",
                    )
                })?,
                session_id,
                session,
                &vfs,
                &input,
                request,
                restore_audio,
            );
        }
        if session.active_system_menu.is_some() {
            if !input.input_edges.is_empty()
                || !input.await_results.is_empty()
                || !input.provider_results.is_empty()
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_MENU_INPUT_WHILE_ACTIVE",
                    "active native system menu must suspend gameplay input and completions",
                ));
            }
            session
                .vm
                .advance_provider_tick(input.tick_index)
                .map_err(runtime_error)?;
            return idle_system_menu_output(session, &vfs, &input, restore_audio, None);
        }
        if session.vm.state().system_ui.page == MinoriSystemPage::None
            && backlog_wheel_direction(&input)? == Some(-1)
            && !session.vm.state().backlog.is_empty()
        {
            if !input.await_results.is_empty() || !input.provider_results.is_empty() {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_BACKLOG_RESULT_UNEXPECTED",
                    "opening backlog cannot consume an await or provider result",
                ));
            }
            session.vm.open_backlog().map_err(runtime_error)?;
            session
                .vm
                .advance_system_tick(input.tick_index)
                .map_err(runtime_error)?;
            return system_ui_output(session, &vfs, &input, restore_audio);
        }
        let save_menu_pressed = input
            .input_edges
            .iter()
            .any(|edge| edge.pressed && edge.control == "escape");
        if save_menu_pressed && session.vm.state().system_ui.page == MinoriSystemPage::None {
            let escape_wait_completion = if input.await_results.len() == 1 {
                let wait_token = session.vm.state().wait.as_ref().map(wait_token);
                let result = &input.await_results[0];
                wait_token.is_some_and(|token| {
                    result.token_id == token
                        && result.status == "completed"
                        && result.payload_len == 0
                })
            } else {
                false
            };
            if input.input_edges.iter().filter(|edge| edge.pressed).count() != 1
                || (!input.await_results.is_empty() && !escape_wait_completion)
                || !input.provider_results.is_empty()
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SAVE_INPUT_AMBIGUOUS",
                    "save menu opening cannot share a tick with another completion or input",
                ));
            }
            // Escape is also a valid completion for the current message/input
            // wait.  Consume only that exact owner-side completion before
            // entering the system page; unrelated results remain blocking.
            if escape_wait_completion {
                input.await_results.clear();
            }
            match session.vm.state().wait.as_ref() {
                Some(
                    MinoriWaitState::Input { .. }
                    | MinoriWaitState::Time { .. }
                    | MinoriWaitState::Voice { .. },
                ) => {}
                Some(MinoriWaitState::Choice { .. }) => {
                    session.poisoned = true;
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_SAVE_CHOICE_ACTIVE",
                        "save menu cannot open while a choice is active",
                    ));
                }
                Some(MinoriWaitState::Media { .. }) => {
                    session.poisoned = true;
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_SAVE_MEDIA_ACTIVE",
                        "save menu cannot open while a movie is active",
                    ));
                }
                Some(MinoriWaitState::AxisScroll { .. })
                | Some(MinoriWaitState::LinearScroll { .. })
                | Some(MinoriWaitState::CharacterTransition { .. })
                | Some(MinoriWaitState::Presentation { .. })
                | Some(MinoriWaitState::Provider { .. })
                | None => {
                    session.poisoned = true;
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_SAVE_WAIT_STATE",
                        "save menu requires a stable message wait",
                    ));
                }
            }
            session.vm.open_save_page().map_err(runtime_error)?;
            refresh_save_slots(
                host_services
                    .as_ref()
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                            "save menu requires current Family ABI Host services",
                        )
                    })?
                    .writable_files
                    .as_ref(),
                session_id,
                session,
            )?;
            session
                .vm
                .advance_system_tick(input.tick_index)
                .map_err(runtime_error)?;
            return system_ui_output(session, &vfs, &input, restore_audio);
        }
        let mut started_game = false;
        if session.vm.state().system_ui.page != MinoriSystemPage::None {
            if matches!(
                session.vm.state().system_ui.page,
                MinoriSystemPage::Save | MinoriSystemPage::Load
            ) {
                refresh_save_slots(
                    host_services
                        .as_ref()
                        .ok_or_else(|| {
                            invalid(
                                "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                                "save/load pages require current Family ABI Host services",
                            )
                        })?
                        .writable_files
                        .as_ref(),
                    session_id,
                    session,
                )?;
            }
            let backlog_replay_completion = if session.vm.state().system_ui.page
                == MinoriSystemPage::Backlog
                && input.provider_results.is_empty()
                && input.await_results.len() == 1
            {
                let token_id = wait_token(
                    session
                        .vm
                        .state()
                        .wait
                        .as_ref()
                        .ok_or_else(|| runtime_error(MinoriRuntimeError::Backlog))?,
                );
                input.await_results[0].token_id == token_id
                    && input.await_results[0].status == "completed"
                    && input.await_results[0].payload_len == 0
            } else {
                false
            };
            if (!input.await_results.is_empty() && !backlog_replay_completion)
                || !input.provider_results.is_empty()
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_RESULT_UNEXPECTED",
                    "system UI cannot consume a result",
                ));
            }
            if backlog_replay_completion {
                input.await_results.clear();
            }
            let title_start = session.vm.state().system_ui.page == MinoriSystemPage::Title;
            let mut action = match apply_system_ui_input(&mut session.vm, &input) {
                Ok(action) => action,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            if backlog_replay_completion {
                action = MinoriSystemUiAction::ReplayBacklogVoice;
            }
            tracing::debug!(
                target: "astra_emu_minori::system_ui",
                event = "astra_emu_minori_system_ui_input_applied",
                page = system_page_name(session.vm.state().system_ui.page),
                focus_index = session.vm.state().system_ui.focus_index,
                action = system_ui_action_name(action),
                input_edge_count = input.input_edges.len(),
                "applied bounded system UI input"
            );
            if let MinoriSystemUiAction::GalleryReplayStart(_)
            | MinoriSystemUiAction::GalleryMovieStart(_) = action
            {
                let (target, page) = match action {
                    MinoriSystemUiAction::GalleryReplayStart(index) => (
                        MINORI_GALLERY_REPLAY_SCRIPT_TARGETS
                            .get(usize::try_from(index).map_err(|_| {
                                invalid(
                                    "ASTRA_EMU_MINORI_GALLERY_REPLAY_FOCUS",
                                    "flashback gallery focus cannot be represented",
                                )
                            })?)
                            .copied()
                            .ok_or_else(|| {
                                invalid(
                                    "ASTRA_EMU_MINORI_GALLERY_REPLAY_FOCUS",
                                    "flashback gallery focus is outside the verified range",
                                )
                            })?,
                        MinoriSystemPage::GalleryReplay,
                    ),
                    MinoriSystemUiAction::GalleryMovieStart(index) => (
                        MINORI_GALLERY_MOVIE_SCRIPT_TARGETS
                            .get(usize::try_from(index).map_err(|_| {
                                invalid(
                                    "ASTRA_EMU_MINORI_GALLERY_MOVIE_FOCUS",
                                    "movie gallery focus cannot be represented",
                                )
                            })?)
                            .copied()
                            .ok_or_else(|| {
                                invalid(
                                    "ASTRA_EMU_MINORI_GALLERY_MOVIE_FOCUS",
                                    "movie gallery focus is outside the verified range",
                                )
                            })?,
                        MinoriSystemPage::GalleryMovie,
                    ),
                    _ => unreachable!("gallery start action was matched above"),
                };
                let (script_uri, script_hash, script) =
                    load_script(&vfs, &session.mount_set_id, target)?;
                session
                    .vm
                    .set_system_page(MinoriSystemPage::None, 0)
                    .map_err(runtime_error)?;
                replace_vm_script(
                    &vfs,
                    &session.mount_set_id,
                    &mut session.vm,
                    script_uri,
                    script_hash,
                    script,
                )?;
                tracing::info!(
                    target: "astra_emu_minori::system_ui",
                    event = "astra_emu_minori_gallery_script_started",
                    page = system_page_name(page),
                    script_identity = %Hash256::from_sha256(target.as_bytes()),
                    "started a verified gallery script"
                );
                action = MinoriSystemUiAction::StartGame;
            }
            if action == MinoriSystemUiAction::StartGame && title_start {
                let (script_uri, script_hash, script) =
                    load_script_uri(&vfs, &session.mount_set_id, &session.entry_script_uri)?;
                replace_vm_script(
                    &vfs,
                    &session.mount_set_id,
                    &mut session.vm,
                    script_uri,
                    script_hash,
                    script,
                )?;
                tracing::debug!(
                    target: "astra_emu_minori::runtime",
                    event = "astra_emu_minori_title_entry_reloaded",
                    script_identity = %Hash256::from_sha256(session.entry_script_uri.as_bytes()),
                    "reloaded the verified title entry before starting a new game"
                );
            }
            if action != MinoriSystemUiAction::StartGame {
                if let MinoriSystemUiAction::SaveSlot(slot) = action {
                    let services = host_services.as_ref().ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                            "save requires current Family ABI Host services",
                        )
                    })?;
                    let sequence = session
                        .vm
                        .allocate_effect_sequence()
                        .map_err(runtime_error)?;
                    let prompt_id = format!("minori.text_input.save_comment.{slot}.{sequence}");
                    let text_input = LegacyTextInputTransactionV1 {
                        sequence,
                        prompt_id: prompt_id.clone(),
                        title: "SAVE".into(),
                        label: "Comment".into(),
                        initial_value: session
                            .save_slot_comments
                            .get(&slot)
                            .cloned()
                            .unwrap_or_default(),
                        accept_label: "OK".into(),
                        cancel_label: "Cancel".into(),
                        max_bytes: MINORI_SAVE_COMMENT_MAX_BYTES as u32,
                    };
                    services
                        .text_inputs
                        .publish(&session_id.0, text_input.clone())?;
                    session.active_text_input = Some(ActiveMinoriTextInput {
                        prompt_id,
                        slot,
                        max_bytes: text_input.max_bytes,
                    });
                    session
                        .vm
                        .advance_provider_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    return system_ui_output(session, &vfs, &input, restore_audio);
                }
                if let MinoriSystemUiAction::LoadSlot(slot) = action {
                    load_slot(
                        host_services
                            .as_ref()
                            .ok_or_else(|| {
                                invalid(
                                    "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                                    "load requires current Family ABI Host services",
                                )
                            })?
                            .writable_files
                            .as_ref(),
                        &vfs,
                        session_id,
                        session,
                        slot,
                        input.tick_index,
                    )?;
                    return gameplay_resume_output(session, &vfs, &input, restore_audio);
                }
                if action == MinoriSystemUiAction::CloseGameplaySystemPage {
                    session
                        .vm
                        .advance_system_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    session
                        .vm
                        .close_gameplay_system_page()
                        .map_err(runtime_error)?;
                    return gameplay_resume_output(session, &vfs, &input, restore_audio);
                }
                let commands = match action {
                    MinoriSystemUiAction::ReplayBacklogVoice => {
                        session.vm.replay_backlog_voice().map_err(runtime_error)?
                    }
                    MinoriSystemUiAction::PresentWithAudioRefresh => session
                        .vm
                        .config_audio_param_commands()
                        .map_err(runtime_error)?,
                    MinoriSystemUiAction::PresentAfterConfigClose { .. } => session
                        .vm
                        .close_config_audio_commands()
                        .map_err(runtime_error)?,
                    MinoriSystemUiAction::PresentWithAudioTest(bus) => session
                        .vm
                        .config_test_audio_commands(bus)
                        .map_err(runtime_error)?,
                    MinoriSystemUiAction::GalleryBgmPlay => {
                        let resource_uri = gallery_bgm_track_resource_uri(
                            session.vm.state().system_ui.focus_index,
                        )?;
                        session
                            .vm
                            .gallery_bgm_play(resource_uri)
                            .map_err(runtime_error)?
                    }
                    MinoriSystemUiAction::GalleryBgmStop => {
                        session.vm.gallery_bgm_stop().map_err(runtime_error)?
                    }
                    _ => Vec::new(),
                };
                if !commands.is_empty() {
                    append_validated_audio_commands(
                        &vfs,
                        &session.mount_set_id,
                        commands.iter(),
                        session.vm.state(),
                        &mut restore_audio,
                    )?;
                }
                if let MinoriSystemUiAction::PresentAfterConfigClose { fullscreen } = action {
                    if let Some(enabled) = fullscreen {
                        let sequence = session
                            .vm
                            .allocate_effect_sequence()
                            .map_err(runtime_error)?;
                        let command_id =
                            format!("minori.system_command.config_fullscreen.{sequence}");
                        let command = LegacySystemCommandKindV1::SetFullscreen { enabled };
                        let services = host_services.as_ref().ok_or_else(|| {
                            invalid(
                                "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                                "config fullscreen requires current Family ABI Host services",
                            )
                        })?;
                        services.system_commands.publish(
                            &session_id.0,
                            LegacySystemCommandTransactionV1 {
                                sequence,
                                command_id: command_id.clone(),
                                command,
                            },
                        )?;
                        session.active_system_command = Some(ActiveMinoriSystemCommand {
                            command_id,
                            command,
                            resume_gameplay: session.vm.state().system_ui.page
                                == MinoriSystemPage::None,
                        });
                        session
                            .vm
                            .advance_provider_tick(input.tick_index)
                            .map_err(runtime_error)?;
                        return if session.vm.state().system_ui.page == MinoriSystemPage::None {
                            gameplay_resume_output(session, &vfs, &input, restore_audio)
                        } else {
                            system_ui_output(session, &vfs, &input, restore_audio)
                        };
                    }
                    if session.config_storage_enabled {
                        let services = host_services.as_ref().ok_or_else(|| {
                            invalid(
                                "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                                "config persistence requires current Family ABI Host services",
                            )
                        })?;
                        store_persistent_config_if_changed(
                            services.writable_files.as_ref(),
                            session_id,
                            session,
                        )?;
                    }
                    if session.vm.state().system_ui.page == MinoriSystemPage::None {
                        session
                            .vm
                            .advance_provider_tick(input.tick_index)
                            .map_err(runtime_error)?;
                        return gameplay_resume_output(session, &vfs, &input, restore_audio);
                    }
                }
                session
                    .vm
                    .advance_system_tick(input.tick_index)
                    .map_err(runtime_error)?;
                if action == MinoriSystemUiAction::CloseBacklog {
                    session.vm.close_backlog().map_err(runtime_error)?;
                    return gameplay_resume_output(session, &vfs, &input, restore_audio);
                }
                if action == MinoriSystemUiAction::Exit {
                    session
                        .vm
                        .terminate_system_session()
                        .map_err(runtime_error)?;
                }
                return system_ui_output(session, &vfs, &input, restore_audio);
            }
            started_game = true;
            session.restore_presentation_pending = false;
            session.last_gameplay_frame = None;
            session.last_text_surface = None;
            session.last_resource_frame = None;
            session.presentation_layers.clear();
        }
        // Control is part of an eligible message's Host-owned input wait, so
        // pressing it completes that wait instead of replacing an already
        // published token. Rebinding here would publish the same token twice
        // and violate RuntimeWorld AwaitQueue uniqueness. Play-mode changes
        // still request an explicit rebind at their owning action below.
        let mut play_mode_wait_rebound = false;
        let game_menu_mode_pressed = input.input_edges.iter().any(|edge| {
            edge.control == MINORI_POINTER_PRIMARY
                && edge.pressed
                && (MINORI_GAME_MENU_PLAY_MODE_LEFT..MINORI_GAME_MENU_PLAY_MODE_RIGHT)
                    .contains(&session.vm.state().system_ui.pointer_x)
                && (MINORI_GAME_MENU_PLAY_MODE_TOP..MINORI_GAME_MENU_PLAY_MODE_BOTTOM)
                    .contains(&session.vm.state().system_ui.pointer_y)
        });
        if game_menu_mode_pressed {
            if !input.provider_results.is_empty() || input.await_results.len() > 1 {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_PLAY_MODE_RESULT_UNEXPECTED",
                    "play-mode menu input cannot consume an unexpected completion",
                ));
            }
            if let Some(result) = input.await_results.first() {
                let expected = session
                    .vm
                    .state()
                    .wait
                    .as_ref()
                    .map(wait_token)
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_MINORI_PLAY_MODE_RESULT_UNEXPECTED",
                            "play-mode menu input completed without an active wait",
                        )
                    })?;
                if result.token_id != expected
                    || result.status != "completed"
                    || result.payload_len != 0
                {
                    session.poisoned = true;
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_PLAY_MODE_RESULT_UNEXPECTED",
                        "play-mode menu input completion does not match the active wait",
                    ));
                }
                // The host generated this completion because the same primary
                // click also satisfies the message wait.  Play-mode toggling
                // is intentionally out-of-band, so consume the owner-side
                // completion without advancing the script.
                input.await_results.clear();
            }
            play_mode_wait_rebound |= session
                .vm
                .toggle_preferred_play_mode()
                .map_err(runtime_error)?;
        }
        // A choice is a modal input wait.  Arrow edges only move the cursor;
        // they never resolve the wait.  Enter/Space is accepted as a direct
        // provider input for embedders that do not translate physical input
        // into an await result first.  The normal host path supplies the
        // await result and therefore rejects a duplicate confirm edge below.
        let mut choice_moved = false;
        let mut choice_committed = false;
        if let Some(MinoriWaitState::Choice { token_id }) = session.vm.state().wait.clone() {
            for edge in input.input_edges.iter().filter(|edge| edge.pressed) {
                let direction = choice_direction(&edge.control);
                if let Some(direction) = direction {
                    session.vm.move_choice(direction).map_err(runtime_error)?;
                    choice_moved = true;
                }
            }
            let confirm_pressed = input.input_edges.iter().any(|edge| {
                edge.pressed
                    && MINORI_CHOICE_CONFIRM_CONTROLS
                        .iter()
                        .any(|control| *control == edge.control)
            });
            if confirm_pressed && !input.await_results.is_empty() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_CHOICE_DUPLICATE_COMPLETION",
                    "choice confirm edge and await result complete the same choice wait",
                ));
            }
            if confirm_pressed && input.await_results.is_empty() {
                session.vm.resolve_wait(&token_id).map_err(runtime_error)?;
                session.vm.commit_choice().map_err(runtime_error)?;
                choice_committed = true;
            }
        }
        // The host normally translates an input edge that satisfies an input
        // wait into an ordered `LegacyAwaitResult`.  Static callers may use the
        // family provider directly, however, so the provider also accepts the
        // canonical key edge and resolves the same wait itself.  Edges that do
        // not satisfy a current input wait are still consumed as physical
        // state notifications; they must never be silently interpreted as a
        // semantic action or make an otherwise valid tick fail.
        if let Some(MinoriWaitState::Input { token_id } | MinoriWaitState::Voice { token_id, .. }) =
            session
                .vm
                .state()
                .wait
                .clone()
                .filter(|_| !game_menu_mode_pressed)
        {
            if !input.await_results.is_empty()
                && input.input_edges.iter().any(|edge| {
                    edge.pressed
                        && MINORI_MESSAGE_INPUT_CONTROLS
                            .iter()
                            .any(|control| *control == edge.control)
                        // The Manager deliberately retains Minori primary
                        // clicks so the play-mode hitbox can be handled by
                        // `game_menu_mode_pressed`.  A click elsewhere still
                        // completes the host-owned await result and must not
                        // be treated as a duplicate completion.
                        && (edge.control != MINORI_POINTER_PRIMARY || game_menu_mode_pressed)
                })
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_INPUT_DUPLICATE_COMPLETION",
                    "input edge and await result complete the same input wait",
                ));
            }
            if input.await_results.is_empty()
                && input.input_edges.iter().any(|edge| {
                    edge.pressed
                        && MINORI_MESSAGE_INPUT_CONTROLS
                            .iter()
                            .any(|control| *control == edge.control)
                })
            {
                session.vm.resolve_wait(&token_id).map_err(runtime_error)?;
            }
        }
        if !input.provider_results.is_empty() {
            let Some(MinoriWaitState::Provider {
                token_id,
                request_id,
            }) = session.vm.state().wait.clone()
            else {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_PROVIDER_RESULT_UNEXPECTED",
                    "provider result was supplied without an active provider wait",
                ));
            };
            if !input.await_results.is_empty()
                || input.provider_results.len() != 1
                || input.provider_results[0].request_id != request_id
                || input.provider_results[0].status != "completed"
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_PROVIDER_RESULT_MISMATCH",
                    "provider result does not match the active provider wait",
                ));
            }
            session.vm.resolve_wait(&token_id).map_err(runtime_error)?;
        }
        if input.input_edges.is_empty()
            && input.await_results.is_empty()
            && input.provider_results.is_empty()
            && !session.vm.state().has_time_animated_presentation()
            && !session.restore_audio_pending
            && !session.restore_presentation_pending
        {
            if let Some(wait @ MinoriWaitState::Input { .. }) = session.vm.state().wait.clone() {
                // A message/input wait with no edge, completion, timer or
                // animation cannot change family state or presentation. The
                // VM still owns the authoritative fixed-tick clock, however;
                // advance it before returning the bounded empty live output.
                // Skipping this transition would make the host tick advance
                // while the family tick stayed behind and the next step would
                // fail closed with ASTRA_EMU_MINORI_RUNTIME_STATE.
                session
                    .vm
                    .advance_waiting_tick(input.tick_index)
                    .map_err(runtime_error)?;
                return waiting_output(
                    session,
                    wait,
                    LegacyLiveOutput::default(),
                    None,
                    play_mode_wait_rebound,
                    &input,
                );
            }
        }
        let animation_enabled = session.vm.state().system_ui.config.animation;
        let screen_effect_enabled = session.vm.state().system_ui.config.screen_effect;
        let animated_effect = if animation_enabled && screen_effect_enabled {
            session
                .vm
                .advance_effect_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        let animated_firefly = if animation_enabled {
            session
                .vm
                .advance_firefly_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        let animated_secondary_effect = if animation_enabled && screen_effect_enabled {
            session
                .vm
                .advance_secondary_effect_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        let animated_screen_shake = if animation_enabled && screen_effect_enabled {
            session
                .vm
                .advance_screen_shake_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        let animated_axis_scroll = if animation_enabled {
            session
                .vm
                .advance_axis_scroll_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        let animated_character = if animation_enabled {
            session
                .vm
                .advance_character_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        let message_character_load = session
            .vm
            .advance_message_load_clock(input.delta_ns, animation_enabled)
            .map_err(runtime_error)?;
        let animated_character = message_character_load.or(animated_character);
        let animated_linear_scroll = if animation_enabled {
            session
                .vm
                .advance_linear_scroll_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        let animated_scroll_xf = if animation_enabled {
            session
                .vm
                .advance_scroll_xf_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        let animated_wscroll2 = if animation_enabled {
            session
                .vm
                .advance_wscroll2_clock(input.delta_ns)
                .map_err(runtime_error)?
        } else {
            None
        };
        if let Some(wait) = session.vm.state().wait.clone() {
            if input.await_results.is_empty() {
                let movie_to_skip = match &wait {
                    MinoriWaitState::Media { media_id, .. }
                        if session.vm.state().system_ui.control_pressed =>
                    {
                        session
                            .vm
                            .state()
                            .movie
                            .as_ref()
                            .filter(|movie| movie.skippable && movie.media_id == media_id.as_str())
                            .map(|movie| movie.media_id.clone())
                    }
                    _ => None,
                };
                session
                    .vm
                    .advance_waiting_tick(input.tick_index)
                    .map_err(runtime_error)?;
                if session.restore_presentation_pending {
                    return gameplay_resume_output(session, &vfs, &input, restore_audio.clone());
                }
                let mut resource_scenes: Vec<LegacySequenced<LegacyRenderResourceFrameV1>> =
                    animated_effect
                        .as_ref()
                        .map(|frame| {
                            effect_presentation(
                                &vfs,
                                &session.mount_set_id,
                                session.stage_size,
                                session.vm.state(),
                                frame,
                            )
                        })
                        .transpose()?
                        .into_iter()
                        .collect();
                if let Some(firefly_event) = &animated_firefly {
                    resource_scenes.push(firefly_event_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        firefly_event,
                    )?);
                }
                if let Some(axis_scroll) = &animated_axis_scroll {
                    resource_scenes.push(axis_scroll_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        axis_scroll,
                    )?);
                }
                if let Some(character) = &animated_character {
                    resource_scenes.push(character_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        character,
                    )?);
                }
                if let Some(linear_scroll) = &animated_linear_scroll {
                    resource_scenes.push(linear_scroll_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        linear_scroll,
                    )?);
                }
                if let Some(scroll_xf) = &animated_scroll_xf {
                    resource_scenes.push(scroll_xf_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        scroll_xf,
                    )?);
                }
                if let Some(wscroll2) = &animated_wscroll2 {
                    resource_scenes.push(wscroll2_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        wscroll2,
                    )?);
                }
                if let Some(secondary_effect) = &animated_secondary_effect {
                    resource_scenes.push(secondary_effect_event_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        secondary_effect,
                    )?);
                }
                if let Some(screen_shake) = &animated_screen_shake {
                    resource_scenes.push(screen_shake_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        screen_shake,
                    )?);
                }
                let mut live = LegacyLiveOutput {
                    resource_scenes,
                    audio_commands: restore_audio.clone(),
                    ..LegacyLiveOutput::default()
                };
                if let Some(playback_id) = movie_to_skip {
                    let sequence = session
                        .vm
                        .allocate_effect_sequence()
                        .map_err(runtime_error)?;
                    live.video.push(LegacySequenced {
                        sequence,
                        value: LegacyVideoCommandV1::Stop { playback_id },
                    });
                }
                if choice_moved {
                    let sequence = session
                        .vm
                        .allocate_effect_sequence()
                        .map_err(runtime_error)?;
                    let choice_event = append_choice_live_output(
                        session,
                        &vfs,
                        input.tick_index,
                        sequence,
                        &mut live,
                    )?;
                    return waiting_output(
                        session,
                        wait,
                        live,
                        Some(choice_event),
                        play_mode_wait_rebound,
                        &input,
                    );
                }
                return waiting_output(session, wait, live, None, play_mode_wait_rebound, &input);
            }
            let expected = wait_token(&wait);
            if input.await_results.len() != 1 {
                tracing::error!(
                    target: "astra_emu_minori::runtime",
                    event = "astra_emu_minori_await_result_count_mismatch",
                    fixed_tick = input.tick_index,
                    expected_token = %Hash256::from_sha256(expected.as_bytes()),
                    received_count = input.await_results.len(),
                    "await completion count does not match the active wait"
                );
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AWAIT_RESULT_COUNT",
                    "await result count does not match the active wait token",
                ));
            }
            let result = &input.await_results[0];
            if result.token_id != expected {
                tracing::error!(
                    target: "astra_emu_minori::runtime",
                    event = "astra_emu_minori_await_result_token_mismatch",
                    fixed_tick = input.tick_index,
                    expected_token = %Hash256::from_sha256(expected.as_bytes()),
                    received_token = %Hash256::from_sha256(result.token_id.as_bytes()),
                    "await completion token does not match the active wait"
                );
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AWAIT_RESULT_TOKEN",
                    "await result token does not match the active wait token",
                ));
            }
            if result.status != "completed" {
                tracing::error!(
                    target: "astra_emu_minori::runtime",
                    event = "astra_emu_minori_await_result_status_mismatch",
                    fixed_tick = input.tick_index,
                    expected_token = %Hash256::from_sha256(expected.as_bytes()),
                    "await completion status is not completed"
                );
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AWAIT_RESULT_STATUS",
                    "await result status is not completed",
                ));
            }
            session.vm.resolve_wait(expected).map_err(runtime_error)?;
            if matches!(wait, MinoriWaitState::Choice { .. }) {
                session.vm.commit_choice().map_err(runtime_error)?;
                choice_committed = true;
            }
        } else if !input.await_results.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_AWAIT_UNEXPECTED",
                "step supplied an await result without an active wait",
            ));
        }
        let before = session.vm.state().instruction_count;
        let event = match session.vm.step(input.tick_index, MAX_INSTRUCTIONS_PER_STEP) {
            Ok(event) => event,
            Err(error) => {
                let command = session.vm.take_executed_commands().last().cloned();
                if let Some(command) = command.as_ref() {
                    tracing::error!(
                        target: "astra_emu_minori::runtime",
                        event = "astra_emu_minori_command_failed",
                        script_hash = %command.script_hash,
                        command_ordinal = command.command_ordinal,
                        opcode_identity = %Hash256::from_sha256(command.opcode.as_bytes()),
                        diagnostic = runtime_error_code(&error),
                        "Minori command execution failed"
                    );
                }
                session.poisoned = true;
                let provider_error = if let Some(command) = command {
                    LegacyProviderError::invalid(
                        runtime_error_code(&error),
                        format!(
                            "{} (script_hash={}, command_ordinal={}, opcode_identity={})",
                            error,
                            command.script_hash,
                            command.command_ordinal,
                            Hash256::from_sha256(command.opcode.as_bytes())
                        ),
                    )
                } else {
                    runtime_error(error)
                };
                return Err(provider_error);
            }
        };
        let executed_commands = session.vm.take_executed_commands();
        if session.collect_evidence_vm_trace {
            if let Err(error) = record_evidence_commands(session, executed_commands) {
                session.poisoned = true;
                return Err(error);
            }
        }
        let chain_target = match &event {
            Some(MinoriVmEvent::Chain { target }) => Some(target.clone()),
            _ => None,
        };
        if let Some(target) = chain_target.as_deref() {
            let switch_result = load_script(&vfs, &session.mount_set_id, target).and_then(
                |(script_uri, script_hash, script)| {
                    replace_vm_script(
                        &vfs,
                        &session.mount_set_id,
                        &mut session.vm,
                        script_uri,
                        script_hash,
                        script,
                    )
                },
            );
            if let Err(error) = switch_result {
                session.poisoned = true;
                return Err(error);
            }
        }
        // A render-clock update is sampled before the VM command for this
        // tick.  A command such as `transition` can replace that native
        // presentation slot while the sampled frame is still in scope.  Do
        // not feed a frame from the retired shake state to the presenter: the
        // event belongs to the previous state and the replacement command is
        // authoritative for this tick.
        let animated_screen_shake = if session.vm.state().screen_shake.is_some() {
            animated_screen_shake
        } else {
            None
        };
        tracing::debug!(
            target: "astra_emu_minori::runtime",
            event = "astra_emu_minori_vm_tick",
            fixed_tick = input.tick_index,
            script_identity = %session.vm.state().script_hash,
            pc_line = session.vm.state().pc_line,
            instruction_count = session.vm.state().instruction_count,
            vm_event = minori_vm_event_name(event.as_ref()),
            waiting = session.vm.state().wait.is_some(),
            system_page = system_page_name(session.vm.state().system_ui.page),
            terminal = session.vm.state().terminal,
            "advanced the verified Minori VM"
        );
        if let Some((timer_ticks, milliseconds)) = event.as_ref().and_then(non_message_time_wait) {
            tracing::info!(
                target: "astra_emu_minori::runtime",
                event = "astra_emu_minori_wait_created",
                fixed_tick = input.tick_index,
                script_identity = %session.vm.state().script_hash,
                instruction_count = session.vm.state().instruction_count,
                timer_ticks,
                milliseconds,
                "created a non-message Minori time wait"
            );
        }
        if let Some(MinoriVmEvent::Message { wait, .. }) = event.as_ref() {
            let timer_ticks = match wait {
                MinoriWaitState::Time { timer_ticks, .. } => Some(*timer_ticks),
                _ => None,
            };
            tracing::info!(
                target: "astra_emu_minori::runtime",
                event = "astra_emu_minori_message_wait_created",
                fixed_tick = input.tick_index,
                script_identity = %session.vm.state().script_hash,
                instruction_count = session.vm.state().instruction_count,
                wait_kind = minori_wait_kind(wait),
                timer_ticks = timer_ticks.unwrap_or_default(),
                "created a Minori message wait"
            );
        }
        if let Some(MinoriVmEvent::Choice {
            option_hashes,
            selected_index,
            ..
        }) = event.as_ref()
        {
            tracing::info!(
                target: "astra_emu_minori::runtime",
                event = "astra_emu_minori_choice_wait_created",
                fixed_tick = input.tick_index,
                script_identity = %session.vm.state().script_hash,
                instruction_count = session.vm.state().instruction_count,
                option_count = option_hashes.len(),
                selected_index,
                "created a Minori choice wait"
            );
        }
        if matches!(
            event,
            Some(MinoriVmEvent::Chain { .. })
                | Some(MinoriVmEvent::Movie(_))
                | Some(MinoriVmEvent::Terminal)
        ) {
            tracing::info!(
                target: "astra_emu_minori::runtime",
                event = "astra_emu_minori_vm_boundary",
                fixed_tick = input.tick_index,
                script_identity = %session.vm.state().script_hash,
                pc_line = session.vm.state().pc_line,
                instruction_count = session.vm.state().instruction_count,
                vm_event = minori_vm_event_name(event.as_ref()),
                waiting = session.vm.state().wait.is_some(),
                system_page = system_page_name(session.vm.state().system_ui.page),
                terminal = session.vm.state().terminal,
                "reached a Minori VM control-flow boundary"
            );
        }
        let after = session.vm.state().instruction_count;
        let mut live = LegacyLiveOutput {
            clear_text: choice_committed,
            audio_commands: restore_audio,
            ..LegacyLiveOutput::default()
        };
        let returned_to_title =
            matches!(event, Some(MinoriVmEvent::Terminal)) && !session.vm.state().terminal;
        if returned_to_title {
            live.clear_text = true;
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            live.resource_scenes.push(LegacySequenced {
                sequence,
                value: describe_system_page(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    &session.vm,
                )?,
            });
        }
        let mut control_events = Vec::new();
        if let Some(frame) = &animated_effect {
            if !matches!(
                event,
                Some(
                    MinoriVmEvent::Stage(_)
                        | MinoriVmEvent::AxisScroll(_)
                        | MinoriVmEvent::Effect(_)
                        | MinoriVmEvent::EffectCleared { .. }
                        | MinoriVmEvent::Panel { .. }
                        | MinoriVmEvent::Character(_)
                )
            ) {
                live.resource_scenes.push(effect_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    frame,
                )?);
            }
        }
        if let Some(firefly_event) = &animated_firefly {
            let is_same_script_event = matches!(
                event,
                Some(MinoriVmEvent::Firefly(_))
                    | Some(MinoriVmEvent::FireflyCleared { .. })
                    | Some(MinoriVmEvent::AxisScroll(_))
                    | Some(MinoriVmEvent::Character(_))
            );
            if !is_same_script_event {
                live.resource_scenes.push(firefly_event_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    firefly_event,
                )?);
            }
        }
        if let Some(axis_scroll) = &animated_axis_scroll {
            if !matches!(event, Some(MinoriVmEvent::AxisScroll(_))) {
                live.resource_scenes.push(axis_scroll_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    axis_scroll,
                )?);
            }
        }
        if let Some(character) = &animated_character {
            if !matches!(event, Some(MinoriVmEvent::Character(_))) {
                live.resource_scenes.push(character_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    character,
                )?);
            }
        }
        if let Some(linear_scroll) = &animated_linear_scroll {
            if !matches!(event, Some(MinoriVmEvent::LinearScroll(_))) {
                live.resource_scenes.push(linear_scroll_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    linear_scroll,
                )?);
            }
        }
        if let Some(scroll_xf) = &animated_scroll_xf {
            if !matches!(
                event,
                Some(
                    MinoriVmEvent::ScrollXf(_)
                        | MinoriVmEvent::AxisScroll(_)
                        | MinoriVmEvent::Character(_)
                )
            ) {
                live.resource_scenes.push(scroll_xf_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    scroll_xf,
                )?);
            }
        }
        if let Some(wscroll2) = &animated_wscroll2 {
            if !matches!(
                event,
                Some(
                    MinoriVmEvent::Stage(_)
                        | MinoriVmEvent::AxisScroll(_)
                        | MinoriVmEvent::WScroll2(_)
                        | MinoriVmEvent::Panel { .. }
                        | MinoriVmEvent::Effect(_)
                        | MinoriVmEvent::EffectCleared { .. }
                        | MinoriVmEvent::Firefly(_)
                        | MinoriVmEvent::FireflyCleared { .. }
                        | MinoriVmEvent::Chain { .. }
                        | MinoriVmEvent::Character(_)
                )
            ) {
                live.resource_scenes.push(wscroll2_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    wscroll2,
                )?);
            }
        }
        if let Some(secondary_effect) = &animated_secondary_effect {
            if !matches!(
                event,
                Some(MinoriVmEvent::SecondaryEffect(_))
                    | Some(MinoriVmEvent::SecondaryEffectCleared { .. })
            ) {
                live.resource_scenes
                    .push(secondary_effect_event_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        secondary_effect,
                    )?);
            }
        }
        if let Some(screen_shake) = &animated_screen_shake {
            if !matches!(event, Some(MinoriVmEvent::ScreenShake(_))) {
                live.resource_scenes.push(screen_shake_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    screen_shake,
                )?);
            }
        }
        if let Some(MinoriVmEvent::Message {
            presentation_sequence,
            capture_sequence,
            text,
            speaker,
            ..
        }) = event
            .as_ref()
            .filter(|_| !session.vm.state().system_ui.message_panel_hidden)
        {
            if text.len() > MAX_EPHEMERAL_TEXT_BYTES
                || speaker
                    .as_ref()
                    .is_some_and(|value| value.len() > MAX_EPHEMERAL_TEXT_BYTES)
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                    "message or speaker exceeds the ephemeral text channel bound",
                ));
            }
            let lease_id = format!("minori.text.{}.{}", input.tick_index, capture_sequence);
            let presentation = LegacyTextPresentationLeaseV1 {
                lease_id: lease_id.clone(),
                presentation: match minori_message_presentation(
                    session.stage_size,
                    session.vm.state().system_ui.config.text_shadow,
                ) {
                    Ok(presentation) => presentation,
                    Err(error) => {
                        session.poisoned = true;
                        return Err(error);
                    }
                },
            };
            presentation.validate().inspect_err(|_| {
                session.poisoned = true;
            })?;
            if session
                .ephemeral_text
                .insert(
                    lease_id.clone(),
                    StagedEphemeralText {
                        lease_id: lease_id.clone(),
                        text: text.clone(),
                        speaker: speaker.clone(),
                        show_advance_indicator: true,
                    },
                )
                .is_some()
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
                    "ephemeral text lease id is duplicated",
                ));
            }
            live.text_presentations.push(LegacySequenced {
                sequence: *presentation_sequence,
                value: presentation,
            });
            live.text.push(StagedTextLease {
                sequence: *capture_sequence,
                lease_id,
                byte_len: text.len().try_into().map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                        "message length cannot be represented by the ABI",
                    )
                })?,
                source_ref: "minori.sc.message".into(),
            });
        }
        if let Some(MinoriVmEvent::Choice {
            sequence,
            option_hashes: _,
            selected_index: _,
        }) = &event
        {
            control_events.push(append_choice_live_output(
                session,
                &vfs,
                input.tick_index,
                *sequence,
                &mut live,
            )?);
        }
        if let Some(firefly_event) = &event {
            if matches!(
                firefly_event,
                MinoriVmEvent::Firefly(_) | MinoriVmEvent::FireflyCleared { .. }
            ) {
                live.resource_scenes.push(firefly_event_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    firefly_event,
                )?);
            }
        }
        if let Some(secondary_effect) = &event {
            if matches!(
                secondary_effect,
                MinoriVmEvent::SecondaryEffect(_) | MinoriVmEvent::SecondaryEffectCleared { .. }
            ) {
                live.resource_scenes
                    .push(secondary_effect_event_presentation(
                        &vfs,
                        &session.mount_set_id,
                        session.stage_size,
                        session.vm.state(),
                        secondary_effect,
                    )?);
            }
        }
        if let Some(MinoriVmEvent::ScreenShake(screen_shake)) = &event {
            live.resource_scenes.push(screen_shake_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                screen_shake,
            )?);
        }
        if let Some(MinoriVmEvent::Stage(stage)) = &event {
            let stage_size = session.stage_size.ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_SIZE",
                    "stage presentation requires explicit host dimensions",
                )
            })?;
            let sequence = session.vm.state().effect_sequence;
            let frame = match describe_stage_frame(
                &vfs,
                &session.mount_set_id,
                session.vm.state(),
                stage,
                stage_size,
            )
            .and_then(|mut frame| {
                if let Some(wscroll2) = session.vm.state().wscroll2.as_ref() {
                    apply_wscroll2_to_frame(&vfs, &session.mount_set_id, &mut frame, wscroll2)?;
                }
                append_secondary_effect_to_frame(
                    &vfs,
                    &session.mount_set_id,
                    session.vm.state(),
                    &mut frame,
                )?;
                apply_screen_shake_to_frame(session.vm.state(), &mut frame)?;
                frame.validate()?;
                Ok(frame)
            }) {
                Ok(frame) => frame,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            live.resource_scenes.push(LegacySequenced {
                sequence,
                value: frame,
            });
        }
        if let Some(MinoriVmEvent::AxisScroll(frame)) = &event {
            live.resource_scenes.push(axis_scroll_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::LinearScroll(frame)) = &event {
            live.resource_scenes.push(linear_scroll_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::ScrollXf(frame)) = &event {
            live.resource_scenes.push(scroll_xf_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::WScroll2(frame)) = &event {
            live.resource_scenes.push(wscroll2_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::Character(frame)) = &event {
            live.resource_scenes.push(character_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            )?);
        }
        if let Some(MinoriVmEvent::Effect(frame)) = &event {
            let effect = match effect_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            ) {
                Ok(effect) => effect,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            live.resource_scenes.push(effect);
        }
        if let Some(MinoriVmEvent::Panel { sequence }) = &event {
            let panel = match panel_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                *sequence,
            ) {
                Ok(effect) => effect,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            live.resource_scenes.push(panel);
        }
        if let Some(MinoriVmEvent::Movie(movie)) = &event {
            tracing::info!(
                target: "astra_emu_minori::media",
                event = "astra_emu_minori_movie_requested",
                skippable = movie.skippable,
                control_pressed = session.vm.state().system_ui.control_pressed,
                control_enabled = session.vm.state().system_ui.control_enabled,
                skip_enabled = session.vm.state().system_ui.skip_enabled,
                "requested bounded modal movie playback"
            );
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            let movie = movie_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                movie,
                sequence,
            )?;
            live.video.push(movie);
        }
        if started_game && live.resource_scenes.is_empty() {
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            let (width, height) = session.stage_size.ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_STAGE_SIZE",
                    "leaving the system UI requires explicit host dimensions",
                )
            })?;
            let frame = LegacyRenderResourceFrameV1 {
                width,
                height,
                texture_resources: Vec::new(),
                draws: Vec::new(),
            };
            frame.validate()?;
            live.resource_scenes.push(LegacySequenced {
                sequence,
                value: frame,
            });
        }
        let mut audio_command_count = u64::try_from(live.audio_commands.len()).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_AUDIO_COMMAND_BOUNDS",
                "restored audio command count cannot be represented",
            )
        })?;
        let event_audio_commands = match &event {
            Some(MinoriVmEvent::Audio { commands })
            | Some(MinoriVmEvent::Message {
                audio_commands: commands,
                ..
            }) => Some(commands),
            _ => None,
        };
        if let Some(commands) = event_audio_commands {
            for command in commands {
                let (sequence, command) = map_audio_command(command, session.vm.state())?;
                if let LegacyAudioCommandV1::LoadResource { resource_uri, .. } = &command {
                    let stat = match vfs.stat_file(&session.mount_set_id, resource_uri) {
                        Ok(stat) if stat.len > 0 && stat.len <= MAX_RESOURCE_BYTES => stat,
                        Ok(_) => {
                            session.poisoned = true;
                            return Err(invalid(
                                "ASTRA_EMU_MINORI_AUDIO_RESOURCE_BOUNDS",
                                "audio resource is empty or exceeds the session bound",
                            ));
                        }
                        Err(error) => {
                            tracing::debug!(
                                target: "astra_emu_minori::resource",
                                event = "astra_emu_minori_audio_resource_stat_failed",
                                resource_identity = %Hash256::from_sha256(resource_uri.as_bytes()),
                                diagnostic = %error.code(),
                                "audio resource stat failed"
                            );
                            session.poisoned = true;
                            return Err(error);
                        }
                    };
                    let _ = stat;
                }
                if let Err(error) = command.validate() {
                    session.poisoned = true;
                    return Err(error);
                }
                live.audio_commands.push(LegacySequenced {
                    sequence,
                    value: command,
                });
                audio_command_count += 1;
            }
        }
        let waits = match &event {
            Some(MinoriVmEvent::Wait(wait)) | Some(MinoriVmEvent::Message { wait, .. }) => {
                vec![legacy_wait(wait, session.vm.state())]
            }
            Some(MinoriVmEvent::Choice { .. }) => {
                let wait = session
                    .vm
                    .state()
                    .wait
                    .as_ref()
                    .ok_or_else(|| runtime_error(MinoriRuntimeError::Choice))?;
                vec![legacy_wait(wait, session.vm.state())]
            }
            Some(MinoriVmEvent::Movie(movie)) => vec![LegacyWaitRequest::MediaFence {
                token_id: movie.fence_id.clone(),
                media_id: movie.media_id.clone(),
            }],
            _ => Vec::new(),
        };
        let status = match &event {
            Some(MinoriVmEvent::Wait(_))
            | Some(MinoriVmEvent::Message { .. })
            | Some(MinoriVmEvent::Choice { .. }) => LegacyRuntimeStatus::Awaiting,
            Some(MinoriVmEvent::Chain { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Audio { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Stage(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::AxisScroll(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::LinearScroll(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::ScrollXf(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::WScroll2(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Character(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Effect(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::EffectCleared { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Firefly(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::FireflyCleared { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::SecondaryEffect(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::SecondaryEffectCleared { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::ScreenShake(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Panel { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Movie(_)) => LegacyRuntimeStatus::Awaiting,
            Some(MinoriVmEvent::Terminal) if session.vm.state().terminal => {
                LegacyRuntimeStatus::Terminal
            }
            Some(MinoriVmEvent::Terminal) => LegacyRuntimeStatus::Active,
            None => LegacyRuntimeStatus::Active,
        };
        let trace = (after > before)
            .then(|| LegacyTraceEntry {
                sequence: after,
                context_id: 0,
                pc: session.vm.state().pc_line as u64,
                opcode: "minori.sc".into(),
                action: match &event {
                    Some(MinoriVmEvent::Chain { .. }) => Some("chain".into()),
                    Some(MinoriVmEvent::Audio { .. }) => Some("audio".into()),
                    Some(MinoriVmEvent::Stage(_)) => Some("stage".into()),
                    Some(MinoriVmEvent::AxisScroll(_)) => Some("axis_scroll".into()),
                    Some(MinoriVmEvent::LinearScroll(_)) => Some("linear_scroll".into()),
                    Some(MinoriVmEvent::ScrollXf(_)) => Some("scroll_xf".into()),
                    Some(MinoriVmEvent::WScroll2(_)) => Some("wscroll2".into()),
                    Some(MinoriVmEvent::Character(_)) => Some("character".into()),
                    Some(MinoriVmEvent::Effect(_)) => Some("effect".into()),
                    Some(MinoriVmEvent::EffectCleared { .. }) => Some("effect_clear".into()),
                    Some(MinoriVmEvent::Firefly(_)) => Some("firefly".into()),
                    Some(MinoriVmEvent::FireflyCleared { .. }) => Some("firefly_clear".into()),
                    Some(MinoriVmEvent::SecondaryEffect(_)) => Some("secondary_effect".into()),
                    Some(MinoriVmEvent::SecondaryEffectCleared { .. }) => {
                        Some("secondary_effect_clear".into())
                    }
                    Some(MinoriVmEvent::ScreenShake(_)) => Some("screen_shake".into()),
                    Some(MinoriVmEvent::Panel { .. }) => Some("panel".into()),
                    Some(MinoriVmEvent::Choice { .. }) => Some("choice".into()),
                    Some(MinoriVmEvent::Movie(_)) => Some("movie".into()),
                    Some(MinoriVmEvent::Terminal) if !session.vm.state().terminal => {
                        Some("route_complete".into())
                    }
                    _ => None,
                },
                yield_reason: waits.first().map(|_| "wait".into()),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let mut output = LegacyStepOutput {
            status,
            live,
            control: LegacyControlTransaction {
                events: control_events,
                waits,
                ..LegacyControlTransaction::default()
            },
            trace,
            diagnostics: Vec::new(),
            coverage: LegacyCoverageDelta {
                instructions: after - before,
                contexts: vec![0],
                audio_commands: audio_command_count,
                ..LegacyCoverageDelta::default()
            },
            state_revision: session.vm.state().fixed_tick,
        };
        let reported_system_page = append_system_page_observation(session, &mut output.control)?;
        let reported_play_mode = append_play_mode_observation(session, &mut output.control)?;
        let reported_gallery_unlock_count =
            append_gallery_unlock_observation(session, &mut output.control)?;
        let reported_choice_active =
            append_choice_active_observation(session, &mut output.control)?;
        let reported_progress_in_background =
            append_progress_in_background_observation(session, &mut output.control)?;
        if matches!(event, Some(MinoriVmEvent::Terminal)) && !session.vm.state().terminal {
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            output.control.blackboard.push(LegacyBlackboardMutation {
                sequence,
                key: "minori.route_complete".into(),
                value: "true".into(),
            });
        }
        if session.global_progress.enabled {
            let writable_files = host_services
                .as_ref()
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                        "global progress requires current Family ABI Host services",
                    )
                })?
                .writable_files
                .as_ref();
            store_global_progress_if_changed(writable_files, session_id, session, &mut output)?;
        }
        if session.config_storage_enabled {
            let writable_files = host_services
                .as_ref()
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_RUNTIME_HOST_SERVICES",
                        "config persistence requires current Family ABI Host services",
                    )
                })?
                .writable_files
                .as_ref();
            store_persistent_config_if_changed(writable_files, session_id, session)?;
        }
        let restored_presentation =
            append_restored_gameplay_scene(session, &vfs, &mut output.live)?;
        output.validate()?;
        if let Some(page) = reported_system_page {
            session.reported_system_page = Some(page);
        }
        if let Some(mode) = reported_play_mode {
            session.reported_play_mode = Some(mode);
        }
        if let Some(count) = reported_gallery_unlock_count {
            session.reported_gallery_unlock_count = Some(count);
        }
        if let Some(active) = reported_choice_active {
            session.reported_choice_active = Some(active);
        }
        if let Some(enabled) = reported_progress_in_background {
            session.reported_progress_in_background = Some(enabled);
        }
        if restored_presentation {
            session.restore_presentation_pending = false;
        }
        Ok(output)
    }

    #[cfg(test)]
    fn test_checkpoint(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
    ) -> Result<TestProviderCheckpoint, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get(session_id.0.as_str())
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_POISONED",
                "poisoned session cannot produce a test checkpoint",
            ));
        }
        Ok(TestProviderCheckpoint {
            family_sections: vec![TestCheckpointSection {
                version: SchemaVersion::new(23, 0, 0),
                bytes: session.vm.snapshot_bytes().map_err(runtime_error)?,
            }],
            global_progress: MinoriGlobalProgressSnapshotV1 {
                schema: MINORI_GLOBAL_PROGRESS_SNAPSHOT_SCHEMA.into(),
                enabled: session.global_progress.enabled,
                loaded: session.global_progress.loaded,
                persisted_unlocks: session.global_progress.persisted_unlocks.clone(),
            },
        })
    }

    #[cfg(test)]
    fn restore_test_checkpoint(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        checkpoint: &TestProviderCheckpoint,
    ) -> Result<(), LegacyProviderError> {
        ctx.validate()?;
        let vfs = Arc::clone(self.vfs()?);
        let section = checkpoint.family_sections.first().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_TEST_CHECKPOINT_SECTION",
                "test checkpoint runtime section is missing",
            )
        })?;
        if checkpoint.family_sections.len() != 1
            || section.version != SchemaVersion::new(23, 0, 0)
            || checkpoint.global_progress.schema != MINORI_GLOBAL_PROGRESS_SNAPSHOT_SCHEMA
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TEST_CHECKPOINT_IDENTITY",
                "test checkpoint identity is invalid",
            ));
        }
        let restored = MinoriVm::decode_snapshot(&section.bytes).map_err(runtime_error)?;
        validate_script_uri(&restored.script_uri)?;
        let bytes = vfs.read_file(&ctx.mount_set_id, &restored.script_uri, MAX_SCRIPT_BYTES)?;
        let script_hash = Hash256::from_sha256(&bytes);
        if script_hash != restored.script_hash {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TEST_CHECKPOINT_SCRIPT_IDENTITY",
                "test checkpoint script does not match the mounted VFS",
            ));
        }
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        replace_vm_script(
            &vfs,
            &ctx.mount_set_id,
            &mut session.vm,
            restored.script_uri,
            script_hash,
            script,
        )?;
        session
            .vm
            .restore_state(&section.bytes)
            .map_err(runtime_error)?;
        session.global_progress.loaded = checkpoint.global_progress.loaded;
        session.global_progress.persisted_unlocks =
            checkpoint.global_progress.persisted_unlocks.clone();
        session.ephemeral_text.clear();
        session.restore_audio_pending = true;
        session.restore_presentation_pending = true;
        session.last_resource_frame = None;
        session.presentation_layers.clear();
        session.reported_system_page = None;
        session.reported_play_mode = None;
        session.reported_gallery_unlock_count = None;
        session.reported_choice_active = None;
        session.reported_progress_in_background = Some(false);
        session.title_pointer_focus = None;
        session.poisoned = false;
        Ok(())
    }

    #[cfg(test)]
    fn take_staged_text(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        lease_id: &str,
    ) -> Result<Option<StagedEphemeralText>, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        Ok(session.ephemeral_text.remove(lease_id))
    }

    #[allow(dead_code)]
    fn read_session_resource(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<OwnedByteBuffer, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if max_bytes == 0 || max_bytes > MAX_RESOURCE_BYTES || !resource_uri.starts_with("minori:/")
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_RESOURCE_BOUNDS",
                "resource request is outside the session VFS or byte budget",
            ));
        }
        self.vfs()?
            .read_file(&ctx.mount_set_id, resource_uri, max_bytes)
    }

    #[allow(dead_code)]
    fn begin_session_resource_read(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<LegacyResourceRead, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if max_bytes == 0 || max_bytes > MAX_RESOURCE_BYTES || !resource_uri.starts_with("minori:/")
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_RESOURCE_BOUNDS",
                "resource request is outside the session VFS or byte budget",
            ));
        }
        let vfs = Arc::clone(self.vfs()?);
        let mount_set_id = ctx.mount_set_id.clone();
        let resource_uri = resource_uri.to_owned();
        LegacyResourceRead::spawn(move || vfs.read_file(&mount_set_id, &resource_uri, max_bytes))
    }

    fn shutdown_session_impl(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
    ) -> Result<LegacyShutdownReport, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .remove(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, &session)?;
        Ok(LegacyShutdownReport {
            final_state_revision: session.vm.state().fixed_tick,
            instruction_count: session.vm.state().instruction_count,
            syscall_count: 0,
            evidence_vm_trace: session
                .evidence_vm_trace
                .into_iter()
                .map(
                    |(context_id, program_counter, opcode)| LegacyVmTraceRecord {
                        context_id,
                        program_counter,
                        opcode,
                    },
                )
                .collect(),
            diagnostics: Vec::new(),
        })
    }
}

fn non_message_time_wait(event: &MinoriVmEvent) -> Option<(u32, u32)> {
    let MinoriVmEvent::Wait(wait @ MinoriWaitState::Time { token_id, .. }) = event else {
        return None;
    };
    if token_id.starts_with("minori.message.") {
        return None;
    }
    let MinoriWaitState::Time {
        timer_ticks,
        milliseconds,
        ..
    } = wait
    else {
        unreachable!("non_message_time_wait matched a non-time wait")
    };
    Some((*timer_ticks, *milliseconds))
}

fn minori_wait_kind(wait: &MinoriWaitState) -> &'static str {
    match wait {
        MinoriWaitState::Time { .. } => "time",
        MinoriWaitState::Voice { .. } => "voice",
        MinoriWaitState::AxisScroll { .. } => "axis_scroll",
        MinoriWaitState::LinearScroll { .. } => "linear_scroll",
        MinoriWaitState::CharacterTransition { .. } => "character_transition",
        MinoriWaitState::Input { .. } => "input",
        MinoriWaitState::Choice { .. } => "choice",
        MinoriWaitState::Media { .. } => "media",
        MinoriWaitState::Presentation { .. } => "presentation",
        MinoriWaitState::Provider { .. } => "provider",
    }
}

fn append_restored_gameplay_scene(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    live: &mut LegacyLiveOutput,
) -> Result<bool, LegacyProviderError> {
    if !session.restore_presentation_pending {
        return Ok(false);
    }
    if !live.resource_scenes.is_empty() {
        return Ok(true);
    }
    let Some(stage_size) = session.stage_size else {
        return Ok(false);
    };
    let scene_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let frame = if session.vm.state().firefly.is_some() {
        describe_firefly_frame(vfs, &session.mount_set_id, session.vm.state(), stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            &session.mount_set_id,
            session.vm.state(),
            &visible_effect_frame(session.vm.state(), scene_sequence)?,
            stage_size,
        )?
    };
    live.resource_scenes.push(LegacySequenced {
        sequence: scene_sequence,
        value: frame,
    });
    Ok(true)
}

fn record_evidence_commands(
    session: &mut MinoriSession,
    commands: Vec<MinoriExecutedCommand>,
) -> Result<(), LegacyProviderError> {
    for command in commands {
        let context_id = u32::from_be_bytes(
            command.script_hash.as_bytes()[..4]
                .try_into()
                .expect("a SHA-256 prefix is exactly four bytes"),
        );
        if let Some(existing) = session.evidence_contexts.get(&context_id) {
            if *existing != command.script_hash {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_EVIDENCE_CONTEXT_COLLISION",
                    "two scripts mapped to the same bounded evidence context",
                ));
            }
        } else {
            session
                .evidence_contexts
                .insert(context_id, command.script_hash);
        }
        let opcode = minori_evidence_opcode(&command.opcode).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_EVIDENCE_OPCODE",
                "executed command has no stable evidence opcode identity",
            )
        })?;
        session
            .evidence_vm_trace
            .insert((context_id, command.command_ordinal, opcode));
    }
    Ok(())
}

fn minori_evidence_opcode(opcode: &str) -> Option<u8> {
    Some(match opcode {
        "message" => 1,
        "transition" => 2,
        "stage" => 3,
        "panel" => 4,
        "playbgm" => 5,
        "char" => 6,
        "playse" => 7,
        "wait" => 8,
        "playse2" => 9,
        "pragma" => 10,
        "setglobal" => 11,
        "set" => 12,
        "playse3" => 13,
        "effect" => 14,
        "movie" => 15,
        "playvoice" => 16,
        "shakescreen" => 17,
        "endscroll" => 18,
        "vscroll" => 19,
        "scrollxf" => 20,
        "effect2" => 21,
        "hscroll" => 22,
        "scroll" => 23,
        "label" => 24,
        "goto" => 25,
        "if" => 26,
        "chain" => 27,
        "end" => 28,
        "select" => 29,
        _ => return None,
    })
}

fn map_audio_command(
    command: &MinoriAudioCommand,
    state: &MinoriRuntimeState,
) -> Result<(u64, LegacyAudioCommandV1), LegacyProviderError> {
    match command {
        MinoriAudioCommand::LoadResource {
            sequence,
            stream_id,
            encoding,
            resource_uri,
        } => Ok((
            *sequence,
            LegacyAudioCommandV1::LoadResource {
                stream_id: *stream_id,
                encoding: map_audio_encoding(*encoding),
                resource_uri: resource_uri.clone(),
            },
        )),
        MinoriAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => Ok((
            *sequence,
            LegacyAudioCommandV1::Play {
                stream_id: *stream_id,
                volume: effective_audio_volume(state, *stream_id, *volume)?,
                pan: *pan,
                repeat: *repeat,
                fade_in_ms: *fade_in_ms,
            },
        )),
        MinoriAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        } => Ok((
            *sequence,
            LegacyAudioCommandV1::Stop {
                stream_id: *stream_id,
                fade_ms: *fade_ms,
            },
        )),
        MinoriAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        } => Ok((
            *sequence,
            LegacyAudioCommandV1::SetParams {
                stream_id: *stream_id,
                volume: effective_audio_volume(state, *stream_id, *volume)?,
                pan: *pan,
                repeat: *repeat,
            },
        )),
    }
}

fn map_audio_encoding(encoding: MinoriAudioEncoding) -> LegacyAudioEncoding {
    match encoding {
        MinoriAudioEncoding::Ogg => LegacyAudioEncoding::Ogg,
        MinoriAudioEncoding::Wav => LegacyAudioEncoding::Wav,
    }
}

fn effective_audio_volume(
    state: &MinoriRuntimeState,
    stream_id: u32,
    base_volume: f32,
) -> Result<f32, LegacyProviderError> {
    if !base_volume.is_finite() || !(0.0..=1.0).contains(&base_volume) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_AUDIO_VOLUME",
            "audio volume is outside the verified normalized range",
        ));
    }
    let audio = state.audio.get(&stream_id).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_AUDIO_STREAM_STATE",
            "audio command has no matching runtime stream state",
        )
    })?;
    let config = state
        .system_ui
        .config_draft
        .as_ref()
        .unwrap_or(&state.system_ui.config);
    let (volume, muted) = match audio.bus.as_str() {
        "bgm" => (config.bgm_volume, config.bgm_muted),
        "voice" => (config.voice_volume, config.voice_muted),
        "se" | "se2" | "se3" => (config.se_volume, config.se_muted),
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_AUDIO_BUS",
                "audio stream has an unsupported bus identity",
            ));
        }
    };
    Ok(if muted {
        0.0
    } else {
        base_volume * (f32::from(volume) / 100.0)
    })
}

fn take_restore_audio_commands(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
) -> Result<Vec<LegacySequenced<LegacyAudioCommandV1>>, LegacyProviderError> {
    if !session.restore_audio_pending {
        return Ok(Vec::new());
    }
    let active = session
        .vm
        .state()
        .audio
        .iter()
        .filter(|(_, state)| state.playing)
        .map(|(stream_id, state)| (*stream_id, state.clone()))
        .collect::<Vec<_>>();
    for (_, state) in &active {
        if state.continuation_pts != 0 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_MEDIA_CONTINUATION_UNSUPPORTED",
                "v9 audio Play cannot restore a non-zero continuation position",
            ));
        }
        match vfs.stat_file(&session.mount_set_id, &state.resource_uri) {
            Ok(stat) if stat.len > 0 && stat.len <= MAX_RESOURCE_BYTES => {}
            Ok(_) => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AUDIO_RESOURCE_BOUNDS",
                    "restored audio resource is empty or exceeds the session bound",
                ));
            }
            Err(error) => return Err(error),
        }
    }
    let mut commands = Vec::with_capacity(active.len().saturating_mul(2));
    for (stream_id, state) in active {
        let load_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        commands.push(LegacySequenced {
            sequence: load_sequence,
            value: LegacyAudioCommandV1::LoadResource {
                stream_id,
                encoding: map_audio_encoding(state.encoding),
                resource_uri: state.resource_uri,
            },
        });
        let play_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        commands.push(LegacySequenced {
            sequence: play_sequence,
            value: LegacyAudioCommandV1::Play {
                stream_id,
                volume: effective_audio_volume(
                    session.vm.state(),
                    stream_id,
                    f32::from(state.volume_milli) / 1000.0,
                )?,
                pan: f32::from(state.pan_milli) / 1000.0,
                repeat: state.looped,
                fade_in_ms: 0,
            },
        });
    }
    session.restore_audio_pending = false;
    Ok(commands)
}

fn describe_stage_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    stage: &MinoriStageCommand,
    (width, height): (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let mut texture_resources = Vec::new();
    let mut draws = Vec::new();
    append_stage_contents(
        vfs,
        mount_set_id,
        stage,
        height,
        &mut texture_resources,
        &mut draws,
    )?;
    append_character_contents(
        vfs,
        mount_set_id,
        &state.characters,
        width,
        height,
        &mut texture_resources,
        &mut draws,
    )?;
    if let Some(panel) = state
        .panel
        .as_ref()
        .filter(|_| !state.system_ui.message_panel_hidden)
    {
        if panel.mode != 1 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PANEL_MODE",
                "panel state contains an unverified mode",
            ));
        }
        append_panel_layer(
            vfs,
            mount_set_id,
            &panel.resource_uri,
            height,
            200,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn apply_scroll_xf_to_frame(
    frame: &mut LegacyRenderResourceFrameV1,
    scroll: &crate::MinoriScrollXfState,
) -> Result<(), LegacyProviderError> {
    let width = scroll.visible_extent[0].min(i32::try_from(frame.width).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SCROLL_XF_BOUNDS",
            "stage width cannot be represented for scrollXF",
        )
    })?);
    let height = scroll.visible_extent[1].min(i32::try_from(frame.height).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SCROLL_XF_BOUNDS",
            "stage height cannot be represented for scrollXF",
        )
    })?);
    if width <= 0 || height <= 0 {
        frame.draws.clear();
        return Ok(());
    }
    let source_left = scroll.visible_offset[0] as f32;
    let source_top = scroll.visible_offset[1] as f32;
    let source_right = source_left + width as f32;
    let source_bottom = source_top + height as f32;
    let scissor = Some(LegacyScissorV1 {
        x: 0,
        y: 0,
        width,
        height,
    });
    let mut clipped = Vec::with_capacity(frame.draws.len());
    for mut draw in frame.draws.drain(..) {
        let draw_left = draw.vertices[0].position[0];
        let draw_top = draw.vertices[0].position[1];
        let draw_right = draw.vertices[3].position[0];
        let draw_bottom = draw.vertices[3].position[1];
        if ![draw_left, draw_top, draw_right, draw_bottom]
            .iter()
            .all(|value| value.is_finite())
            || draw_right <= draw_left
            || draw_bottom <= draw_top
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SCROLL_XF_DRAW",
                "scrollXF requires bounded axis-aligned stage draws",
            ));
        }
        let left = draw_left.max(source_left);
        let top = draw_top.max(source_top);
        let right = draw_right.min(source_right);
        let bottom = draw_bottom.min(source_bottom);
        if right <= left || bottom <= top {
            continue;
        }
        let u0 = draw.vertices[0].tex_coord[0];
        let v0 = draw.vertices[0].tex_coord[1];
        let u1 = draw.vertices[3].tex_coord[0];
        let v1 = draw.vertices[3].tex_coord[1];
        let map_u = |x: f32| u0 + (u1 - u0) * ((x - draw_left) / (draw_right - draw_left));
        let map_v = |y: f32| v0 + (v1 - v0) * ((y - draw_top) / (draw_bottom - draw_top));
        let color = draw.vertices[0].color;
        let vertex = |x: f32, y: f32, u: f32, v: f32| LegacyVertexV1 {
            position: [x - source_left, y - source_top],
            tex_coord: [u, v],
            color,
        };
        draw.vertices = [
            vertex(left, top, map_u(left), map_v(top)),
            vertex(right, top, map_u(right), map_v(top)),
            vertex(left, bottom, map_u(left), map_v(bottom)),
            vertex(right, bottom, map_u(right), map_v(bottom)),
        ];
        draw.scissor = scissor;
        clipped.push(draw);
    }
    frame.draws = clipped;
    Ok(())
}

fn apply_wscroll2_to_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    frame: &mut LegacyRenderResourceFrameV1,
    scroll: &crate::MinoriWScroll2State,
) -> Result<(), LegacyProviderError> {
    if (frame.width, frame.height) != (1280, 720) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_STAGE_IDENTITY",
            "WScroll2 requires the verified 1280x720 reference stage",
        ));
    }
    let sync_bytes = vfs
        .read_file(
            mount_set_id,
            &scroll.sync_resource_uri,
            MAX_WSCROLL2_SYNC_BYTES,
        )
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_SYNC_READ",
                "WScroll2 sync resource could not be read",
            )
        })?;
    let _sync_values = parse_wscroll2_sync(&sync_bytes)?;
    let stage_draw_count = frame
        .draws
        .iter()
        .filter(|draw| draw.texture_id < 100)
        .count();
    if stage_draw_count != 2 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_STAGE_LAYERS",
            "WScroll2 requires one far panorama and one near panorama",
        ));
    }
    let mut wrapped = Vec::with_capacity(frame.draws.len() + 2);
    for draw in frame.draws.drain(..) {
        if draw.texture_id >= 100 {
            wrapped.push(draw);
            continue;
        }
        let resource = frame
            .texture_resources
            .iter()
            .find(|resource| resource.texture_id == draw.texture_id)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_WSCROLL2_TEXTURE",
                    "WScroll2 stage draw has no bound texture descriptor",
                )
            })?;
        if resource.decoded_width < frame.width || resource.decoded_height < frame.height {
            return Err(invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_PANORAMA_BOUNDS",
                "WScroll2 panorama is smaller than the reference viewport",
            ));
        }
        let offset = if draw.texture_id == 0 {
            scroll.background_offset
        } else {
            scroll.foreground_offset
        };
        append_wrapped_panorama_draw(
            &mut wrapped,
            &draw,
            resource,
            frame.width,
            frame.height,
            offset,
        )?;
    }
    frame.draws = wrapped;
    Ok(())
}

fn parse_wscroll2_sync(bytes: &[u8]) -> Result<Vec<i32>, LegacyProviderError> {
    let source = std::str::from_utf8(bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_SYNC_ENCODING",
            "WScroll2 sync resource is not bounded ASCII text",
        )
    })?;
    let mut values = Vec::new();
    for line in source.lines() {
        let token = line.trim();
        if token.is_empty() || token.starts_with(';') {
            continue;
        }
        if values.len() >= MAX_WSCROLL2_SYNC_VALUES {
            return Err(invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_SYNC_BOUNDS",
                "WScroll2 sync resource exceeds the value limit",
            ));
        }
        let value = token.parse::<i32>().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_SYNC_VALUE",
                "WScroll2 sync resource contains a non-integer row",
            )
        })?;
        if !(-16_384..=16_384).contains(&value) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_SYNC_VALUE",
                "WScroll2 sync value exceeds the verified bound",
            ));
        }
        values.push(value);
    }
    if values.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_SYNC_EMPTY",
            "WScroll2 sync resource contains no values",
        ));
    }
    Ok(values)
}

fn append_wrapped_panorama_draw(
    output: &mut Vec<LegacyDrawV1>,
    template: &LegacyDrawV1,
    resource: &LegacyTextureResourceV1,
    frame_width: u32,
    frame_height: u32,
    offset: i64,
) -> Result<(), LegacyProviderError> {
    let source_width = i64::from(resource.decoded_width);
    let source_x = offset.rem_euclid(source_width);
    let first_width = (source_width - source_x).min(i64::from(frame_width));
    let second_width = i64::from(frame_width) - first_width;
    let mut append_segment = |source_left: i64, output_left: i64, width: i64| {
        if width == 0 {
            return Ok(());
        }
        let source_right = source_left.checked_add(width).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_BOUNDS",
                "panorama range overflowed",
            )
        })?;
        let output_right = output_left.checked_add(width).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_WSCROLL2_BOUNDS",
                "viewport range overflowed",
            )
        })?;
        let u0 = source_left as f32 / resource.decoded_width as f32;
        let u1 = source_right as f32 / resource.decoded_width as f32;
        let v1 = frame_height as f32 / resource.decoded_height as f32;
        let color = template.vertices[0].color;
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color,
        };
        output.push(LegacyDrawV1 {
            texture_id: template.texture_id,
            vertices: [
                vertex(output_left as f32, 0.0, u0, 0.0),
                vertex(output_right as f32, 0.0, u1, 0.0),
                vertex(output_left as f32, frame_height as f32, u0, v1),
                vertex(output_right as f32, frame_height as f32, u1, v1),
            ],
            blend: template.blend,
            texture_filter: template.texture_filter,
            scissor: Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: i32::try_from(frame_width).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_WSCROLL2_BOUNDS",
                        "viewport width cannot be represented",
                    )
                })?,
                height: i32::try_from(frame_height).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_WSCROLL2_BOUNDS",
                        "viewport height cannot be represented",
                    )
                })?,
            }),
        });
        Ok::<(), LegacyProviderError>(())
    };
    append_segment(source_x, 0, first_width)?;
    append_segment(0, first_width, second_width)?;
    Ok(())
}

fn describe_effect_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
    stage_size: (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let mut frame = describe_effect_frame_without_secondary(
        vfs,
        mount_set_id,
        state,
        effect,
        stage_size,
        true,
    )?;
    append_secondary_effect_to_frame(vfs, mount_set_id, state, &mut frame)?;
    apply_screen_shake_to_frame(state, &mut frame)?;
    frame.validate()?;
    Ok(frame)
}

fn describe_effect_frame_without_secondary(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
    (width, height): (u32, u32),
    include_panel: bool,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let mut texture_resources = Vec::new();
    let mut draws = Vec::new();
    if let Some(stage) = state.stage.as_ref() {
        append_stage_contents(
            vfs,
            mount_set_id,
            stage,
            height,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    append_character_contents(
        vfs,
        mount_set_id,
        &state.characters,
        width,
        height,
        &mut texture_resources,
        &mut draws,
    )?;
    let alpha = f32::from(effect.alpha_255) / 255.0;
    if let Some(resource_uri) = &effect.current_resource_uri {
        append_resource_layer(
            vfs,
            mount_set_id,
            resource_uri,
            0,
            0,
            if effect.next_resource_uri.is_some() {
                1.0
            } else {
                1.0 - alpha
            },
            100,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    if let Some(resource_uri) = &effect.next_resource_uri {
        append_resource_layer(
            vfs,
            mount_set_id,
            resource_uri,
            0,
            0,
            alpha,
            101,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    if let Some(panel) = state
        .panel
        .as_ref()
        .filter(|_| include_panel && !state.system_ui.message_panel_hidden)
    {
        if panel.mode != 1 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PANEL_MODE",
                "panel state contains an unverified mode",
            ));
        }
        append_panel_layer(
            vfs,
            mount_set_id,
            &panel.resource_uri,
            height,
            200,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    let mut frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    if let Some(scroll_xf) = state.scroll_xf.as_ref() {
        apply_scroll_xf_to_frame(&mut frame, scroll_xf)?;
    }
    if let Some(wscroll2) = state.wscroll2.as_ref() {
        apply_wscroll2_to_frame(vfs, mount_set_id, &mut frame, wscroll2)?;
    }
    frame.validate()?;
    Ok(frame)
}

fn append_panel_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource_uri: &str,
    stage_height: u32,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    append_resource_layer(
        vfs,
        mount_set_id,
        resource_uri,
        0,
        0,
        1.0,
        texture_id,
        texture_resources,
        draws,
    )?;
    let image_height = texture_resources
        .last()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_PANEL_RESOURCE",
                "panel texture metadata was not appended",
            )
        })?
        .decoded_height;
    let top = i64::from(stage_height)
        .checked_sub(i64::from(image_height))
        .and_then(|value| value.checked_add(64))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_PANEL_POSITION",
                "panel position overflowed the verified coordinate range",
            )
        })?;
    let top = i32::try_from(top).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_PANEL_POSITION",
            "panel position cannot be represented by the render contract",
        )
    })? as f32;
    let draw = draws.last_mut().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_PANEL_RESOURCE",
            "panel draw was not appended",
        )
    })?;
    for vertex in &mut draw.vertices {
        vertex.position[1] += top;
    }
    Ok(())
}

fn panel_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    sequence: u64,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "panel presentation requires explicit host dimensions",
        )
    })?;
    let effect = visible_effect_frame(state, sequence)?;
    let frame = describe_effect_frame(vfs, mount_set_id, state, &effect, stage_size)?;
    Ok(LegacySequenced {
        sequence,
        value: frame,
    })
}

fn visible_effect_frame(
    state: &MinoriRuntimeState,
    sequence: u64,
) -> Result<MinoriEffectFrame, LegacyProviderError> {
    let Some(effect) = &state.effect else {
        return Ok(MinoriEffectFrame {
            sequence,
            current_resource_uri: None,
            next_resource_uri: None,
            alpha_255: 0,
        });
    };
    let current = usize::try_from(effect.visible_current_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_EFFECT_STATE",
            "visible effect resource index cannot be represented",
        )
    })?;
    let next = usize::try_from(effect.visible_next_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_EFFECT_STATE",
            "visible effect resource index cannot be represented",
        )
    })?;
    Ok(MinoriEffectFrame {
        sequence,
        current_resource_uri: effect
            .resources
            .get(current)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_EFFECT_STATE",
                    "visible current effect resource is outside the sequence",
                )
            })?
            .clone(),
        next_resource_uri: effect
            .resources
            .get(next)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_EFFECT_STATE",
                    "visible next effect resource is outside the sequence",
                )
            })?
            .clone(),
        alpha_255: effect.visible_alpha_255,
    })
}

fn effect_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "effect presentation requires explicit host dimensions",
        )
    })?;
    let frame = describe_effect_frame(vfs, mount_set_id, state, effect, stage_size)?;
    Ok(LegacySequenced {
        sequence: effect.sequence,
        value: frame,
    })
}

fn character_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    character: &MinoriCharacterFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "character presentation requires explicit host dimensions",
        )
    })?;
    let frame = if state.firefly.is_some() {
        describe_firefly_frame(vfs, mount_set_id, state, stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            mount_set_id,
            state,
            &visible_effect_frame(state, character.sequence)?,
            stage_size,
        )?
    };
    Ok(LegacySequenced {
        sequence: character.sequence,
        value: frame,
    })
}

fn axis_scroll_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    axis_scroll: &MinoriAxisScrollFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let effect = visible_effect_frame(state, axis_scroll.sequence)?;
    effect_presentation(vfs, mount_set_id, stage_size, state, &effect)
}

fn linear_scroll_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    linear_scroll: &MinoriLinearScrollFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let effect = visible_effect_frame(state, linear_scroll.sequence)?;
    effect_presentation(vfs, mount_set_id, stage_size, state, &effect)
}

fn scroll_xf_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    scroll_xf: &MinoriScrollXfFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let effect = visible_effect_frame(state, scroll_xf.sequence)?;
    effect_presentation(vfs, mount_set_id, stage_size, state, &effect)
}

fn wscroll2_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    frame: &MinoriWScroll2Frame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    if state.wscroll2.is_none() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_WSCROLL2_STATE",
            "WScroll2 presentation has no active runtime state",
        ));
    }
    let visible = visible_effect_frame(state, frame.sequence)?;
    effect_presentation(vfs, mount_set_id, stage_size, state, &visible)
}

fn firefly_event_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    event: &MinoriVmEvent,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "Firefly presentation requires explicit host dimensions",
        )
    })?;
    let (sequence, frame) = match event {
        MinoriVmEvent::Firefly(frame) => (
            frame.sequence,
            describe_firefly_frame(vfs, mount_set_id, state, stage_size)?,
        ),
        MinoriVmEvent::FireflyCleared { sequence } => (
            *sequence,
            describe_effect_frame(
                vfs,
                mount_set_id,
                state,
                &MinoriEffectFrame {
                    sequence: *sequence,
                    current_resource_uri: None,
                    next_resource_uri: None,
                    alpha_255: 0,
                },
                stage_size,
            )?,
        ),
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FIREFLY_EVENT",
                "non-Firefly event was sent to the Firefly presentation mapper",
            ));
        }
    };
    Ok(LegacySequenced {
        sequence,
        value: frame,
    })
}

fn secondary_effect_event_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    event: &MinoriVmEvent,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "secondary effect presentation requires explicit host dimensions",
        )
    })?;
    let sequence = match event {
        MinoriVmEvent::SecondaryEffect(MinoriSecondaryEffectFrame { sequence })
        | MinoriVmEvent::SecondaryEffectCleared { sequence } => *sequence,
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SECONDARY_EFFECT_EVENT",
                "non-secondary event was sent to the secondary effect mapper",
            ));
        }
    };
    let frame = if state.firefly.is_some() {
        describe_firefly_frame(vfs, mount_set_id, state, stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            mount_set_id,
            state,
            &visible_effect_frame(state, sequence)?,
            stage_size,
        )?
    };
    Ok(LegacySequenced {
        sequence,
        value: frame,
    })
}

fn screen_shake_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    frame: &MinoriScreenShakeFrame,
) -> Result<LegacySequenced<LegacyRenderResourceFrameV1>, LegacyProviderError> {
    if state.screen_shake.is_none() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCREEN_SHAKE_STATE",
            "screen shake presentation has no active runtime state",
        ));
    }
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "screen shake presentation requires explicit host dimensions",
        )
    })?;
    let rendered = if state.firefly.is_some() {
        describe_firefly_frame(vfs, mount_set_id, state, stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            mount_set_id,
            state,
            &visible_effect_frame(state, frame.sequence)?,
            stage_size,
        )?
    };
    Ok(LegacySequenced {
        sequence: frame.sequence,
        value: rendered,
    })
}

fn describe_firefly_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    (width, height): (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    if (width, height) != (1280, 720) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_FIREFLY_STAGE_IDENTITY",
            "the verified Firefly effect requires the 1280x720 reference stage",
        ));
    }
    let firefly = state.firefly.as_ref().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_FIREFLY_STATE",
            "Firefly presentation has no active effect state",
        )
    })?;
    if firefly.resources.len() != 3
        || firefly.particles.is_empty()
        || firefly.particles.len() > 256
        || firefly
            .particles
            .iter()
            .any(|particle| usize::from(particle.kind) >= firefly.resources.len())
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_FIREFLY_STATE",
            "Firefly particle state is outside the verified bounds",
        ));
    }
    let base = describe_effect_frame_without_secondary(
        vfs,
        mount_set_id,
        state,
        &MinoriEffectFrame {
            sequence: state.effect_sequence,
            current_resource_uri: None,
            next_resource_uri: None,
            alpha_255: 0,
        },
        (width, height),
        true,
    )?;
    let mut texture_resources = base.texture_resources;
    let mut draws = base.draws;
    let mut sprite_sizes = Vec::with_capacity(firefly.resources.len());
    for (index, resource_uri) in firefly.resources.iter().enumerate() {
        let texture_id = 300u32
            .checked_add(u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_FIREFLY_TEXTURE_ID",
                    "Firefly texture index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_FIREFLY_TEXTURE_ID",
                    "Firefly texture id overflowed",
                )
            })?;
        let resource = read_texture_resource(vfs, mount_set_id, resource_uri, texture_id)?;
        sprite_sizes.push((resource.decoded_width, resource.decoded_height));
        texture_resources.push(resource);
    }
    let scissor = Some(LegacyScissorV1 {
        x: 0,
        y: 0,
        width: i32::try_from(width).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_FIREFLY_STAGE_IDENTITY",
                "Firefly stage width cannot be represented",
            )
        })?,
        height: i32::try_from(height).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_FIREFLY_STAGE_IDENTITY",
                "Firefly stage height cannot be represented",
            )
        })?,
    });
    let global_alpha = f32::from(firefly.fade_alpha_256) / 256.0;
    for particle in &firefly.particles {
        if !particle.active || particle.opacity_255 == 0 || global_alpha == 0.0 {
            continue;
        }
        let texture_index = usize::from(particle.kind);
        let texture_id = 300u32
            .checked_add(u32::try_from(texture_index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_FIREFLY_TEXTURE_ID",
                    "Firefly texture index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_FIREFLY_TEXTURE_ID",
                    "Firefly texture id overflowed",
                )
            })?;
        let (sprite_width, sprite_height) = sprite_sizes[texture_index];
        let left = particle.position[0] as f32;
        let top = particle.position[1] as f32;
        let right = left + sprite_width as f32;
        let bottom = top + sprite_height as f32;
        let opacity = global_alpha * f32::from(particle.opacity_255) / 255.0;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_FIREFLY_ALPHA",
                "Firefly particle alpha is outside the normalized range",
            ));
        }
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0, 1.0, 1.0, opacity],
        };
        draws.push(LegacyDrawV1 {
            texture_id,
            vertices: [
                vertex(left, top, 0.0, 0.0),
                vertex(right, top, 1.0, 0.0),
                vertex(left, bottom, 0.0, 1.0),
                vertex(right, bottom, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            texture_filter: LegacyTextureFilter::Linear,
            scissor,
        });
    }
    let mut frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    append_secondary_effect_to_frame(vfs, mount_set_id, state, &mut frame)?;
    apply_screen_shake_to_frame(state, &mut frame)?;
    frame.validate()?;
    Ok(frame)
}

fn append_secondary_effect_to_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    frame: &mut LegacyRenderResourceFrameV1,
) -> Result<(), LegacyProviderError> {
    let Some(effect) = state.secondary_effect.as_ref() else {
        return Ok(());
    };
    if (frame.width, frame.height) != (1280, 720)
        || effect.particles.len() != 50
        || effect.alpha_256 > 256
        || effect
            .particles
            .iter()
            .any(|particle| !particle.active || particle.kind >= 3)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SECONDARY_EFFECT_STATE",
            "secondary effect state is outside the verified SnowH bounds",
        ));
    }
    let mut sprite_sizes = Vec::with_capacity(effect.resources.len());
    for (index, resource_uri) in effect.resources.iter().enumerate() {
        let texture_id = 600u32
            .checked_add(u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_SECONDARY_EFFECT_TEXTURE_ID",
                    "secondary effect texture index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SECONDARY_EFFECT_TEXTURE_ID",
                    "secondary effect texture id overflowed",
                )
            })?;
        let resource = read_texture_resource(vfs, mount_set_id, resource_uri, texture_id)?;
        sprite_sizes.push((resource.decoded_width, resource.decoded_height));
        frame.texture_resources.push(resource);
    }
    let alpha = f32::from(effect.alpha_256) / 256.0;
    if alpha == 0.0 {
        return Ok(());
    }
    let scissor = Some(LegacyScissorV1 {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    });
    for particle in &effect.particles {
        let texture_index = usize::from(particle.kind);
        let texture_id = 600u32
            .checked_add(u32::try_from(texture_index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_SECONDARY_EFFECT_TEXTURE_ID",
                    "secondary effect texture index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SECONDARY_EFFECT_TEXTURE_ID",
                    "secondary effect texture id overflowed",
                )
            })?;
        let (sprite_width, sprite_height) = sprite_sizes[texture_index];
        let left = particle.position[0] as f32;
        let top = particle.position[1] as f32;
        let right = left + sprite_width as f32;
        let bottom = top + sprite_height as f32;
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0, 1.0, 1.0, alpha],
        };
        frame.draws.push(LegacyDrawV1 {
            texture_id,
            vertices: [
                vertex(left, top, 0.0, 0.0),
                vertex(right, top, 1.0, 0.0),
                vertex(left, bottom, 0.0, 1.0),
                vertex(right, bottom, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            texture_filter: LegacyTextureFilter::Linear,
            scissor,
        });
    }
    Ok(())
}

fn apply_screen_shake_to_frame(
    state: &MinoriRuntimeState,
    frame: &mut LegacyRenderResourceFrameV1,
) -> Result<(), LegacyProviderError> {
    let Some(shake) = state.screen_shake.as_ref() else {
        return Ok(());
    };
    if (frame.width, frame.height) != (1280, 720)
        || !(1..=1280).contains(&shake.amplitude)
        || shake
            .offset
            .iter()
            .any(|value| value.unsigned_abs() > shake.amplitude as u32)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCREEN_SHAKE_STATE",
            "screen shake state is outside the verified render bounds",
        ));
    }
    let offset = [shake.offset[0] as f32, shake.offset[1] as f32];
    let scissor = Some(LegacyScissorV1 {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    });
    for draw in &mut frame.draws {
        for vertex in &mut draw.vertices {
            vertex.position[0] += offset[0];
            vertex.position[1] += offset[1];
            if !vertex.position[0].is_finite() || !vertex.position[1].is_finite() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SCREEN_SHAKE_DRAW",
                    "screen shake produced a non-finite draw position",
                ));
            }
        }
        // Native Musica shifts the already-composited screen buffer. The
        // source geometry has already been clipped by the family adapters, so
        // the translated result is clipped only to the final viewport.
        draw.scissor = scissor;
    }
    Ok(())
}

fn append_stage_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    layer: &MinoriStageLayer,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    append_resource_layer(
        vfs,
        mount_set_id,
        &layer.resource_uri,
        layer.x,
        layer.y,
        1.0,
        texture_id,
        texture_resources,
        draws,
    )
}

fn append_stage_contents(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage: &MinoriStageCommand,
    stage_height: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    if let Some(background) = &stage.background {
        append_stage_layer(vfs, mount_set_id, background, 1, texture_resources, draws)?;
    }
    for (index, stand) in stage.stands.iter().enumerate() {
        let texture_id = 16u32
            .checked_add(u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_LAYER_ID",
                    "stand layer index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_LAYER_ID",
                    "stand layer id overflowed the render resource namespace",
                )
            })?;
        append_stand_layer(
            vfs,
            mount_set_id,
            stand,
            stage_height,
            texture_id,
            texture_resources,
            draws,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_character_contents(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    characters: &BTreeMap<u32, MinoriCharacterState>,
    stage_width: u32,
    stage_height: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    let scissor = LegacyScissorV1 {
        x: 0,
        y: 0,
        width: i32::try_from(stage_width).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_BOUNDS",
                "character viewport width cannot be represented",
            )
        })?,
        height: i32::try_from(stage_height).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_BOUNDS",
                "character viewport height cannot be represented",
            )
        })?,
    };
    for character in characters.values().filter(|character| character.visible) {
        let [resource_uri] = character.resource_uris.as_slice() else {
            return Err(invalid(
                "ASTRA_EMU_MINORI_CHARACTER_RESOURCE_COUNT",
                "character presentation requires the verified single-resource form",
            ));
        };
        if !resource_uri.to_ascii_lowercase().ends_with(".png") {
            return Err(invalid(
                "ASTRA_EMU_MINORI_CHARACTER_CODEC",
                "character presentation requires a static PNG resource",
            ));
        }
        append_character_sprite(
            vfs,
            mount_set_id,
            character,
            resource_uri,
            MINORI_CHARACTER_TEXTURE_BASE,
            character.opacity_256,
            stage_height,
            scissor,
            texture_resources,
            draws,
        )?;
        if let Some(replacement) = character.replacement.as_ref() {
            append_character_sprite(
                vfs,
                mount_set_id,
                character,
                &replacement.resource_uri,
                MINORI_CHARACTER_REPLACEMENT_TEXTURE_BASE,
                replacement.next_opacity_256,
                stage_height,
                scissor,
                texture_resources,
                draws,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_character_sprite(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    character: &MinoriCharacterState,
    resource_uri: &str,
    texture_base: u32,
    opacity_256: u16,
    stage_height: u32,
    scissor: LegacyScissorV1,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    if !resource_uri.to_ascii_lowercase().ends_with(".png") {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHARACTER_CODEC",
            "character presentation requires a static PNG resource",
        ));
    }
    let texture_id = texture_base.checked_add(character.slot_id).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_CHARACTER_TEXTURE_ID",
            "character texture id overflowed",
        )
    })?;
    let resource = read_texture_resource(vfs, mount_set_id, resource_uri, texture_id)?;
    let left = i64::from(character.anchor_position[0])
        .checked_sub(i64::from(resource.decoded_width) / 2)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_POSITION",
                "character horizontal anchor overflowed",
            )
        })?;
    let top = i64::from(stage_height)
        .checked_sub(i64::from(resource.decoded_height))
        .and_then(|value| value.checked_sub(i64::from(character.anchor_position[1])))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_CHARACTER_POSITION",
                "character bottom-relative anchor overflowed",
            )
        })?;
    let left = i32::try_from(left).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CHARACTER_POSITION",
            "character horizontal anchor cannot be represented",
        )
    })?;
    let top = i32::try_from(top).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CHARACTER_POSITION",
            "character vertical anchor cannot be represented",
        )
    })?;
    append_texture_draw(&resource, left, top, f32::from(opacity_256) / 256.0, draws)?;
    let draw = draws.last_mut().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_CHARACTER_RESOURCE",
            "character draw was not appended",
        )
    })?;
    draw.scissor = Some(scissor);
    if !character.positive_orientation {
        for vertex in &mut draw.vertices {
            vertex.tex_coord[0] = 1.0 - vertex.tex_coord[0];
        }
    }
    texture_resources.push(resource);
    Ok(())
}

fn append_stand_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stand: &MinoriStandLayer,
    stage_height: u32,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    if !stand.resource_uri.to_ascii_lowercase().ends_with(".png") {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_STAND_CODEC",
            "verified stand positioning requires a static PNG resource",
        ));
    }
    let resource = read_texture_resource(vfs, mount_set_id, &stand.resource_uri, texture_id)?;
    let left = i64::from(stand.position) - i64::from(resource.decoded_width) / 2;
    let top = i64::from(stage_height) - i64::from(resource.decoded_height);
    let left = i32::try_from(left).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_STAND_POSITION",
            "centered stand X position cannot be represented",
        )
    })?;
    let top = i32::try_from(top).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_STAND_POSITION",
            "bottom-anchored stand Y position cannot be represented",
        )
    })?;
    append_texture_draw(&resource, left, top, 1.0, draws)?;
    texture_resources.push(resource);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_resource_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource_uri: &str,
    x: i32,
    y: i32,
    opacity: f32,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    let resource = read_texture_resource(vfs, mount_set_id, resource_uri, texture_id)?;
    append_texture_draw(&resource, x, y, opacity, draws)?;
    texture_resources.push(resource);
    Ok(())
}

fn append_texture_draw(
    resource: &LegacyTextureResourceV1,
    x: i32,
    y: i32,
    opacity: f32,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    append_texture_draw_with_scissor(resource, x, y, opacity, None, draws)
}

fn append_texture_draw_with_scissor(
    resource: &LegacyTextureResourceV1,
    x: i32,
    y: i32,
    opacity: f32,
    scissor: Option<LegacyScissorV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_EFFECT_ALPHA",
            "effect alpha is outside the normalized bound",
        ));
    }
    let left = x as f32;
    let top = y as f32;
    let right = left + resource.decoded_width as f32;
    let bottom = top + resource.decoded_height as f32;
    let vertex = |x, y, u, v| LegacyVertexV1 {
        position: [x, y],
        tex_coord: [u, v],
        color: [1.0, 1.0, 1.0, opacity],
    };
    draws.push(LegacyDrawV1 {
        texture_id: resource.texture_id,
        vertices: [
            vertex(left, top, 0.0, 0.0),
            vertex(right, top, 1.0, 0.0),
            vertex(left, bottom, 0.0, 1.0),
            vertex(right, bottom, 1.0, 1.0),
        ],
        blend: LegacyBlendMode::Alpha,
        texture_filter: LegacyTextureFilter::Linear,
        scissor,
    });
    Ok(())
}

fn read_texture_resource(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource_uri: &str,
    texture_id: u32,
) -> Result<LegacyTextureResourceV1, LegacyProviderError> {
    let stat = vfs
        .stat_file(mount_set_id, resource_uri)
        .inspect_err(|error| {
            tracing::debug!(
                target: "astra_emu_minori::resource",
                event = "astra_emu_minori_resource_stat_failed",
                resource_identity = %Hash256::from_sha256(resource_uri.as_bytes()),
                texture_id,
                diagnostic = %error.code(),
                "resource stat failed"
            );
        })?;
    if stat.len == 0 || stat.len > MAX_RESOURCE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_BOUNDS",
            "stage image is empty or exceeds the resource byte bound",
        ));
    }
    let bytes = vfs
        .read_file_range(
            mount_set_id,
            resource_uri,
            stat.revision,
            ByteRange {
                offset: 0,
                len: stat.len,
            },
            MAX_RESOURCE_BYTES,
        )
        .inspect_err(|error| {
            tracing::debug!(
                target: "astra_emu_minori::resource",
                event = "astra_emu_minori_resource_read_failed",
                resource_identity = %Hash256::from_sha256(resource_uri.as_bytes()),
                texture_id,
                diagnostic = %error.code(),
                "resource read failed"
            );
        })?
        .bytes;
    let codec = image_codec(resource_uri)?;
    let (image_width, image_height) = match codec {
        "ani" => {
            let archive = MinoriAniArchive::parse(Arc::<[u8]>::from(bytes.as_slice()))
                .map_err(minori_image_container_error)?;
            let frame = archive.frames().first().ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_ANI_FRAME_COUNT",
                    "ANI has no frame available for presentation",
                )
            })?;
            (frame.width, frame.height)
        }
        "sqz" => {
            let archive = MinoriSqzArchive::parse(Arc::<[u8]>::from(bytes.as_slice()))
                .map_err(minori_image_container_error)?;
            (archive.width(), archive.height())
        }
        _ => {
            let image_reader = image::ImageReader::new(Cursor::new(bytes.as_slice()))
                .with_guessed_format()
                .map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_STAGE_IMAGE_FORMAT",
                        "stage image format could not be determined",
                    )
                })?;
            image_reader.into_dimensions().map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_IMAGE_METADATA",
                    "stage image dimensions could not be read safely",
                )
            })?
        }
    };
    if image_width == 0 || image_height == 0 || image_width > 16_384 || image_height > 16_384 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_BOUNDS",
            "stage image dimensions are outside the supported bound",
        ));
    }
    let revision = texture_binding_revision(resource_uri, stat.revision.0);
    Ok(LegacyTextureResourceV1 {
        texture_id,
        resource_uri: resource_uri.to_owned(),
        codec: codec.into(),
        revision,
        decoded_width: image_width,
        decoded_height: image_height,
        decoded_format: LegacyTextureFormat::Rgba8,
    })
}

fn texture_binding_revision(resource_uri: &str, source_revision: u64) -> u64 {
    let mut identity = Vec::with_capacity(resource_uri.len() + std::mem::size_of::<u64>());
    identity.extend_from_slice(&source_revision.to_le_bytes());
    identity.extend_from_slice(resource_uri.as_bytes());
    let mut revision = u64::from_le_bytes(
        Hash256::from_sha256(&identity).as_bytes()[..8]
            .try_into()
            .expect("sha256 prefix has a fixed width"),
    );
    if revision == 0 {
        revision = 1;
    }
    revision
}

fn image_codec(resource_uri: &str) -> Result<&'static str, LegacyProviderError> {
    let extension = resource_uri
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("png") {
        Ok("png")
    } else if extension.eq_ignore_ascii_case("bmp") {
        Ok("bmp")
    } else if extension.eq_ignore_ascii_case("jpg") {
        Ok("jpg")
    } else if extension.eq_ignore_ascii_case("jpeg") {
        Ok("jpeg")
    } else if extension.eq_ignore_ascii_case("webp") {
        Ok("webp")
    } else if extension.eq_ignore_ascii_case("ani") {
        Ok("ani")
    } else if extension.eq_ignore_ascii_case("sqz") {
        Ok("sqz")
    } else {
        Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_CODEC",
            "stage image extension has no explicitly bound decode codec",
        ))
    }
}

fn minori_image_container_error(error: LegacyCoreError) -> LegacyProviderError {
    LegacyProviderError::invalid(
        "ASTRA_EMU_MINORI_IMAGE_CONTAINER",
        format!("{}: {}", error.code(), error.message()),
    )
}

fn load_script(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    target: &str,
) -> Result<(String, Hash256, crate::ScScript), LegacyProviderError> {
    let script_uri = format!("minori:/scr/{target}");
    load_script_uri(vfs, mount_set_id, &script_uri)
}

fn load_script_uri(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    script_uri: &str,
) -> Result<(String, Hash256, crate::ScScript), LegacyProviderError> {
    validate_script_uri(script_uri)?;
    let source = vfs
        .read_file(mount_set_id, script_uri, MAX_SCRIPT_BYTES)
        .inspect_err(|error| {
            tracing::debug!(
                target: "astra_emu_minori::resource",
                event = "astra_emu_minori_script_read_failed",
                resource_identity = %Hash256::from_sha256(script_uri.as_bytes()),
                diagnostic = %error.code(),
                "script read failed"
            );
        })?;
    let script_hash = Hash256::from_sha256(&source);
    let mut include_stack = vec![script_uri.to_owned()];
    let mut expanded_bytes = 0usize;
    let bytes = expand_script_includes(
        vfs,
        mount_set_id,
        script_uri,
        source.as_slice(),
        &mut include_stack,
        &mut expanded_bytes,
    )?;
    let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
    Ok((script_uri.to_owned(), script_hash, script))
}

fn decode_message_voice_durations(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    script: &crate::ScScript,
) -> Result<BTreeMap<String, u32>, LegacyProviderError> {
    let resources = message_voice_wait_resources(script).map_err(runtime_error)?;
    if resources.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut durations_ms = BTreeMap::new();
    for resource_uri in resources {
        let source = Arc::new(LegacyRuntimeVfsByteSource::new(
            Arc::clone(vfs),
            mount_set_id,
            resource_uri.clone(),
        )?);
        let reader = BoundedByteSourceReader::new(source, 4 * 1024 * 1024).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_MESSAGE_VOICE_READER",
                "voice metadata reader could not bind the revision-pinned VFS source",
            )
        })?;
        if reader.stat().len == 0 || reader.stat().len > MAX_RESOURCE_BYTES {
            return Err(invalid(
                "ASTRA_EMU_MINORI_MESSAGE_VOICE_BOUNDS",
                "voice resource is empty or exceeds the runtime input bound",
            ));
        }
        let byte_len = reader.stat().len;
        let metadata = probe_symphonia_audio_metadata_reader("ogg", reader, byte_len)
            .map_err(minori_media_decode_error)?;
        let milliseconds = metadata
            .duration_us
            .checked_add(999)
            .map(|value| value / 1000)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value != 0)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_MESSAGE_VOICE_DURATION",
                    "decoded voice duration is empty or exceeds the runtime clock",
                )
            })?;
        durations_ms.insert(resource_uri, milliseconds);
    }
    Ok(durations_ms)
}

fn replace_vm_script(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    vm: &mut MinoriVm,
    script_uri: String,
    script_hash: Hash256,
    script: crate::ScScript,
) -> Result<(), LegacyProviderError> {
    let durations_ms = decode_message_voice_durations(vfs, mount_set_id, &script)?;
    vm.replace_script(script_uri, script_hash, script)
        .map_err(runtime_error)?;
    vm.set_message_voice_durations(durations_ms)
        .map_err(runtime_error)
}

/// Validates the resource references of every `.sc` entry without loading an
/// archive directory or retaining commercial payload. The host VFS owns the
/// bounded enumeration and all reads; this function keeps only URI identities,
/// lengths and revisions long enough to form a local audit digest.
fn audit_script_resources(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
) -> Result<(u64, Hash256), LegacyProviderError> {
    let scripts = vfs
        .enumerate_by_extension(
            mount_set_id,
            "minori:/scr",
            "sc",
            MINORI_MAX_RESOURCE_AUDIT_SCRIPTS,
        )
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_RESOURCE_AUDIT_UNSUPPORTED",
                "the bound VFS cannot perform the required bounded script enumeration",
            )
        })?;
    if scripts.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_RESOURCE_AUDIT_EMPTY",
            "the Minori script mount contains no scripts",
        ));
    }
    if scripts.len() > MINORI_MAX_RESOURCE_AUDIT_SCRIPTS as usize {
        return Err(invalid(
            "ASTRA_EMU_MINORI_RESOURCE_AUDIT_BOUNDS",
            "the Minori script enumeration exceeds its bound",
        ));
    }
    let mut references = BTreeSet::new();
    let mut identity = Vec::with_capacity(scripts.len() * 48);
    identity.extend_from_slice(&(scripts.len() as u64).to_le_bytes());
    for listed in scripts {
        validate_script_uri(&listed.uri)?;
        if listed.stat.len == 0 || listed.stat.len > MAX_SCRIPT_BYTES {
            return Err(invalid(
                "ASTRA_EMU_MINORI_RESOURCE_AUDIT_SCRIPT_BOUNDS",
                "a script entry is empty or exceeds the bounded source size",
            ));
        }
        let (_, _, script) = load_script_uri(vfs, mount_set_id, &listed.uri).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_RESOURCE_AUDIT_SCRIPT_READ",
                "a script entry could not be read consistently",
            )
        })?;
        let script_references = collect_resource_references(&script).map_err(runtime_error)?;
        identity.extend_from_slice(Hash256::from_sha256(listed.uri.as_bytes()).as_bytes());
        identity.extend_from_slice(&listed.stat.len.to_le_bytes());
        identity.extend_from_slice(&listed.stat.revision.0.to_le_bytes());
        references.extend(script_references);
    }
    let mut resource_count = 0u64;
    identity.extend_from_slice(&(references.len() as u64).to_le_bytes());
    for resource_uri in references {
        let stat = vfs.stat_file(mount_set_id, &resource_uri).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_RESOURCE_AUDIT_MISSING",
                "a script resource reference is missing from the mounted VFS",
            )
        })?;
        if stat.len == 0 || stat.len > MAX_RESOURCE_BYTES {
            return Err(invalid(
                "ASTRA_EMU_MINORI_RESOURCE_AUDIT_BOUNDS",
                "a referenced resource is empty or exceeds the session bound",
            ));
        }
        resource_count = resource_count.checked_add(1).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_RESOURCE_AUDIT_BOUNDS",
                "the referenced resource count overflowed",
            )
        })?;
        identity.extend_from_slice(Hash256::from_sha256(resource_uri.as_bytes()).as_bytes());
        identity.extend_from_slice(&stat.len.to_le_bytes());
        identity.extend_from_slice(&stat.revision.0.to_le_bytes());
    }
    Ok((resource_count, Hash256::from_sha256(&identity)))
}

fn expand_script_includes(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    source_uri: &str,
    source: &[u8],
    include_stack: &mut Vec<String>,
    expanded_bytes: &mut usize,
) -> Result<Vec<u8>, LegacyProviderError> {
    const MAX_INCLUDE_DEPTH: usize = 32;
    if include_stack.len() > MAX_INCLUDE_DEPTH {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_DEPTH",
            "script include depth exceeds the verified bound",
        ));
    }
    let mut expanded = Vec::with_capacity(source.len());
    for segment in source.split_inclusive(|byte| *byte == b'\n') {
        let line = segment.strip_suffix(b"\n").unwrap_or(segment);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let trimmed = trim_ascii_script_space(line);
        if !trimmed.starts_with(b".include") {
            expanded.extend_from_slice(segment);
            *expanded_bytes = expanded_bytes.checked_add(segment.len()).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_SIZE",
                    "expanded script size overflowed",
                )
            })?;
            if *expanded_bytes > MAX_SCRIPT_BYTES as usize {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_SIZE",
                    "expanded script exceeds the bounded source size",
                ));
            }
            continue;
        }
        let include_target = parse_include_target(trimmed).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_OPERAND",
                "script include requires one safe .sc target",
            )
        })?;
        let include_uri = format!("minori:/scr/{include_target}");
        validate_script_uri(&include_uri)?;
        if include_stack.iter().any(|uri| uri == &include_uri) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_CYCLE",
                "script include cycle is not allowed",
            ));
        }
        let included = vfs
            .read_file(mount_set_id, &include_uri, MAX_SCRIPT_BYTES)
            .inspect_err(|error| {
                tracing::debug!(
                    target: "astra_emu_minori::resource",
                    event = "astra_emu_minori_script_include_read_failed",
                    resource_identity = %Hash256::from_sha256(include_uri.as_bytes()),
                    source_identity = %Hash256::from_sha256(source_uri.as_bytes()),
                    diagnostic = %error.code(),
                    "script include read failed"
                );
            })?;
        include_stack.push(include_uri.clone());
        let included_expanded = expand_script_includes(
            vfs,
            mount_set_id,
            &include_uri,
            included.as_slice(),
            include_stack,
            expanded_bytes,
        )?;
        include_stack.pop();
        expanded.extend_from_slice(&included_expanded);
        if segment.ends_with(b"\n") && !included_expanded.ends_with(b"\n") {
            expanded.extend_from_slice(b"\r\n");
            *expanded_bytes = expanded_bytes.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_SIZE",
                    "expanded script size overflowed",
                )
            })?;
        }
        if *expanded_bytes > MAX_SCRIPT_BYTES as usize {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_SIZE",
                "expanded script exceeds the bounded source size",
            ));
        }
    }
    Ok(expanded)
}

fn trim_ascii_script_space(value: &[u8]) -> &[u8] {
    let start = value
        .iter()
        .position(|byte| !matches!(*byte, b' ' | b'\t'))
        .unwrap_or(value.len());
    let end = value
        .iter()
        .rposition(|byte| !matches!(*byte, b' ' | b'\t'))
        .map_or(start, |index| index + 1);
    &value[start..end]
}

fn parse_include_target(line: &[u8]) -> Option<String> {
    let mut tokens = line.split(|byte| matches!(*byte, b' ' | b'\t'));
    if tokens.next()? != b".include" {
        return None;
    }
    let raw_target = tokens.next()?;
    let target = raw_target.strip_suffix(b"\r").unwrap_or(raw_target);
    if target.is_empty() || tokens.next().is_some() {
        return None;
    }
    // Include operands are part of the same CP932 source stream as the rest
    // of the script.  Decode them through the bound original-game locale
    // before applying the URI's ASCII-safe filename policy; using UTF-8 here
    // would make the parser's encoding contract depend on the host console.
    let target = MinoriLocaleHook::japanese_cp932().decode(target).ok()?;
    if !target.to_ascii_lowercase().ends_with(".sc") {
        return None;
    }
    Some(target)
}

fn validate_script_uri(script_uri: &str) -> Result<(), LegacyProviderError> {
    let Some(target) = script_uri.strip_prefix("minori:/scr/") else {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCRIPT_URI",
            "script URI is outside the Minori script mount",
        ));
    };
    if target.is_empty()
        || target.len() > 256
        || !target.to_ascii_lowercase().ends_with(".sc")
        || target.contains('/')
        || target.contains('\\')
        || target.contains("..")
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCRIPT_URI",
            "script URI contains an invalid direct-entry name",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MinoriSystemUiAction {
    Present,
    PresentWithAudioRefresh,
    PresentAfterConfigClose { fullscreen: Option<bool> },
    PresentWithAudioTest(MinoriConfigAudioBus),
    StartGame,
    CloseBacklog,
    ReplayBacklogVoice,
    SaveSlot(u32),
    LoadSlot(u32),
    CloseGameplaySystemPage,
    GalleryBgmPlay,
    GalleryBgmStop,
    GalleryReplayStart(u32),
    GalleryMovieStart(u32),
    Exit,
}

fn system_ui_action_name(action: MinoriSystemUiAction) -> &'static str {
    match action {
        MinoriSystemUiAction::Present => "present",
        MinoriSystemUiAction::PresentWithAudioRefresh => "present_with_audio_refresh",
        MinoriSystemUiAction::PresentAfterConfigClose { .. } => "present_after_config_close",
        MinoriSystemUiAction::PresentWithAudioTest(_) => "present_with_audio_test",
        MinoriSystemUiAction::StartGame => "start_game",
        MinoriSystemUiAction::CloseBacklog => "close_backlog",
        MinoriSystemUiAction::ReplayBacklogVoice => "replay_backlog_voice",
        MinoriSystemUiAction::SaveSlot(_) => "save_slot",
        MinoriSystemUiAction::LoadSlot(_) => "load_slot",
        MinoriSystemUiAction::CloseGameplaySystemPage => "close_gameplay_system_page",
        MinoriSystemUiAction::GalleryBgmPlay => "gallery_bgm_play",
        MinoriSystemUiAction::GalleryBgmStop => "gallery_bgm_stop",
        MinoriSystemUiAction::GalleryReplayStart(_) => "gallery_replay_start",
        MinoriSystemUiAction::GalleryMovieStart(_) => "gallery_movie_start",
        MinoriSystemUiAction::Exit => "exit",
    }
}

fn system_page_name(page: MinoriSystemPage) -> &'static str {
    match page {
        MinoriSystemPage::None => "none",
        MinoriSystemPage::Title => "title",
        MinoriSystemPage::Load => "load",
        MinoriSystemPage::Save => "save",
        MinoriSystemPage::Config => "config",
        MinoriSystemPage::Backlog => "backlog",
        MinoriSystemPage::Memories => "memories",
        MinoriSystemPage::GalleryCg => "gallery_cg",
        MinoriSystemPage::GalleryBgm => "gallery_bgm",
        MinoriSystemPage::GalleryReplay => "gallery_replay",
        MinoriSystemPage::GalleryMovie => "gallery_movie",
    }
}

fn append_system_page_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<MinoriSystemPage>, LegacyProviderError> {
    let page = session.vm.state().system_ui.page;
    if session.reported_system_page == Some(page) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.system_page".into(),
        value: system_page_name(page).into(),
    });
    let activity_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence: activity_sequence,
        key: LEGACY_SYSTEM_UI_ACTIVE_BLACKBOARD_KEY.into(),
        value: (page != MinoriSystemPage::None).to_string(),
    });
    Ok(Some(page))
}

fn play_mode_name(mode: MinoriPlayMode) -> &'static str {
    match mode {
        MinoriPlayMode::Normal => "normal",
        MinoriPlayMode::Auto => "auto",
        MinoriPlayMode::Skip => "skip",
    }
}

fn append_play_mode_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<MinoriPlayMode>, LegacyProviderError> {
    let mode = session.vm.state().system_ui.play_mode;
    if session.reported_play_mode == Some(mode) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.play_mode".into(),
        value: play_mode_name(mode).into(),
    });
    Ok(Some(mode))
}

fn append_gallery_unlock_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<usize>, LegacyProviderError> {
    let count = session.vm.state().gallery_unlocks.len();
    if session.reported_gallery_unlock_count == Some(count) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.gallery_unlock_count".into(),
        value: count.to_string(),
    });
    Ok(Some(count))
}

fn append_choice_active_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<bool>, LegacyProviderError> {
    let active = matches!(
        session.vm.state().wait.as_ref(),
        Some(MinoriWaitState::Choice { .. })
    );
    if session.reported_choice_active == Some(active) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.choice_active".into(),
        value: active.to_string(),
    });
    Ok(Some(active))
}

fn append_progress_in_background_observation(
    session: &mut MinoriSession,
    control: &mut LegacyControlTransaction,
) -> Result<Option<bool>, LegacyProviderError> {
    let enabled = session.vm.persistent_config().progress_in_background;
    if session.reported_progress_in_background == Some(enabled) {
        return Ok(None);
    }
    let sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    control.blackboard.push(LegacyBlackboardMutation {
        sequence,
        key: "minori.progress_in_background".into(),
        value: enabled.to_string(),
    });
    Ok(Some(enabled))
}

fn encode_global_progress(unlocks: &[Hash256]) -> Result<Vec<u8>, LegacyProviderError> {
    let progress = MinoriGlobalProgressV1 {
        schema: MINORI_GLOBAL_PROGRESS_SCHEMA.into(),
        gallery_unlocks: unlocks.to_vec(),
    };
    postcard::to_allocvec(&progress).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_ENCODE",
            "global progress payload could not be encoded",
        )
    })
}

fn decode_global_progress(bytes: &[u8]) -> Result<Vec<Hash256>, LegacyProviderError> {
    let progress: MinoriGlobalProgressV1 = postcard::from_bytes(bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_DECODE",
            "global progress payload could not be decoded",
        )
    })?;
    if progress.schema != MINORI_GLOBAL_PROGRESS_SCHEMA {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_SCHEMA",
            "global progress payload schema is unsupported",
        ));
    }
    Ok(progress.gallery_unlocks)
}

fn load_global_progress(
    writable_files: &dyn astra_emu_family_api::LegacyWritableFileHostV1,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
) -> Result<(), LegacyProviderError> {
    if !session.global_progress.enabled || session.global_progress.loaded {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_LOAD_STATE",
            "global progress load was requested in an invalid session state",
        ));
    }
    let stat = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::Stat {
            path: MINORI_GLOBAL_PROGRESS_PATH.into(),
        },
    )?;
    if !stat.exists {
        if stat.is_file || stat.length != 0 || !stat.entries.is_empty() || !stat.bytes.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_STAT",
                "missing global progress returned contradictory metadata",
            ));
        }
        session.global_progress.loaded = true;
        return Ok(());
    }
    if !stat.is_file
        || stat.length == 0
        || stat.length > MAX_GLOBAL_PROGRESS_BYTES
        || !stat.entries.is_empty()
        || !stat.bytes.is_empty()
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_STAT",
            "global progress stat metadata is invalid or outside its byte bound",
        ));
    }
    let read = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::ReadRange {
            path: MINORI_GLOBAL_PROGRESS_PATH.into(),
            offset: 0,
            length: stat.length,
        },
    )?;
    if !read.exists
        || !read.is_file
        || read.length != stat.length
        || read.bytes.len() as u64 != stat.length
        || !read.entries.is_empty()
        || read.written != 0
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_READ",
            "global progress read result does not match the prior stat",
        ));
    }
    let unlocks = decode_global_progress(read.bytes.as_slice())?;
    session
        .vm
        .merge_verified_gallery_unlocks(&unlocks)
        .map_err(runtime_error)?;
    session.global_progress.loaded = true;
    session.global_progress.persisted_unlocks = unlocks;
    Ok(())
}

fn store_global_progress_if_changed(
    writable_files: &dyn astra_emu_family_api::LegacyWritableFileHostV1,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
    output: &mut LegacyStepOutput,
) -> Result<(), LegacyProviderError> {
    if !session.global_progress.enabled
        || !session.global_progress.loaded
        || session.vm.state().gallery_unlocks == session.global_progress.persisted_unlocks
    {
        return Ok(());
    }
    let unlocks = session.vm.state().gallery_unlocks.clone();
    let payload = encode_global_progress(&unlocks)?;
    let create = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::CreateDir {
            path: MINORI_GLOBAL_PROGRESS_DIRECTORY.into(),
        },
    )?;
    validate_writable_mutation_result(
        &create,
        0,
        "create directory",
        "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_WRITE",
    )?;
    let truncate = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
            path: MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH.into(),
            length: 0,
        },
    )?;
    validate_writable_mutation_result(
        &truncate,
        0,
        "truncate temporary progress",
        "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_WRITE",
    )?;
    let write = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::WriteRange {
            path: MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH.into(),
            offset: 0,
            bytes: payload.clone(),
        },
    )?;
    validate_writable_mutation_result(
        &write,
        payload.len() as u64,
        "write global progress",
        "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_WRITE",
    )?;
    let length = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
            path: MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH.into(),
            length: payload.len() as u64,
        },
    )?;
    validate_writable_mutation_result(
        &length,
        0,
        "finalize global progress length",
        "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_WRITE",
    )?;
    let replace = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::AtomicReplace {
            temporary_path: MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH.into(),
            destination_path: MINORI_GLOBAL_PROGRESS_PATH.into(),
        },
    )?;
    validate_writable_mutation_result(
        &replace,
        0,
        "replace global progress",
        "ASTRA_EMU_MINORI_GLOBAL_PROGRESS_WRITE",
    )?;
    session.global_progress.persisted_unlocks = unlocks;
    output.state_revision = session.vm.state().fixed_tick;
    Ok(())
}

fn validate_writable_mutation_result(
    result: &astra_emu_family_api::LegacyWritableFileResultV1,
    expected_written: u64,
    operation: &'static str,
    diagnostic_code: &'static str,
) -> Result<(), LegacyProviderError> {
    if !result.entries.is_empty() || !result.bytes.is_empty() || result.written != expected_written
    {
        return Err(LegacyProviderError::invalid(
            diagnostic_code,
            format!("writable-file result for {operation} is invalid"),
        ));
    }
    Ok(())
}

fn load_persistent_config(
    writable_files: &dyn astra_emu_family_api::LegacyWritableFileHostV1,
    session_id: &LegacyRuntimeSessionId,
    case_fingerprint: Hash256,
    package_hash: Hash256,
    profile_fingerprint: Hash256,
) -> Result<(MinoriConfigState, u32), LegacyProviderError> {
    let stat = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::Stat {
            path: MINORI_CONFIG_PATH.into(),
        },
    )?;
    if !stat.exists {
        if stat.is_file || stat.length != 0 || !stat.entries.is_empty() || !stat.bytes.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_CONFIG_STAT",
                "missing config returned contradictory metadata",
            ));
        }
        return Ok((MinoriConfigState::default(), 0));
    }
    if !stat.is_file
        || stat.length == 0
        || stat.length > MINORI_CONFIG_MAX_BYTES as u64
        || !stat.entries.is_empty()
        || !stat.bytes.is_empty()
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_STAT",
            "config stat metadata is invalid or outside its byte bound",
        ));
    }
    let read = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::ReadRange {
            path: MINORI_CONFIG_PATH.into(),
            offset: 0,
            length: stat.length,
        },
    )?;
    if !read.exists
        || !read.is_file
        || read.length != stat.length
        || read.bytes.len() as u64 != stat.length
        || !read.entries.is_empty()
        || read.written != 0
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_READ",
            "config read result does not match the prior stat",
        ));
    }
    let envelope = decode_config(read.bytes.as_slice()).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CONFIG_FORMAT",
            "config envelope is malformed",
        )
    })?;
    if envelope.schema != MINORI_CONFIG_SCHEMA
        || envelope.case_fingerprint != case_fingerprint
        || envelope.package_hash != package_hash
        || envelope.profile_fingerprint != profile_fingerprint
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_IDENTITY",
            "config identity does not match the active case",
        ));
    }
    envelope.config.validate().map_err(runtime_error)?;
    if envelope.quick_save_cursor >= MINORI_QUICK_SAVE_SLOT_COUNT {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_CURSOR",
            "quick save cursor is outside the quick-save page width",
        ));
    }
    Ok((envelope.config, envelope.quick_save_cursor))
}

fn store_persistent_config_if_changed(
    writable_files: &dyn astra_emu_family_api::LegacyWritableFileHostV1,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
) -> Result<(), LegacyProviderError> {
    if !session.config_storage_enabled
        || (session.vm.persistent_config() == &session.config_persisted
            && session.quick_save_cursor == session.config_persisted_cursor)
    {
        return Ok(());
    }
    let envelope = MinoriConfigEnvelope {
        schema: MINORI_CONFIG_SCHEMA.into(),
        case_fingerprint: session.case_fingerprint,
        package_hash: session.package_hash,
        profile_fingerprint: session.profile_fingerprint,
        config: session.vm.persistent_config().clone(),
        quick_save_cursor: session.quick_save_cursor,
    };
    let payload = encode_config(&envelope).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CONFIG_ENCODE",
            "config envelope could not be encoded",
        )
    })?;
    if payload.is_empty() || payload.len() > MINORI_CONFIG_MAX_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_BOUNDS",
            "config envelope exceeds the bounded size",
        ));
    }
    let create = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::CreateDir {
            path: MINORI_CONFIG_ROOT.into(),
        },
    )?;
    validate_writable_mutation_result(
        &create,
        0,
        "create config directory",
        "ASTRA_EMU_MINORI_CONFIG_WRITE",
    )?;
    let truncate = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
            path: MINORI_CONFIG_TEMPORARY_PATH.into(),
            length: 0,
        },
    )?;
    validate_writable_mutation_result(
        &truncate,
        0,
        "truncate config temporary",
        "ASTRA_EMU_MINORI_CONFIG_WRITE",
    )?;
    let write = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::WriteRange {
            path: MINORI_CONFIG_TEMPORARY_PATH.into(),
            offset: 0,
            bytes: payload.clone(),
        },
    )?;
    validate_writable_mutation_result(
        &write,
        payload.len() as u64,
        "write config",
        "ASTRA_EMU_MINORI_CONFIG_WRITE",
    )?;
    let length = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
            path: MINORI_CONFIG_TEMPORARY_PATH.into(),
            length: payload.len() as u64,
        },
    )?;
    validate_writable_mutation_result(
        &length,
        0,
        "finalize config length",
        "ASTRA_EMU_MINORI_CONFIG_WRITE",
    )?;
    let replace = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::AtomicReplace {
            temporary_path: MINORI_CONFIG_TEMPORARY_PATH.into(),
            destination_path: MINORI_CONFIG_PATH.into(),
        },
    )?;
    validate_writable_mutation_result(
        &replace,
        0,
        "replace config",
        "ASTRA_EMU_MINORI_CONFIG_WRITE",
    )?;
    session.config_persisted = envelope.config;
    session.config_persisted_cursor = session.quick_save_cursor;
    Ok(())
}

fn validate_save_slot(slot: u32) -> Result<(), LegacyProviderError> {
    if slot >= MINORI_SAVE_MAX_SLOTS {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_SLOT",
            "save slot is outside the verified 100-slot range",
        ));
    }
    Ok(())
}

fn refresh_save_slots(
    writable_files: &dyn astra_emu_family_api::LegacyWritableFileHostV1,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
) -> Result<(), LegacyProviderError> {
    let create = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::CreateDir {
            path: MINORI_SAVE_ROOT.into(),
        },
    )?;
    validate_writable_mutation_result(
        &create,
        0,
        "prepare save directory",
        "ASTRA_EMU_MINORI_SAVE_WRITE",
    )?;
    let result = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::List {
            path: MINORI_SAVE_ROOT.into(),
        },
    )?;
    if result.written != 0 || !result.bytes.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_LIST",
            "save slot listing returned an unexpected payload",
        ));
    }
    let mut slots = BTreeSet::new();
    let mut slot_lengths = BTreeMap::new();
    let mut slot_comments = BTreeMap::new();
    let mut slot_metadata = BTreeMap::new();
    for entry in result.entries {
        if !entry.is_file {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_LIST",
                "save slot directory contains a non-file entry",
            ));
        }
        let Some(slot) = entry
            .name
            .strip_prefix("slot-")
            .and_then(|name| name.strip_suffix(".bin"))
            .filter(|name| name.len() == 3)
            .and_then(|name| name.parse::<u32>().ok())
        else {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_LIST",
                "save slot directory contains an invalid file name",
            ));
        };
        validate_save_slot(slot)?;
        if entry.length == 0 || entry.length > MINORI_SAVE_MAX_BYTES as u64 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_SLOT_BOUNDS",
                "save slot file length is outside the bounded format",
            ));
        }
        if !slots.insert(slot) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_LIST",
                "save slot directory contains a duplicate slot",
            ));
        }
        let metadata = if session.save_slot_lengths.get(&slot) == Some(&entry.length) {
            session
                .save_slot_metadata
                .get(&slot)
                .cloned()
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_SAVE_LIST_STATE",
                        "save slot metadata cache is incomplete",
                    )
                })?
        } else {
            read_save_slot_metadata(writable_files, session_id, session, slot, entry.length)?
        };
        slot_lengths.insert(slot, entry.length);
        slot_comments.insert(slot, metadata.comment.clone());
        slot_metadata.insert(slot, metadata);
    }
    let metadata_changed = session.save_slot_metadata != slot_metadata;
    session.save_slots = slots;
    session.save_slot_lengths = slot_lengths;
    session.save_slot_comments = slot_comments;
    session.save_slot_metadata = slot_metadata;
    if metadata_changed {
        // The retained panel descriptor does not include private save metadata.
        // Invalidate it so a changed timestamp/comment/thumbnail is published
        // on the next system-page output instead of leaving stale pixels in the
        // host-owned surface.
        session.last_resource_frame = None;
    }
    Ok(())
}

fn read_save_slot_metadata(
    writable_files: &dyn astra_emu_family_api::LegacyWritableFileHostV1,
    session_id: &LegacyRuntimeSessionId,
    session: &MinoriSession,
    slot: u32,
    expected_length: u64,
) -> Result<MinoriSaveSlotMetadata, LegacyProviderError> {
    let path = slot_path(slot);
    let stat = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::Stat { path: path.clone() },
    )?;
    if !stat.exists || !stat.is_file || stat.length != expected_length {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_LIST_CHANGED",
            "save slot changed while its metadata was being read",
        ));
    }
    let read = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::ReadRange {
            path,
            offset: 0,
            length: expected_length,
        },
    )?;
    if read.written != 0 || read.bytes.len() as u64 != expected_length {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_LIST_READ",
            "save slot comment read returned a short or unexpected payload",
        ));
    }
    decode_save_slot_metadata(
        read.bytes.as_slice(),
        session,
        SaveSlotMetadataContext::List,
    )
}

fn decode_save_slot_metadata(
    bytes: &[u8],
    session: &MinoriSession,
    context: SaveSlotMetadataContext,
) -> Result<MinoriSaveSlotMetadata, LegacyProviderError> {
    let envelope = decode_save(bytes).map_err(|_| {
        invalid(
            context.diagnostic(SaveSlotMetadataDiagnostic::Format),
            "save slot envelope is malformed",
        )
    })?;
    save_slot_metadata_from_envelope(&envelope, session, context)
}

fn save_slot_metadata_from_envelope(
    envelope: &MinoriSaveEnvelope,
    session: &MinoriSession,
    context: SaveSlotMetadataContext,
) -> Result<MinoriSaveSlotMetadata, LegacyProviderError> {
    if envelope.schema != MINORI_SAVE_SCHEMA
        || envelope.case_fingerprint != session.case_fingerprint
        || envelope.package_hash != session.package_hash
        || envelope.profile_fingerprint != session.profile_fingerprint
    {
        return Err(invalid(
            context.diagnostic(SaveSlotMetadataDiagnostic::Identity),
            "save slot identity does not match the active case",
        ));
    }
    validate_save_timestamp(&envelope.timestamp).map_err(|_| {
        invalid(
            context.diagnostic(SaveSlotMetadataDiagnostic::Timestamp),
            "save slot timestamp is malformed",
        )
    })?;
    if envelope.comment.len() > MINORI_SAVE_COMMENT_MAX_BYTES
        || envelope.comment.chars().any(char::is_control)
    {
        return Err(invalid(
            context.diagnostic(SaveSlotMetadataDiagnostic::Comment),
            "save slot comment is invalid or exceeds the bounded text length",
        ));
    }
    let thumbnail_rgba = decode_save_thumbnail(&envelope.thumbnail_png).map_err(|_| {
        invalid(
            context.diagnostic(SaveSlotMetadataDiagnostic::Thumbnail),
            "save slot thumbnail is malformed or outside the verified dimensions",
        )
    })?;
    Ok(MinoriSaveSlotMetadata {
        timestamp: envelope.timestamp.clone(),
        comment: envelope.comment.clone(),
        thumbnail_rgba,
    })
}

fn validate_save_timestamp(timestamp: &str) -> Result<(), ()> {
    let bytes = timestamp.as_bytes();
    if bytes.len() != MINORI_SAVE_TIMESTAMP_MAX_BYTES
        || bytes[4] != b'/'
        || bytes[7] != b'/'
        || bytes[10] != b' '
        || bytes[13] != b':'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7 | 10 | 13) && !byte.is_ascii_digit())
    {
        return Err(());
    }
    let parse = |start: usize, end: usize| {
        timestamp
            .get(start..end)
            .ok_or(())
            .and_then(|value| value.parse::<u32>().map_err(|_| ()))
    };
    let year = parse(0, 4)?;
    let month = parse(5, 7)?;
    let day = parse(8, 10)?;
    let hour = parse(11, 13)?;
    let minute = parse(14, 16)?;
    if !(1..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
    {
        return Err(());
    }
    Ok(())
}

fn decode_save_thumbnail(bytes: &[u8]) -> Result<Arc<[u8]>, ()> {
    if bytes.is_empty() || bytes.len() > MINORI_SAVE_THUMBNAIL_MAX_BYTES {
        return Err(());
    }
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png).map_err(|_| ())?;
    if image.width() != MINORI_SAVE_THUMBNAIL_WIDTH
        || image.height() != MINORI_SAVE_THUMBNAIL_HEIGHT
    {
        return Err(());
    }
    let mut rgba = image.into_rgba8().into_raw();
    let expected = usize::try_from(MINORI_SAVE_THUMBNAIL_WIDTH)
        .ok()
        .and_then(|width| width.checked_mul(usize::try_from(MINORI_SAVE_THUMBNAIL_HEIGHT).ok()?))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(())?;
    if rgba.len() != expected {
        return Err(());
    }
    premultiply_rgba8(&mut rgba);
    Ok(Arc::from(rgba))
}

fn format_save_timestamp() -> Result<String, LegacyProviderError> {
    let now = time::OffsetDateTime::now_local().map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_TIMESTAMP",
            "local time is unavailable for the original save-card format",
        )
    })?;
    let timestamp = format!(
        "{:04}/{:02}/{:02} {:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute()
    );
    validate_save_timestamp(&timestamp).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_TIMESTAMP",
            "current local time cannot be represented in the Minori save format",
        )
    })?;
    Ok(timestamp)
}

fn encode_save_thumbnail(frame: &[u8]) -> Result<Vec<u8>, LegacyProviderError> {
    let expected = usize::try_from(1280_u32)
        .ok()
        .and_then(|width| width.checked_mul(usize::try_from(720_u32).ok()?))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "thumbnail size overflowed",
            )
        })?;
    if frame.len() != expected {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "current gameplay frame does not match the verified stage",
        ));
    }
    let mut straight_rgba = frame.to_vec();
    unpremultiply_rgba8(&mut straight_rgba);
    let image = image::RgbaImage::from_raw(1280, 720, straight_rgba).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "current gameplay frame could not be wrapped as RGBA8",
        )
    })?;
    let thumbnail = image::imageops::resize(
        &image,
        MINORI_SAVE_THUMBNAIL_WIDTH,
        MINORI_SAVE_THUMBNAIL_HEIGHT,
        image::imageops::FilterType::Triangle,
    );
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(
            thumbnail.as_raw(),
            MINORI_SAVE_THUMBNAIL_WIDTH,
            MINORI_SAVE_THUMBNAIL_HEIGHT,
            ExtendedColorType::Rgba8,
        )
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "save thumbnail PNG encoding failed",
            )
        })?;
    if encoded.is_empty() || encoded.len() > MINORI_SAVE_THUMBNAIL_MAX_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "save thumbnail exceeds the bounded PNG size",
        ));
    }
    Ok(encoded)
}

fn unpremultiply_rgba8(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel =
                u8::try_from((u16::from(*channel) * 255_u16 + alpha / 2) / alpha).unwrap_or(255);
        }
    }
}

fn save_slot(
    writable_files: &dyn astra_emu_family_api::LegacyWritableFileHostV1,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
    slot: u32,
    comment: &str,
) -> Result<u64, LegacyProviderError> {
    validate_save_slot(slot)?;
    if session.vm.state().system_ui.page != MinoriSystemPage::Save {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_PAGE",
            "save slot writes require the save page",
        ));
    }
    if comment.len() > MINORI_SAVE_COMMENT_MAX_BYTES || comment.chars().any(char::is_control) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_COMMENT",
            "save comment is invalid or exceeds the bounded text length",
        ));
    }
    let gameplay_frame = session.last_gameplay_frame.as_deref().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL_MISSING",
            "save requires a previously presented gameplay frame",
        )
    })?;
    let thumbnail_png = encode_save_thumbnail(gameplay_frame)?;
    let thumbnail_rgba = decode_save_thumbnail(&thumbnail_png).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "encoded save thumbnail could not be decoded",
        )
    })?;
    let timestamp = format_save_timestamp()?;
    let mut state = MinoriVm::decode_snapshot(&session.vm.snapshot_bytes().map_err(runtime_error)?)
        .map_err(runtime_error)?;
    state.system_ui.page = MinoriSystemPage::None;
    state.system_ui.focus_index = 0;
    state.system_ui.pending_save_slot = None;
    state.system_ui.pending_load_slot = None;
    state.system_ui.backlog_cursor = None;
    let vm_snapshot = postcard::to_allocvec(&state).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_ENCODE",
            "save state could not be encoded",
        )
    })?;
    let envelope = MinoriSaveEnvelope {
        schema: MINORI_SAVE_SCHEMA.into(),
        case_fingerprint: session.case_fingerprint,
        package_hash: session.package_hash,
        profile_fingerprint: session.profile_fingerprint,
        script_uri: state.script_uri.clone(),
        script_hash: state.script_hash,
        timestamp: timestamp.clone(),
        comment: comment.into(),
        thumbnail_png,
        vm_snapshot,
    };
    let payload = encode_save(&envelope).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_ENCODE",
            "save envelope could not be encoded",
        )
    })?;
    if payload.is_empty() || payload.len() > MINORI_SAVE_MAX_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_BOUNDS",
            "save envelope exceeds the bounded slot size",
        ));
    }
    let create = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::CreateDir {
            path: MINORI_SAVE_ROOT.into(),
        },
    )?;
    validate_writable_mutation_result(
        &create,
        0,
        "create save directory",
        "ASTRA_EMU_MINORI_SAVE_WRITE",
    )?;
    let temporary_path = slot_temporary_path(slot);
    let destination_path = slot_path(slot);
    let truncate = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
            path: temporary_path.clone(),
            length: 0,
        },
    )?;
    validate_writable_mutation_result(
        &truncate,
        0,
        "truncate save temporary",
        "ASTRA_EMU_MINORI_SAVE_WRITE",
    )?;
    let write = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::WriteRange {
            path: temporary_path.clone(),
            offset: 0,
            bytes: payload.clone(),
        },
    )?;
    validate_writable_mutation_result(
        &write,
        payload.len() as u64,
        "write save slot",
        "ASTRA_EMU_MINORI_SAVE_WRITE",
    )?;
    let length = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
            path: temporary_path,
            length: payload.len() as u64,
        },
    )?;
    validate_writable_mutation_result(
        &length,
        0,
        "finalize save slot length",
        "ASTRA_EMU_MINORI_SAVE_WRITE",
    )?;
    let replace = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::AtomicReplace {
            temporary_path: slot_temporary_path(slot),
            destination_path,
        },
    )?;
    validate_writable_mutation_result(
        &replace,
        0,
        "replace save slot",
        "ASTRA_EMU_MINORI_SAVE_WRITE",
    )?;
    session.save_slots.insert(slot);
    session.save_slot_lengths.insert(slot, payload.len() as u64);
    session.save_slot_comments.insert(slot, comment.to_owned());
    session.save_slot_metadata.insert(
        slot,
        MinoriSaveSlotMetadata {
            timestamp,
            comment: comment.to_owned(),
            thumbnail_rgba,
        },
    );
    Ok(payload.len() as u64)
}

fn load_slot(
    writable_files: &dyn astra_emu_family_api::LegacyWritableFileHostV1,
    vfs: &Arc<dyn LegacyVfsReader>,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
    slot: u32,
    host_tick: u64,
) -> Result<(), LegacyProviderError> {
    validate_save_slot(slot)?;
    if session.vm.state().system_ui.page != MinoriSystemPage::Load {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LOAD_PAGE",
            "load slot reads require the load page",
        ));
    }
    let path = slot_path(slot);
    let stat = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::Stat { path: path.clone() },
    )?;
    if !stat.exists || !stat.is_file {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LOAD_SLOT_EMPTY",
            "selected load slot is empty",
        ));
    }
    if stat.length == 0 || stat.length > MINORI_SAVE_MAX_BYTES as u64 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LOAD_SLOT_BOUNDS",
            "selected load slot exceeds the bounded format",
        ));
    }
    let read = writable_files.execute(
        &session_id.0,
        astra_emu_family_api::LegacyWritableFileRequestV1::ReadRange {
            path,
            offset: 0,
            length: stat.length,
        },
    )?;
    if read.written != 0 || read.bytes.len() as u64 != stat.length {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LOAD_SLOT_READ",
            "load slot read returned a short or unexpected payload",
        ));
    }
    let envelope = decode_save(read.bytes.as_slice()).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_LOAD_SLOT_FORMAT",
            "load slot envelope is malformed",
        )
    })?;
    let metadata =
        save_slot_metadata_from_envelope(&envelope, session, SaveSlotMetadataContext::Load)?;
    validate_script_uri(&envelope.script_uri)?;
    let target = envelope
        .script_uri
        .strip_prefix("minori:/scr/")
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_LOAD_SLOT_SCRIPT",
                "load script URI is invalid",
            )
        })?;
    let (script_uri, script_hash, script) = load_script(vfs, &session.mount_set_id, target)?;
    if script_uri != envelope.script_uri || script_hash != envelope.script_hash {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LOAD_SLOT_SCRIPT",
            "load slot script identity does not match the mounted VFS",
        ));
    }
    let mut state = MinoriVm::decode_snapshot(&envelope.vm_snapshot).map_err(runtime_error)?;
    if state.system_ui.page != MinoriSystemPage::None || state.terminal {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LOAD_SLOT_STATE",
            "load slot contains an invalid gameplay continuation state",
        ));
    }
    if state
        .audio
        .values()
        .any(|audio| audio.playing && audio.continuation_pts != 0)
        || state
            .movie
            .as_ref()
            .is_some_and(|movie| movie.continuation_pts != 0)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MEDIA_CONTINUATION_UNSUPPORTED",
            "v9 media Play cannot restore a non-zero continuation position",
        ));
    }
    // The original configuration store is installation-scoped rather than a
    // gameplay slot. Keep the active persisted settings when restoring the
    // VM so loading an older slot cannot silently roll back audio, text, or
    // input preferences.
    state.system_ui.config = session.vm.persistent_config().clone();
    // The slot's fixed tick belongs to the previous host session. The
    // continuation is rebased to the current host tick below; rejecting a
    // valid save merely because the new session has fewer elapsed ticks would
    // make title-screen load impossible.
    state.fixed_tick = host_tick;
    let adjusted_snapshot = postcard::to_allocvec(&state).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_LOAD_SLOT_STATE",
            "load continuation state could not be encoded",
        )
    })?;
    replace_vm_script(
        vfs,
        &session.mount_set_id,
        &mut session.vm,
        script_uri,
        script_hash,
        script,
    )?;
    session
        .vm
        .restore_state(&adjusted_snapshot)
        .map_err(runtime_error)?;
    session.restore_audio_pending = true;
    session.restore_presentation_pending = true;
    session.reported_system_page = None;
    session.reported_play_mode = None;
    session.reported_gallery_unlock_count = None;
    session.reported_choice_active = None;
    session.reported_progress_in_background = Some(false);
    session.save_slots.insert(slot);
    session.save_slot_lengths.insert(slot, stat.length);
    session
        .save_slot_comments
        .insert(slot, metadata.comment.clone());
    session.save_slot_metadata.insert(slot, metadata);
    session.last_gameplay_frame = None;
    session.last_text_surface = None;
    session.last_resource_frame = None;
    session.presentation_layers.clear();
    session.title_pointer_focus = None;
    Ok(())
}

fn title_menu_item_count(variant: u8) -> u32 {
    if variant == 2 {
        MINORI_TITLE_MEMORIES_ITEM_COUNT
    } else {
        MINORI_TITLE_BASE_ITEM_COUNT
    }
}

fn title_menu_focus_at(variant: u8, x: i32, y: i32) -> Option<u32> {
    if !(MINORI_TITLE_MENU_LEFT..MINORI_TITLE_MENU_RIGHT).contains(&x) {
        return None;
    }
    let row_tops = if variant == 2 {
        &MINORI_TITLE_MENU_ROW_TOPS_MEMORIES[..]
    } else {
        &MINORI_TITLE_MENU_ROW_TOPS_BASE[..]
    };
    row_tops
        .iter()
        .position(|top| (*top..(*top + MINORI_TITLE_MENU_ROW_HEIGHT)).contains(&y))
        .and_then(|index| u32::try_from(index).ok())
}

fn title_menu_hover_scissor(
    variant: u8,
    focus_index: u32,
) -> Result<LegacyScissorV1, LegacyProviderError> {
    let row = usize::try_from(focus_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_TITLE_POINTER",
            "title pointer focus cannot be represented",
        )
    })?;
    let row_tops = if variant == 2 {
        &MINORI_TITLE_MENU_ROW_TOPS_MEMORIES[..]
    } else {
        &MINORI_TITLE_MENU_ROW_TOPS_BASE[..]
    };
    let top = *row_tops.get(row).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_TITLE_POINTER",
            "title pointer focus is outside the verified menu bounds",
        )
    })?;
    Ok(LegacyScissorV1 {
        x: MINORI_TITLE_MENU_HOVER_SCISSOR_X,
        y: top,
        width: MINORI_TITLE_MENU_RIGHT - MINORI_TITLE_MENU_HOVER_SCISSOR_X,
        height: MINORI_TITLE_MENU_ROW_HEIGHT,
    })
}

fn activate_title_focus(vm: &mut MinoriVm) -> Result<MinoriSystemUiAction, LegacyProviderError> {
    match (vm.title_variant(), vm.state().system_ui.focus_index) {
        (_, 0) => {
            vm.set_system_page(MinoriSystemPage::None, 0)
                .map_err(runtime_error)?;
            Ok(MinoriSystemUiAction::StartGame)
        }
        (_, 1) => {
            vm.set_system_page(MinoriSystemPage::Load, 0)
                .map_err(runtime_error)?;
            Ok(MinoriSystemUiAction::Present)
        }
        (_, 2) => {
            vm.open_config().map_err(runtime_error)?;
            Ok(MinoriSystemUiAction::Present)
        }
        (2, 3) => {
            vm.set_system_page(MinoriSystemPage::Memories, 0)
                .map_err(runtime_error)?;
            Ok(MinoriSystemUiAction::Present)
        }
        (_, 3) | (2, 4) => Ok(MinoriSystemUiAction::Exit),
        _ => Err(invalid(
            "ASTRA_EMU_MINORI_TITLE_FOCUS",
            "title focus is outside the verified menu bounds",
        )),
    }
}

fn apply_system_ui_input(
    vm: &mut MinoriVm,
    input: &LegacyStepInput,
) -> Result<MinoriSystemUiAction, LegacyProviderError> {
    if vm.state().system_ui.page == MinoriSystemPage::Config {
        return apply_config_input(vm, input);
    }
    if vm.state().system_ui.page == MinoriSystemPage::Backlog {
        let wheel = backlog_wheel_direction(input)?;
        let replay = input
            .input_edges
            .iter()
            .any(|edge| edge.pressed && edge.control == "enter");
        if replay && wheel.is_some() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_BACKLOG_INPUT_AMBIGUOUS",
                "backlog voice replay cannot share a tick with wheel navigation",
            ));
        }
        return match (wheel, replay) {
            (Some(-1), false) => {
                vm.move_backlog(-1).map_err(runtime_error)?;
                Ok(MinoriSystemUiAction::Present)
            }
            (Some(1), false) => Ok(MinoriSystemUiAction::CloseBacklog),
            (None, true) => Ok(MinoriSystemUiAction::ReplayBacklogVoice),
            (None, false) => Ok(MinoriSystemUiAction::Present),
            (Some(_), _) => unreachable!("backlog wheel direction is normalized"),
        };
    }
    if matches!(
        vm.state().system_ui.page,
        MinoriSystemPage::Save | MinoriSystemPage::Load
    ) {
        let page = vm.state().system_ui.page;
        for edge in input.input_edges.iter().filter(|edge| edge.pressed) {
            match edge.control.as_str() {
                "arrow_up" => vm
                    .move_system_focus(-1, MINORI_SAVE_MAX_SLOTS)
                    .map_err(runtime_error)?,
                "arrow_down" => vm
                    .move_system_focus(1, MINORI_SAVE_MAX_SLOTS)
                    .map_err(runtime_error)?,
                "arrow_left" => vm.move_save_page(-1).map_err(runtime_error)?,
                "arrow_right" => vm.move_save_page(1).map_err(runtime_error)?,
                "escape" => {
                    if page == MinoriSystemPage::Load && vm.state().wait.is_none() {
                        vm.set_system_page(MinoriSystemPage::Title, 0)
                            .map_err(runtime_error)?;
                        return Ok(MinoriSystemUiAction::Present);
                    }
                    return Ok(MinoriSystemUiAction::CloseGameplaySystemPage);
                }
                "load" if page == MinoriSystemPage::Save => {
                    vm.open_load_page().map_err(runtime_error)?;
                    return Ok(MinoriSystemUiAction::Present);
                }
                "enter" | "space" => {
                    return Ok(match page {
                        MinoriSystemPage::Save => {
                            MinoriSystemUiAction::SaveSlot(vm.state().system_ui.focus_index)
                        }
                        MinoriSystemPage::Load => {
                            MinoriSystemUiAction::LoadSlot(vm.state().system_ui.focus_index)
                        }
                        _ => unreachable!("save/load branch has a verified page"),
                    });
                }
                _ => {}
            }
        }
        if input.input_edges.iter().any(|edge| {
            edge.pressed
                && edge.control == MINORI_POINTER_PRIMARY
                && (462..818).contains(&vm.state().system_ui.pointer_x)
                && (650..720).contains(&vm.state().system_ui.pointer_y)
        }) {
            let x = vm.state().system_ui.pointer_x;
            if (462..578).contains(&x) {
                vm.move_save_page(-1).map_err(runtime_error)?;
                return Ok(MinoriSystemUiAction::Present);
            }
            if (578..700).contains(&x) {
                vm.move_save_page(1).map_err(runtime_error)?;
                return Ok(MinoriSystemUiAction::Present);
            }
            if page == MinoriSystemPage::Load && vm.state().wait.is_none() {
                vm.set_system_page(MinoriSystemPage::Title, 0)
                    .map_err(runtime_error)?;
                return Ok(MinoriSystemUiAction::Present);
            }
            return Ok(MinoriSystemUiAction::CloseGameplaySystemPage);
        }
        if input.input_edges.iter().any(|edge| {
            edge.pressed
                && edge.control == MINORI_POINTER_PRIMARY
                && (64..800).contains(&vm.state().system_ui.pointer_x)
                && (81..611).contains(&vm.state().system_ui.pointer_y)
        }) {
            let x = vm.state().system_ui.pointer_x;
            let y = vm.state().system_ui.pointer_y;
            let column = if (64..408).contains(&x) {
                Some(0u32)
            } else if (456..800).contains(&x) {
                Some(1u32)
            } else {
                None
            };
            let row = [81, 189, 297, 405, 513]
                .iter()
                .position(|top| (*top..(*top + 98)).contains(&y))
                .map(|row| u32::try_from(row).expect("five save rows fit u32"));
            if let (Some(column), Some(row)) = (column, row) {
                let slot = (vm.state().system_ui.focus_index / 10) * 10 + row * 2 + column;
                vm.set_save_focus(slot).map_err(runtime_error)?;
                return Ok(match page {
                    MinoriSystemPage::Save => MinoriSystemUiAction::SaveSlot(slot),
                    MinoriSystemPage::Load => MinoriSystemUiAction::LoadSlot(slot),
                    _ => unreachable!("save/load branch has a verified page"),
                });
            }
        }
        return Ok(MinoriSystemUiAction::Present);
    }
    let title_pointer_pressed = vm.state().system_ui.page == MinoriSystemPage::Title
        && input
            .input_edges
            .iter()
            .any(|edge| edge.pressed && edge.control == MINORI_POINTER_PRIMARY);
    let title_pointer_axis = vm.state().system_ui.page == MinoriSystemPage::Title
        && input
            .input_edges
            .iter()
            .any(|edge| matches!(edge.control.as_str(), MINORI_POINTER_X | MINORI_POINTER_Y));
    if title_pointer_pressed || title_pointer_axis {
        if input.input_edges.iter().any(|edge| {
            edge.pressed
                && !matches!(
                    edge.control.as_str(),
                    MINORI_POINTER_X | MINORI_POINTER_Y | MINORI_POINTER_PRIMARY
                )
        }) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TITLE_POINTER_INPUT_AMBIGUOUS",
                "title pointer interaction cannot share a tick with another pressed control",
            ));
        }
        let focus = title_menu_focus_at(
            vm.title_variant(),
            vm.state().system_ui.pointer_x,
            vm.state().system_ui.pointer_y,
        );
        if let Some(focus) = focus {
            vm.set_system_focus(focus, title_menu_item_count(vm.title_variant()))
                .map_err(runtime_error)?;
        }
        if title_pointer_pressed {
            return focus
                .map(|_| activate_title_focus(vm))
                .unwrap_or(Ok(MinoriSystemUiAction::Present));
        }
        return Ok(MinoriSystemUiAction::Present);
    }

    let mut action = MinoriSystemUiAction::Present;
    for edge in input.input_edges.iter().filter(|edge| edge.pressed) {
        let page = vm.state().system_ui.page;
        let title_item_count = title_menu_item_count(vm.title_variant());
        match (page, edge.control.as_str()) {
            (MinoriSystemPage::Title, "arrow_up") => vm
                .move_system_focus(-1, title_item_count)
                .map_err(runtime_error)?,
            (MinoriSystemPage::Title, "arrow_down") => vm
                .move_system_focus(1, title_item_count)
                .map_err(runtime_error)?,
            (MinoriSystemPage::Title, "enter" | "space") => {
                action = activate_title_focus(vm)?;
            }
            (MinoriSystemPage::Memories, "arrow_up") => vm
                .move_system_focus(-1, MINORI_MEMORIES_ITEM_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::Memories, "arrow_down") => vm
                .move_system_focus(1, MINORI_MEMORIES_ITEM_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::Memories, "enter" | "space") => {
                let target = match vm.state().system_ui.focus_index {
                    0 => MinoriSystemPage::GalleryCg,
                    1 => MinoriSystemPage::GalleryReplay,
                    2 => MinoriSystemPage::GalleryBgm,
                    3 => MinoriSystemPage::GalleryMovie,
                    4 => MinoriSystemPage::Title,
                    _ => {
                        return Err(invalid(
                            "ASTRA_EMU_MINORI_MEMORIES_FOCUS",
                            "Memories focus is outside the verified menu bounds",
                        ));
                    }
                };
                vm.set_system_page(target, 0).map_err(runtime_error)?;
            }
            (MinoriSystemPage::Memories, "escape") => {
                vm.set_system_page(MinoriSystemPage::Title, 0)
                    .map_err(runtime_error)?;
            }
            (MinoriSystemPage::GalleryBgm, "escape") => {
                vm.set_system_page(MinoriSystemPage::Memories, 0)
                    .map_err(runtime_error)?;
                action = MinoriSystemUiAction::GalleryBgmStop;
            }
            (
                MinoriSystemPage::GalleryCg
                | MinoriSystemPage::GalleryReplay
                | MinoriSystemPage::GalleryMovie,
                "escape",
            ) => {
                vm.set_system_page(MinoriSystemPage::Memories, 0)
                    .map_err(runtime_error)?;
            }
            (MinoriSystemPage::GalleryCg, "arrow_up") => vm
                .move_system_focus(-1, MINORI_GALLERY_CG_PAGE_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::GalleryCg, "arrow_down") => vm
                .move_system_focus(1, MINORI_GALLERY_CG_PAGE_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::GalleryBgm, "arrow_up") => vm
                .move_system_focus(-1, MINORI_GALLERY_BGM_TRACK_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::GalleryBgm, "arrow_down") => vm
                .move_system_focus(1, MINORI_GALLERY_BGM_TRACK_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::GalleryBgm, "arrow_left") => {
                let current = vm.state().system_ui.focus_index;
                let target = current.checked_sub(16).unwrap_or_else(|| {
                    MINORI_GALLERY_BGM_TRACK_COUNT
                        - ((16 - current) % MINORI_GALLERY_BGM_TRACK_COUNT)
                });
                vm.set_system_focus(target, MINORI_GALLERY_BGM_TRACK_COUNT)
                    .map_err(runtime_error)?;
            }
            (MinoriSystemPage::GalleryBgm, "arrow_right") => {
                let current = vm.state().system_ui.focus_index;
                let target = current
                    .checked_add(16)
                    .map(|value| value % MINORI_GALLERY_BGM_TRACK_COUNT)
                    .unwrap_or(0);
                vm.set_system_focus(target, MINORI_GALLERY_BGM_TRACK_COUNT)
                    .map_err(runtime_error)?;
            }
            (MinoriSystemPage::GalleryBgm, "enter" | "space") => {
                action = MinoriSystemUiAction::GalleryBgmPlay;
            }
            (MinoriSystemPage::GalleryReplay, "arrow_up") => vm
                .move_system_focus(-1, MINORI_GALLERY_REPLAY_PAGE_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::GalleryReplay, "arrow_down") => vm
                .move_system_focus(1, MINORI_GALLERY_REPLAY_PAGE_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::GalleryReplay, "enter" | "space") => {
                action = MinoriSystemUiAction::GalleryReplayStart(vm.state().system_ui.focus_index);
            }
            (MinoriSystemPage::GalleryMovie, "arrow_up") => vm
                .move_system_focus(-1, MINORI_GALLERY_MOVIE_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::GalleryMovie, "arrow_down") => vm
                .move_system_focus(1, MINORI_GALLERY_MOVIE_COUNT)
                .map_err(runtime_error)?,
            (MinoriSystemPage::GalleryMovie, "enter" | "space") => {
                action = MinoriSystemUiAction::GalleryMovieStart(vm.state().system_ui.focus_index);
            }
            (MinoriSystemPage::Load, "escape") => {
                vm.set_system_page(MinoriSystemPage::Title, 0)
                    .map_err(runtime_error)?;
            }
            (MinoriSystemPage::Load, "enter" | "space") => {
                return Ok(MinoriSystemUiAction::LoadSlot(
                    vm.state().system_ui.focus_index,
                ));
            }
            _ => {}
        }
        if vm.state().system_ui.page == MinoriSystemPage::GalleryBgm
            && input.input_edges.iter().any(|edge| {
                edge.pressed
                    && edge.control == MINORI_POINTER_PRIMARY
                    && (140..420).contains(&vm.state().system_ui.pointer_x)
                    && (96..592).contains(&vm.state().system_ui.pointer_y)
            })
        {
            let row = u32::try_from((vm.state().system_ui.pointer_y - 96) / 32).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_GALLERY_BGM_POINTER",
                    "BGM gallery pointer row cannot be represented",
                )
            })?;
            let page = vm.state().system_ui.focus_index / 16;
            let track = page
                .checked_mul(16)
                .and_then(|base| base.checked_add(row))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_GALLERY_BGM_POINTER",
                        "BGM gallery pointer track overflowed",
                    )
                })?;
            if track >= MINORI_GALLERY_BGM_TRACK_COUNT {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_GALLERY_BGM_POINTER",
                    "BGM gallery pointer selected an empty row",
                ));
            }
            vm.set_system_focus(track, MINORI_GALLERY_BGM_TRACK_COUNT)
                .map_err(runtime_error)?;
            return Ok(MinoriSystemUiAction::GalleryBgmPlay);
        }
        if vm.state().system_ui.page == MinoriSystemPage::GalleryBgm
            && input.input_edges.iter().any(|edge| {
                edge.pressed
                    && edge.control == MINORI_POINTER_PRIMARY
                    && (578..894).contains(&vm.state().system_ui.pointer_x)
                    && (558..628).contains(&vm.state().system_ui.pointer_y)
            })
        {
            let x = vm.state().system_ui.pointer_x;
            return Ok(if (578..644).contains(&x) {
                let current = vm.state().system_ui.focus_index;
                let target = current
                    .checked_sub(1)
                    .unwrap_or(MINORI_GALLERY_BGM_TRACK_COUNT - 1);
                vm.set_system_focus(target, MINORI_GALLERY_BGM_TRACK_COUNT)
                    .map_err(runtime_error)?;
                MinoriSystemUiAction::Present
            } else if (656..722).contains(&x) {
                MinoriSystemUiAction::GalleryBgmPlay
            } else if (736..802).contains(&x) {
                MinoriSystemUiAction::GalleryBgmStop
            } else if (816..882).contains(&x) {
                let target =
                    (vm.state().system_ui.focus_index + 1) % MINORI_GALLERY_BGM_TRACK_COUNT;
                vm.set_system_focus(target, MINORI_GALLERY_BGM_TRACK_COUNT)
                    .map_err(runtime_error)?;
                MinoriSystemUiAction::Present
            } else {
                MinoriSystemUiAction::Present
            });
        }
        if vm.state().system_ui.page == MinoriSystemPage::GalleryBgm
            && input.input_edges.iter().any(|edge| {
                edge.pressed
                    && edge.control == MINORI_POINTER_PRIMARY
                    && (558..906).contains(&vm.state().system_ui.pointer_x)
                    && (650..710).contains(&vm.state().system_ui.pointer_y)
            })
        {
            let x = vm.state().system_ui.pointer_x;
            if (558..665).contains(&x) {
                let current = vm.state().system_ui.focus_index;
                let target = current.checked_sub(16).unwrap_or_else(|| {
                    let remainder = (16 - current) % MINORI_GALLERY_BGM_TRACK_COUNT;
                    MINORI_GALLERY_BGM_TRACK_COUNT - remainder
                });
                vm.set_system_focus(target, MINORI_GALLERY_BGM_TRACK_COUNT)
                    .map_err(runtime_error)?;
            } else if (700..810).contains(&x) {
                let target =
                    (vm.state().system_ui.focus_index + 16) % MINORI_GALLERY_BGM_TRACK_COUNT;
                vm.set_system_focus(target, MINORI_GALLERY_BGM_TRACK_COUNT)
                    .map_err(runtime_error)?;
            } else if (830..906).contains(&x) {
                vm.set_system_page(MinoriSystemPage::Memories, 2)
                    .map_err(runtime_error)?;
            }
            return Ok(MinoriSystemUiAction::Present);
        }
        if action != MinoriSystemUiAction::Present {
            break;
        }
    }
    Ok(action)
}

fn handle_system_menu_request(
    services: &LegacyFamilyHostServicesV9,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    request: &astra_emu_family_api::LegacySystemMenuRequestV1,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    match request.action {
        LegacySystemMenuActionV1::Open => {
            if session.active_system_menu.is_some() {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_MENU_ALREADY_ACTIVE",
                    "right-click cannot open a second system menu",
                ));
            }
            validate_system_menu_open(&session.vm, input)?;
            let sequence = session
                .vm
                .allocate_effect_sequence()
                .map_err(runtime_error)?;
            let menu =
                minori_system_menu(&session.vm, session.resize_antialias, request, sequence)?;
            services.system_menus.publish(&session_id.0, menu.clone())?;
            session.active_system_menu = Some(menu);
            session
                .vm
                .advance_provider_tick(input.tick_index)
                .map_err(runtime_error)?;
            idle_system_menu_output(session, vfs, input, audio_commands, None)
        }
        LegacySystemMenuActionV1::Dismiss => {
            validate_active_system_menu(session, request)?;
            session.active_system_menu = None;
            session
                .vm
                .advance_provider_tick(input.tick_index)
                .map_err(runtime_error)?;
            idle_system_menu_output(session, vfs, input, audio_commands, None)
        }
        LegacySystemMenuActionV1::Select => {
            validate_active_system_menu(session, request)?;
            session.active_system_menu = None;
            let item_id = request.item_id.as_deref().ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_MENU_ITEM",
                    "system-menu selection is missing its item id",
                )
            })?;
            match item_id {
                "message_panel" => {
                    session
                        .vm
                        .toggle_message_panel_hidden()
                        .map_err(runtime_error)?;
                    session
                        .vm
                        .advance_provider_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    gameplay_resume_output(session, vfs, input, audio_commands)
                }
                "auto" | "skip" => {
                    let rebound = session
                        .vm
                        .toggle_play_mode(if item_id == "auto" {
                            MinoriPlayMode::Auto
                        } else {
                            MinoriPlayMode::Skip
                        })
                        .map_err(runtime_error)?;
                    session
                        .vm
                        .advance_provider_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    let wait = session
                        .vm
                        .state()
                        .wait
                        .clone()
                        .ok_or_else(|| runtime_error(MinoriRuntimeError::Waiting))?;
                    waiting_output(
                        session,
                        wait,
                        LegacyLiveOutput {
                            audio_commands,
                            ..LegacyLiveOutput::default()
                        },
                        None,
                        rebound,
                        input,
                    )
                }
                "quick_save" => {
                    // The original quick save skips a request whose script
                    // line cursor is unchanged since the previous quick save,
                    // then writes `quickSaveFileNumber + 10` and advances the
                    // persisted cursor modulo the Page1 width.
                    let pc_line = session.vm.state().pc_line;
                    if session.last_quick_save_pc_line != Some(pc_line) {
                        let cursor = session.quick_save_cursor;
                        let slot = quick_save_file_number(cursor);
                        session.vm.open_save_page().map_err(runtime_error)?;
                        let save_length = save_slot(
                            services.writable_files.as_ref(),
                            session_id,
                            session,
                            slot,
                            "",
                        )?;
                        session
                            .vm
                            .close_gameplay_system_page()
                            .map_err(runtime_error)?;
                        session.last_quick_save_pc_line = Some(pc_line);
                        session.quick_save_cursor = (cursor + 1) % MINORI_QUICK_SAVE_SLOT_COUNT;
                        session.save_slots.insert(slot);
                        session.save_slot_lengths.insert(slot, save_length);
                        session.save_slot_comments.insert(slot, String::new());
                    }
                    if session.config_storage_enabled {
                        store_persistent_config_if_changed(
                            services.writable_files.as_ref(),
                            session_id,
                            session,
                        )?;
                    }
                    session
                        .vm
                        .advance_provider_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    idle_system_menu_output(session, vfs, input, audio_commands, None)
                }
                "save" | "load" => {
                    if item_id == "save" {
                        session.vm.open_save_page().map_err(runtime_error)?;
                    } else {
                        session.vm.open_load_page().map_err(runtime_error)?;
                    }
                    refresh_save_slots(services.writable_files.as_ref(), session_id, session)?;
                    session
                        .vm
                        .advance_system_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    system_ui_output(session, vfs, input, audio_commands)
                }
                "config" => {
                    session.vm.open_gameplay_config().map_err(runtime_error)?;
                    session
                        .vm
                        .advance_system_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    system_ui_output(session, vfs, input, audio_commands)
                }
                "game_return_title" | "game_exit" => {
                    let action = if item_id == "game_exit" {
                        MinoriConfirmationAction::Exit
                    } else {
                        MinoriConfirmationAction::ReturnTitle
                    };
                    if action == MinoriConfirmationAction::ReturnTitle
                        && session.vm.state().system_ui.page != MinoriSystemPage::None
                    {
                        return Err(invalid(
                            "ASTRA_EMU_MINORI_CONFIRMATION_CONTEXT",
                            "return-to-title confirmation requires active gameplay",
                        ));
                    }
                    let sequence = session
                        .vm
                        .allocate_effect_sequence()
                        .map_err(runtime_error)?;
                    let confirmation_id = format!("minori.confirmation.{item_id}.{sequence}");
                    let confirmation = LegacyConfirmationTransactionV1 {
                        sequence,
                        confirmation_id: confirmation_id.clone(),
                        title: MINORI_CONFIRMATION_TITLE.into(),
                        message: minori_confirmation_message(action).into(),
                        accept_label: MINORI_CONFIRMATION_ACCEPT_LABEL.into(),
                        cancel_label: MINORI_CONFIRMATION_CANCEL_LABEL.into(),
                    };
                    services
                        .confirmations
                        .publish(&session_id.0, confirmation)?;
                    session.active_confirmation = Some(ActiveMinoriConfirmation {
                        confirmation_id,
                        action,
                    });
                    session
                        .vm
                        .advance_provider_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    idle_system_menu_output(session, vfs, input, audio_commands, None)
                }
                "window_precision" => Err(invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_MENU_ITEM_DISABLED",
                    "the observed Minori precision resize item is disabled",
                )),
                "window_fullscreen"
                | "window_original_size"
                | "window_antialias"
                | "help_manual"
                | "help_about"
                | "help_homepage" => {
                    let command = match item_id {
                        "window_fullscreen" => LegacySystemCommandKindV1::SetFullscreen {
                            enabled: !session.vm.state().system_ui.config.fullscreen,
                        },
                        "window_original_size" => LegacySystemCommandKindV1::RestoreOriginalSize,
                        "window_antialias" => LegacySystemCommandKindV1::SetResizeAntialias {
                            enabled: !session.resize_antialias,
                        },
                        "help_manual" => LegacySystemCommandKindV1::OpenManual,
                        "help_about" => LegacySystemCommandKindV1::ShowAbout,
                        "help_homepage" => LegacySystemCommandKindV1::OpenHomepage,
                        _ => unreachable!("system menu command arm is exhaustive"),
                    };
                    let sequence = session
                        .vm
                        .allocate_effect_sequence()
                        .map_err(runtime_error)?;
                    let command_id = format!("minori.system_command.{item_id}.{sequence}");
                    services.system_commands.publish(
                        &session_id.0,
                        LegacySystemCommandTransactionV1 {
                            sequence,
                            command_id: command_id.clone(),
                            command,
                        },
                    )?;
                    session.active_system_command = Some(ActiveMinoriSystemCommand {
                        command_id,
                        command,
                        resume_gameplay: false,
                    });
                    session
                        .vm
                        .advance_provider_tick(input.tick_index)
                        .map_err(runtime_error)?;
                    idle_system_menu_output(session, vfs, input, audio_commands, None)
                }
                _ => Err(invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_MENU_ITEM_UNKNOWN",
                    "system-menu selection references an unknown Minori command",
                )),
            }
        }
    }
}

fn handle_confirmation_step(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    if input.system_menu.is_some()
        || input.text_input.is_some()
        || !input.input_edges.is_empty()
        || !input.await_results.is_empty()
        || !input.provider_results.is_empty()
    {
        session.poisoned = true;
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIRMATION_INPUT_WHILE_ACTIVE",
            "active native confirmation must suspend gameplay input and completions",
        ));
    }
    let active = session.active_confirmation.clone().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_CONFIRMATION_NOT_ACTIVE",
            "confirmation result has no active Minori transaction",
        )
    })?;
    let Some(result) = input.confirmation.as_ref() else {
        session
            .vm
            .advance_provider_tick(input.tick_index)
            .map_err(runtime_error)?;
        return idle_system_menu_output(session, vfs, input, audio_commands, None);
    };
    validate_confirmation_result(&active, result)?;
    session.active_confirmation = None;
    session
        .vm
        .advance_provider_tick(input.tick_index)
        .map_err(runtime_error)?;
    match result.choice {
        LegacyConfirmationChoiceV1::Cancelled => {
            idle_system_menu_output(session, vfs, input, audio_commands, None)
        }
        LegacyConfirmationChoiceV1::Accepted => match active.action {
            MinoriConfirmationAction::Exit => {
                session
                    .vm
                    .terminate_from_confirmation()
                    .map_err(runtime_error)?;
                system_ui_output(session, vfs, input, audio_commands)
            }
            MinoriConfirmationAction::ReturnTitle => {
                session
                    .vm
                    .return_to_title_from_gameplay()
                    .map_err(runtime_error)?;
                system_ui_output(session, vfs, input, audio_commands)
            }
        },
    }
}

fn handle_text_input_step(
    services: &LegacyFamilyHostServicesV9,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    if input.system_menu.is_some()
        || input.confirmation.is_some()
        || input.system_command.is_some()
        || !input.input_edges.is_empty()
        || !input.await_results.is_empty()
        || !input.provider_results.is_empty()
    {
        session.poisoned = true;
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_INPUT_INPUT_WHILE_ACTIVE",
            "active native text input must suspend gameplay input and completions",
        ));
    }
    let active = session.active_text_input.clone().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_TEXT_INPUT_NOT_ACTIVE",
            "text-input result has no active Minori transaction",
        )
    })?;
    let Some(result) = input.text_input.as_ref() else {
        session
            .vm
            .advance_provider_tick(input.tick_index)
            .map_err(runtime_error)?;
        return system_ui_output(session, vfs, input, audio_commands);
    };
    result.validate()?;
    if result.prompt_id != active.prompt_id {
        session.poisoned = true;
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_INPUT_ID_MISMATCH",
            "text-input result does not match the active Minori transaction",
        ));
    }
    if result.value.len() > active.max_bytes as usize {
        session.poisoned = true;
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_INPUT_VALUE",
            "text-input result exceeds the active Minori transaction bound",
        ));
    }
    session.active_text_input = None;
    session
        .vm
        .advance_provider_tick(input.tick_index)
        .map_err(runtime_error)?;
    match result.choice {
        LegacyTextInputChoiceV1::Accepted => {
            let save_length = save_slot(
                services.writable_files.as_ref(),
                session_id,
                session,
                active.slot,
                &result.value,
            )?;
            session.save_slots.insert(active.slot);
            session
                .save_slot_comments
                .insert(active.slot, result.value.clone());
            session.save_slot_lengths.insert(active.slot, save_length);
        }
        LegacyTextInputChoiceV1::Cancelled => {}
    }
    session
        .vm
        .close_gameplay_system_page()
        .map_err(runtime_error)?;
    gameplay_resume_output(session, vfs, input, audio_commands)
}

fn handle_system_command_step(
    services: &LegacyFamilyHostServicesV9,
    session_id: &LegacyRuntimeSessionId,
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    if input.system_menu.is_some()
        || input.confirmation.is_some()
        || input.text_input.is_some()
        || !input.input_edges.is_empty()
        || !input.await_results.is_empty()
        || !input.provider_results.is_empty()
    {
        session.poisoned = true;
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_COMMAND_INPUT_WHILE_ACTIVE",
            "active native system command must suspend gameplay input and completions",
        ));
    }
    let active = session.active_system_command.clone().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_SYSTEM_COMMAND_NOT_ACTIVE",
            "system command result has no active Minori transaction",
        )
    })?;
    let Some(result) = input.system_command.as_ref() else {
        session
            .vm
            .advance_provider_tick(input.tick_index)
            .map_err(runtime_error)?;
        return idle_system_menu_output(session, vfs, input, audio_commands, None);
    };
    result.validate()?;
    if result.command_id != active.command_id {
        session.poisoned = true;
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_COMMAND_ID_MISMATCH",
            "system command result does not match the active Minori transaction",
        ));
    }
    session.active_system_command = None;
    session
        .vm
        .advance_provider_tick(input.tick_index)
        .map_err(runtime_error)?;
    let mut persist_config = false;
    match result.status {
        LegacySystemCommandStatusV1::Applied => {
            match active.command {
                LegacySystemCommandKindV1::SetFullscreen { enabled } => {
                    session
                        .vm
                        .set_runtime_fullscreen(enabled)
                        .map_err(runtime_error)?;
                    persist_config = true;
                }
                LegacySystemCommandKindV1::RestoreOriginalSize => {
                    // The native host leaves fullscreen before restoring the
                    // authored window geometry.  Keep the family-owned menu
                    // state in lockstep with that platform side effect so a
                    // subsequent transaction exposes the fullscreen command
                    // again and a save/restart does not re-enter fullscreen.
                    session
                        .vm
                        .set_runtime_fullscreen(false)
                        .map_err(runtime_error)?;
                    persist_config = true;
                }
                LegacySystemCommandKindV1::SetResizePrecision { .. }
                | LegacySystemCommandKindV1::OpenManual
                | LegacySystemCommandKindV1::ShowAbout
                | LegacySystemCommandKindV1::OpenHomepage => {
                    // These operations are deliberately host-owned.  Their
                    // native side effects (window geometry, sampling policy,
                    // help viewer, About dialog and browser) do not belong in
                    // the deterministic Minori VM state.  An Applied result
                    // therefore only releases the suspended menu transaction;
                    // Rejected/Unsupported below remain blocking so a missing
                    // host capability can never be mistaken for success.
                }
                LegacySystemCommandKindV1::SetResizeAntialias { enabled } => {
                    session.resize_antialias = enabled;
                }
            }
            if persist_config {
                store_persistent_config_if_changed(
                    services.writable_files.as_ref(),
                    session_id,
                    session,
                )?;
            }
            if active.resume_gameplay {
                gameplay_resume_output(session, vfs, input, audio_commands)
            } else {
                idle_system_menu_output(session, vfs, input, audio_commands, None)
            }
        }
        LegacySystemCommandStatusV1::Rejected => {
            session.poisoned = true;
            Err(invalid(
                "ASTRA_EMU_MINORI_SYSTEM_COMMAND_REJECTED",
                "native host rejected the Minori system command",
            ))
        }
        LegacySystemCommandStatusV1::Unsupported => {
            session.poisoned = true;
            Err(invalid(
                "ASTRA_EMU_MINORI_SYSTEM_COMMAND_UNSUPPORTED",
                "native host does not support the Minori system command",
            ))
        }
    }
}

fn validate_confirmation_result(
    active: &ActiveMinoriConfirmation,
    result: &LegacyConfirmationResultV1,
) -> Result<(), LegacyProviderError> {
    result.validate()?;
    if result.confirmation_id != active.confirmation_id {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIRMATION_ID_MISMATCH",
            "confirmation result does not match the active Minori transaction",
        ));
    }
    Ok(())
}

fn idle_system_menu_output(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
    event: Option<LegacyEvent>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    if session.vm.state().system_ui.page == MinoriSystemPage::Title {
        let mut output = system_ui_output(session, vfs, input, audio_commands)?;
        if let Some(event) = event {
            output.control.events.push(event);
            output.validate()?;
        }
        return Ok(output);
    }
    let wait = session
        .vm
        .state()
        .wait
        .clone()
        .ok_or_else(|| runtime_error(MinoriRuntimeError::Waiting))?;
    waiting_output(
        session,
        wait,
        LegacyLiveOutput {
            audio_commands,
            ..LegacyLiveOutput::default()
        },
        event,
        false,
        input,
    )
}

fn validate_active_system_menu(
    session: &MinoriSession,
    request: &astra_emu_family_api::LegacySystemMenuRequestV1,
) -> Result<(), LegacyProviderError> {
    let menu = session.active_system_menu.as_ref().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_SYSTEM_MENU_ID_MISMATCH",
            "system-menu result has no active Minori menu",
        )
    })?;
    if Some(menu.menu_id.as_str()) != request.menu_id.as_deref() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_MENU_ID_MISMATCH",
            "system-menu result does not match the active Minori menu",
        ));
    }
    if request.action == LegacySystemMenuActionV1::Select {
        let item_id = request.item_id.as_deref().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SYSTEM_MENU_ITEM",
                "system-menu selection is missing its item id",
            )
        })?;
        let item = menu
            .items
            .iter()
            .find(|item| item.item_id == item_id)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SYSTEM_MENU_ITEM_UNKNOWN",
                    "system-menu selection references an item outside the active transaction",
                )
            })?;
        if item.kind != LegacySystemMenuItemKindV1::Command || !item.enabled {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SYSTEM_MENU_ITEM_DISABLED",
                "system-menu selection references a disabled or non-command item",
            ));
        }
    }
    Ok(())
}

fn minori_system_menu(
    vm: &MinoriVm,
    resize_antialias: bool,
    request: &astra_emu_family_api::LegacySystemMenuRequestV1,
    sequence: u64,
) -> Result<LegacySystemMenuTransactionV1, LegacyProviderError> {
    let title = vm.state().system_ui.page == MinoriSystemPage::Title;
    let mut items = Vec::new();
    let mut push = |item_id: &str,
                    parent_id: Option<&str>,
                    order: u16,
                    kind: LegacySystemMenuItemKindV1,
                    label: &str,
                    enabled: bool,
                    checked: bool| {
        items.push(LegacySystemMenuItemV1 {
            item_id: item_id.into(),
            parent_id: parent_id.map(Into::into),
            order,
            kind,
            label: label.into(),
            enabled,
            checked,
        });
    };
    if !title {
        push(
            "message_panel",
            None,
            0,
            LegacySystemMenuItemKindV1::Command,
            if vm.state().system_ui.message_panel_hidden {
                "メッセージパネルを表示する (&H)"
            } else {
                "メッセージパネルを隠す (&H)"
            },
            true,
            false,
        );
        push(
            "auto",
            None,
            1,
            LegacySystemMenuItemKindV1::Command,
            "オートプレイ (&A)",
            true,
            false,
        );
        push(
            "skip",
            None,
            2,
            LegacySystemMenuItemKindV1::Command,
            "スキップ (&K)",
            vm.state().system_ui.skip_enabled,
            false,
        );
        push(
            "quick_save",
            None,
            3,
            LegacySystemMenuItemKindV1::Command,
            "クイックセーブ (&Q)",
            true,
            false,
        );
        push(
            "save",
            None,
            4,
            LegacySystemMenuItemKindV1::Command,
            "セーブ (&S)",
            true,
            false,
        );
        push(
            "load",
            None,
            5,
            LegacySystemMenuItemKindV1::Command,
            "ロード (&L)",
            true,
            false,
        );
        push(
            "config",
            None,
            6,
            LegacySystemMenuItemKindV1::Command,
            "システム設定 (&C)",
            true,
            false,
        );
        push(
            "sep_gameplay",
            None,
            7,
            LegacySystemMenuItemKindV1::Separator,
            "",
            false,
            false,
        );
    }
    let base = if title { 0 } else { 8 };
    let fullscreen = vm.state().system_ui.config.fullscreen;
    let mut window_order = base;
    // The original title window hides the fullscreen toggle while it is
    // already fullscreen.  The remaining Window entries keep their native
    // order, so the Host menu has three rows in fullscreen and four rows in a
    // normal window.
    if !fullscreen {
        push(
            "window_fullscreen",
            None,
            window_order,
            LegacySystemMenuItemKindV1::Command,
            "フルスクリーン (&O)",
            true,
            false,
        );
        window_order += 1;
    }
    push(
        "window_original_size",
        None,
        window_order,
        LegacySystemMenuItemKindV1::Command,
        "ウインドウをオリジナルサイズに (&W)",
        true,
        false,
    );
    window_order += 1;
    push(
        "window_precision",
        None,
        window_order,
        LegacySystemMenuItemKindV1::Command,
        "高精度サイズ変更 (&Y)",
        false,
        true,
    );
    window_order += 1;
    push(
        "window_antialias",
        None,
        window_order,
        LegacySystemMenuItemKindV1::Command,
        "サイズ変更時にアンチエイリアス (&A)",
        true,
        resize_antialias,
    );
    window_order += 1;
    push(
        "sep_window",
        None,
        window_order,
        LegacySystemMenuItemKindV1::Separator,
        "",
        false,
        false,
    );
    window_order += 1;
    push(
        "help",
        None,
        window_order,
        LegacySystemMenuItemKindV1::Submenu,
        "ヘルプ (&H)",
        true,
        false,
    );
    push(
        "help_manual",
        Some("help"),
        0,
        LegacySystemMenuItemKindV1::Command,
        "ヘルプ (&H)",
        true,
        false,
    );
    push(
        "help_about",
        Some("help"),
        1,
        LegacySystemMenuItemKindV1::Command,
        "アプリケーションについて (&A)",
        true,
        false,
    );
    push(
        "help_homepage",
        Some("help"),
        2,
        LegacySystemMenuItemKindV1::Command,
        "minoriホームページ (&P)",
        true,
        false,
    );
    push(
        "game",
        None,
        window_order + 1,
        LegacySystemMenuItemKindV1::Submenu,
        "ゲーム (&G)",
        true,
        false,
    );
    if !title {
        push(
            "game_return_title",
            Some("game"),
            0,
            LegacySystemMenuItemKindV1::Command,
            "トップメニューに戻る (&M)",
            true,
            false,
        );
    }
    push(
        "game_exit",
        Some("game"),
        if title { 0 } else { 1 },
        LegacySystemMenuItemKindV1::Command,
        "プログラムの終了 (&X)",
        true,
        false,
    );
    let menu = LegacySystemMenuTransactionV1 {
        sequence,
        menu_id: format!("minori.system_menu.{sequence}"),
        pointer_x: request.pointer_x,
        pointer_y: request.pointer_y,
        items,
    };
    menu.validate()?;
    Ok(menu)
}

fn validate_system_menu_open(
    vm: &MinoriVm,
    input: &LegacyStepInput,
) -> Result<(), LegacyProviderError> {
    if !input.await_results.is_empty() || !input.provider_results.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_MENU_RESULT_UNEXPECTED",
            "right-click system-menu open cannot share a tick with a completion",
        ));
    }
    if input.input_edges.iter().any(|edge| {
        edge.pressed
            && !matches!(
                edge.control.as_str(),
                MINORI_POINTER_X | MINORI_POINTER_Y | "pointer.secondary"
            )
    }) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_MENU_INPUT_AMBIGUOUS",
            "right-click system-menu open cannot share a tick with gameplay input",
        ));
    }
    if vm.state().system_ui.page == MinoriSystemPage::Title {
        return Ok(());
    }
    if vm.state().system_ui.page != MinoriSystemPage::None {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_MENU_PAGE_ACTIVE",
            "right-click system menu is unavailable while a family system page is active",
        ));
    }
    match vm.state().wait.as_ref() {
        Some(
            MinoriWaitState::Input { .. }
            | MinoriWaitState::Time { .. }
            | MinoriWaitState::Voice { .. },
        ) => Ok(()),
        Some(MinoriWaitState::Choice { .. }) => Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_MENU_CHOICE_ACTIVE",
            "right-click system-menu open is not valid while a choice is active",
        )),
        Some(MinoriWaitState::Media { .. }) => Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_MENU_MEDIA_ACTIVE",
            "right-click system-menu open is not valid while a movie is active",
        )),
        Some(
            MinoriWaitState::AxisScroll { .. }
            | MinoriWaitState::LinearScroll { .. }
            | MinoriWaitState::CharacterTransition { .. }
            | MinoriWaitState::Presentation { .. }
            | MinoriWaitState::Provider { .. },
        )
        | None => Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_MENU_WAIT_STATE",
            "right-click system-menu open requires a stable message wait",
        )),
    }
}

fn apply_config_input(
    vm: &mut MinoriVm,
    input: &LegacyStepInput,
) -> Result<MinoriSystemUiAction, LegacyProviderError> {
    let apply = input
        .input_edges
        .iter()
        .any(|edge| edge.pressed && matches!(edge.control.as_str(), "enter" | "space"));
    let cancel = input
        .input_edges
        .iter()
        .any(|edge| edge.pressed && edge.control == "escape");
    let pointer_pressed = input
        .input_edges
        .iter()
        .any(|edge| edge.pressed && edge.control == MINORI_POINTER_PRIMARY);
    let pointer_moved = input
        .input_edges
        .iter()
        .any(|edge| matches!(edge.control.as_str(), MINORI_POINTER_X | MINORI_POINTER_Y));
    let pointer_action =
        pointer_pressed || (pointer_moved && vm.state().system_ui.pointer_primary_pressed);
    let action_count = usize::from(apply) + usize::from(cancel) + usize::from(pointer_action);
    if action_count > 1 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_INPUT_AMBIGUOUS",
            "config apply, cancel, and pointer controls cannot share one fixed tick",
        ));
    }
    let control = if apply {
        Some(MinoriConfigControl::Apply)
    } else if cancel {
        Some(MinoriConfigControl::Cancel)
    } else if pointer_action {
        config_control_at(
            vm.state().system_ui.pointer_x,
            vm.state().system_ui.pointer_y,
        )
    } else {
        None
    };
    let Some(control) = control else {
        return Ok(MinoriSystemUiAction::Present);
    };
    let fullscreen_before = vm.state().system_ui.config.fullscreen;
    match vm.apply_config_control(control).map_err(runtime_error)? {
        MinoriConfigChange::Present => Ok(MinoriSystemUiAction::Present),
        MinoriConfigChange::AudioParamsChanged => Ok(MinoriSystemUiAction::PresentWithAudioRefresh),
        MinoriConfigChange::Applied => Ok(MinoriSystemUiAction::PresentAfterConfigClose {
            fullscreen: (vm.state().system_ui.config.fullscreen != fullscreen_before)
                .then_some(vm.state().system_ui.config.fullscreen),
        }),
        MinoriConfigChange::Cancelled => {
            Ok(MinoriSystemUiAction::PresentAfterConfigClose { fullscreen: None })
        }
        MinoriConfigChange::TestAudio(bus) => Ok(MinoriSystemUiAction::PresentWithAudioTest(bus)),
    }
}

fn config_control_at(x: i32, y: i32) -> Option<MinoriConfigControl> {
    let slider_value = |track_left: i32| {
        let position = (x - track_left - 11).clamp(0, 200);
        u8::try_from(position * 100 / 200).expect("clamped config slider fits u8")
    };
    if (40..260).contains(&x) {
        return match y {
            152..192 => Some(MinoriConfigControl::MessageSpeedUnread(slider_value(40))),
            228..268 => Some(MinoriConfigControl::MessageSpeedRead(slider_value(40))),
            304..340 => Some(MinoriConfigControl::MessageSpeedAutoPlay(slider_value(40))),
            _ => config_non_slider_control_at(x, y),
        };
    }
    if (576..796).contains(&x) {
        return match y {
            148..188 => Some(MinoriConfigControl::BgmVolume(slider_value(576))),
            224..264 => Some(MinoriConfigControl::VoiceVolume(slider_value(576))),
            300..336 => Some(MinoriConfigControl::SeVolume(slider_value(576))),
            _ => config_non_slider_control_at(x, y),
        };
    }
    config_non_slider_control_at(x, y)
}

fn config_non_slider_control_at(x: i32, y: i32) -> Option<MinoriConfigControl> {
    let hit = |left, top, right, bottom| (left..right).contains(&x) && (top..bottom).contains(&y);
    let control = if hit(248, 492, 276, 512) {
        MinoriConfigControl::FontPrevious
    } else if hit(248, 524, 276, 548) {
        MinoriConfigControl::FontNext
    } else if hit(36, 592, 132, 616) {
        MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Auto)
    } else if hit(148, 592, 268, 616) {
        MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Skip)
    } else if hit(312, 120, 456, 152) {
        MinoriConfigControl::Fullscreen(true)
    } else if hit(312, 164, 456, 196) {
        MinoriConfigControl::Fullscreen(false)
    } else if hit(312, 248, 544, 280) {
        MinoriConfigControl::ToggleScreenEffect
    } else if hit(312, 292, 544, 320) {
        MinoriConfigControl::ToggleTextShadow
    } else if hit(312, 336, 544, 364) {
        MinoriConfigControl::ToggleAnimation
    } else if hit(312, 424, 512, 472) {
        MinoriConfigControl::ToggleBacklogVoicePlayback
    } else if hit(312, 476, 512, 524) {
        MinoriConfigControl::ToggleStopVoiceAtNextMessage
    } else if hit(312, 572, 512, 620) {
        MinoriConfigControl::ToggleProgressInBackground
    } else if hit(680, 120, 740, 144) {
        MinoriConfigControl::ToggleBgmMute
    } else if hit(680, 196, 740, 220) {
        MinoriConfigControl::ToggleVoiceMute
    } else if hit(680, 268, 740, 292) {
        MinoriConfigControl::ToggleSeMute
    } else if hit(744, 120, 780, 144) {
        MinoriConfigControl::TestAudio(MinoriConfigAudioBus::Bgm)
    } else if hit(744, 196, 780, 220) {
        MinoriConfigControl::TestAudio(MinoriConfigAudioBus::Voice)
    } else if hit(744, 268, 780, 292) {
        MinoriConfigControl::TestAudio(MinoriConfigAudioBus::Se)
    } else if hit(578, 433, 688, 465) {
        MinoriConfigControl::ToggleCharacterVoice(0)
    } else if hit(578, 470, 688, 502) {
        MinoriConfigControl::ToggleCharacterVoice(1)
    } else if hit(578, 508, 688, 540) {
        MinoriConfigControl::ToggleCharacterVoice(2)
    } else if hit(578, 545, 688, 577) {
        MinoriConfigControl::ToggleCharacterVoice(3)
    } else if hit(699, 433, 776, 465) {
        MinoriConfigControl::ToggleCharacterVoice(4)
    } else if hit(592, 600, 648, 640) {
        MinoriConfigControl::Apply
    } else if hit(701, 600, 775, 640) {
        MinoriConfigControl::Cancel
    } else {
        return None;
    };
    Some(control)
}

fn backlog_wheel_direction(input: &LegacyStepInput) -> Result<Option<i32>, LegacyProviderError> {
    let mut direction = None;
    for edge in input
        .input_edges
        .iter()
        .filter(|edge| edge.control == "wheel")
    {
        let current = if edge.value < 0.0 {
            Some(-1)
        } else if edge.value > 0.0 {
            Some(1)
        } else {
            None
        };
        if let Some(current) = current {
            if direction.is_some_and(|existing| existing != current) {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_BACKLOG_WHEEL_AMBIGUOUS",
                    "one fixed tick contains conflicting backlog wheel directions",
                ));
            }
            direction = Some(current);
        }
    }
    Ok(direction)
}

fn system_ui_output(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    tracing::info!(
        target: "astra_emu_minori::system_ui",
        event = "astra_emu_minori_system_ui_output",
        page = system_page_name(session.vm.state().system_ui.page),
        terminal = session.vm.state().terminal,
        fixed_tick = input.tick_index,
        "emitting system UI output"
    );
    let status = if session.vm.state().terminal {
        LegacyRuntimeStatus::Terminal
    } else {
        LegacyRuntimeStatus::Active
    };
    let audio_command_count = u64::try_from(audio_commands.len()).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_AUDIO_COMMAND_BOUNDS",
            "system UI audio command count cannot be represented",
        )
    })?;
    let mut live = LegacyLiveOutput {
        clear_text: status == LegacyRuntimeStatus::Terminal,
        audio_commands,
        ..LegacyLiveOutput::default()
    };
    // Scene2D is retained by the host. A system page that did not change in
    // this fixed tick must not rebuild its resource frame: doing so would
    // reopen and parse the same PAZ image on every tick and would also submit
    // a semantically redundant scene transaction. Input is the only way a
    // system page can change its focus/variant here; restore explicitly asks
    // for a fresh presentation.
    let system_page_changed = session.reported_system_page
        != Some(session.vm.state().system_ui.page)
        || !input.input_edges.is_empty()
        || session.restore_presentation_pending;
    if status != LegacyRuntimeStatus::Terminal && system_page_changed {
        live.clear_text = true;
        let is_backlog = session.vm.state().system_ui.page == MinoriSystemPage::Backlog;
        let sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        live.resource_scenes.push(LegacySequenced {
            sequence,
            value: if is_backlog {
                describe_backlog_frame(
                    vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    sequence,
                )?
            } else {
                describe_system_page_with_slots_and_hover(
                    vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    &session.vm,
                    &session.save_slots,
                    (session.vm.state().system_ui.page == MinoriSystemPage::Title)
                        .then_some(session.title_pointer_focus)
                        .flatten(),
                )?
            },
        });
        if is_backlog {
            append_backlog_text(session, input.tick_index, &mut live)?;
        } else if session.vm.state().system_ui.page == MinoriSystemPage::GalleryMovie {
            append_gallery_movie_text(session, input.tick_index, &mut live)?;
        } else if matches!(
            session.vm.state().system_ui.page,
            MinoriSystemPage::Save | MinoriSystemPage::Load
        ) {
            append_save_load_text(session, input.tick_index, &mut live)?;
        }
    }
    let mut output = LegacyStepOutput {
        status,
        live,
        control: LegacyControlTransaction::default(),
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta {
            audio_commands: audio_command_count,
            ..LegacyCoverageDelta::default()
        },
        state_revision: session.vm.state().fixed_tick,
    };
    if session.vm.state().system_ui.page == MinoriSystemPage::Backlog
        && input.await_results.is_empty()
        && input.input_edges.is_empty()
        && audio_command_count > 0
    {
        let wait = session
            .vm
            .state()
            .wait
            .as_ref()
            .ok_or_else(|| runtime_error(MinoriRuntimeError::Backlog))?;
        output
            .control
            .waits
            .push(legacy_wait(wait, session.vm.state()));
    }
    let reported_system_page = append_system_page_observation(session, &mut output.control)?;
    let reported_play_mode = append_play_mode_observation(session, &mut output.control)?;
    let reported_gallery_unlock_count =
        append_gallery_unlock_observation(session, &mut output.control)?;
    let reported_choice_active = append_choice_active_observation(session, &mut output.control)?;
    let reported_progress_in_background =
        append_progress_in_background_observation(session, &mut output.control)?;
    output.validate()?;
    if let Some(page) = reported_system_page {
        session.reported_system_page = Some(page);
    }
    if let Some(mode) = reported_play_mode {
        session.reported_play_mode = Some(mode);
    }
    if let Some(count) = reported_gallery_unlock_count {
        session.reported_gallery_unlock_count = Some(count);
    }
    if let Some(active) = reported_choice_active {
        session.reported_choice_active = Some(active);
    }
    if let Some(enabled) = reported_progress_in_background {
        session.reported_progress_in_background = Some(enabled);
    }
    session.restore_presentation_pending = false;
    Ok(output)
}

fn append_validated_audio_commands<'a>(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    commands: impl IntoIterator<Item = &'a MinoriAudioCommand>,
    state: &MinoriRuntimeState,
    output: &mut Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<(), LegacyProviderError> {
    for command in commands {
        let (sequence, command) = map_audio_command(command, state)?;
        if let LegacyAudioCommandV1::LoadResource { resource_uri, .. } = &command {
            let stat = vfs.stat_file(mount_set_id, resource_uri)?;
            if stat.len == 0 || stat.len > MAX_RESOURCE_BYTES {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AUDIO_RESOURCE_BOUNDS",
                    "audio resource is empty or exceeds the session bound",
                ));
            }
        }
        command.validate()?;
        output.push(LegacySequenced {
            sequence,
            value: command,
        });
    }
    Ok(())
}

fn gameplay_resume_output(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    input: &LegacyStepInput,
    audio_commands: Vec<LegacySequenced<LegacyAudioCommandV1>>,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    let stage_size = session.stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_STAGE_SIZE",
            "closing backlog requires explicit host dimensions",
        )
    })?;
    let scene_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let frame = if session.vm.state().firefly.is_some() {
        describe_firefly_frame(vfs, &session.mount_set_id, session.vm.state(), stage_size)?
    } else {
        describe_effect_frame(
            vfs,
            &session.mount_set_id,
            session.vm.state(),
            &visible_effect_frame(session.vm.state(), scene_sequence)?,
            stage_size,
        )?
    };
    let current_message = session.vm.state().message.as_ref().map(|message| {
        session
            .vm
            .state()
            .backlog
            .iter()
            .rev()
            .find(|entry| {
                entry.source == message.source
                    && entry.message_id == message.message_id
                    && entry.text_hash == message.text_hash
                    && entry.speaker_hash == message.speaker_hash
                    && entry.voice_hash == message.voice_hash
            })
            .map(|entry| (entry.text.clone(), entry.speaker.clone()))
    });
    let current_message = match current_message {
        Some(Some(message)) => Some(message),
        Some(None) => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_BACKLOG_MESSAGE_IDENTITY",
                "current message is missing from the retained backlog",
            ));
        }
        None => None,
    };
    let audio_command_count = u64::try_from(audio_commands.len()).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_AUDIO_COMMAND_BOUNDS",
            "backlog audio command count cannot be represented",
        )
    })?;
    let mut live = LegacyLiveOutput {
        clear_text: true,
        resource_scenes: vec![LegacySequenced {
            sequence: scene_sequence,
            value: frame,
        }],
        audio_commands,
        ..LegacyLiveOutput::default()
    };
    let current_message =
        current_message.filter(|_| !session.vm.state().system_ui.message_panel_hidden);
    if let Some((text, speaker)) = current_message {
        append_resumed_message_text(session, input.tick_index, text, speaker, &mut live)?;
    }
    if matches!(
        session.vm.state().wait.as_ref(),
        Some(MinoriWaitState::Choice { .. })
    ) {
        let sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        let _ = append_choice_live_output(session, vfs, input.tick_index, sequence, &mut live)?;
    }
    let mut output = LegacyStepOutput {
        status: if session.vm.state().wait.is_some() {
            LegacyRuntimeStatus::Awaiting
        } else {
            LegacyRuntimeStatus::Active
        },
        live,
        control: LegacyControlTransaction::default(),
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta {
            audio_commands: audio_command_count,
            ..LegacyCoverageDelta::default()
        },
        state_revision: session.vm.state().fixed_tick,
    };
    let reported_system_page = append_system_page_observation(session, &mut output.control)?;
    let reported_play_mode = append_play_mode_observation(session, &mut output.control)?;
    let reported_gallery_unlock_count =
        append_gallery_unlock_observation(session, &mut output.control)?;
    let reported_choice_active = append_choice_active_observation(session, &mut output.control)?;
    let reported_progress_in_background =
        append_progress_in_background_observation(session, &mut output.control)?;
    output.validate()?;
    if let Some(page) = reported_system_page {
        session.reported_system_page = Some(page);
    }
    if let Some(mode) = reported_play_mode {
        session.reported_play_mode = Some(mode);
    }
    if let Some(count) = reported_gallery_unlock_count {
        session.reported_gallery_unlock_count = Some(count);
    }
    if let Some(active) = reported_choice_active {
        session.reported_choice_active = Some(active);
    }
    if let Some(enabled) = reported_progress_in_background {
        session.reported_progress_in_background = Some(enabled);
    }
    session.restore_presentation_pending = false;
    Ok(output)
}

fn append_resumed_message_text(
    session: &mut MinoriSession,
    tick_index: u64,
    text: String,
    speaker: Option<String>,
    live: &mut LegacyLiveOutput,
) -> Result<(), LegacyProviderError> {
    if text.len() > MAX_EPHEMERAL_TEXT_BYTES
        || speaker
            .as_ref()
            .is_some_and(|value| value.len() > MAX_EPHEMERAL_TEXT_BYTES)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
            "restored message exceeds the ephemeral text channel bound",
        ));
    }
    let presentation_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let capture_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let lease_id = format!("minori.text.resume.{tick_index}.{capture_sequence}");
    let presentation = LegacyTextPresentationLeaseV1 {
        lease_id: lease_id.clone(),
        presentation: minori_message_presentation(
            session.stage_size,
            session.vm.state().system_ui.config.text_shadow,
        )?,
    };
    presentation.validate()?;
    if session
        .ephemeral_text
        .insert(
            lease_id.clone(),
            StagedEphemeralText {
                lease_id: lease_id.clone(),
                text: text.clone(),
                speaker,
                show_advance_indicator: true,
            },
        )
        .is_some()
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
            "resumed message lease id is duplicated",
        ));
    }
    live.text_presentations.push(LegacySequenced {
        sequence: presentation_sequence,
        value: presentation,
    });
    live.text.push(StagedTextLease {
        sequence: capture_sequence,
        lease_id,
        byte_len: text.len().try_into().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                "resumed message length cannot be represented",
            )
        })?,
        source_ref: "minori.sc.message.resume".into(),
    });
    Ok(())
}

fn append_save_load_text(
    session: &mut MinoriSession,
    tick_index: u64,
    live: &mut LegacyLiveOutput,
) -> Result<(), LegacyProviderError> {
    let page_base = (session.vm.state().system_ui.focus_index / 10) * 10;
    let slot_left = [64_i32, 456_i32];
    let slot_top = [81_i32, 189_i32, 297_i32, 405_i32, 513_i32];
    for visible_index in 0..10_u32 {
        let slot = page_base.checked_add(visible_index).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_LOAD_TEXT",
                "save slot index overflowed",
            )
        })?;
        let Some(metadata) = session.save_slot_metadata.get(&slot).cloned() else {
            continue;
        };
        let text = if metadata.comment.is_empty() {
            metadata.timestamp
        } else {
            format!("{}\n{}", metadata.timestamp, metadata.comment)
        };
        if text.len() > MAX_EPHEMERAL_TEXT_BYTES {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_LOAD_TEXT",
                "save slot metadata exceeds the bounded text channel",
            ));
        }
        let row = usize::try_from(visible_index / 2).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_LOAD_TEXT",
                "save slot row cannot be represented",
            )
        })?;
        let column = usize::try_from(visible_index % 2).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_LOAD_TEXT",
                "save slot column cannot be represented",
            )
        })?;
        let presentation_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        let capture_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        let lease_id = format!("minori.save-slot.{tick_index}.{slot}.{capture_sequence}");
        let presentation = LegacyTextPresentationLeaseV1 {
            lease_id: lease_id.clone(),
            presentation: minori_save_slot_text_presentation(
                session.stage_size,
                slot_left[column] + 124,
                slot_top[row] + 11,
            )?,
        };
        presentation.validate()?;
        if session
            .ephemeral_text
            .insert(
                lease_id.clone(),
                StagedEphemeralText {
                    lease_id: lease_id.clone(),
                    text: text.clone(),
                    speaker: None,
                    show_advance_indicator: false,
                },
            )
            .is_some()
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
                "save slot text lease id is duplicated",
            ));
        }
        live.text_presentations.push(LegacySequenced {
            sequence: presentation_sequence,
            value: presentation,
        });
        live.text.push(StagedTextLease {
            sequence: capture_sequence,
            lease_id,
            byte_len: text.len().try_into().map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_SAVE_LOAD_TEXT",
                    "save slot text length cannot be represented",
                )
            })?,
            source_ref: "minori.save_slot.metadata".into(),
        });
    }
    Ok(())
}

fn minori_save_slot_text_presentation(
    stage_size: Option<(u32, u32)>,
    x: i32,
    y: i32,
) -> Result<LegacyTextPresentationV1, LegacyProviderError> {
    if stage_size != Some((1280, 720)) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_LOAD_TEXT_STAGE",
            "save slot metadata requires the verified 1280x720 stage",
        ));
    }
    let presentation = LegacyTextPresentationV1 {
        layout_id: format!("minori.save-slot.{x}.{y}"),
        language: "ja-JP".into(),
        font_families: vec!["Noto Sans JP".into()],
        body: LegacyTextRegionV1 {
            x,
            y,
            width: 218,
            height: 58,
            font_size: 18.0,
            line_height: 23.0,
            max_lines: 2,
            horizontal_alignment: LegacyTextHorizontalAlignmentV1::Start,
        },
        speaker: None,
        rgba: [255, 0, 0, 255],
        outline: None,
    };
    presentation.validate()?;
    Ok(presentation)
}

fn append_gallery_movie_text(
    session: &mut MinoriSession,
    tick_index: u64,
    live: &mut LegacyLiveOutput,
) -> Result<(), LegacyProviderError> {
    let selected = usize::try_from(session.vm.state().system_ui.focus_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_GALLERY_MOVIE_FOCUS",
            "movie gallery focus cannot be represented",
        )
    })?;
    if selected >= MINORI_GALLERY_MOVIE_LABELS.len() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GALLERY_MOVIE_FOCUS",
            "movie gallery focus is outside the verified range",
        ));
    }
    let text = MINORI_GALLERY_MOVIE_LABELS
        .iter()
        .enumerate()
        .map(|(index, label)| {
            if index == selected {
                format!("> {label}")
            } else {
                format!("  {label}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.len() > MAX_EPHEMERAL_TEXT_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GALLERY_MOVIE_TEXT_BOUNDS",
            "movie gallery text exceeds the ephemeral text bound",
        ));
    }
    let presentation_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let capture_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let lease_id = format!("minori.gallery.movie.{tick_index}.{capture_sequence}");
    let presentation = LegacyTextPresentationLeaseV1 {
        lease_id: lease_id.clone(),
        presentation: minori_gallery_movie_presentation(session.stage_size)?,
    };
    presentation.validate()?;
    if session
        .ephemeral_text
        .insert(
            lease_id.clone(),
            StagedEphemeralText {
                lease_id: lease_id.clone(),
                text: text.clone(),
                speaker: None,
                show_advance_indicator: false,
            },
        )
        .is_some()
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
            "movie gallery text lease id is duplicated",
        ));
    }
    live.text_presentations.push(LegacySequenced {
        sequence: presentation_sequence,
        value: presentation,
    });
    live.text.push(StagedTextLease {
        sequence: capture_sequence,
        lease_id,
        byte_len: text.len().try_into().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_GALLERY_MOVIE_TEXT_BOUNDS",
                "movie gallery text length cannot be represented",
            )
        })?,
        source_ref: "minori.gallery.movie".into(),
    });
    Ok(())
}

fn append_backlog_text(
    session: &mut MinoriSession,
    tick_index: u64,
    live: &mut LegacyLiveOutput,
) -> Result<(), LegacyProviderError> {
    let cursor = usize::try_from(session.vm.state().system_ui.backlog_cursor.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog text presentation has no active cursor",
        )
    })?)
    .map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog text cursor cannot be represented",
        )
    })?;
    let entry = session
        .vm
        .state()
        .backlog
        .get(cursor)
        .cloned()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
                "backlog text cursor is outside the retained history",
            )
        })?;
    if entry.text.len() > MAX_EPHEMERAL_TEXT_BYTES
        || entry
            .speaker
            .as_ref()
            .is_some_and(|value| value.len() > MAX_EPHEMERAL_TEXT_BYTES)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
            "backlog record exceeds the ephemeral text channel bound",
        ));
    }
    let presentation_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let capture_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let lease_id = format!("minori.text.backlog.{tick_index}.{capture_sequence}");
    let presentation = LegacyTextPresentationLeaseV1 {
        lease_id: lease_id.clone(),
        // IDA confirms that backlog state 11 keeps CMessagePanel mode 1 and
        // state 12 submits the selected CLog record through the same layout.
        presentation: minori_message_presentation(
            session.stage_size,
            session.vm.state().system_ui.config.text_shadow,
        )?,
    };
    presentation.validate()?;
    if session
        .ephemeral_text
        .insert(
            lease_id.clone(),
            StagedEphemeralText {
                lease_id: lease_id.clone(),
                text: entry.text.clone(),
                speaker: entry.speaker,
                show_advance_indicator: false,
            },
        )
        .is_some()
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
            "backlog text lease id is duplicated",
        ));
    }
    live.text_presentations.push(LegacySequenced {
        sequence: presentation_sequence,
        value: presentation,
    });
    live.text.push(StagedTextLease {
        sequence: capture_sequence,
        lease_id,
        byte_len: entry.text.len().try_into().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                "backlog message length cannot be represented",
            )
        })?,
        source_ref: "minori.sc.backlog".into(),
    });
    Ok(())
}

fn describe_backlog_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    sequence: u64,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_STAGE_SIZE",
            "backlog presentation requires explicit host dimensions",
        )
    })?;
    if stage_size != (1280, 720) || state.system_ui.page != MinoriSystemPage::Backlog {
        return Err(invalid(
            "ASTRA_EMU_MINORI_BACKLOG_STAGE_IDENTITY",
            "verified backlog presentation requires the 1280x720 reference stage",
        ));
    }
    let cursor = usize::try_from(state.system_ui.backlog_cursor.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog presentation has no active cursor",
        )
    })?)
    .map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog cursor cannot be represented",
        )
    })?;
    if cursor >= state.backlog.len() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
            "backlog cursor is outside the retained history",
        ));
    }
    let effect = visible_effect_frame(state, sequence)?;
    let mut frame = describe_effect_frame_without_secondary(
        vfs,
        mount_set_id,
        state,
        &effect,
        stage_size,
        true,
    )?;
    append_secondary_effect_to_frame(vfs, mount_set_id, state, &mut frame)?;
    apply_screen_shake_to_frame(state, &mut frame)?;

    let gauge_index = frame.texture_resources.len();
    append_resource_layer(
        vfs,
        mount_set_id,
        "minori:/sys/backlogGauge.png",
        0,
        0,
        1.0,
        MINORI_BACKLOG_GAUGE_TEXTURE_ID,
        &mut frame.texture_resources,
        &mut frame.draws,
    )?;
    let gauge = frame.texture_resources.get(gauge_index).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_RESOURCE",
            "backlog gauge resource was not appended",
        )
    })?;
    if (gauge.decoded_width, gauge.decoded_height) != (18, 144) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_BACKLOG_RESOURCE_DIMENSIONS",
            "backlog gauge does not match the verified dimensions",
        ));
    }
    let count = state.backlog.len();
    let ball_y = 138usize
        .checked_mul(cursor)
        .and_then(|value| value.checked_div(count))
        .map(|value| value.saturating_sub(7).min(124) + 3)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
                "backlog gauge position overflowed",
            )
        })?;
    let ball_index = frame.texture_resources.len();
    append_resource_layer(
        vfs,
        mount_set_id,
        "minori:/sys/ball.png",
        2,
        i32::try_from(ball_y).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_BACKLOG_CURSOR",
                "backlog gauge position cannot be represented",
            )
        })?,
        1.0,
        MINORI_BACKLOG_BALL_TEXTURE_ID,
        &mut frame.texture_resources,
        &mut frame.draws,
    )?;
    let ball = frame.texture_resources.get(ball_index).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_BACKLOG_RESOURCE",
            "backlog ball resource was not appended",
        )
    })?;
    if (ball.decoded_width, ball.decoded_height) != (14, 14) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_BACKLOG_RESOURCE_DIMENSIONS",
            "backlog ball does not match the verified dimensions",
        ));
    }
    frame.validate()?;
    Ok(frame)
}

fn describe_system_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    vm: &MinoriVm,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    describe_system_page_with_slots(vfs, mount_set_id, stage_size, vm, &BTreeSet::new())
}

fn describe_system_page_with_slots(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    vm: &MinoriVm,
    save_slots: &BTreeSet<u32>,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    describe_system_page_with_slots_and_hover(vfs, mount_set_id, stage_size, vm, save_slots, None)
}

fn describe_system_page_with_slots_and_hover(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    vm: &MinoriVm,
    save_slots: &BTreeSet<u32>,
    title_pointer_focus: Option<u32>,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let (width, height) = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_SYSTEM_STAGE_SIZE",
            "system UI requires explicit host dimensions",
        )
    })?;
    if (width, height) != (1280, 720) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_STAGE_IDENTITY",
            "verified Minori system pages require the 1280x720 reference stage",
        ));
    }
    if vm.state().system_ui.page == MinoriSystemPage::Config {
        return describe_config_page(vfs, mount_set_id, width, height, vm);
    }
    if matches!(
        vm.state().system_ui.page,
        MinoriSystemPage::Save | MinoriSystemPage::Load
    ) {
        return describe_save_load_page(vfs, mount_set_id, width, height, vm, save_slots);
    }
    if vm.state().system_ui.page == MinoriSystemPage::GalleryBgm {
        return describe_gallery_bgm_page(vfs, mount_set_id, width, height, vm);
    }
    if vm.state().system_ui.page == MinoriSystemPage::GalleryReplay {
        return describe_gallery_replay_page(vfs, mount_set_id, width, height, vm);
    }
    if vm.state().system_ui.page == MinoriSystemPage::GalleryMovie {
        return describe_gallery_movie_page(vfs, mount_set_id, width, height);
    }
    if vm.state().system_ui.page == MinoriSystemPage::GalleryCg {
        return describe_gallery_cg_page(vfs, mount_set_id, width, height, vm);
    }
    let title_variant = vm.title_variant();
    let resource_uri = match vm.state().system_ui.page {
        MinoriSystemPage::Title => match vm.title_variant() {
            0 => "minori:/sys/topMenu0.png",
            1 => "minori:/sys/topMenu1.png",
            2 => "minori:/sys/topMenu2.png",
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TITLE_VARIANT",
                    "verified title variant is outside the supported range",
                ));
            }
        },
        MinoriSystemPage::Load => unreachable!("save/load uses the stateful presentation path"),
        MinoriSystemPage::Config => unreachable!("config uses its stateful presentation path"),
        MinoriSystemPage::Memories => "minori:/sys/memories.png",
        MinoriSystemPage::GalleryCg
        | MinoriSystemPage::GalleryBgm
        | MinoriSystemPage::GalleryReplay => {
            gallery_resource_uri(vm.state().system_ui.page, vm.state().system_ui.focus_index)?
        }
        MinoriSystemPage::GalleryMovie => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_GALLERY_MOVIE_SCRIPT_REQUIRED",
                "the original movie gallery is script-driven and is not a static system page",
            ));
        }
        MinoriSystemPage::None => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SYSTEM_PAGE",
                "gameplay does not have a system-page presentation",
            ));
        }
        MinoriSystemPage::Save | MinoriSystemPage::Backlog => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SYSTEM_PAGE_PRESENTATION_ROUTE",
                "system page must use its verified dedicated presentation path",
            ));
        }
    };
    let resource =
        read_texture_resource(vfs, mount_set_id, resource_uri, MINORI_SYSTEM_TEXTURE_ID)?;
    if (resource.decoded_width, resource.decoded_height) != (width, height) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SYSTEM_RESOURCE_DIMENSIONS",
            "system page resource dimensions do not match the reference stage",
        ));
    }
    let mut texture_resources = vec![resource];
    let mut draws = Vec::with_capacity(2);
    let resource = texture_resources
        .first()
        .ok_or_else(|| invalid("ASTRA_EMU_MINORI_SYSTEM_RESOURCE", "title resource missing"))?;
    append_texture_draw(resource, 0, 0, 1.0, &mut draws)?;
    if vm.state().system_ui.page == MinoriSystemPage::Title {
        if let Some(focus) = title_pointer_focus {
            let over_uri = match title_variant {
                0 => "minori:/sys/topMenu0Over.png",
                1 => "minori:/sys/topMenu1Over.png",
                2 => "minori:/sys/topMenu2Over.png",
                _ => {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_TITLE_VARIANT",
                        "verified title variant is outside the supported range",
                    ));
                }
            };
            let over =
                read_texture_resource(vfs, mount_set_id, over_uri, MINORI_SYSTEM_TEXTURE_ID + 1)?;
            if (over.decoded_width, over.decoded_height) != (width, height) {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TITLE_HOVER_RESOURCE_DIMENSIONS",
                    "title hover resource does not match the reference stage",
                ));
            }
            let scissor = title_menu_hover_scissor(title_variant, focus)?;
            append_texture_draw_with_scissor(&over, 0, 0, 1.0, Some(scissor), &mut draws)?;
            texture_resources.push(over);
        }
    }
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn describe_gallery_bgm_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    width: u32,
    height: u32,
    vm: &MinoriVm,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let page_uri = gallery_resource_uri(
        MinoriSystemPage::GalleryBgm,
        vm.state().system_ui.focus_index,
    )?;
    let page = read_texture_resource(vfs, mount_set_id, page_uri, MINORI_SYSTEM_TEXTURE_ID)?;
    if (page.decoded_width, page.decoded_height) != (width, height) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GALLERY_BGM_RESOURCE_DIMENSIONS",
            "BGM gallery page does not match the reference stage",
        ));
    }
    let note = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/musicNote.png",
        MINORI_SYSTEM_TEXTURE_ID + 1,
    )?;
    if (note.decoded_width, note.decoded_height) != (32, 32) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GALLERY_BGM_RESOURCE_DIMENSIONS",
            "BGM gallery cursor resource does not match the verified dimensions",
        ));
    }
    let row = vm.state().system_ui.focus_index % 16;
    let note_y = 96i32
        .checked_add(
            i32::try_from(row).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_GALLERY_BGM_FOCUS",
                    "BGM gallery focus row cannot be represented",
                )
            })? * 32,
        )
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_GALLERY_BGM_FOCUS",
                "BGM gallery cursor position overflowed",
            )
        })?;
    let mut draws = Vec::with_capacity(2);
    append_texture_draw(&page, 0, 0, 1.0, &mut draws)?;
    append_texture_draw(&note, 143, note_y, 1.0, &mut draws)?;
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources: vec![page, note],
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn describe_gallery_replay_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    width: u32,
    height: u32,
    vm: &MinoriVm,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let index = usize::try_from(vm.state().system_ui.focus_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_GALLERY_REPLAY_FOCUS",
            "flashback gallery focus cannot be represented",
        )
    })?;
    let page_uri = MINORI_GALLERY_REPLAY_PAGE_URIS.get(index).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_GALLERY_REPLAY_FOCUS",
            "flashback gallery focus is outside the verified range",
        )
    })?;
    let menu_uri = format!("minori:/sys/flash{index}menu.png");
    let page = read_texture_resource(vfs, mount_set_id, page_uri, MINORI_SYSTEM_TEXTURE_ID)?;
    let menu = read_texture_resource(vfs, mount_set_id, &menu_uri, MINORI_SYSTEM_TEXTURE_ID + 1)?;
    if (page.decoded_width, page.decoded_height) != (width, height)
        || (menu.decoded_width, menu.decoded_height) != (384, 64)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GALLERY_REPLAY_RESOURCE_DIMENSIONS",
            "flashback gallery resources do not match the verified stage shape",
        ));
    }
    let mut draws = Vec::with_capacity(2);
    append_texture_draw(&page, 0, 0, 1.0, &mut draws)?;
    append_texture_draw(&menu, 0, 656, 1.0, &mut draws)?;
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources: vec![page, menu],
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn describe_gallery_movie_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    width: u32,
    height: u32,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    // The movie page shares the verified sunflower system backdrop. The
    // selectable movie names are emitted through the existing bounded text
    // presentation channel below; no synthetic bitmap or guessed title art is
    // introduced for the page.
    let background = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/memories.png",
        MINORI_SYSTEM_TEXTURE_ID,
    )?;
    if (background.decoded_width, background.decoded_height) != (width, height) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GALLERY_MOVIE_RESOURCE_DIMENSIONS",
            "movie gallery backdrop does not match the verified stage shape",
        ));
    }
    let mut draws = Vec::with_capacity(1);
    append_texture_draw(&background, 0, 0, 1.0, &mut draws)?;
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources: vec![background],
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn describe_gallery_cg_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    width: u32,
    height: u32,
    vm: &MinoriVm,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let base = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/cgmode0.png",
        MINORI_SYSTEM_TEXTURE_ID,
    )?;
    let boxes = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/cgmode0box.png",
        MINORI_SYSTEM_TEXTURE_ID + 1,
    )?;
    let menu = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/cgmode0menu.png",
        MINORI_SYSTEM_TEXTURE_ID + 2,
    )?;
    if (base.decoded_width, base.decoded_height) != (width, height)
        || (boxes.decoded_width, boxes.decoded_height) != (width, height)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE_DIMENSIONS",
            "CG gallery base resources do not match the reference stage",
        ));
    }
    let page_index = usize::try_from(vm.state().system_ui.focus_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE",
            "CG gallery page index cannot be represented",
        )
    })?;
    let page_label_uri = MINORI_GALLERY_CG_PAGE_URIS.get(page_index).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE",
            "CG gallery page index is outside the verified range",
        )
    })?;
    let page_label = read_texture_resource(
        vfs,
        mount_set_id,
        page_label_uri,
        MINORI_SYSTEM_TEXTURE_ID + 3,
    )?;
    let mut thumbnails =
        vfs.enumerate_by_extension(mount_set_id, "minori:/sys/cgthumb", "png", 256)?;
    thumbnails.sort_by(|left, right| left.uri.cmp(&right.uri));
    let page_start = page_index.checked_mul(16).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE",
            "CG gallery thumbnail range overflowed",
        )
    })?;
    let mut resources = vec![base, boxes, menu, page_label];
    let mut draws = Vec::with_capacity(20);
    append_texture_draw(&resources[0], 0, 0, 1.0, &mut draws)?;
    append_texture_draw(&resources[1], 0, 0, 1.0, &mut draws)?;
    append_texture_draw(&resources[3], 64, 48, 1.0, &mut draws)?;
    let positions: [(i32, i32); 4] = [(64, 96), (240, 96), (416, 96), (592, 96)];
    for (visible_index, thumbnail) in thumbnails.iter().skip(page_start).take(16).enumerate() {
        let thumbnail = read_texture_resource(
            vfs,
            mount_set_id,
            &thumbnail.uri,
            MINORI_GALLERY_CG_THUMB_TEXTURE_BASE
                + u32::try_from(visible_index).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE",
                        "CG gallery thumbnail index cannot be represented",
                    )
                })?,
        )?;
        if (thumbnail.decoded_width, thumbnail.decoded_height) != (128, 72) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE_DIMENSIONS",
                "CG gallery thumbnail does not match the verified 128x72 asset shape",
            ));
        }
        let row = visible_index / 4;
        let column = visible_index % 4;
        let x = positions[column].0 + 1;
        let y = positions[column]
            .1
            .checked_add(
                i32::try_from(row).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE",
                        "CG gallery thumbnail row cannot be represented",
                    )
                })? * 112
                    + 1,
            )
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE",
                    "CG gallery thumbnail position overflowed",
                )
            })?;
        append_texture_draw(&thumbnail, x, y, 0.4, &mut draws)?;
        resources.push(thumbnail);
    }
    append_texture_draw(
        resources.get(2).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_GALLERY_CG_RESOURCE",
                "CG gallery menu resource is missing",
            )
        })?,
        560,
        640,
        1.0,
        &mut draws,
    )?;
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources: resources,
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn describe_save_load_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    width: u32,
    height: u32,
    vm: &MinoriVm,
    save_slots: &BTreeSet<u32>,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let base = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/saveloadBase.png",
        MINORI_SYSTEM_TEXTURE_ID,
    )?;
    let title_uri = match vm.state().system_ui.page {
        MinoriSystemPage::Save => "minori:/sys/saveloadSave.png",
        MinoriSystemPage::Load => "minori:/sys/saveloadLoad.png",
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_LOAD_PAGE",
                "save/load renderer received an unrelated system page",
            ));
        }
    };
    let title = read_texture_resource(vfs, mount_set_id, title_uri, MINORI_SYSTEM_TEXTURE_ID + 1)?;
    let select = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/saveloadSelect.png",
        MINORI_SYSTEM_TEXTURE_ID + 2,
    )?;
    let page_index = vm.state().system_ui.focus_index / 10;
    if page_index >= 10 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_LOAD_PAGE",
            "save/load page index is outside the verified range",
        ));
    }
    let page_uri = format!("minori:/sys/saveload_Page{page_index}.png");
    let page = read_texture_resource(vfs, mount_set_id, &page_uri, MINORI_SYSTEM_TEXTURE_ID + 3)?;
    let buttons = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/saveloadButtons.png",
        MINORI_SYSTEM_TEXTURE_ID + 4,
    )?;
    let not_saved = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/notsaved.png",
        MINORI_SYSTEM_TEXTURE_ID + 5,
    )?;
    if (base.decoded_width, base.decoded_height) != (width, height)
        || (title.decoded_width, title.decoded_height) != (352, 48)
        || (select.decoded_width, select.decoded_height) != (344, 98)
        || (page.decoded_width, page.decoded_height) != (208, 48)
        || (buttons.decoded_width, buttons.decoded_height) != (356, 48)
        || (not_saved.decoded_width, not_saved.decoded_height) != (106, 60)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_LOAD_RESOURCE_DIMENSIONS",
            "save/load resources do not match the verified 1280x720 layout",
        ));
    }

    let mut draws = Vec::with_capacity(16);
    append_texture_draw(&base, 0, 0, 1.0, &mut draws)?;
    append_texture_draw(
        &title,
        MINORI_SAVE_LOAD_TITLE_X,
        MINORI_SAVE_LOAD_HEADER_Y,
        1.0,
        &mut draws,
    )?;
    append_texture_draw_with_scissor(
        &buttons,
        462,
        656,
        1.0,
        (vm.state().system_ui.page == MinoriSystemPage::Save).then_some(LegacyScissorV1 {
            x: 578,
            y: 656,
            width: 240,
            height: 48,
        }),
        &mut draws,
    )?;
    append_texture_draw(
        &page,
        MINORI_SAVE_LOAD_PAGE_X,
        MINORI_SAVE_LOAD_HEADER_Y,
        1.0,
        &mut draws,
    )?;

    let page_base = (vm.state().system_ui.focus_index / 10) * 10;
    let slot_index = vm.state().system_ui.focus_index % 10;
    let slot_left = [64, 456];
    let slot_top = [81, 189, 297, 405, 513];
    for row in 0..5u32 {
        for column in 0..2u32 {
            let visible_index = row * 2 + column;
            let slot = page_base + visible_index;
            let left = slot_left[column as usize];
            let top = slot_top[row as usize];
            if save_slots.contains(&slot) {
                continue;
            }
            append_texture_draw(&not_saved, left + 4, top + 18, 1.0, &mut draws)?;
        }
    }
    let selected_row = slot_index / 2;
    let selected_column = slot_index % 2;
    append_texture_draw(
        &select,
        slot_left[selected_column as usize],
        slot_top[selected_row as usize],
        1.0,
        &mut draws,
    )?;
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources: vec![base, title, select, page, buttons, not_saved],
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn gallery_resource_uri(
    page: MinoriSystemPage,
    focus_index: u32,
) -> Result<&'static str, LegacyProviderError> {
    let resource = match page {
        MinoriSystemPage::GalleryCg => MINORI_GALLERY_CG_PAGE_URIS
            .get(usize::try_from(focus_index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                    "CG gallery focus cannot be represented",
                )
            })?)
            .copied()
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                    "CG gallery focus is outside the verified page range",
                )
            })?,
        MinoriSystemPage::GalleryBgm => {
            if focus_index >= MINORI_GALLERY_BGM_TRACK_COUNT {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                    "BGM gallery focus is outside the verified track range",
                ));
            }
            MINORI_GALLERY_BGM_PAGE_URIS
                .get(usize::try_from(focus_index / 16).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                        "BGM gallery focus cannot be represented",
                    )
                })?)
                .copied()
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                        "BGM gallery focus is outside the verified page range",
                    )
                })?
        }
        MinoriSystemPage::GalleryReplay => MINORI_GALLERY_REPLAY_PAGE_URIS
            .get(usize::try_from(focus_index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                    "replay gallery focus cannot be represented",
                )
            })?)
            .copied()
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                    "replay gallery focus is outside the verified page range",
                )
            })?,
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_GALLERY_PAGE",
                "gallery resource requested for a non-gallery page",
            ));
        }
    };
    Ok(resource)
}

fn gallery_bgm_track_resource_uri(focus_index: u32) -> Result<&'static str, LegacyProviderError> {
    MINORI_GALLERY_BGM_TRACK_URIS
        .get(usize::try_from(focus_index).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                "BGM gallery track cannot be represented",
            )
        })?)
        .copied()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_GALLERY_FOCUS",
                "BGM gallery track is outside the verified range",
            )
        })
}

fn describe_config_page(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    width: u32,
    height: u32,
    vm: &MinoriVm,
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let base = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/configBase.png",
        MINORI_SYSTEM_TEXTURE_ID,
    )?;
    let knob = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/knob.png",
        MINORI_CONFIG_KNOB_TEXTURE_ID,
    )?;
    let checkmark = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/checkmark.png",
        MINORI_CONFIG_CHECKMARK_TEXTURE_ID,
    )?;
    let circle = read_texture_resource(
        vfs,
        mount_set_id,
        "minori:/sys/circle.png",
        MINORI_CONFIG_CIRCLE_TEXTURE_ID,
    )?;
    if (base.decoded_width, base.decoded_height) != (width, height)
        || (knob.decoded_width, knob.decoded_height) != (15, 25)
        || (checkmark.decoded_width, checkmark.decoded_height) != (21, 32)
        || (circle.decoded_width, circle.decoded_height) != (74, 74)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CONFIG_RESOURCE_DIMENSIONS",
            "config resources do not match the verified dimensions",
        ));
    }
    let config = vm.config_for_presentation().map_err(runtime_error)?;
    let mut draws = Vec::with_capacity(24);
    append_texture_draw(&base, 0, 0, 1.0, &mut draws)?;
    for (value, left, top) in [
        (config.message_speed_unread, 42, 159),
        (config.message_speed_read, 42, 235),
        (config.message_speed_auto_play, 42, 310),
        (config.bgm_volume, 578, 156),
        (config.voice_volume, 578, 231),
        (config.se_volume, 578, 305),
    ] {
        append_texture_draw(&knob, left + i32::from(value) * 2, top, 1.0, &mut draws)?;
    }
    let mut checks = Vec::with_capacity(15);
    checks.push(match config.preferred_play_mode {
        MinoriPlayMode::Auto => (40, 588),
        MinoriPlayMode::Skip => (153, 588),
        MinoriPlayMode::Normal => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_CONFIG_PLAY_MODE",
                "config preferred play mode is not verified",
            ));
        }
    });
    checks.push(if config.fullscreen {
        (319, 120)
    } else {
        (319, 164)
    });
    for (enabled, position) in [
        (config.screen_effect, (319, 248)),
        (config.text_shadow, (319, 292)),
        (config.animation, (319, 336)),
        (config.backlog_voice_playback, (319, 424)),
        (config.stop_voice_at_next_message, (319, 476)),
        (config.progress_in_background, (319, 572)),
        (config.bgm_muted, (684, 117)),
        (config.voice_muted, (684, 193)),
        (config.se_muted, (684, 265)),
    ] {
        if enabled {
            checks.push(position);
        }
    }
    for (enabled, position) in config.character_voice_enabled.iter().copied().zip([
        (575, 424),
        (575, 461),
        (575, 499),
        (575, 536),
        (696, 424),
    ]) {
        if enabled {
            checks.push(position);
        }
    }
    for (left, top) in checks {
        append_texture_draw(&checkmark, left, top, 1.0, &mut draws)?;
    }
    let pointer = (
        vm.state().system_ui.pointer_x,
        vm.state().system_ui.pointer_y,
    );
    let hover_circle = if (592..648).contains(&pointer.0) && (600..640).contains(&pointer.1) {
        Some((584, 584))
    } else if (701..775).contains(&pointer.0) && (600..640).contains(&pointer.1) {
        Some((701, 584))
    } else {
        None
    };
    if let Some((left, top)) = hover_circle {
        append_texture_draw(&circle, left, top, 1.0, &mut draws)?;
    }
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources: vec![base, knob, checkmark, circle],
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn waiting_output(
    session: &mut MinoriSession,
    wait: MinoriWaitState,
    live: LegacyLiveOutput,
    event: Option<LegacyEvent>,
    publish_rebound_wait: bool,
    _input: &LegacyStepInput,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    let mut output = LegacyStepOutput {
        status: LegacyRuntimeStatus::Awaiting,
        live,
        // A wait request is edge-triggered: it is published only by the
        // command that creates the token. Re-emitting the same pending token
        // on later ticks would violate RuntimeWorld AwaitQueue uniqueness.
        control: LegacyControlTransaction {
            events: event.into_iter().collect(),
            waits: publish_rebound_wait
                .then(|| legacy_wait(&wait, session.vm.state()))
                .into_iter()
                .collect(),
            ..LegacyControlTransaction::default()
        },
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta::default(),
        state_revision: session.vm.state().fixed_tick,
    };
    let reported_system_page = append_system_page_observation(session, &mut output.control)?;
    let reported_play_mode = append_play_mode_observation(session, &mut output.control)?;
    let reported_gallery_unlock_count =
        append_gallery_unlock_observation(session, &mut output.control)?;
    let reported_choice_active = append_choice_active_observation(session, &mut output.control)?;
    let reported_progress_in_background =
        append_progress_in_background_observation(session, &mut output.control)?;
    output.validate()?;
    if let Some(page) = reported_system_page {
        session.reported_system_page = Some(page);
    }
    if let Some(mode) = reported_play_mode {
        session.reported_play_mode = Some(mode);
    }
    if let Some(count) = reported_gallery_unlock_count {
        session.reported_gallery_unlock_count = Some(count);
    }
    if let Some(active) = reported_choice_active {
        session.reported_choice_active = Some(active);
    }
    if let Some(enabled) = reported_progress_in_background {
        session.reported_progress_in_background = Some(enabled);
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy)]
struct ChoiceVisualLayout {
    left: i32,
    top: i32,
    width: u32,
    row_height: u32,
}

fn append_choice_live_output(
    session: &mut MinoriSession,
    vfs: &Arc<dyn LegacyVfsReader>,
    tick_index: u64,
    sequence: u64,
    live: &mut LegacyLiveOutput,
) -> Result<LegacyEvent, LegacyProviderError> {
    let (option_hashes, selected_index) = session
        .vm
        .state()
        .choice
        .as_ref()
        .map(|choice| {
            (
                choice.option_hashes.clone(),
                choice.selected_index.unwrap_or_default(),
            )
        })
        .ok_or_else(|| runtime_error(MinoriRuntimeError::Choice))?;
    let options = session.vm.choice_display_texts().map_err(runtime_error)?;
    if options.len() != option_hashes.len()
        || options
            .iter()
            .any(|option| option.is_empty() || option.len() > MAX_EPHEMERAL_TEXT_BYTES)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_TEXT_BOUNDS",
            "choice text violates the ephemeral text bounds",
        ));
    }
    let scene_sequence = session
        .vm
        .allocate_effect_sequence()
        .map_err(runtime_error)?;
    let (scene, layout) = choice_resource_presentation(
        vfs,
        &session.mount_set_id,
        session.stage_size,
        session.vm.state(),
        option_hashes.len(),
        selected_index,
        scene_sequence,
    )?;
    live.resource_scenes.push(scene);
    for (index, option) in options.into_iter().enumerate() {
        let index_u32 = u32::try_from(index).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_STATE",
                "choice option index cannot be represented",
            )
        })?;
        let row_offset = layout.row_height.checked_mul(index_u32).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                "choice row offset overflowed",
            )
        })?;
        let vertical_padding = layout.row_height.saturating_sub(30) / 2;
        let y = i64::from(layout.top)
            .checked_add(i64::from(row_offset))
            .and_then(|value| value.checked_add(i64::from(vertical_padding)))
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                    "choice text position overflowed",
                )
            })?;
        let presentation_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        let capture_sequence = session
            .vm
            .allocate_effect_sequence()
            .map_err(runtime_error)?;
        let lease_id = format!("minori.choice.{tick_index}.{capture_sequence}.{index}");
        let presentation = LegacyTextPresentationLeaseV1 {
            lease_id: lease_id.clone(),
            presentation: LegacyTextPresentationV1 {
                layout_id: format!("minori.choice.option.{index}"),
                language: "ja-JP".into(),
                font_families: vec!["Noto Sans JP".into()],
                body: LegacyTextRegionV1 {
                    x: layout.left,
                    y,
                    width: layout.width,
                    height: 30,
                    font_size: 26.0,
                    line_height: 30.0,
                    max_lines: 1,
                    horizontal_alignment: LegacyTextHorizontalAlignmentV1::Center,
                },
                speaker: None,
                rgba: [255, 255, 255, 255],
                outline: Some(LegacyTextOutlineV1 {
                    radius: 2,
                    rgba: [0, 0, 0, 192],
                }),
            },
        };
        presentation.validate()?;
        if session
            .ephemeral_text
            .insert(
                lease_id.clone(),
                StagedEphemeralText {
                    lease_id: lease_id.clone(),
                    text: option.clone(),
                    speaker: None,
                    show_advance_indicator: false,
                },
            )
            .is_some()
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
                "choice text lease id is duplicated",
            ));
        }
        live.text_presentations.push(LegacySequenced {
            sequence: presentation_sequence,
            value: presentation,
        });
        live.text.push(StagedTextLease {
            sequence: capture_sequence,
            lease_id,
            byte_len: option.len().try_into().map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_TEXT_BOUNDS",
                    "choice text length cannot be represented",
                )
            })?,
            source_ref: "minori.sc.select".into(),
        });
    }
    choice_presentation_from_parts(&option_hashes, selected_index, sequence)
}

fn choice_resource_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    option_count: usize,
    selected_index: u32,
    sequence: u64,
) -> Result<
    (
        LegacySequenced<LegacyRenderResourceFrameV1>,
        ChoiceVisualLayout,
    ),
    LegacyProviderError,
> {
    let (stage_width, stage_height) = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_CHOICE_STAGE_IDENTITY",
            "choice presentation requires explicit host dimensions",
        )
    })?;
    if (stage_width, stage_height) != (1280, 720)
        || !(1..=4).contains(&option_count)
        || usize::try_from(selected_index)
            .ok()
            .is_none_or(|index| index >= option_count)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_STAGE_IDENTITY",
            "choice presentation is outside the verified stage or option bounds",
        ));
    }
    let mut frame = if state.firefly.is_some() {
        describe_firefly_frame(vfs, mount_set_id, state, (stage_width, stage_height))?
    } else {
        describe_effect_frame(
            vfs,
            mount_set_id,
            state,
            &visible_effect_frame(state, sequence)?,
            (stage_width, stage_height),
        )?
    };
    let mut choice_resources = Vec::with_capacity(MINORI_CHOICE_RESOURCE_URIS.len());
    for (index, uri) in MINORI_CHOICE_RESOURCE_URIS.iter().enumerate() {
        let texture_id = MINORI_CHOICE_TEXTURE_BASE
            .checked_add(u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_RESOURCE",
                    "choice resource index cannot be represented",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_RESOURCE",
                    "choice texture id overflowed",
                )
            })?;
        choice_resources.push(read_texture_resource(vfs, mount_set_id, uri, texture_id)?);
    }
    let width = choice_resources[0].decoded_width;
    let row_height = choice_resources[0].decoded_height;
    if width == 0
        || row_height < 30
        || choice_resources.iter().any(|resource| {
            resource.decoded_width != width || resource.decoded_height != row_height
        })
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_RESOURCE_IDENTITY",
            "choice state resources do not share the verified dimensions",
        ));
    }
    let total_height = row_height
        .checked_mul(u32::try_from(option_count).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                "choice option count cannot be represented",
            )
        })?)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                "choice layout height overflowed",
            )
        })?;
    if width > stage_width || total_height > stage_height {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
            "choice resources exceed the stage bounds",
        ));
    }
    let left = i32::try_from((stage_width - width) / 2).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
            "choice horizontal position cannot be represented",
        )
    })?;
    let top = i32::try_from((stage_height - total_height) / 2).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
            "choice vertical position cannot be represented",
        )
    })?;
    for index in 0..option_count {
        let texture_index = if index == usize::try_from(selected_index).unwrap_or_default() {
            1
        } else {
            0
        };
        let resource = &choice_resources[texture_index];
        let index = i64::try_from(index).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                "choice row index cannot be represented",
            )
        })?;
        let y = i64::from(top)
            .checked_add(i64::from(row_height) * index)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_CHOICE_LAYOUT",
                    "choice row position overflowed",
                )
            })?;
        append_texture_draw(resource, left, y, 1.0, &mut frame.draws)?;
    }
    frame.texture_resources.extend(choice_resources);
    frame.validate()?;
    Ok((
        LegacySequenced {
            sequence,
            value: frame,
        },
        ChoiceVisualLayout {
            left,
            top,
            width,
            row_height,
        },
    ))
}

fn choice_direction(control: &str) -> Option<i32> {
    if control == MINORI_CHOICE_NAVIGATION_CONTROLS[0] {
        Some(-1)
    } else if control == MINORI_CHOICE_NAVIGATION_CONTROLS[1] {
        Some(1)
    } else {
        None
    }
}

fn choice_presentation_from_parts(
    option_hashes: &[Hash256],
    selected_index: u32,
    sequence: u64,
) -> Result<LegacyEvent, LegacyProviderError> {
    if !(1..=4).contains(&option_hashes.len())
        || usize::try_from(selected_index)
            .ok()
            .is_none_or(|index| index >= option_hashes.len())
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_CHOICE_STATE",
            "choice presentation has an invalid option selection",
        ));
    }
    let payload = postcard::to_allocvec(&MinoriChoicePresentation {
        schema: MINORI_CHOICE_PRESENTATION_SCHEMA.into(),
        option_hashes: option_hashes.to_vec(),
        selected_index,
    })
    .map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_CHOICE_ENCODE",
            "choice presentation could not be encoded",
        )
    })?;
    Ok(LegacyEvent {
        sequence,
        event: MINORI_CHOICE_PRESENTATION_SCHEMA.into(),
        value: Hash256::from_sha256(&payload).to_string(),
    })
}

fn movie_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    movie: &MinoriMovieState,
    sequence: u64,
) -> Result<LegacySequenced<LegacyVideoCommandV1>, LegacyProviderError> {
    if movie.continuation_pts != 0 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MEDIA_CONTINUATION_UNSUPPORTED",
            "v9 video Play cannot restore a non-zero continuation position",
        ));
    }
    if stage_size != Some((movie.width, movie.height)) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOVIE_STAGE_IDENTITY",
            "movie dimensions do not match the explicit runtime stage",
        ));
    }
    let stat = vfs
        .stat_file(mount_set_id, &movie.resource_uri)
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_MOVIE_RESOURCE",
                "movie resource is unavailable",
            )
        })?;
    if stat.len == 0 || stat.len > MAX_RESOURCE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOVIE_RESOURCE",
            "movie resource violates the bounded VFS contract",
        ));
    }
    let command = LegacyVideoCommandV1::Play {
        playback_id: movie.media_id.clone(),
        resource_uri: movie.resource_uri.clone(),
        mode: LegacyVideoMode::ModalWithAudio,
        stage_width: movie.width,
        stage_height: movie.height,
    };
    command.validate()?;
    Ok(LegacySequenced {
        sequence,
        value: command,
    })
}

fn legacy_wait(wait: &MinoriWaitState, state: &MinoriRuntimeState) -> LegacyWaitRequest {
    match wait {
        MinoriWaitState::Time {
            token_id,
            timer_ticks: _,
            milliseconds,
        }
        | MinoriWaitState::Voice {
            token_id,
            timer_ticks: _,
            milliseconds,
            ..
        } => LegacyWaitRequest::Time {
            token_id: token_id.clone(),
            milliseconds: *milliseconds,
        },
        MinoriWaitState::AxisScroll {
            token_id,
            milliseconds,
        }
        | MinoriWaitState::LinearScroll {
            token_id,
            milliseconds,
        }
        | MinoriWaitState::CharacterTransition {
            token_id,
            milliseconds,
            ..
        } => LegacyWaitRequest::Time {
            token_id: token_id.clone(),
            milliseconds: *milliseconds,
        },
        MinoriWaitState::Input { token_id } => LegacyWaitRequest::Input {
            token_id: token_id.clone(),
            keys: message_input_keys(
                state.system_ui.skip_enabled && state.system_ui.control_enabled,
            ),
        },
        MinoriWaitState::Choice { token_id } => LegacyWaitRequest::Input {
            token_id: token_id.clone(),
            keys: choice_input_keys(),
        },
        MinoriWaitState::Media { token_id, media_id } => LegacyWaitRequest::MediaFence {
            token_id: token_id.clone(),
            media_id: media_id.clone(),
        },
        MinoriWaitState::Presentation { token_id, fence_id } => {
            LegacyWaitRequest::PresentationFence {
                token_id: token_id.clone(),
                fence_id: fence_id.clone(),
            }
        }
        MinoriWaitState::Provider {
            token_id,
            request_id,
        } => LegacyWaitRequest::ProviderCompletion {
            token_id: token_id.clone(),
            request_id: request_id.clone(),
        },
    }
}

fn wait_token(wait: &MinoriWaitState) -> &str {
    match wait {
        MinoriWaitState::Time { token_id, .. }
        | MinoriWaitState::Voice { token_id, .. }
        | MinoriWaitState::AxisScroll { token_id, .. }
        | MinoriWaitState::LinearScroll { token_id, .. }
        | MinoriWaitState::CharacterTransition { token_id, .. }
        | MinoriWaitState::Input { token_id }
        | MinoriWaitState::Choice { token_id }
        | MinoriWaitState::Media { token_id, .. }
        | MinoriWaitState::Presentation { token_id, .. }
        | MinoriWaitState::Provider { token_id, .. } => token_id,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MinoriLayerRole {
    Background,
    Foreground,
    Effect,
    Panel,
}

impl MinoriLayerRole {
    const ALL: [Self; 4] = [
        Self::Background,
        Self::Foreground,
        Self::Effect,
        Self::Panel,
    ];

    const fn symbol(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Foreground => "foreground",
            Self::Effect => "effect",
            Self::Panel => "panel",
        }
    }

    const fn z_index(self) -> i32 {
        match self {
            Self::Background => 0,
            Self::Foreground => 100,
            Self::Effect => 200,
            Self::Panel => 300,
        }
    }

    fn classify(texture_id: u32) -> Result<Self, LegacyProviderError> {
        match texture_id {
            1 => Ok(Self::Background),
            16..=99 | MINORI_CHARACTER_TEXTURE_BASE..=19_999 => Ok(Self::Foreground),
            100 | 101 | 300..=302 | 600..=602 => Ok(Self::Effect),
            200 | MINORI_CHOICE_TEXTURE_BASE..=502 | MINORI_SYSTEM_TEXTURE_ID.. => Ok(Self::Panel),
            _ => Err(invalid(
                "ASTRA_EMU_MINORI_LAYER_CLASSIFICATION",
                "render texture id has no verified Minori layer role",
            )),
        }
    }
}

struct PreparedMinoriLayer {
    role: MinoriLayerRole,
    rgba8_premultiplied: Arc<[u8]>,
}

#[derive(Debug, Clone, PartialEq)]
struct CachedMinoriLayer {
    width: u32,
    height: u32,
    resources: Vec<LegacyTextureResourceV1>,
    draws: Vec<LegacyDrawV1>,
    rgba8_premultiplied: Arc<[u8]>,
}

fn publish_v9_output(
    services: &LegacyFamilyHostServicesV9,
    vfs: &Arc<dyn LegacyVfsReader>,
    session_id: &LegacyRuntimeSessionId,
    fixed_step: u64,
    session: &mut MinoriSession,
    staged: LegacyStepOutput,
) -> Result<LegacyStepOutputV9, LegacyProviderError> {
    let prepared_text = prepare_text_surface(session, &staged)?;
    let next_text_surface = match prepared_text.as_ref() {
        Some(prepared) => Some(Arc::<[u8]>::from(prepared.rgba8_premultiplied.clone())),
        None if staged.live.clear_text => None,
        None => session.last_text_surface.clone(),
    };
    let layer_sequence = next_layer_sequence(&staged, session.last_layer_sequence)?;
    // Resource-backed scenes are retained by the current Family ABI host. Minori emits a
    // new Layer2D transaction only when the bounded descriptor actually
    // changes; otherwise re-rendering all four full-size layer surfaces would
    // turn a fixed-tick wait into repeated PAZ reads, image decodes and CPU
    // composites with no visible effect.
    let changed_resource_scene = staged.live.resource_scenes.last().filter(|resource_scene| {
        session.last_resource_frame.as_ref() != Some(&resource_scene.value)
    });
    let save_overlay = matches!(
        session.vm.state().system_ui.page,
        MinoriSystemPage::Save | MinoriSystemPage::Load
    )
    .then_some((
        &session.save_slot_metadata,
        (session.vm.state().system_ui.focus_index / 10) * 10,
    ));
    let mut layers = if let Some(resource_scene) = changed_resource_scene {
        let mount_set_id = session.mount_set_id.clone();
        let layers = publish_resource_scene(
            services,
            vfs,
            session_id,
            &mount_set_id,
            fixed_step,
            layer_sequence,
            &mut session.published_layers,
            &mut session.presentation_layers,
            &resource_scene.value,
            save_overlay,
        )?;
        session.last_resource_frame = Some(resource_scene.value.clone());
        layers
    } else {
        Vec::new()
    };
    let text_operation = match prepared_text {
        Some(prepared) => Some(publish_text_surface(
            services, session_id, fixed_step, session, prepared,
        )?),
        None if staged.live.clear_text && session.published_layers.remove(MINORI_TEXT_LAYER_ID) => {
            Some(LegacyLayerOperationV9::Destroy {
                layer_id: MINORI_TEXT_LAYER_ID.into(),
            })
        }
        None => None,
    };
    if let Some(operation) = text_operation {
        if let Some(transaction) = layers.first_mut() {
            transaction.operations.push(operation);
            transaction.validate()?;
        } else {
            let (viewport_width, viewport_height) = session.stage_size.ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY",
                    "text layer output requires the verified Minori stage",
                )
            })?;
            let transaction = LegacyLayerTransactionV9 {
                sequence: layer_sequence,
                viewport_width,
                viewport_height,
                operations: vec![operation],
            };
            transaction.validate()?;
            layers.push(transaction);
        }
    }
    session.last_text_surface = next_text_surface.clone();
    if session.vm.state().system_ui.page == MinoriSystemPage::None {
        if let Some(frame) = compose_minori_frame(
            session.stage_size,
            &session.presentation_layers,
            next_text_surface.as_deref(),
        )? {
            session.last_gameplay_frame = Some(Arc::from(frame));
        }
    }
    if !layers.is_empty() {
        session.last_layer_sequence = layer_sequence;
    }
    let output = LegacyStepOutputV9 {
        status: staged.status,
        live: LegacyLiveOutputV9 {
            layers,
            audio: staged.live.audio,
            audio_commands: staged.live.audio_commands,
            video: staged.live.video,
        },
        control: staged.control,
        trace: staged.trace,
        diagnostics: staged.diagnostics,
        coverage: staged.coverage,
        state_revision: staged.state_revision,
    };
    output.validate()?;
    Ok(output)
}

struct PreparedTextSurface {
    rgba8_premultiplied: Vec<u8>,
    width: u32,
    height: u32,
}

fn prepare_text_surface(
    session: &mut MinoriSession,
    staged: &LegacyStepOutput,
) -> Result<Option<PreparedTextSurface>, LegacyProviderError> {
    if staged.live.text.is_empty() && staged.live.text_presentations.is_empty() {
        return Ok(None);
    }
    if staged.live.text.is_empty()
        || staged.live.text.len() != staged.live.text_presentations.len()
        || staged.live.text.len() > 16
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_BATCH_IDENTITY",
            "text leases and presentation descriptors must form one bounded batch",
        ));
    }
    let presentations = staged
        .live
        .text_presentations
        .iter()
        .map(|presentation| {
            (
                &presentation.value.lease_id,
                &presentation.value.presentation,
            )
        })
        .collect::<BTreeMap<_, _>>();
    if presentations.len() != staged.live.text_presentations.len() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_PRESENTATION_DUPLICATE",
            "text presentation lease id is duplicated",
        ));
    }
    let mut requests = Vec::with_capacity(staged.live.text.len());
    let mut consumed = Vec::with_capacity(staged.live.text.len());
    for lease in &staged.live.text {
        let captured = session.ephemeral_text.get(&lease.lease_id).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_LEASE_MISSING",
                "text lease is missing from the family session",
            )
        })?;
        if captured.lease_id != lease.lease_id
            || captured.text.len() != lease.byte_len as usize
            || captured.text.len() > MAX_EPHEMERAL_TEXT_BYTES
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_TEXT_LEASE_IDENTITY",
                "text lease metadata does not match its family-owned capture",
            ));
        }
        let presentation = presentations.get(&lease.lease_id).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_PRESENTATION_MISSING",
                "text lease has no presentation descriptor",
            )
        })?;
        // The original Japanese build is the only supported Minori variant.
        // Text has already crossed the strict CP932 locale hook at VFS/script
        // decode time, so the presentation path must preserve it byte-for-byte
        // and must never invoke a translation or overlay provider.
        let text = captured.text.clone();
        let speaker = captured.speaker.clone();
        requests.push(TextSurfaceRequest {
            key: presentation.layout_id.clone(),
            text,
            speaker,
            body: text_region(presentation.body),
            speaker_region: presentation.speaker.map(text_region),
            show_advance_indicator: captured.show_advance_indicator,
            rgba: presentation.rgba,
            outline: presentation.outline.map(|outline| TextOutline {
                radius: outline.radius,
                rgba: outline.rgba,
            }),
        });
        consumed.push(lease.lease_id.clone());
    }
    let (width, height) = session.stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY",
            "text output requires the verified Minori stage",
        )
    })?;
    let rgba8_premultiplied = session
        .text_renderer
        .as_mut()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_RENDERER_MISSING",
                "text output has no family-owned CosmicText renderer",
            )
        })?
        .render(&requests)
        .map_err(|code| invalid(code, "Minori text rasterization failed"))?;
    for lease_id in consumed {
        session.ephemeral_text.remove(&lease_id).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_TEXT_LEASE_STATE",
                "text lease disappeared before publication",
            )
        })?;
    }
    Ok(Some(PreparedTextSurface {
        rgba8_premultiplied,
        width,
        height,
    }))
}

fn text_region(region: LegacyTextRegionV1) -> TextRegion {
    TextRegion {
        x: region.x,
        y: region.y,
        width: region.width,
        height: region.height,
        font_size: region.font_size,
        line_height: region.line_height,
        max_lines: region.max_lines,
        alignment: match region.horizontal_alignment {
            LegacyTextHorizontalAlignmentV1::Start => TextAlignment::Start,
            LegacyTextHorizontalAlignmentV1::Center => TextAlignment::Center,
        },
    }
}

fn publish_text_surface(
    services: &LegacyFamilyHostServicesV9,
    session_id: &LegacyRuntimeSessionId,
    fixed_step: u64,
    session: &mut MinoriSession,
    prepared: PreparedTextSurface,
) -> Result<LegacyLayerOperationV9, LegacyProviderError> {
    let mut lease = services.surfaces.acquire(
        &session_id.0,
        fixed_step,
        MINORI_TEXT_SURFACE_ID,
        prepared.width,
        prepared.height,
        LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
    )?;
    write_surface_rows(&mut lease, &prepared.rgba8_premultiplied)?;
    let state = LegacyLayerStateV9 {
        layer_id: MINORI_TEXT_LAYER_ID.into(),
        role: "text".into(),
        z_index: 400,
        surface_id: lease.surface_id.clone(),
        generation: lease.generation,
        width: lease.width,
        height: lease.height,
        stride: lease.stride,
        format: lease.format,
        damage: LegacySurfaceDamageV9::Full,
        transform: LegacyLayerTransformV9 {
            m11: 1.0,
            m12: 0.0,
            m21: 0.0,
            m22: 1.0,
            tx: 0.0,
            ty: 0.0,
        },
        clip: None,
        opacity: 1.0,
        texture_filter: LegacyLayerFilterV9::Linear,
        blend: LegacyLayerBlendV9::Alpha,
        filter_graph: None,
    };
    services.surfaces.commit(
        &session_id.0,
        fixed_step,
        LegacySurfaceCommitV9 {
            lease,
            damage: LegacySurfaceDamageV9::Full,
        },
    )?;
    Ok(
        if session.published_layers.insert(MINORI_TEXT_LAYER_ID.into()) {
            LegacyLayerOperationV9::Create(state)
        } else {
            LegacyLayerOperationV9::Update(state)
        },
    )
}

fn compose_minori_frame(
    stage_size: Option<(u32, u32)>,
    layers: &BTreeMap<MinoriLayerRole, CachedMinoriLayer>,
    text_surface: Option<&[u8]>,
) -> Result<Option<Vec<u8>>, LegacyProviderError> {
    let Some((width, height)) = stage_size else {
        return Ok(None);
    };
    let pixel_count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "frame dimensions overflowed",
            )
        })?;
    let byte_count = pixel_count
        .checked_mul(4)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| invalid("ASTRA_EMU_MINORI_SAVE_THUMBNAIL", "frame size overflowed"))?;
    let mut frame = vec![0_u8; byte_count];
    let mut has_surface = false;
    for role in MinoriLayerRole::ALL {
        if let Some(layer) = layers.get(&role) {
            if layer.width != width
                || layer.height != height
                || layer.rgba8_premultiplied.len() != byte_count
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                    "cached Minori layer does not match the verified stage",
                ));
            }
            alpha_over_premultiplied(&mut frame, &layer.rgba8_premultiplied);
            has_surface = true;
        }
    }
    if let Some(text) = text_surface {
        if text.len() != byte_count {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "cached Minori text surface does not match the verified stage",
            ));
        }
        alpha_over_premultiplied(&mut frame, text);
        has_surface = true;
    }
    Ok(has_surface.then_some(frame))
}

fn alpha_over_premultiplied(destination: &mut [u8], source: &[u8]) {
    for (dst, src) in destination
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(source.as_chunks::<4>().0)
    {
        let source_alpha = u16::from(src[3]);
        let inverse_alpha = 255_u16 - source_alpha;
        for channel in 0..3 {
            dst[channel] = (u16::from(src[channel]).saturating_add(
                u16::from(dst[channel])
                    .saturating_mul(inverse_alpha)
                    .saturating_add(127)
                    / 255,
            )) as u8;
        }
        dst[3] = (u16::from(src[3]).saturating_add(
            u16::from(dst[3])
                .saturating_mul(inverse_alpha)
                .saturating_add(127)
                / 255,
        )) as u8;
    }
}

fn next_layer_sequence(
    staged: &LegacyStepOutput,
    last_layer_sequence: u64,
) -> Result<u64, LegacyProviderError> {
    let current_step_sequence = staged
        .live
        .resource_scenes
        .iter()
        .map(|value| value.sequence)
        .chain(staged.live.audio.iter().map(|value| value.sequence))
        .chain(
            staged
                .live
                .audio_commands
                .iter()
                .map(|value| value.sequence),
        )
        .chain(staged.live.video.iter().map(|value| value.sequence))
        .chain(staged.control.events.iter().map(|value| value.sequence))
        .chain(staged.control.blackboard.iter().map(|value| value.sequence))
        .max()
        .unwrap_or(0);
    current_step_sequence
        .max(last_layer_sequence)
        .checked_add(1)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_SEQUENCE",
                "layer transaction sequence overflowed",
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn publish_resource_scene(
    services: &LegacyFamilyHostServicesV9,
    vfs: &Arc<dyn LegacyVfsReader>,
    session_id: &LegacyRuntimeSessionId,
    mount_set_id: &str,
    fixed_step: u64,
    sequence: u64,
    published_layers: &mut BTreeSet<String>,
    presentation_layers: &mut BTreeMap<MinoriLayerRole, CachedMinoriLayer>,
    frame: &LegacyRenderResourceFrameV1,
    save_overlay: Option<(&BTreeMap<u32, MinoriSaveSlotMetadata>, u32)>,
) -> Result<Vec<LegacyLayerTransactionV9>, LegacyProviderError> {
    frame.validate()?;
    let prepared = prepare_resource_layers(vfs, mount_set_id, frame, presentation_layers)?;
    let prepared = prepared
        .into_iter()
        .map(|layer| {
            if layer.role == MinoriLayerRole::Panel {
                if let Some((metadata, page_base)) = save_overlay {
                    return overlay_save_thumbnails(
                        layer,
                        metadata,
                        page_base,
                        frame.width,
                        frame.height,
                    );
                }
            }
            Ok(layer)
        })
        .collect::<Result<Vec<_>, LegacyProviderError>>()?;
    let mut leases = Vec::with_capacity(prepared.len());
    for layer in prepared {
        let surface_id = format!("minori.surface.{}", layer.role.symbol());
        let mut lease = services.surfaces.acquire(
            &session_id.0,
            fixed_step,
            &surface_id,
            frame.width,
            frame.height,
            LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
        )?;
        lease.validate()?;
        write_surface_rows(&mut lease, &layer.rgba8_premultiplied)?;
        leases.push((layer.role, lease));
    }

    let mut operations = Vec::with_capacity(leases.len());
    for (role, lease) in leases {
        let layer_id = format!("minori.layer.{}", role.symbol());
        let state = LegacyLayerStateV9 {
            layer_id: layer_id.clone(),
            role: role.symbol().to_owned(),
            z_index: role.z_index(),
            surface_id: lease.surface_id.clone(),
            generation: lease.generation,
            width: lease.width,
            height: lease.height,
            stride: lease.stride,
            format: lease.format,
            damage: LegacySurfaceDamageV9::Full,
            transform: LegacyLayerTransformV9 {
                m11: 1.0,
                m12: 0.0,
                m21: 0.0,
                m22: 1.0,
                tx: 0.0,
                ty: 0.0,
            },
            clip: None,
            opacity: 1.0,
            texture_filter: LegacyLayerFilterV9::Linear,
            blend: LegacyLayerBlendV9::Alpha,
            filter_graph: None,
        };
        services.surfaces.commit(
            &session_id.0,
            fixed_step,
            LegacySurfaceCommitV9 {
                lease,
                damage: LegacySurfaceDamageV9::Full,
            },
        )?;
        let operation = if published_layers.insert(layer_id) {
            LegacyLayerOperationV9::Create(state)
        } else {
            LegacyLayerOperationV9::Update(state)
        };
        operations.push(operation);
    }
    let transaction = LegacyLayerTransactionV9 {
        sequence,
        viewport_width: frame.width,
        viewport_height: frame.height,
        operations,
    };
    transaction.validate()?;
    Ok(vec![transaction])
}

fn overlay_save_thumbnails(
    layer: PreparedMinoriLayer,
    metadata: &BTreeMap<u32, MinoriSaveSlotMetadata>,
    page_base: u32,
    width: u32,
    height: u32,
) -> Result<PreparedMinoriLayer, LegacyProviderError> {
    if width != 1280 || height != 720 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL_STAGE",
            "save thumbnail composition requires the verified 1280x720 stage",
        ));
    }
    let expected = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(usize::try_from(height).ok()?))
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| invalid("ASTRA_EMU_MINORI_SAVE_THUMBNAIL", "panel size overflowed"))?;
    if layer.rgba8_premultiplied.len() != expected {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "save panel layer does not match the verified stage",
        ));
    }
    let mut rgba = layer.rgba8_premultiplied.to_vec();
    let slot_left = [64_i32, 456_i32];
    let slot_top = [81_i32, 189_i32, 297_i32, 405_i32, 513_i32];
    for visible_index in 0..10_u32 {
        let Some(slot) = page_base.checked_add(visible_index) else {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "save slot index overflowed",
            ));
        };
        let Some(slot_metadata) = metadata.get(&slot) else {
            continue;
        };
        let thumbnail_expected = usize::try_from(MINORI_SAVE_THUMBNAIL_WIDTH)
            .ok()
            .and_then(|value| {
                value.checked_mul(usize::try_from(MINORI_SAVE_THUMBNAIL_HEIGHT).ok()?)
            })
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                    "thumbnail dimensions overflowed",
                )
            })?;
        if slot_metadata.thumbnail_rgba.len() != thumbnail_expected {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "save slot thumbnail does not match the verified dimensions",
            ));
        }
        let row = usize::try_from(visible_index / 2).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "save slot row cannot be represented",
            )
        })?;
        let column = usize::try_from(visible_index % 2).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "save slot column cannot be represented",
            )
        })?;
        let x = slot_left[column] + 10;
        let y = slot_top[row] + 15;
        blit_save_thumbnail(
            &mut rgba,
            width,
            height,
            &slot_metadata.thumbnail_rgba,
            x,
            y,
        )?;
    }
    Ok(PreparedMinoriLayer {
        role: layer.role,
        rgba8_premultiplied: Arc::from(rgba),
    })
}

fn blit_save_thumbnail(
    destination: &mut [u8],
    destination_width: u32,
    destination_height: u32,
    source: &[u8],
    x: i32,
    y: i32,
) -> Result<(), LegacyProviderError> {
    let source_width = usize::try_from(MINORI_SAVE_THUMBNAIL_WIDTH).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "thumbnail width cannot be represented",
        )
    })?;
    let source_height = usize::try_from(MINORI_SAVE_THUMBNAIL_HEIGHT).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "thumbnail height cannot be represented",
        )
    })?;
    let destination_width = usize::try_from(destination_width).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "save panel width cannot be represented",
        )
    })?;
    let destination_height = usize::try_from(destination_height).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "save panel height cannot be represented",
        )
    })?;
    let source_len = source_width
        .checked_mul(source_height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                "thumbnail size overflowed",
            )
        })?;
    if source.len() != source_len {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "thumbnail buffer length is invalid",
        ));
    }
    let x = usize::try_from(x).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "thumbnail X coordinate is invalid",
        )
    })?;
    let y = usize::try_from(y).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "thumbnail Y coordinate is invalid",
        )
    })?;
    if x.checked_add(source_width)
        .is_none_or(|right| right > destination_width)
        || y.checked_add(source_height)
            .is_none_or(|bottom| bottom > destination_height)
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
            "thumbnail lies outside the save panel",
        ));
    }
    for row in 0..source_height {
        let destination_start = (y + row)
            .checked_mul(destination_width)
            .and_then(|index| index.checked_add(x))
            .and_then(|index| index.checked_mul(4))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                    "thumbnail destination offset overflowed",
                )
            })?;
        let source_start = row
            .checked_mul(source_width)
            .and_then(|index| index.checked_mul(4))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                    "thumbnail source offset overflowed",
                )
            })?;
        let destination_row = destination
            .get_mut(destination_start..destination_start + source_width * 4)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_SAVE_THUMBNAIL",
                    "thumbnail destination row is out of bounds",
                )
            })?;
        let source_row = &source[source_start..source_start + source_width * 4];
        alpha_over_premultiplied(destination_row, source_row);
    }
    Ok(())
}

fn prepare_resource_layers(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    frame: &LegacyRenderResourceFrameV1,
    presentation_layers: &mut BTreeMap<MinoriLayerRole, CachedMinoriLayer>,
) -> Result<Vec<PreparedMinoriLayer>, LegacyProviderError> {
    let layer_bytes = u64::from(frame.width)
        .checked_mul(u64::from(frame.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| bytes.checked_mul(MinoriLayerRole::ALL.len() as u64))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_BOUNDS",
                "Minori layer surface size overflowed",
            )
        })?;
    if layer_bytes > MAX_LAYER_SURFACE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_BOUNDS",
            "Minori layer surfaces exceed the decoded byte budget",
        ));
    }
    let resources = frame
        .texture_resources
        .iter()
        .map(|resource| (resource.texture_id, resource))
        .collect::<BTreeMap<_, _>>();
    let mut draws = BTreeMap::<MinoriLayerRole, Vec<&LegacyDrawV1>>::new();
    for draw in &frame.draws {
        draws
            .entry(MinoriLayerRole::classify(draw.texture_id)?)
            .or_default()
            .push(draw);
    }
    MinoriLayerRole::ALL
        .into_iter()
        .map(|role| {
            let role_draws = draws.remove(&role).unwrap_or_default();
            let mut role_resources = BTreeMap::new();
            for draw in &role_draws {
                let resource = resources.get(&draw.texture_id).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_MINORI_LAYER_RESOURCE",
                        "layer draw references an unknown texture resource",
                    )
                })?;
                role_resources.insert(draw.texture_id, (*resource).clone());
            }
            let role_resources = role_resources.into_values().collect::<Vec<_>>();
            let owned_draws = role_draws
                .iter()
                .map(|draw| (*draw).clone())
                .collect::<Vec<_>>();
            if let Some(cached) = presentation_layers.get(&role) {
                if cached.width == frame.width
                    && cached.height == frame.height
                    && cached.resources == role_resources
                    && cached.draws == owned_draws
                {
                    validate_cached_layer_sources(vfs, mount_set_id, &role_resources)?;
                    return Ok(PreparedMinoriLayer {
                        role,
                        rgba8_premultiplied: Arc::clone(&cached.rgba8_premultiplied),
                    });
                }
            }
            let rgba8_premultiplied = render_resource_layer(
                vfs,
                mount_set_id,
                frame.width,
                frame.height,
                &resources,
                role_draws,
            )?;
            let rgba8_premultiplied: Arc<[u8]> = Arc::from(rgba8_premultiplied);
            presentation_layers.insert(
                role,
                CachedMinoriLayer {
                    width: frame.width,
                    height: frame.height,
                    resources: role_resources,
                    draws: owned_draws,
                    rgba8_premultiplied: Arc::clone(&rgba8_premultiplied),
                },
            );
            Ok(PreparedMinoriLayer {
                role,
                rgba8_premultiplied,
            })
        })
        .collect()
}

fn validate_cached_layer_sources(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resources: &[LegacyTextureResourceV1],
) -> Result<(), LegacyProviderError> {
    for resource in resources {
        let stat = vfs.stat_file(mount_set_id, &resource.resource_uri)?;
        if stat.len == 0
            || stat.len > MAX_RESOURCE_BYTES
            || texture_binding_revision(&resource.resource_uri, stat.revision.0)
                != resource.revision
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_LAYER_TEXTURE_REVISION",
                "cached Minori layer source changed after the resource scene was staged",
            ));
        }
    }
    Ok(())
}

fn render_resource_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    width: u32,
    height: u32,
    resources: &BTreeMap<u32, &LegacyTextureResourceV1>,
    draws: Vec<&LegacyDrawV1>,
) -> Result<Vec<u8>, LegacyProviderError> {
    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width,
            height,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "astra.emu.minori.layer.v1".into(),
        })
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_RENDERER",
                "Astra Renderer2D rejected the Minori layer target",
            )
        })?;
    let mut commands = vec![SceneCommand::Clear { rgba: [0, 0, 0, 0] }];
    let mut uploaded = BTreeSet::new();
    for (draw_index, draw) in draws.into_iter().enumerate() {
        let resource = resources.get(&draw.texture_id).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_RESOURCE",
                "layer draw references an unknown texture resource",
            )
        })?;
        let texture_symbol = format!("minori.texture.{}", draw.texture_id);
        if uploaded.insert(draw.texture_id) {
            commands.push(SceneCommand::UploadTexture {
                resource_id: texture_symbol.clone(),
                frame: decode_texture(vfs, mount_set_id, resource)?,
            });
        }
        let vertices = draw
            .vertices
            .iter()
            .map(convert_vertex)
            .collect::<Result<Vec<_>, _>>()?;
        let scissor = draw.scissor.map(convert_scissor).transpose()?;
        if let Some(rect) = scissor {
            commands.push(SceneCommand::PushClip { rect });
        }
        commands.push(SceneCommand::Mesh2D {
            id: format!("minori.draw.{draw_index}"),
            vertices: vertices.into(),
            indices: Arc::from([0_u32, 2, 1, 1, 2, 3]),
            material: MeshMaterial2D::ColorTexture,
            texture_id: Some(texture_symbol),
            texture_filter: match draw.texture_filter {
                LegacyTextureFilter::Nearest => TextureFilter2D::Nearest,
                LegacyTextureFilter::Linear => TextureFilter2D::Linear,
            },
            opacity: 1.0,
            blend: match draw.blend {
                LegacyBlendMode::Alpha => BlendMode::Alpha,
                LegacyBlendMode::Add => BlendMode::Add,
                LegacyBlendMode::Opaque => BlendMode::Opaque,
                LegacyBlendMode::Multiply => BlendMode::Multiply,
                LegacyBlendMode::Screen => BlendMode::Screen,
            },
        });
        if scissor.is_some() {
            commands.push(SceneCommand::PopClip);
        }
    }
    let mut rgba8 = renderer
        .capture_frame(&commands)
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_RENDER",
                "Astra Renderer2D failed the bounded Minori layer render",
            )
        })?
        .bytes;
    premultiply_rgba8(&mut rgba8);
    Ok(rgba8)
}

fn decode_texture(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource: &LegacyTextureResourceV1,
) -> Result<TextureFrame, LegacyProviderError> {
    if resource.decoded_format != LegacyTextureFormat::Rgba8 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_TEXTURE_FORMAT",
            "Minori layer texture is not RGBA8",
        ));
    }
    let decoded_bytes = u64::from(resource.decoded_width)
        .checked_mul(u64::from(resource.decoded_height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_TEXTURE_BOUNDS",
                "decoded texture byte size overflowed",
            )
        })?;
    if decoded_bytes > MAX_DECODED_TEXTURE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_TEXTURE_BOUNDS",
            "decoded texture exceeds the byte budget",
        ));
    }
    let stat = vfs.stat_file(mount_set_id, &resource.resource_uri)?;
    if texture_binding_revision(&resource.resource_uri, stat.revision.0) != resource.revision {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_TEXTURE_REVISION",
            "texture source changed after the resource scene was staged",
        ));
    }
    if stat.len == 0 || stat.len > MAX_RESOURCE_BYTES {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_TEXTURE_BOUNDS",
            "encoded texture is empty or exceeds the byte budget",
        ));
    }
    let encoded = vfs
        .read_file_range(
            mount_set_id,
            &resource.resource_uri,
            stat.revision,
            ByteRange {
                offset: 0,
                len: stat.len,
            },
            MAX_RESOURCE_BYTES,
        )?
        .bytes;
    let provider_id = match resource.codec.as_str() {
        "ani" | "sqz" => MINORI_IMAGE_DECODE_PROVIDER_ID,
        "png" | "bmp" | "jpg" | "jpeg" | "webp" => "astra.decode.image",
        _ => {
            return Err(invalid(
                "ASTRA_EMU_MINORI_LAYER_TEXTURE_CODEC",
                "staged texture codec has no image provider binding",
            ));
        }
    };
    if provider_id == "astra.decode.image" {
        let reader = image::ImageReader::new(Cursor::new(encoded.as_slice()))
            .with_guessed_format()
            .map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_LAYER_TEXTURE_CODEC",
                    "image provider could not identify the Minori layer texture",
                )
            })?;
        let expected_format = match resource.codec.as_str() {
            "png" => image::ImageFormat::Png,
            "bmp" => image::ImageFormat::Bmp,
            "jpg" | "jpeg" => image::ImageFormat::Jpeg,
            "webp" => image::ImageFormat::WebP,
            _ => unreachable!("standard image codec was matched above"),
        };
        if reader.format() != Some(expected_format) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_LAYER_TEXTURE_CODEC",
                "texture content does not match the staged codec identity",
            ));
        }
    }
    let mut decoders = DecodeProviderRegistry::default();
    decoders
        .register(Box::new(ImageDecodeProvider))
        .map_err(minori_media_decode_error)?;
    decoders
        .register(Box::new(MinoriImageDecodeProvider))
        .map_err(minori_media_decode_error)?;
    let result = decoders
        .decode(
            &DecodeRequest {
                kind: DecodeKind::Image,
                codec: resource.codec.clone(),
                bytes: encoded,
                profile: "astra.emu.minori.layer.v1".into(),
            },
            &DecodeBindingContext::shipping(
                provider_id,
                "astra-emu-minori",
                "astra.emu.minori.layer.v1",
            ),
        )
        .map_err(minori_media_decode_error)?;
    let DecodeOutput::CpuBuffer { bytes, format } = result.output else {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_TEXTURE_DECODE",
            "bound Minori image provider did not return a CPU texture buffer",
        ));
    };
    if provider_id == MINORI_IMAGE_DECODE_PROVIDER_ID {
        let dimensions = format
            .strip_prefix("rgba8:first_frame:")
            .and_then(|value| value.split_once('x'))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_LAYER_TEXTURE_CODEC",
                    "Minori image provider returned an invalid first-frame format",
                )
            })?;
        let width = dimensions.0.parse::<u32>().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_TEXTURE_CODEC",
                "Minori image provider returned an invalid first-frame width",
            )
        })?;
        let height = dimensions.1.parse::<u32>().map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_LAYER_TEXTURE_CODEC",
                "Minori image provider returned an invalid first-frame height",
            )
        })?;
        if (width, height) != (resource.decoded_width, resource.decoded_height) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_LAYER_TEXTURE_IDENTITY",
                "decoded first-frame dimensions differ from the staged resource identity",
            ));
        }
    } else if format != "rgba8" {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_TEXTURE_DECODE",
            "bound image provider returned an unsupported pixel format",
        ));
    }
    let mut rgba8 = bytes.as_slice().to_vec();
    premultiply_rgba8(&mut rgba8);
    TextureFrame::from_vec(resource.decoded_width, resource.decoded_height, rgba8).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_LAYER_TEXTURE_BOUNDS",
            "decoded texture violates the Renderer2D texture contract",
        )
    })
}

fn minori_media_decode_error(error: astra_media::MediaError) -> LegacyProviderError {
    LegacyProviderError::invalid("ASTRA_EMU_MINORI_LAYER_TEXTURE_DECODE", error.to_string())
}

fn convert_vertex(vertex: &LegacyVertexV1) -> Result<MeshVertex2D, LegacyProviderError> {
    if vertex
        .position
        .iter()
        .chain(vertex.tex_coord.iter())
        .any(|v| !v.is_finite())
        || vertex
            .color
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_VERTEX",
            "Minori layer vertex is outside the Renderer2D contract",
        ));
    }
    let alpha = (vertex.color[3] * 255.0).round() as u8;
    let channel = |value: f32| ((value * vertex.color[3]) * 255.0).round() as u8;
    Ok(MeshVertex2D {
        position: vertex.position,
        uv: vertex.tex_coord,
        premultiplied_rgba: [
            channel(vertex.color[0]),
            channel(vertex.color[1]),
            channel(vertex.color[2]),
            alpha,
        ],
    })
}

fn convert_scissor(scissor: LegacyScissorV1) -> Result<RectI, LegacyProviderError> {
    let width = u32::try_from(scissor.width).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_LAYER_SCISSOR",
            "Minori layer scissor width is invalid",
        )
    })?;
    let height = u32::try_from(scissor.height).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_LAYER_SCISSOR",
            "Minori layer scissor height is invalid",
        )
    })?;
    if width == 0 || height == 0 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_LAYER_SCISSOR",
            "Minori layer scissor is empty",
        ));
    }
    Ok(RectI::new(scissor.x, scissor.y, width, height))
}

fn premultiply_rgba8(bytes: &mut [u8]) {
    for pixel in bytes.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
}

fn write_surface_rows(
    lease: &mut astra_emu_family_api::LegacySurfaceLeaseV9,
    rgba8: &[u8],
) -> Result<(), LegacyProviderError> {
    let row_bytes = usize::try_from(lease.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| invalid("ASTRA_EMU_MINORI_SURFACE_BOUNDS", "surface row overflowed"))?;
    let expected = row_bytes
        .checked_mul(usize::try_from(lease.height).map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_SURFACE_BOUNDS",
                "surface height cannot be represented",
            )
        })?)
        .ok_or_else(|| invalid("ASTRA_EMU_MINORI_SURFACE_BOUNDS", "surface size overflowed"))?;
    if rgba8.len() != expected {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SURFACE_BOUNDS",
            "rendered layer size differs from the acquired surface",
        ));
    }
    let stride = usize::try_from(lease.stride).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_SURFACE_BOUNDS",
            "surface stride cannot be represented",
        )
    })?;
    for (source, destination) in rgba8
        .chunks_exact(row_bytes)
        .zip(lease.pixels.as_mut_slice().chunks_exact_mut(stride))
    {
        destination.fill(0);
        destination[..row_bytes].copy_from_slice(source);
    }
    Ok(())
}

fn validate_session_binding(
    ctx: &LegacyRuntimeHostCtx,
    session: &MinoriSession,
) -> Result<(), LegacyProviderError> {
    if ctx.mount_set_id != session.mount_set_id {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOUNT_BINDING",
            "host mount does not match the open session",
        ));
    }
    Ok(())
}

fn script_error(_error: crate::ScParseError) -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_MINORI_SCRIPT_PARSE",
        "Minori script failed strict parsing",
    )
}

fn runtime_error(error: MinoriRuntimeError) -> LegacyProviderError {
    LegacyProviderError::invalid(runtime_error_code(&error), error.to_string())
}

fn minori_vm_event_name(event: Option<&MinoriVmEvent>) -> &'static str {
    match event {
        Some(MinoriVmEvent::Wait(_)) => "wait",
        Some(MinoriVmEvent::Message { .. }) => "message",
        Some(MinoriVmEvent::Audio { .. }) => "audio",
        Some(MinoriVmEvent::Stage(_)) => "stage",
        Some(MinoriVmEvent::Character(_)) => "character",
        Some(MinoriVmEvent::AxisScroll(_)) => "axis_scroll",
        Some(MinoriVmEvent::LinearScroll(_)) => "linear_scroll",
        Some(MinoriVmEvent::ScrollXf(_)) => "scroll_xf",
        Some(MinoriVmEvent::WScroll2(_)) => "wscroll2",
        Some(MinoriVmEvent::Effect(_)) => "effect",
        Some(MinoriVmEvent::EffectCleared { .. }) => "effect_cleared",
        Some(MinoriVmEvent::Firefly(_)) => "firefly",
        Some(MinoriVmEvent::FireflyCleared { .. }) => "firefly_cleared",
        Some(MinoriVmEvent::SecondaryEffect(_)) => "secondary_effect",
        Some(MinoriVmEvent::SecondaryEffectCleared { .. }) => "secondary_effect_cleared",
        Some(MinoriVmEvent::ScreenShake(_)) => "screen_shake",
        Some(MinoriVmEvent::Panel { .. }) => "panel",
        Some(MinoriVmEvent::Choice { .. }) => "choice",
        Some(MinoriVmEvent::Movie(_)) => "movie",
        Some(MinoriVmEvent::Chain { .. }) => "chain",
        Some(MinoriVmEvent::Terminal) => "terminal",
        None => "none",
    }
}

fn runtime_error_code(error: &MinoriRuntimeError) -> &'static str {
    match error {
        MinoriRuntimeError::State => "ASTRA_EMU_MINORI_RUNTIME_STATE",
        MinoriRuntimeError::ProgramCounter => "ASTRA_EMU_MINORI_RUNTIME_PC",
        MinoriRuntimeError::Label => "ASTRA_EMU_MINORI_RUNTIME_LABEL",
        MinoriRuntimeError::Operand => "ASTRA_EMU_MINORI_RUNTIME_OPERAND",
        MinoriRuntimeError::UnsupportedOpcode { .. } => "ASTRA_EMU_MINORI_RUNTIME_OPCODE",
        MinoriRuntimeError::UnsupportedPragma { .. } => "ASTRA_EMU_MINORI_RUNTIME_PRAGMA",
        MinoriRuntimeError::Budget => "ASTRA_EMU_MINORI_RUNTIME_BUDGET",
        MinoriRuntimeError::Waiting => "ASTRA_EMU_MINORI_RUNTIME_WAIT",
        MinoriRuntimeError::Overflow => "ASTRA_EMU_MINORI_RUNTIME_OVERFLOW",
        MinoriRuntimeError::Snapshot => "ASTRA_EMU_MINORI_RUNTIME_SNAPSHOT",
        MinoriRuntimeError::ChainTarget => "ASTRA_EMU_MINORI_RUNTIME_CHAIN",
        MinoriRuntimeError::AudioResource => "ASTRA_EMU_MINORI_RUNTIME_AUDIO_RESOURCE",
        MinoriRuntimeError::Effect { .. } => "ASTRA_EMU_MINORI_RUNTIME_EFFECT",
        MinoriRuntimeError::UnsupportedEffectKind { .. } => "ASTRA_EMU_MINORI_RUNTIME_EFFECT_KIND",
        MinoriRuntimeError::Panel { .. } => "ASTRA_EMU_MINORI_RUNTIME_PANEL",
        MinoriRuntimeError::Choice => "ASTRA_EMU_MINORI_RUNTIME_CHOICE",
        MinoriRuntimeError::Firefly => "ASTRA_EMU_MINORI_RUNTIME_FIREFLY",
        MinoriRuntimeError::SecondaryEffect => "ASTRA_EMU_MINORI_RUNTIME_SECONDARY_EFFECT",
        MinoriRuntimeError::ScreenShake => "ASTRA_EMU_MINORI_RUNTIME_SCREEN_SHAKE",
        MinoriRuntimeError::ScrollXf => "ASTRA_EMU_MINORI_RUNTIME_SCROLL_XF",
        MinoriRuntimeError::WScroll2 => "ASTRA_EMU_MINORI_RUNTIME_WSCROLL2",
        MinoriRuntimeError::Character => "ASTRA_EMU_MINORI_RUNTIME_CHARACTER",
        MinoriRuntimeError::AxisScroll => "ASTRA_EMU_MINORI_RUNTIME_AXIS_SCROLL",
        MinoriRuntimeError::LinearScroll => "ASTRA_EMU_MINORI_RUNTIME_LINEAR_SCROLL",
        MinoriRuntimeError::Backlog => "ASTRA_EMU_MINORI_RUNTIME_BACKLOG",
        MinoriRuntimeError::MessageVoiceDuration => "ASTRA_EMU_MINORI_MESSAGE_VOICE_DURATION",
        MinoriRuntimeError::MessageControl(error) => match error {
            MinoriMessageMarkupError::Truncated => "ASTRA_EMU_MINORI_MESSAGE_CONTROL_TRUNCATED",
            MinoriMessageMarkupError::Unsupported => "ASTRA_EMU_MINORI_MESSAGE_CONTROL_UNSUPPORTED",
            MinoriMessageMarkupError::NonCanonical => {
                "ASTRA_EMU_MINORI_MESSAGE_CONTROL_NONCANONICAL"
            }
            MinoriMessageMarkupError::Bounds => "ASTRA_EMU_MINORI_MESSAGE_CONTROL_BOUNDS",
            MinoriMessageMarkupError::LoadSchema => "ASTRA_EMU_MINORI_MESSAGE_LOAD_SCHEMA",
        },
    }
}

fn session_missing() -> LegacyProviderError {
    invalid("ASTRA_EMU_MINORI_SESSION_MISSING", "session is not active")
}

fn invalid(code: &'static str, message: &'static str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use astra_byte_source::{ByteRange, ByteSourceStat, RangeReadResult, SourceRevision};
    use astra_emu_family_api::{
        LegacyAwaitResult, LegacyInputEdge, LegacyReplayMode, LegacySystemMenuActionV1,
        LegacySystemMenuRequestV1, LegacyVfsListedFile, LegacyWritableFileHostV1,
    };
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

    use super::*;
    use crate::MinoriPlayMode;

    #[test]
    fn v9_layer_sequence_advances_across_runtime_ticks() {
        let staged = LegacyStepOutput {
            status: LegacyRuntimeStatus::Active,
            live: LegacyLiveOutput::default(),
            control: LegacyControlTransaction::default(),
            trace: Vec::new(),
            diagnostics: Vec::new(),
            coverage: LegacyCoverageDelta::default(),
            state_revision: 1,
        };

        let first = next_layer_sequence(&staged, 0).unwrap();
        let second = next_layer_sequence(&staged, first).unwrap();
        assert_eq!((first, second), (1, 2));
    }

    #[test]
    fn title_pointer_hit_regions_follow_the_original_stage_menu() {
        assert_eq!(title_menu_focus_at(0, 1_174, 47), Some(0));
        assert_eq!(title_menu_focus_at(0, 1_174, 95), Some(1));
        assert_eq!(title_menu_focus_at(0, 1_174, 143), Some(2));
        assert_eq!(title_menu_focus_at(0, 1_174, 239), Some(3));
        assert_eq!(title_menu_focus_at(2, 1_174, 191), Some(3));
        assert_eq!(title_menu_focus_at(2, 1_174, 239), Some(4));
        assert_eq!(title_menu_focus_at(0, 1_023, 47), None);
        assert_eq!(title_menu_focus_at(0, 1_174, 20), None);
        assert_eq!(title_menu_focus_at(0, 1_174, 264), None);
    }

    #[test]
    fn provider_title_pointer_hover_and_click_follow_original_menu() {
        let encode_stage = |rgba: u8| {
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(
                    &vec![rgba; 1280 * 720 * 4],
                    1280,
                    720,
                    ExtendedColorType::Rgba8,
                )
                .unwrap();
            png
        };
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                (
                    "minori:/scr/test.sc".into(),
                    b".wait 20\r\n.end\r\n".to_vec(),
                ),
                ("minori:/sys/topMenu0.png".into(), encode_stage(0)),
                ("minori:/sys/topMenu0Over.png".into(), encode_stage(255)),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.title-pointer".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        let title = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(title.live.resource_scenes.len(), 1);
        assert_eq!(
            title.live.resource_scenes[0].value.texture_resources.len(),
            1
        );

        let hover = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: MINORI_POINTER_X.into(),
                            pressed: false,
                            value: 1_174.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_Y.into(),
                            pressed: false,
                            value: 47.0,
                            sequence: 2,
                        },
                    ],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        let hover_frame = &hover.live.resource_scenes[0].value;
        assert_eq!(hover_frame.texture_resources.len(), 2);
        assert_eq!(
            hover_frame.texture_resources[1].resource_uri,
            "minori:/sys/topMenu0Over.png"
        );
        assert_eq!(hover_frame.draws.len(), 2);
        assert_eq!(
            hover_frame.draws[1].scissor,
            Some(LegacyScissorV1 {
                x: 1024,
                y: 24,
                width: 256,
                height: 48,
            })
        );

        let click = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: MINORI_POINTER_PRIMARY.into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(click.status, LegacyRuntimeStatus::Awaiting);
        assert_eq!(
            provider
                .sessions
                .get(&session.0)
                .expect("title-pointer session remains active")
                .vm
                .state()
                .system_ui
                .page,
            MinoriSystemPage::None
        );
    }

    struct MemoryReader {
        scripts: BTreeMap<String, Vec<u8>>,
    }

    impl LegacyVfsReader for MemoryReader {
        fn stat_file(
            &self,
            mount_set_id: &str,
            uri: &str,
        ) -> Result<ByteSourceStat, LegacyProviderError> {
            if mount_set_id != "mount.test" {
                return Err(invalid("TEST_VFS_NOT_FOUND", "fixture entry is missing"));
            }
            let script = self
                .scripts
                .get(uri)
                .ok_or_else(|| LegacyProviderError::invalid("TEST_VFS_NOT_FOUND", uri))?;
            let digest = Hash256::from_sha256(script);
            let revision = u64::from_le_bytes(digest.as_bytes()[..8].try_into().unwrap());
            Ok(ByteSourceStat {
                len: script.len() as u64,
                revision: SourceRevision(revision),
            })
        }

        fn read_file_range(
            &self,
            mount_set_id: &str,
            uri: &str,
            expected_revision: SourceRevision,
            range: ByteRange,
            max_bytes: u64,
        ) -> Result<RangeReadResult, LegacyProviderError> {
            let stat = self.stat_file(mount_set_id, uri)?;
            range
                .validate(stat.len, max_bytes)
                .map_err(|_| invalid("TEST_VFS_BOUNDS", "fixture range is invalid"))?;
            if expected_revision != stat.revision {
                return Err(invalid("TEST_VFS_REVISION", "fixture revision changed"));
            }
            let script = self
                .scripts
                .get(uri)
                .ok_or_else(|| LegacyProviderError::invalid("TEST_VFS_NOT_FOUND", uri))?;
            let bytes = script[range.offset as usize..(range.offset + range.len) as usize].to_vec();
            Ok(RangeReadResult {
                range,
                revision: stat.revision,
                bytes: bytes.into(),
            })
        }

        fn enumerate_by_extension(
            &self,
            mount_set_id: &str,
            root: &str,
            extension_without_dot: &str,
            max_entries: u32,
        ) -> Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
            if mount_set_id != "mount.test" || max_entries == 0 {
                return Err(invalid("TEST_VFS_ENUM", "fixture enumeration is invalid"));
            }
            let root = root.trim_end_matches('/');
            let suffix = format!(".{}", extension_without_dot.to_ascii_lowercase());
            let mut entries = self
                .scripts
                .keys()
                .filter(|uri| {
                    uri.starts_with(&format!("{root}/"))
                        && uri.to_ascii_lowercase().ends_with(&suffix)
                })
                .map(|uri| {
                    let stat = self.stat_file(mount_set_id, uri)?;
                    Ok(LegacyVfsListedFile {
                        uri: uri.clone(),
                        stat,
                    })
                })
                .collect::<Result<Vec<_>, LegacyProviderError>>()?;
            entries.sort_by(|left, right| left.uri.cmp(&right.uri));
            if entries.len() > max_entries as usize {
                return Err(invalid(
                    "TEST_VFS_ENUM",
                    "fixture enumeration exceeds bound",
                ));
            }
            Ok(entries)
        }
    }

    #[test]
    fn resource_descriptor_reads_minori_ani_dimensions_without_generic_image_fallback() {
        let mut ani = Vec::from([0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]);
        ani.extend_from_slice(b"frame\0");
        ani.extend_from_slice(&2_u16.to_le_bytes());
        ani.extend_from_slice(&1_u16.to_le_bytes());
        ani.extend_from_slice(&24_u16.to_le_bytes());
        ani.extend_from_slice(&0_i16.to_le_bytes());
        ani.extend_from_slice(&0_i16.to_le_bytes());
        ani.extend_from_slice(&[0, 0, 255, 255, 255, 255]);
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/st/frame.ani".into(), ani)]),
        });

        let resource = read_texture_resource(&vfs, "mount.test", "minori:/st/frame.ani", 7)
            .expect("ANI metadata should be accepted by the family resource resolver");
        assert_eq!(resource.codec, "ani");
        assert_eq!((resource.decoded_width, resource.decoded_height), (2, 1));
    }

    #[test]
    fn layer_texture_decode_uses_the_family_provider_for_ani() {
        let mut ani = Vec::from([0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]);
        ani.extend_from_slice(b"frame\0");
        ani.extend_from_slice(&2_u16.to_le_bytes());
        ani.extend_from_slice(&1_u16.to_le_bytes());
        ani.extend_from_slice(&24_u16.to_le_bytes());
        ani.extend_from_slice(&0_i16.to_le_bytes());
        ani.extend_from_slice(&0_i16.to_le_bytes());
        ani.extend_from_slice(&[0, 0, 255, 255, 255, 255]);
        let revision = u64::from_le_bytes(
            Hash256::from_sha256(&ani).as_bytes()[..8]
                .try_into()
                .expect("hash prefix has a fixed width"),
        );
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/st/frame.ani".into(), ani)]),
        });
        let resource = LegacyTextureResourceV1 {
            texture_id: 1,
            resource_uri: "minori:/st/frame.ani".into(),
            codec: "ani".into(),
            revision: texture_binding_revision("minori:/st/frame.ani", revision),
            decoded_width: 2,
            decoded_height: 1,
            decoded_format: LegacyTextureFormat::Rgba8,
        };

        let frame = decode_texture(&vfs, "mount.test", &resource).unwrap();
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(frame.rgba8.len(), 8);
    }

    #[derive(Default)]
    struct RecordingSurfaceHost {
        commits: std::sync::Mutex<Vec<RecordedSurfaceCommit>>,
        hook_count: Option<Arc<std::sync::atomic::AtomicUsize>>,
        generations: std::sync::Mutex<BTreeMap<String, u64>>,
    }

    struct RecordedSurfaceCommit {
        surface_id: String,
        generation: u64,
        stride: u32,
        pixels: Vec<u8>,
    }

    impl astra_emu_family_api::LegacySurfaceHostV9 for RecordingSurfaceHost {
        fn acquire(
            &self,
            session_id: &str,
            fixed_step: u64,
            surface_id: &str,
            width: u32,
            height: u32,
            format: LegacySurfaceFormatV9,
        ) -> Result<astra_emu_family_api::LegacySurfaceLeaseV9, LegacyProviderError> {
            if let Some(hook_count) = &self.hook_count {
                assert!(hook_count.load(std::sync::atomic::Ordering::Acquire) > 0);
            }
            assert_eq!(session_id, "session.surface");
            assert!(matches!(fixed_step, 1..=3));
            assert_eq!(format, LegacySurfaceFormatV9::Rgba8SrgbPremultiplied);
            let stride = width.checked_mul(4).unwrap().checked_add(8).unwrap();
            let len = usize::try_from(u64::from(stride) * u64::from(height)).unwrap();
            let generation = {
                let mut generations = self.generations.lock().unwrap();
                let generation = generations.entry(surface_id.into()).or_default();
                *generation += 1;
                *generation
            };
            Ok(astra_emu_family_api::LegacySurfaceLeaseV9 {
                lease_id: format!("lease.{surface_id}"),
                surface_id: surface_id.into(),
                generation,
                width,
                height,
                stride,
                format,
                pixels: astra_byte_source::OwnedWritableByteBuffer::from_vec(vec![0xcc; len]),
            })
        }

        fn commit(
            &self,
            session_id: &str,
            fixed_step: u64,
            commit: LegacySurfaceCommitV9,
        ) -> Result<(), LegacyProviderError> {
            assert_eq!(session_id, "session.surface");
            assert!(matches!(fixed_step, 1..=3));
            commit.validate()?;
            assert_eq!(commit.damage, LegacySurfaceDamageV9::Full);
            self.commits.lock().unwrap().push(RecordedSurfaceCommit {
                surface_id: commit.lease.surface_id,
                generation: commit.lease.generation,
                stride: commit.lease.stride,
                pixels: commit.lease.pixels.as_slice().to_vec(),
            });
            Ok(())
        }
    }

    struct UnboundHookHost;

    impl astra_emu_family_api::LegacyHookHostV1 for UnboundHookHost {
        fn invoke(
            &self,
            _invocation: astra_emu_family_api::LegacyHookInvocationV1,
        ) -> Result<astra_emu_family_api::LegacyHookResultV1, LegacyProviderError> {
            Ok(astra_emu_family_api::LegacyHookResultV1 {
                status: astra_emu_family_api::LegacyHookStatusV1::Unbound,
                payload: Vec::new().into(),
                diagnostics: Vec::new(),
            })
        }
    }

    #[derive(Default)]
    struct RecordingSystemMenuHost {
        published: std::sync::Mutex<Vec<(String, LegacySystemMenuTransactionV1)>>,
    }

    impl astra_emu_family_api::LegacySystemMenuHostV1 for RecordingSystemMenuHost {
        fn publish(
            &self,
            session_id: &str,
            menu: LegacySystemMenuTransactionV1,
        ) -> Result<(), LegacyProviderError> {
            menu.validate()?;
            self.published
                .lock()
                .unwrap()
                .push((session_id.into(), menu));
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingSystemCommandHost {
        published: std::sync::Mutex<Vec<(String, LegacySystemCommandTransactionV1)>>,
    }

    impl astra_emu_family_api::LegacySystemCommandHostV1 for RecordingSystemCommandHost {
        fn publish(
            &self,
            session_id: &str,
            command: LegacySystemCommandTransactionV1,
        ) -> Result<(), LegacyProviderError> {
            command.validate()?;
            self.published
                .lock()
                .unwrap()
                .push((session_id.into(), command));
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingConfirmationHost {
        published: std::sync::Mutex<Vec<(String, LegacyConfirmationTransactionV1)>>,
    }

    impl astra_emu_family_api::LegacyConfirmationHostV1 for RecordingConfirmationHost {
        fn publish(
            &self,
            session_id: &str,
            confirmation: LegacyConfirmationTransactionV1,
        ) -> Result<(), LegacyProviderError> {
            confirmation.validate()?;
            self.published
                .lock()
                .unwrap()
                .push((session_id.into(), confirmation));
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingTextInputHost {
        published: std::sync::Mutex<Vec<(String, LegacyTextInputTransactionV1)>>,
    }

    impl astra_emu_family_api::LegacyTextInputHostV1 for RecordingTextInputHost {
        fn publish(
            &self,
            session_id: &str,
            text_input: LegacyTextInputTransactionV1,
        ) -> Result<(), LegacyProviderError> {
            text_input.validate()?;
            self.published
                .lock()
                .unwrap()
                .push((session_id.into(), text_input));
            Ok(())
        }
    }

    struct RejectWritableFiles;

    impl astra_emu_family_api::LegacyWritableFileHostV1 for RejectWritableFiles {
        fn execute(
            &self,
            _session_id: &str,
            _request: astra_emu_family_api::LegacyWritableFileRequestV1,
        ) -> Result<astra_emu_family_api::LegacyWritableFileResultV1, LegacyProviderError> {
            Err(invalid(
                "TEST_WRITABLE_UNEXPECTED",
                "surface test must not access writable files",
            ))
        }
    }

    #[derive(Default)]
    struct InMemoryWritableFiles {
        files: std::sync::Mutex<BTreeMap<String, Vec<u8>>>,
        directories: std::sync::Mutex<BTreeSet<String>>,
    }

    impl astra_emu_family_api::LegacyWritableFileHostV1 for InMemoryWritableFiles {
        fn execute(
            &self,
            _session_id: &str,
            request: astra_emu_family_api::LegacyWritableFileRequestV1,
        ) -> Result<astra_emu_family_api::LegacyWritableFileResultV1, LegacyProviderError> {
            use astra_emu_family_api::{
                LegacyWritableFileEntryV1, LegacyWritableFileRequestV1, LegacyWritableFileResultV1,
            };
            let result = |exists: bool,
                          is_file: bool,
                          length: u64,
                          entries: Vec<LegacyWritableFileEntryV1>,
                          bytes: Vec<u8>,
                          written: u64| {
                Ok(LegacyWritableFileResultV1 {
                    exists,
                    is_file,
                    length,
                    entries,
                    bytes: bytes.into(),
                    written,
                })
            };
            match request {
                LegacyWritableFileRequestV1::Stat { path } => {
                    if let Some(bytes) = self.files.lock().unwrap().get(&path) {
                        result(true, true, bytes.len() as u64, Vec::new(), Vec::new(), 0)
                    } else if self.directories.lock().unwrap().contains(&path) {
                        result(true, false, 0, Vec::new(), Vec::new(), 0)
                    } else {
                        result(false, false, 0, Vec::new(), Vec::new(), 0)
                    }
                }
                LegacyWritableFileRequestV1::List { path } => {
                    let prefix = format!("{path}/");
                    let mut entries = Vec::new();
                    for (file_path, bytes) in self.files.lock().unwrap().iter() {
                        let Some(name) = file_path.strip_prefix(&prefix) else {
                            continue;
                        };
                        if name.contains('/') {
                            continue;
                        }
                        entries.push(LegacyWritableFileEntryV1 {
                            name: name.into(),
                            is_file: true,
                            length: bytes.len() as u64,
                        });
                    }
                    entries.sort_by(|left, right| left.name.cmp(&right.name));
                    result(true, false, 0, entries, Vec::new(), 0)
                }
                LegacyWritableFileRequestV1::CreateDir { path } => {
                    self.directories.lock().unwrap().insert(path);
                    result(true, false, 0, Vec::new(), Vec::new(), 0)
                }
                LegacyWritableFileRequestV1::ReadRange {
                    path,
                    offset,
                    length,
                } => {
                    let files = self.files.lock().unwrap();
                    let bytes = files.get(&path).ok_or_else(|| {
                        invalid("TEST_WRITABLE_MISSING", "requested file does not exist")
                    })?;
                    let start = usize::try_from(offset).map_err(|_| {
                        invalid("TEST_WRITABLE_RANGE", "read offset does not fit usize")
                    })?;
                    let end = start
                        .checked_add(usize::try_from(length).map_err(|_| {
                            invalid("TEST_WRITABLE_RANGE", "read length does not fit usize")
                        })?)
                        .ok_or_else(|| invalid("TEST_WRITABLE_RANGE", "read range overflowed"))?;
                    let payload = bytes.get(start..end).ok_or_else(|| {
                        invalid("TEST_WRITABLE_RANGE", "read range is outside the file")
                    })?;
                    result(
                        true,
                        true,
                        bytes.len() as u64,
                        Vec::new(),
                        payload.to_vec(),
                        0,
                    )
                }
                LegacyWritableFileRequestV1::WriteRange {
                    path,
                    offset,
                    bytes,
                } => {
                    let mut files = self.files.lock().unwrap();
                    let target = files.entry(path).or_default();
                    let start = usize::try_from(offset).map_err(|_| {
                        invalid("TEST_WRITABLE_RANGE", "write offset does not fit usize")
                    })?;
                    if start > target.len() {
                        return Err(invalid(
                            "TEST_WRITABLE_RANGE",
                            "test writable file does not create sparse gaps",
                        ));
                    }
                    let end = start
                        .checked_add(bytes.len())
                        .ok_or_else(|| invalid("TEST_WRITABLE_RANGE", "write range overflowed"))?;
                    if end > target.len() {
                        target.resize(end, 0);
                    }
                    target[start..end].copy_from_slice(&bytes);
                    result(
                        true,
                        true,
                        target.len() as u64,
                        Vec::new(),
                        Vec::new(),
                        bytes.len() as u64,
                    )
                }
                LegacyWritableFileRequestV1::SetLength { path, length } => {
                    let mut files = self.files.lock().unwrap();
                    let target = files.entry(path).or_default();
                    let length = usize::try_from(length).map_err(|_| {
                        invalid("TEST_WRITABLE_LENGTH", "file length does not fit usize")
                    })?;
                    target.resize(length, 0);
                    result(true, true, length as u64, Vec::new(), Vec::new(), 0)
                }
                LegacyWritableFileRequestV1::Remove { path } => {
                    self.files.lock().unwrap().remove(&path);
                    result(false, false, 0, Vec::new(), Vec::new(), 0)
                }
                LegacyWritableFileRequestV1::AtomicReplace {
                    temporary_path,
                    destination_path,
                } => {
                    let mut files = self.files.lock().unwrap();
                    let bytes = files.remove(&temporary_path).ok_or_else(|| {
                        invalid("TEST_WRITABLE_RENAME", "temporary save file is missing")
                    })?;
                    let length = bytes.len() as u64;
                    files.insert(destination_path, bytes);
                    result(true, true, length, Vec::new(), Vec::new(), 0)
                }
            }
        }
    }

    #[test]
    fn gallery_pages_resolve_only_verified_system_assets() {
        assert_eq!(
            gallery_resource_uri(MinoriSystemPage::GalleryCg, 0).unwrap(),
            "minori:/sys/cgpage001.png"
        );
        assert_eq!(
            gallery_resource_uri(MinoriSystemPage::GalleryCg, 11).unwrap(),
            "minori:/sys/cgpage012.png"
        );
        assert_eq!(
            gallery_resource_uri(MinoriSystemPage::GalleryBgm, 32).unwrap(),
            "minori:/sys/musicPage3.png"
        );
        assert_eq!(
            gallery_resource_uri(MinoriSystemPage::GalleryReplay, 3).unwrap(),
            "minori:/sys/flash3.png"
        );
        assert!(gallery_resource_uri(MinoriSystemPage::GalleryCg, 12).is_err());
        assert!(gallery_resource_uri(MinoriSystemPage::GalleryBgm, 47).is_err());
        assert!(gallery_resource_uri(MinoriSystemPage::GalleryReplay, 4).is_err());
    }

    #[test]
    fn leaving_bgm_gallery_stops_the_shared_stream_before_returning_to_memories() {
        let script = parse_sc(b".end\r\n", &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(b"gallery-bgm"),
            script,
            7,
        )
        .unwrap();
        vm.begin_title_launch().unwrap();
        vm.set_system_page(MinoriSystemPage::GalleryBgm, 0).unwrap();
        vm.gallery_bgm_play("minori:/bgm/BGM001.ogg").unwrap();

        let action = apply_system_ui_input(
            &mut vm,
            &LegacyStepInput {
                input_edges: vec![LegacyInputEdge {
                    control: "escape".into(),
                    pressed: true,
                    value: 1.0,
                    sequence: 1,
                }],
                ..step_input(1, Vec::new())
            },
        )
        .unwrap();
        assert_eq!(action, MinoriSystemUiAction::GalleryBgmStop);
        assert_eq!(vm.state().system_ui.page, MinoriSystemPage::Memories);
        let stop = vm.gallery_bgm_stop().unwrap();
        assert!(matches!(
            stop.as_slice(),
            [MinoriAudioCommand::Stop {
                stream_id: crate::runtime::MINORI_BGM_STREAM_ID,
                ..
            }]
        ));
    }

    #[test]
    fn script_loader_expands_bounded_includes_and_rejects_cycles() {
        let reader: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                (
                    "minori:/scr/root.sc".into(),
                    b".include part.sc\r\n.end\r\n".to_vec(),
                ),
                (
                    "minori:/scr/part.sc".into(),
                    b".set included = 1\r\n".to_vec(),
                ),
            ]),
        });
        let (_, _, expanded) = load_script(&reader, "mount.test", "root.sc").unwrap();
        assert_eq!(expanded.lines.len(), 2);

        let cycle_reader: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/a.sc".into(), b".include b.sc\r\n".to_vec()),
                ("minori:/scr/b.sc".into(), b".include a.sc\r\n".to_vec()),
            ]),
        });
        let error = load_script(&cycle_reader, "mount.test", "a.sc").unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_CYCLE");
    }

    #[test]
    fn script_include_target_uses_the_bound_cp932_locale() {
        let reader: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/scr/root.sc".into(),
                b".include \x82.sc\r\n".to_vec(),
            )]),
        });
        let error = load_script(&reader, "mount.test", "root.sc").unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_MINORI_SCRIPT_INCLUDE_OPERAND");
    }

    #[test]
    fn initial_open_executes_the_same_bounded_include_expansion_as_chain() {
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                (
                    "minori:/scr/root.sc".into(),
                    b".include part.sc\r\n".to_vec(),
                ),
                (
                    "minori:/scr/part.sc".into(),
                    b".set included = 1\r\n.end\r\n".to_vec(),
                ),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.initial-include".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/root.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Terminal);
        assert_eq!(
            provider.sessions[&session.0]
                .vm
                .state()
                .variables
                .get("included"),
            Some(&1)
        );
    }

    #[test]
    fn full_script_resource_audit_is_bounded_and_fails_on_missing_assets() {
        let reader: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                (
                    "minori:/scr/test.sc".into(),
                    b".stage * bg.png 0 0\r\n.playbgm theme.ogg\r\n.message 1 voice.ogg speaker text\r\n.end\r\n".to_vec(),
                ),
                ("minori:/bg/bg.png".into(), vec![1]),
                ("minori:/bgm/theme.ogg".into(), vec![2]),
                ("minori:/voice/voice.ogg".into(), vec![3]),
            ]),
        });
        let (count, digest) = audit_script_resources(&reader, "mount.test").unwrap();
        assert_eq!(count, 3);
        assert_ne!(digest, Hash256::from_sha256(&[]));

        let missing: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/scr/test.sc".into(),
                b".stage * missing.png 0 0\r\n".to_vec(),
            )]),
        });
        let error = audit_script_resources(&missing, "mount.test").unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_MINORI_RESOURCE_AUDIT_MISSING");
    }

    #[test]
    fn save_slot_round_trips_through_v9_writable_file_port() {
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/scr/test.sc".into(),
                b".wait 20\r\n.end\r\n".to_vec(),
            )]),
        });
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::clone(&vfs));
        let ctx = context();
        let session_id = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.save".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session_id, step_input(1, Vec::new()))
            .unwrap();
        let writable = InMemoryWritableFiles::default();
        let session = provider.sessions.get_mut(&session_id.0).unwrap();
        session.stage_size = Some((1280, 720));
        session.last_gameplay_frame = Some(Arc::from(vec![0x20; 1280 * 720 * 4]));
        refresh_save_slots(&writable, &session_id, session).unwrap();
        assert!(session.save_slots.is_empty());
        session.vm.open_save_page().unwrap();
        save_slot(&writable, &session_id, session, 7, "memo").unwrap();
        assert!(writable.files.lock().unwrap().contains_key(&slot_path(7)));
        session.save_slots.clear();
        session.save_slot_comments.clear();
        session.save_slot_lengths.clear();
        refresh_save_slots(&writable, &session_id, session).unwrap();
        assert_eq!(session.save_slots, BTreeSet::from([7]));
        assert_eq!(session.save_slot_comments.get(&7), Some(&"memo".to_owned()));
        session.vm.close_gameplay_system_page().unwrap();
        session.vm.open_load_page().unwrap();
        load_slot(&writable, &vfs, &session_id, session, 7, 2).unwrap();
        assert_eq!(session.vm.state().system_ui.page, MinoriSystemPage::None);
        assert_eq!(session.vm.state().fixed_tick, 2);
        assert!(session.vm.state().wait.is_some());
        assert_eq!(session.save_slot_comments.get(&7), Some(&"memo".to_owned()));

        session.vm.open_save_page().unwrap();
        let mut envelope = {
            let files = writable.files.lock().unwrap();
            decode_save(files.get(&slot_path(7)).unwrap()).unwrap()
        };
        envelope.package_hash = Hash256::from_sha256(b"different-package");
        writable
            .files
            .lock()
            .unwrap()
            .insert(slot_path(7), encode_save(&envelope).unwrap());
        session.save_slots.clear();
        session.save_slot_comments.clear();
        session.save_slot_lengths.clear();
        assert_eq!(
            refresh_save_slots(&writable, &session_id, session)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_SAVE_LIST_IDENTITY"
        );
    }

    #[test]
    fn load_page_escape_returns_to_its_actual_owner() {
        let script =
            parse_sc(b".wait 20\r\n.end\r\n", &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut gameplay = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(b"gameplay-load"),
            script.clone(),
            7,
        )
        .unwrap();
        gameplay.step(1, 100).unwrap();
        assert!(gameplay.state().wait.is_some());
        gameplay.open_load_page().unwrap();

        let escape = LegacyStepInput {
            input_edges: vec![LegacyInputEdge {
                control: "escape".into(),
                pressed: true,
                value: 1.0,
                sequence: 1,
            }],
            ..step_input(2, Vec::new())
        };
        assert_eq!(
            apply_system_ui_input(&mut gameplay, &escape).unwrap(),
            MinoriSystemUiAction::CloseGameplaySystemPage
        );
        assert_eq!(gameplay.state().system_ui.page, MinoriSystemPage::Load);

        let mut title = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(b"title-load"),
            script,
            7,
        )
        .unwrap();
        title.begin_title_launch().unwrap();
        title.set_system_page(MinoriSystemPage::Load, 0).unwrap();
        assert_eq!(
            apply_system_ui_input(&mut title, &escape).unwrap(),
            MinoriSystemUiAction::Present
        );
        assert_eq!(title.state().system_ui.page, MinoriSystemPage::Title);
    }

    #[test]
    fn typed_system_menu_publishes_original_commands_before_save_selection() {
        let encode_rgba = |width: u32, height: u32| {
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(
                    &vec![0; usize::try_from(width * height * 4).unwrap()],
                    width,
                    height,
                    ExtendedColorType::Rgba8,
                )
                .unwrap();
            png
        };
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                (
                    "minori:/scr/test.sc".into(),
                    b".wait 20\r\n.end\r\n".to_vec(),
                ),
                (
                    "minori:/sys/saveloadBase.png".into(),
                    encode_rgba(1280, 720),
                ),
                ("minori:/sys/saveloadSave.png".into(), encode_rgba(352, 48)),
                (
                    "minori:/sys/saveloadSelect.png".into(),
                    encode_rgba(344, 98),
                ),
                (
                    "minori:/sys/saveloadButtons.png".into(),
                    encode_rgba(356, 48),
                ),
                (
                    "minori:/sys/saveload_Page0.png".into(),
                    encode_rgba(208, 48),
                ),
                (
                    "minori:/sys/saveload_Page1.png".into(),
                    encode_rgba(208, 48),
                ),
                (
                    "minori:/sys/saveload_Page2.png".into(),
                    encode_rgba(208, 48),
                ),
                ("minori:/sys/notsaved.png".into(), encode_rgba(106, 60)),
            ]),
        });
        let surfaces = Arc::new(RecordingSurfaceHost::default());
        let writable = Arc::new(InMemoryWritableFiles::default());
        let system_menus = Arc::new(RecordingSystemMenuHost::default());
        let text_inputs = Arc::new(RecordingTextInputHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs: Arc::clone(&vfs),
            surfaces,
            hooks: Arc::new(UnboundHookHost),
            writable_files: writable.clone(),
            system_menus: system_menus.clone(),
            confirmations: Arc::new(RecordingConfirmationHost::default()),
            system_commands: Arc::new(RecordingSystemCommandHost::default()),
            text_inputs: text_inputs.clone(),
        };
        let mut provider = MinoriRuntimeProvider::with_host_services(services);
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.surface".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        provider
            .sessions
            .get_mut(&session.0)
            .unwrap()
            .last_gameplay_frame = Some(Arc::from(vec![0x20; 1280 * 720 * 4]));
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Open,
                        menu_id: None,
                        item_id: None,
                        pointer_x: Some(640),
                        pointer_y: Some(360),
                        sequence: 1,
                    }),
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();

        let session_state = provider.sessions.get(&session.0).unwrap();
        assert_eq!(
            session_state.vm.state().system_ui.page,
            MinoriSystemPage::None
        );
        assert_eq!(session_state.vm.state().system_ui.pointer_x, 640);
        assert_eq!(session_state.vm.state().system_ui.pointer_y, 360);
        assert!(output.live.resource_scenes.is_empty());
        let published = system_menus.published.lock().unwrap();
        let menu = &published[0].1;
        assert!(menu
            .items
            .iter()
            .any(|item| item.item_id == "message_panel"));
        assert!(menu.items.iter().any(|item| item.item_id == "save"));
        let precision = menu
            .items
            .iter()
            .find(|item| item.item_id == "window_precision")
            .expect("the precision resize item is part of the native menu");
        assert!(!precision.enabled);
        assert!(precision.checked);
        let fullscreen = menu
            .items
            .iter()
            .find(|item| item.item_id == "window_fullscreen")
            .expect("the fullscreen item is visible in a normal window");
        assert!(!fullscreen.checked);
        assert!(menu
            .items
            .iter()
            .filter(|item| matches!(item.item_id.as_str(), "auto" | "skip"))
            .all(|item| !item.checked));
        assert_eq!(
            menu.items
                .iter()
                .find(|item| item.item_id == "help")
                .map(|item| item.label.as_str()),
            Some("ヘルプ (&H)")
        );
        assert_eq!(
            menu.items
                .iter()
                .find(|item| item.item_id == "game")
                .map(|item| item.label.as_str()),
            Some("ゲーム (&G)")
        );
        let menu_id = menu.menu_id.clone();
        drop(published);

        let invalid_selection = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Select,
                        menu_id: Some(menu_id.clone()),
                        item_id: Some("not-in-active-menu".into()),
                        pointer_x: None,
                        pointer_y: None,
                        sequence: 2,
                    }),
                    ..step_input(3, Vec::new())
                },
            )
            .expect_err("a selection outside the published transaction must block");
        assert_eq!(
            invalid_selection.code(),
            "ASTRA_EMU_MINORI_SYSTEM_MENU_ITEM_UNKNOWN"
        );

        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Select,
                        menu_id: Some(menu_id),
                        item_id: Some("save".into()),
                        pointer_x: None,
                        pointer_y: None,
                        sequence: 2,
                    }),
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(
            provider.sessions[&session.0].vm.state().system_ui.page,
            MinoriSystemPage::Save
        );
        assert!(output.live.resource_scenes.iter().any(|scene| {
            scene
                .value
                .texture_resources
                .iter()
                .any(|resource| resource.resource_uri == "minori:/sys/saveloadBase.png")
        }));

        let prompt_output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 3,
                    }],
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        let prompt = text_inputs.published.lock().unwrap()[0].1.clone();
        assert!(prompt
            .prompt_id
            .starts_with("minori.text_input.save_comment.20."));
        assert_eq!(prompt.title, "SAVE");
        assert_eq!(prompt.label, "Comment");
        assert_eq!(prompt.initial_value, "");
        assert_eq!(prompt.max_bytes, 256);
        assert!(prompt_output.live.resource_scenes.iter().any(|scene| {
            scene
                .value
                .texture_resources
                .iter()
                .any(|resource| resource.resource_uri == "minori:/sys/saveloadBase.png")
        }));

        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    text_input: Some(LegacyTextInputResultV1 {
                        prompt_id: prompt.prompt_id,
                        choice: LegacyTextInputChoiceV1::Accepted,
                        value: "memo".into(),
                        sequence: 4,
                    }),
                    ..step_input(5, Vec::new())
                },
            )
            .unwrap();
        assert!(writable.files.lock().unwrap().contains_key(&slot_path(20)));
        assert_eq!(
            provider.sessions[&session.0].save_slot_comments.get(&20),
            Some(&"memo".to_owned())
        );

        // The original filename builder uses page * 10 + slot, and the quick
        // save rotates the ten Page1 file numbers 10..19 from the persisted
        // cursor.  The first quick save therefore writes slot 10 while slot 0
        // remains the title-page Auto Save range.
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Open,
                        menu_id: None,
                        item_id: None,
                        pointer_x: Some(640),
                        pointer_y: Some(360),
                        sequence: 6,
                    }),
                    ..step_input(6, Vec::new())
                },
            )
            .unwrap();
        assert!(output.live.resource_scenes.is_empty());
        let quick_menu_id = system_menus
            .published
            .lock()
            .unwrap()
            .last()
            .expect("quick-save menu is published before selection")
            .1
            .menu_id
            .clone();
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Select,
                        menu_id: Some(quick_menu_id),
                        item_id: Some("quick_save".into()),
                        pointer_x: None,
                        pointer_y: None,
                        sequence: 7,
                    }),
                    ..step_input(7, Vec::new())
                },
            )
            .unwrap();
        let files = writable.files.lock().unwrap();
        assert!(files.contains_key(&slot_path(quick_save_file_number(0))));
        assert!(!files.contains_key(&slot_path(0)));
    }

    #[test]
    fn quick_save_rotates_page1_slots_and_skips_unchanged_lines() {
        let encode_rgba = |width: u32, height: u32| {
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(
                    &vec![0; usize::try_from(width * height * 4).unwrap()],
                    width,
                    height,
                    ExtendedColorType::Rgba8,
                )
                .unwrap();
            png
        };
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                (
                    "minori:/scr/test.sc".into(),
                    b".wait 20\r\n.wait 20\r\n.end\r\n".to_vec(),
                ),
                (
                    "minori:/sys/saveloadBase.png".into(),
                    encode_rgba(1280, 720),
                ),
                ("minori:/sys/saveloadSave.png".into(), encode_rgba(352, 48)),
                (
                    "minori:/sys/saveloadSelect.png".into(),
                    encode_rgba(344, 98),
                ),
                (
                    "minori:/sys/saveloadButtons.png".into(),
                    encode_rgba(356, 48),
                ),
                (
                    "minori:/sys/saveload_Page0.png".into(),
                    encode_rgba(208, 48),
                ),
                (
                    "minori:/sys/saveload_Page1.png".into(),
                    encode_rgba(208, 48),
                ),
                (
                    "minori:/sys/saveload_Page2.png".into(),
                    encode_rgba(208, 48),
                ),
                ("minori:/sys/notsaved.png".into(), encode_rgba(106, 60)),
            ]),
        });
        let writable = Arc::new(InMemoryWritableFiles::default());
        let system_menus = Arc::new(RecordingSystemMenuHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs,
            surfaces: Arc::new(RecordingSurfaceHost::default()),
            hooks: Arc::new(UnboundHookHost),
            writable_files: writable.clone(),
            system_menus: system_menus.clone(),
            confirmations: Arc::new(RecordingConfirmationHost::default()),
            system_commands: Arc::new(RecordingSystemCommandHost::default()),
            text_inputs: Arc::new(RecordingTextInputHost::default()),
        };
        let mut provider = MinoriRuntimeProvider::with_host_services(services);
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.quick".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        provider
            .sessions
            .get_mut(&session.0)
            .unwrap()
            .last_gameplay_frame = Some(Arc::from(vec![0x20; 1280 * 720 * 4]));

        let mut sequence = 1u64;
        let mut tick = 1u64;
        let mut quick_save = |provider: &mut MinoriRuntimeProvider,
                              session: &LegacyRuntimeSessionId,
                              sequence: &mut u64,
                              tick: &mut u64| {
            *tick += 1;
            *sequence += 1;
            provider
                .step(
                    &ctx,
                    session,
                    LegacyStepInput {
                        system_menu: Some(LegacySystemMenuRequestV1 {
                            action: LegacySystemMenuActionV1::Open,
                            menu_id: None,
                            item_id: None,
                            pointer_x: Some(640),
                            pointer_y: Some(360),
                            sequence: *sequence,
                        }),
                        ..step_input(*tick, Vec::new())
                    },
                )
                .unwrap();
            let menu_id = system_menus
                .published
                .lock()
                .unwrap()
                .last()
                .expect("quick-save menu is published before selection")
                .1
                .menu_id
                .clone();
            *tick += 1;
            *sequence += 1;
            provider
                .step(
                    &ctx,
                    session,
                    LegacyStepInput {
                        system_menu: Some(LegacySystemMenuRequestV1 {
                            action: LegacySystemMenuActionV1::Select,
                            menu_id: Some(menu_id),
                            item_id: Some("quick_save".into()),
                            pointer_x: None,
                            pointer_y: None,
                            sequence: *sequence,
                        }),
                        ..step_input(*tick, Vec::new())
                    },
                )
                .unwrap();
        };

        // The first quick save writes Page1 slot 10.
        quick_save(&mut provider, &session, &mut sequence, &mut tick);
        let initial_pc_line = provider.sessions[&session.0].vm.state().pc_line;
        eprintln!(
            "DEBUG after #1: cursor={} pc_line={}",
            provider.sessions[&session.0].quick_save_cursor,
            initial_pc_line
        );
        assert!(writable
            .files
            .lock()
            .unwrap()
            .contains_key(&slot_path(quick_save_file_number(0))));
        assert_eq!(provider.sessions[&session.0].quick_save_cursor, 1);

        // A repeated quick save at the same script line is skipped entirely:
        // no new file and no further cursor advance.
        quick_save(&mut provider, &session, &mut sequence, &mut tick);
        eprintln!(
            "DEBUG after #2: cursor={} pc_line={} last={:?}",
            provider.sessions[&session.0].quick_save_cursor,
            provider.sessions[&session.0].vm.state().pc_line,
            provider.sessions[&session.0].last_quick_save_pc_line
        );
        assert_eq!(provider.sessions[&session.0].quick_save_cursor, 1);
        assert!(!writable
            .files
            .lock()
            .unwrap()
            .contains_key(&slot_path(quick_save_file_number(1))));

        // Script progress moves the line cursor, so the next quick save
        // writes slot 11.
        while provider.sessions[&session.0].vm.state().pc_line == initial_pc_line {
            tick += 1;
            provider
                .step(&ctx, &session, step_input(tick, Vec::new()))
                .unwrap();
        }
        eprintln!(
            "DEBUG before #3: cursor={} pc_line={} last={:?}",
            provider.sessions[&session.0].quick_save_cursor,
            provider.sessions[&session.0].vm.state().pc_line,
            provider.sessions[&session.0].last_quick_save_pc_line
        );
        quick_save(&mut provider, &session, &mut sequence, &mut tick);
        eprintln!(
            "DEBUG after #3: cursor={} pc_line={}",
            provider.sessions[&session.0].quick_save_cursor,
            provider.sessions[&session.0].vm.state().pc_line
        );
        {
            let files = writable.files.lock().unwrap();
            assert!(files.contains_key(&slot_path(quick_save_file_number(1))));
            assert!(!files.contains_key(&slot_path(quick_save_file_number(2))));
        }
        assert_eq!(provider.sessions[&session.0].quick_save_cursor, 2);
    }

    #[test]
    fn native_system_command_is_typed_and_applied_only_after_host_completion() {
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/scr/test.sc".into(),
                b".wait 20\r\n.end\r\n".to_vec(),
            )]),
        });
        let system_menus = Arc::new(RecordingSystemMenuHost::default());
        let system_commands = Arc::new(RecordingSystemCommandHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs,
            surfaces: Arc::new(RecordingSurfaceHost::default()),
            hooks: Arc::new(UnboundHookHost),
            writable_files: Arc::new(RejectWritableFiles),
            system_menus: system_menus.clone(),
            confirmations: Arc::new(RecordingConfirmationHost::default()),
            system_commands: system_commands.clone(),
            text_inputs: Arc::new(RecordingTextInputHost::default()),
        };
        let mut provider = MinoriRuntimeProvider::with_host_services(services);
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.command".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Open,
                        menu_id: None,
                        item_id: None,
                        pointer_x: Some(640),
                        pointer_y: Some(360),
                        sequence: 1,
                    }),
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        let menu_id = system_menus.published.lock().unwrap()[0].1.menu_id.clone();
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Select,
                        menu_id: Some(menu_id),
                        item_id: Some("window_fullscreen".into()),
                        pointer_x: None,
                        pointer_y: None,
                        sequence: 2,
                    }),
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        let command = system_commands.published.lock().unwrap()[0].1.clone();
        assert!(matches!(
            command.command,
            LegacySystemCommandKindV1::SetFullscreen { enabled: true }
        ));
        assert!(
            !provider.sessions[&session.0]
                .vm
                .state()
                .system_ui
                .config
                .fullscreen
        );
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_command: Some(LegacySystemCommandResultV1 {
                        command_id: command.command_id,
                        status: LegacySystemCommandStatusV1::Applied,
                        sequence: 3,
                    }),
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        assert!(
            provider.sessions[&session.0]
                .vm
                .state()
                .system_ui
                .config
                .fullscreen
        );

        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Open,
                        menu_id: None,
                        item_id: None,
                        pointer_x: Some(640),
                        pointer_y: Some(360),
                        sequence: 4,
                    }),
                    ..step_input(5, Vec::new())
                },
            )
            .unwrap();
        let reopened = system_menus
            .published
            .lock()
            .unwrap()
            .last()
            .expect("fullscreen menu must be published")
            .1
            .clone();
        assert!(!reopened
            .items
            .iter()
            .any(|item| item.item_id == "window_fullscreen"));
        assert_eq!(
            reopened
                .items
                .iter()
                .find(|item| item.item_id == "window_original_size")
                .map(|item| item.order),
            Some(8)
        );

        // Restoring the authored window size also leaves fullscreen on the
        // native host.  The next Family transaction must therefore publish
        // the fullscreen command again instead of retaining stale VM config.
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Select,
                        menu_id: Some(reopened.menu_id.clone()),
                        item_id: Some("window_original_size".into()),
                        pointer_x: None,
                        pointer_y: None,
                        sequence: 5,
                    }),
                    ..step_input(6, Vec::new())
                },
            )
            .unwrap();
        let restore_command = system_commands
            .published
            .lock()
            .unwrap()
            .last()
            .expect("original-size selection must publish a host command")
            .1
            .clone();
        assert_eq!(
            restore_command.command,
            LegacySystemCommandKindV1::RestoreOriginalSize
        );
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_command: Some(LegacySystemCommandResultV1 {
                        command_id: restore_command.command_id,
                        status: LegacySystemCommandStatusV1::Applied,
                        sequence: 6,
                    }),
                    ..step_input(7, Vec::new())
                },
            )
            .unwrap();
        assert!(
            !provider.sessions[&session.0]
                .vm
                .state()
                .system_ui
                .config
                .fullscreen
        );

        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_menu: Some(LegacySystemMenuRequestV1 {
                        action: LegacySystemMenuActionV1::Open,
                        menu_id: None,
                        item_id: None,
                        pointer_x: Some(640),
                        pointer_y: Some(360),
                        sequence: 7,
                    }),
                    ..step_input(8, Vec::new())
                },
            )
            .unwrap();
        let restored = system_menus
            .published
            .lock()
            .unwrap()
            .last()
            .expect("restored window must republish a menu")
            .1
            .clone();
        assert!(restored
            .items
            .iter()
            .any(|item| item.item_id == "window_fullscreen"));
    }

    #[test]
    fn config_fullscreen_uses_host_command_before_resuming_gameplay() {
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/scr/test.sc".into(),
                b".wait 20\r\n.end\r\n".to_vec(),
            )]),
        });
        let system_commands = Arc::new(RecordingSystemCommandHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs,
            surfaces: Arc::new(RecordingSurfaceHost::default()),
            hooks: Arc::new(UnboundHookHost),
            writable_files: Arc::new(RejectWritableFiles),
            system_menus: Arc::new(RecordingSystemMenuHost::default()),
            confirmations: Arc::new(RecordingConfirmationHost::default()),
            system_commands: system_commands.clone(),
            text_inputs: Arc::new(RecordingTextInputHost::default()),
        };
        let mut provider = MinoriRuntimeProvider::with_host_services(services);
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.config.fullscreen".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        // Establish a title-launched gameplay wait without going through the
        // title resource presentation.  The provider path under test starts
        // from the same stable wait that the native menu uses.
        {
            let session_state = provider.sessions.get_mut(&session.0).unwrap();
            session_state.vm.begin_title_launch().unwrap();
            session_state
                .vm
                .set_system_page(MinoriSystemPage::None, 0)
                .unwrap();
            session_state.vm.step(1, 100).unwrap();
            session_state.vm.open_gameplay_config().unwrap();
            assert_eq!(
                session_state
                    .vm
                    .apply_config_control(MinoriConfigControl::Fullscreen(true))
                    .unwrap(),
                MinoriConfigChange::Present
            );
        }

        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        let command = system_commands
            .published
            .lock()
            .unwrap()
            .last()
            .expect("config apply must publish a native fullscreen command")
            .1
            .clone();
        assert_eq!(
            command.command,
            LegacySystemCommandKindV1::SetFullscreen { enabled: true }
        );
        assert!(provider.sessions[&session.0]
            .active_system_command
            .is_some());

        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    system_command: Some(LegacySystemCommandResultV1 {
                        command_id: command.command_id,
                        status: LegacySystemCommandStatusV1::Applied,
                        sequence: 2,
                    }),
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        let session_state = &provider.sessions[&session.0];
        assert!(session_state.active_system_command.is_none());
        assert!(session_state.vm.state().system_ui.config.fullscreen);
        assert_eq!(
            session_state.vm.state().system_ui.page,
            MinoriSystemPage::None
        );
    }

    #[test]
    fn host_owned_system_commands_release_the_suspended_session_after_apply() {
        let commands = [
            (
                "window_original_size",
                LegacySystemCommandKindV1::RestoreOriginalSize,
            ),
            (
                "window_antialias",
                LegacySystemCommandKindV1::SetResizeAntialias { enabled: false },
            ),
            ("help_manual", LegacySystemCommandKindV1::OpenManual),
            ("help_about", LegacySystemCommandKindV1::ShowAbout),
            ("help_homepage", LegacySystemCommandKindV1::OpenHomepage),
        ];
        for (index, (item_id, expected)) in commands.into_iter().enumerate() {
            let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
                scripts: BTreeMap::from([(
                    "minori:/scr/test.sc".into(),
                    b".wait 20\r\n.end\r\n".to_vec(),
                )]),
            });
            let system_menus = Arc::new(RecordingSystemMenuHost::default());
            let system_commands = Arc::new(RecordingSystemCommandHost::default());
            let services = LegacyFamilyHostServicesV9 {
                vfs,
                surfaces: Arc::new(RecordingSurfaceHost::default()),
                hooks: Arc::new(UnboundHookHost),
                writable_files: Arc::new(RejectWritableFiles),
                system_menus: system_menus.clone(),
                confirmations: Arc::new(RecordingConfirmationHost::default()),
                system_commands: system_commands.clone(),
                text_inputs: Arc::new(RecordingTextInputHost::default()),
            };
            let mut provider = MinoriRuntimeProvider::with_host_services(services);
            let ctx = context();
            let session = provider
                .open(
                    &ctx,
                    LegacyOpenRequest {
                        requested_session_id: LegacyRuntimeSessionId(format!(
                            "session.command.host_owned.{index}"
                        )),
                        case_fingerprint: Hash256::from_sha256(b"case"),
                        script_uri: "minori:/scr/test.sc".into(),
                        fixed_delta_ns: 16_666_667,
                        session_seed: 7,
                        compatibility_profile: "minori.reference".into(),
                        family_options: BTreeMap::new(),
                    },
                )
                .unwrap();
            provider
                .step(&ctx, &session, step_input(1, Vec::new()))
                .unwrap();
            provider
                .step(
                    &ctx,
                    &session,
                    LegacyStepInput {
                        system_menu: Some(LegacySystemMenuRequestV1 {
                            action: LegacySystemMenuActionV1::Open,
                            menu_id: None,
                            item_id: None,
                            pointer_x: Some(640),
                            pointer_y: Some(360),
                            sequence: 1,
                        }),
                        ..step_input(2, Vec::new())
                    },
                )
                .unwrap();
            let menu_id = system_menus.published.lock().unwrap()[0].1.menu_id.clone();
            provider
                .step(
                    &ctx,
                    &session,
                    LegacyStepInput {
                        system_menu: Some(LegacySystemMenuRequestV1 {
                            action: LegacySystemMenuActionV1::Select,
                            menu_id: Some(menu_id),
                            item_id: Some(item_id.into()),
                            pointer_x: None,
                            pointer_y: None,
                            sequence: 2,
                        }),
                        ..step_input(3, Vec::new())
                    },
                )
                .unwrap();
            let command = system_commands.published.lock().unwrap()[0].1.clone();
            assert_eq!(command.command, expected);
            provider
                .step(
                    &ctx,
                    &session,
                    LegacyStepInput {
                        system_command: Some(LegacySystemCommandResultV1 {
                            command_id: command.command_id,
                            status: LegacySystemCommandStatusV1::Applied,
                            sequence: 3,
                        }),
                        ..step_input(4, Vec::new())
                    },
                )
                .unwrap();
            assert!(!provider.sessions[&session.0].poisoned);
            assert!(provider.sessions[&session.0]
                .active_system_command
                .is_none());
            if item_id == "window_antialias" {
                provider
                    .step(
                        &ctx,
                        &session,
                        LegacyStepInput {
                            system_menu: Some(LegacySystemMenuRequestV1 {
                                action: LegacySystemMenuActionV1::Open,
                                menu_id: None,
                                item_id: None,
                                pointer_x: Some(640),
                                pointer_y: Some(360),
                                sequence: 4,
                            }),
                            ..step_input(5, Vec::new())
                        },
                    )
                    .unwrap();
                let menus = system_menus.published.lock().unwrap();
                let reopened = menus
                    .last()
                    .expect("the menu must be republished")
                    .1
                    .clone();
                let antialias = reopened
                    .items
                    .iter()
                    .find(|item| item.item_id == "window_antialias")
                    .expect("the antialiasing item must remain visible");
                assert!(!antialias.checked);
            }
        }
    }

    #[test]
    fn exit_confirmation_cancels_without_consuming_gameplay_then_accepts_once() {
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/scr/test.sc".into(),
                b".wait 20\r\n.end\r\n".to_vec(),
            )]),
        });
        let system_menus = Arc::new(RecordingSystemMenuHost::default());
        let confirmations = Arc::new(RecordingConfirmationHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs,
            surfaces: Arc::new(RecordingSurfaceHost::default()),
            hooks: Arc::new(UnboundHookHost),
            writable_files: Arc::new(RejectWritableFiles),
            system_menus: system_menus.clone(),
            confirmations: confirmations.clone(),
            system_commands: Arc::new(RecordingSystemCommandHost::default()),
            text_inputs: Arc::new(RecordingTextInputHost::default()),
        };
        let mut provider = MinoriRuntimeProvider::with_host_services(services);
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.confirmation".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        assert_eq!(
            provider
                .step(&ctx, &session, step_input(1, Vec::new()))
                .unwrap()
                .status,
            LegacyRuntimeStatus::Awaiting
        );

        let select_exit = |provider: &mut MinoriRuntimeProvider, tick: u64, menu_id: String| {
            provider
                .step(
                    &ctx,
                    &session,
                    LegacyStepInput {
                        system_menu: Some(LegacySystemMenuRequestV1 {
                            action: LegacySystemMenuActionV1::Select,
                            menu_id: Some(menu_id),
                            item_id: Some("game_exit".into()),
                            pointer_x: None,
                            pointer_y: None,
                            sequence: tick,
                        }),
                        ..step_input(tick, Vec::new())
                    },
                )
                .unwrap()
        };
        let open_menu = |provider: &mut MinoriRuntimeProvider, tick: u64| {
            provider
                .step(
                    &ctx,
                    &session,
                    LegacyStepInput {
                        system_menu: Some(LegacySystemMenuRequestV1 {
                            action: LegacySystemMenuActionV1::Open,
                            menu_id: None,
                            item_id: None,
                            pointer_x: Some(640),
                            pointer_y: Some(360),
                            sequence: tick,
                        }),
                        ..step_input(tick, Vec::new())
                    },
                )
                .unwrap();
            system_menus
                .published
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .1
                .menu_id
                .clone()
        };

        let first_menu = open_menu(&mut provider, 2);
        assert_eq!(
            select_exit(&mut provider, 3, first_menu).status,
            LegacyRuntimeStatus::Awaiting
        );
        let first_confirmation = confirmations
            .published
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .confirmation_id
            .clone();
        {
            let published = confirmations.published.lock().unwrap();
            let confirmation = &published.last().unwrap().1;
            assert_eq!(confirmation.title, "確認");
            assert_eq!(confirmation.message, "終了してもよろしいですか?");
            assert_eq!(confirmation.accept_label, "是(Y)");
            assert_eq!(confirmation.cancel_label, "否(N)");
        }
        let cancelled = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    confirmation: Some(LegacyConfirmationResultV1 {
                        confirmation_id: first_confirmation,
                        choice: LegacyConfirmationChoiceV1::Cancelled,
                        sequence: 4,
                    }),
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(cancelled.status, LegacyRuntimeStatus::Awaiting);
        assert!(provider.sessions[&session.0].vm.state().wait.is_some());

        let second_menu = open_menu(&mut provider, 5);
        select_exit(&mut provider, 6, second_menu);
        let second_confirmation = confirmations
            .published
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .confirmation_id
            .clone();
        let accepted = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    confirmation: Some(LegacyConfirmationResultV1 {
                        confirmation_id: second_confirmation,
                        choice: LegacyConfirmationChoiceV1::Accepted,
                        sequence: 7,
                    }),
                    ..step_input(7, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(accepted.status, LegacyRuntimeStatus::Terminal);
    }

    #[test]
    fn confirmation_messages_match_original_ascii_question_marks() {
        assert_eq!(
            minori_confirmation_message(MinoriConfirmationAction::Exit),
            "終了してもよろしいですか?"
        );
        assert_eq!(
            minori_confirmation_message(MinoriConfirmationAction::ReturnTitle),
            "ゲームを中断してメニューに戻ります。よろしいですか?"
        );
    }

    #[test]
    fn title_exit_terminates_directly_without_confirmation_transaction() {
        let mut title_png = Vec::new();
        PngEncoder::new(&mut title_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let confirmations = Arc::new(RecordingConfirmationHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs: Arc::new(MemoryReader {
                scripts: BTreeMap::from([
                    ("minori:/scr/test.sc".into(), b".end\r\n".to_vec()),
                    ("minori:/sys/topMenu0.png".into(), title_png),
                ]),
            }),
            surfaces: Arc::new(RecordingSurfaceHost::default()),
            hooks: Arc::new(UnboundHookHost),
            writable_files: Arc::new(RejectWritableFiles),
            system_menus: Arc::new(RecordingSystemMenuHost::default()),
            confirmations: confirmations.clone(),
            system_commands: Arc::new(RecordingSystemCommandHost::default()),
            text_inputs: Arc::new(RecordingTextInputHost::default()),
        };
        let mut provider = MinoriRuntimeProvider::with_host_services(services);
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.title-exit".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        assert_eq!(
            provider
                .step(&ctx, &session, step_input(1, Vec::new()))
                .unwrap()
                .status,
            LegacyRuntimeStatus::Active
        );

        let mut input_edges = Vec::with_capacity(4);
        for sequence in 1..=3 {
            input_edges.push(LegacyInputEdge {
                control: "arrow_down".into(),
                pressed: true,
                value: 1.0,
                sequence,
            });
        }
        input_edges.push(LegacyInputEdge {
            control: "enter".into(),
            pressed: true,
            value: 1.0,
            sequence: 4,
        });
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges,
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Terminal);
        assert!(provider.sessions[&session.0].vm.state().terminal);
        assert!(confirmations.published.lock().unwrap().is_empty());
    }

    #[test]
    fn host_window_close_publishes_family_confirmation_before_exit() {
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/scr/test.sc".into(),
                b".wait 20\r\n.end\r\n".to_vec(),
            )]),
        });
        let confirmations = Arc::new(RecordingConfirmationHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs,
            surfaces: Arc::new(RecordingSurfaceHost::default()),
            hooks: Arc::new(UnboundHookHost),
            writable_files: Arc::new(RejectWritableFiles),
            system_menus: Arc::new(RecordingSystemMenuHost::default()),
            confirmations: confirmations.clone(),
            system_commands: Arc::new(RecordingSystemCommandHost::default()),
            text_inputs: Arc::new(RecordingTextInputHost::default()),
        };
        let mut provider = MinoriRuntimeProvider::with_host_services(services);
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.window-close".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let pending = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "window.close".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(pending.status, LegacyRuntimeStatus::Awaiting);
        let confirmation_id = confirmations
            .published
            .lock()
            .unwrap()
            .last()
            .expect("window close must publish a typed confirmation")
            .1
            .confirmation_id
            .clone();
        assert!(confirmation_id.starts_with("minori.confirmation.window_close."));
        let accepted = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    confirmation: Some(LegacyConfirmationResultV1 {
                        confirmation_id,
                        choice: LegacyConfirmationChoiceV1::Accepted,
                        sequence: 2,
                    }),
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(accepted.status, LegacyRuntimeStatus::Terminal);
    }

    #[test]
    fn save_load_page_uses_the_verified_assets_and_slot_grid() {
        let encode_rgba = |width: u32, height: u32| {
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(
                    &vec![0; usize::try_from(width * height * 4).unwrap()],
                    width,
                    height,
                    ExtendedColorType::Rgba8,
                )
                .unwrap();
            png
        };
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), b".end\r\n".to_vec()),
                (
                    "minori:/sys/saveloadBase.png".into(),
                    encode_rgba(1280, 720),
                ),
                ("minori:/sys/saveloadSave.png".into(), encode_rgba(352, 48)),
                (
                    "minori:/sys/saveloadSelect.png".into(),
                    encode_rgba(344, 98),
                ),
                (
                    "minori:/sys/saveloadButtons.png".into(),
                    encode_rgba(356, 48),
                ),
                (
                    "minori:/sys/saveload_Page0.png".into(),
                    encode_rgba(208, 48),
                ),
                (
                    "minori:/sys/saveload_Page1.png".into(),
                    encode_rgba(208, 48),
                ),
                (
                    "minori:/sys/saveload_Page2.png".into(),
                    encode_rgba(208, 48),
                ),
                ("minori:/sys/notsaved.png".into(), encode_rgba(106, 60)),
            ]),
        });
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(b"script"),
            parse_sc(b".end\r\n", &ScOpcodeCatalog::observed_minori()).unwrap(),
            7,
        )
        .unwrap();
        vm.begin_title_launch().unwrap();
        vm.set_system_page(MinoriSystemPage::Save, 0).unwrap();
        let frame = describe_system_page_with_slots(
            &vfs,
            "mount.test",
            Some((1280, 720)),
            &vm,
            &BTreeSet::from([0, 7]),
        )
        .unwrap();
        assert_eq!(frame.texture_resources.len(), 6);
        assert_eq!(frame.draws.len(), 13);
        assert_eq!(
            frame.texture_resources[0].resource_uri,
            "minori:/sys/saveloadBase.png"
        );
        assert_eq!(
            frame.texture_resources[1].resource_uri,
            "minori:/sys/saveloadSave.png"
        );
        assert!(frame.draws.iter().any(|draw| {
            draw.texture_id == MINORI_SYSTEM_TEXTURE_ID + 1
                && draw.vertices[0].position
                    == [
                        MINORI_SAVE_LOAD_TITLE_X as f32,
                        MINORI_SAVE_LOAD_HEADER_Y as f32,
                    ]
        }));
        assert!(frame.draws.iter().any(|draw| {
            draw.texture_id == MINORI_SYSTEM_TEXTURE_ID + 3
                && draw.vertices[0].position
                    == [
                        MINORI_SAVE_LOAD_PAGE_X as f32,
                        MINORI_SAVE_LOAD_HEADER_Y as f32,
                    ]
        }));
        assert!(frame.draws.iter().any(|draw| {
            draw.texture_id == MINORI_SYSTEM_TEXTURE_ID + 2
                && draw.vertices[0].position == [64.0, 81.0]
        }));
        vm.set_system_page(MinoriSystemPage::Save, 20).unwrap();
        let manual_frame = describe_system_page_with_slots(
            &vfs,
            "mount.test",
            Some((1280, 720)),
            &vm,
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            manual_frame.texture_resources[3].resource_uri,
            "minori:/sys/saveload_Page2.png"
        );
        assert!(manual_frame.draws.iter().any(|draw| {
            draw.texture_id == MINORI_SYSTEM_TEXTURE_ID + 4
                && draw.scissor
                    == Some(LegacyScissorV1 {
                        x: 578,
                        y: 656,
                        width: 240,
                        height: 48,
                    })
        }));
        vm.set_save_focus(17).unwrap();
        vm.move_save_page(1).unwrap();
        assert_eq!(vm.state().system_ui.focus_index, 27);
    }

    #[test]
    fn occupied_save_slot_overlays_verified_thumbnail_coordinates() {
        let stage_bytes = 1280 * 720 * 4;
        let thumbnail = Arc::<[u8]>::from([255, 0, 0, 255].repeat(96 * 54));
        let metadata = BTreeMap::from([(
            0,
            MinoriSaveSlotMetadata {
                timestamp: "2026/09/01 12:34".into(),
                comment: "memo".into(),
                thumbnail_rgba: thumbnail,
            },
        )]);
        let prepared = overlay_save_thumbnails(
            PreparedMinoriLayer {
                role: MinoriLayerRole::Panel,
                rgba8_premultiplied: Arc::from(vec![0; stage_bytes]),
            },
            &metadata,
            0,
            1280,
            720,
        )
        .unwrap();
        let pixel = (96 * 1280 + 74) * 4;
        assert_eq!(
            &prepared.rgba8_premultiplied[pixel..pixel + 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(&prepared.rgba8_premultiplied[..4], &[0; 4]);
    }

    #[test]
    fn save_metadata_timestamp_and_thumbnail_validation_are_strict() {
        assert!(validate_save_timestamp("2026/09/01 12:34").is_ok());
        assert!(validate_save_timestamp("2026-09-01 12:34").is_err());
        assert!(validate_save_timestamp("2026/13/01 12:34").is_err());

        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&vec![64; 96 * 54 * 4], 96, 54, ExtendedColorType::Rgba8)
            .unwrap();
        let decoded = decode_save_thumbnail(&png).unwrap();
        assert_eq!(decoded.len(), 96 * 54 * 4);
        assert_eq!(&decoded[..4], &[16, 16, 16, 64]);

        let mut wrong_size = Vec::new();
        PngEncoder::new(&mut wrong_size)
            .write_image(&vec![0; 95 * 54 * 4], 95, 54, ExtendedColorType::Rgba8)
            .unwrap();
        assert!(decode_save_thumbnail(&wrong_size).is_err());
    }

    #[test]
    fn v9_resource_scene_writes_exclusive_host_surfaces_and_multilayer_transaction() {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(
                &[
                    255, 0, 0, 128, 255, 0, 0, 128, 255, 0, 0, 128, 255, 0, 0, 128,
                ],
                2,
                2,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/bg/test.png".into(), png)]),
        });
        let resource = read_texture_resource(&vfs, "mount.test", "minori:/bg/test.png", 1).unwrap();
        let mut draws = Vec::new();
        append_texture_draw(&resource, 0, 0, 1.0, &mut draws).unwrap();
        let frame = LegacyRenderResourceFrameV1 {
            width: 2,
            height: 2,
            texture_resources: vec![resource],
            draws,
        };
        let surfaces = Arc::new(RecordingSurfaceHost::default());
        let services = LegacyFamilyHostServicesV9 {
            vfs: Arc::clone(&vfs),
            surfaces: surfaces.clone(),
            hooks: Arc::new(UnboundHookHost),
            writable_files: Arc::new(RejectWritableFiles),
            system_menus: Arc::new(RecordingSystemMenuHost::default()),
            confirmations: Arc::new(RecordingConfirmationHost::default()),
            system_commands: Arc::new(RecordingSystemCommandHost::default()),
            text_inputs: Arc::new(RecordingTextInputHost::default()),
        };
        let mut published_layers = BTreeSet::new();
        let mut presentation_layers = BTreeMap::new();
        let transactions = publish_resource_scene(
            &services,
            &vfs,
            &LegacyRuntimeSessionId("session.surface".into()),
            "mount.test",
            1,
            7,
            &mut published_layers,
            &mut presentation_layers,
            &frame,
            None,
        )
        .unwrap();
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].sequence, 7);
        assert_eq!(transactions[0].operations.len(), 4);
        assert!(transactions[0]
            .operations
            .iter()
            .all(|operation| matches!(operation, LegacyLayerOperationV9::Create(_))));
        assert_eq!(published_layers.len(), 4);

        let commits = surfaces.commits.lock().unwrap();
        assert_eq!(commits.len(), 4);
        let background = commits
            .iter()
            .find(|commit| commit.surface_id == "minori.surface.background")
            .unwrap();
        assert_eq!(background.generation, 1);
        assert_eq!(background.stride, 16);
        assert_eq!(&background.pixels[..4], &[128, 0, 0, 128]);
        assert_eq!(&background.pixels[8..16], &[0; 8]);
        for commit in commits
            .iter()
            .filter(|commit| commit.surface_id != "minori.surface.background")
        {
            assert_eq!(commit.stride, 16);
            assert!(commit.pixels.iter().all(|byte| *byte == 0));
        }
    }

    #[test]
    fn provider_lifecycle_wait_snapshot_restore_and_shutdown() {
        let script = b".setglobal REN_CLEAR = 1\r\n.wait 20\r\n.end\r\n".to_vec();
        let case_fingerprint = Hash256::from_sha256(b"case");
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.test".into()),
                    case_fingerprint,
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([(
                        "astra.hosted_trace_profile".into(),
                        "evidence".into(),
                    )]),
                },
            )
            .unwrap();
        let first = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(first.status, LegacyRuntimeStatus::Awaiting);
        assert!(first.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.gallery_unlock_count" && mutation.value == "1"
        }));
        let token = match &first.control.waits[0] {
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => {
                assert_eq!(*milliseconds, 200);
                token_id.clone()
            }
            _ => panic!("expected time wait"),
        };
        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        let waiting = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
        let completed = provider
            .step(
                &ctx,
                &session,
                step_input(
                    2,
                    vec![LegacyAwaitResult {
                        token_id: token,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
        let shutdown = provider.shutdown(&ctx, &session).unwrap();
        assert_eq!(shutdown.instruction_count, 3);
        assert_eq!(shutdown.evidence_vm_trace.len(), 3);
        assert_eq!(shutdown.evidence_vm_trace[0].program_counter, 0);
        assert_eq!(shutdown.evidence_vm_trace[0].opcode, 11);
        assert_eq!(shutdown.evidence_vm_trace[1].program_counter, 1);
        assert_eq!(shutdown.evidence_vm_trace[1].opcode, 8);
        assert_eq!(shutdown.evidence_vm_trace[2].program_counter, 2);
        assert_eq!(shutdown.evidence_vm_trace[2].opcode, 28);
        assert!(!provider.has_active_sessions());
    }

    struct ScriptedWritableFiles {
        exchanges: std::sync::Mutex<
            std::collections::VecDeque<(
                astra_emu_family_api::LegacyWritableFileRequestV1,
                astra_emu_family_api::LegacyWritableFileResultV1,
            )>,
        >,
    }

    impl ScriptedWritableFiles {
        fn new(
            exchanges: impl IntoIterator<
                Item = (
                    astra_emu_family_api::LegacyWritableFileRequestV1,
                    astra_emu_family_api::LegacyWritableFileResultV1,
                ),
            >,
        ) -> Self {
            Self {
                exchanges: std::sync::Mutex::new(exchanges.into_iter().collect()),
            }
        }

        fn assert_consumed(&self) {
            assert!(self.exchanges.lock().unwrap().is_empty());
        }
    }

    impl astra_emu_family_api::LegacyWritableFileHostV1 for ScriptedWritableFiles {
        fn execute(
            &self,
            session_id: &str,
            request: astra_emu_family_api::LegacyWritableFileRequestV1,
        ) -> Result<astra_emu_family_api::LegacyWritableFileResultV1, LegacyProviderError> {
            assert_eq!(session_id, "session.progress");
            let (expected, result) = self
                .exchanges
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected writable-file request");
            assert_eq!(request, expected);
            Ok(result)
        }
    }

    fn writable_result(
        exists: bool,
        is_file: bool,
        length: u64,
        bytes: Vec<u8>,
        written: u64,
    ) -> astra_emu_family_api::LegacyWritableFileResultV1 {
        astra_emu_family_api::LegacyWritableFileResultV1 {
            exists,
            is_file,
            length,
            entries: Vec::new(),
            bytes: bytes.into(),
            written,
        }
    }

    #[test]
    fn global_progress_round_trips_through_synchronous_writable_file_port() {
        let script = b".end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session_id = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.progress".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let unlock = Hash256::from_sha256(b"REN_CLEAR");
        let payload = encode_global_progress(&[unlock]).unwrap();
        let writable = ScriptedWritableFiles::new([
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::Stat {
                    path: MINORI_GLOBAL_PROGRESS_PATH.into(),
                },
                writable_result(false, false, 0, Vec::new(), 0),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::CreateDir {
                    path: MINORI_GLOBAL_PROGRESS_DIRECTORY.into(),
                },
                writable_result(true, false, 0, Vec::new(), 0),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
                    path: MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH.into(),
                    length: 0,
                },
                writable_result(true, true, 0, Vec::new(), 0),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::WriteRange {
                    path: MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH.into(),
                    offset: 0,
                    bytes: payload.clone(),
                },
                writable_result(
                    true,
                    true,
                    payload.len() as u64,
                    Vec::new(),
                    payload.len() as u64,
                ),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
                    path: MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH.into(),
                    length: payload.len() as u64,
                },
                writable_result(true, true, payload.len() as u64, Vec::new(), 0),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::AtomicReplace {
                    temporary_path: MINORI_GLOBAL_PROGRESS_TEMPORARY_PATH.into(),
                    destination_path: MINORI_GLOBAL_PROGRESS_PATH.into(),
                },
                writable_result(true, true, payload.len() as u64, Vec::new(), 0),
            ),
        ]);
        let state = provider.sessions.get_mut(&session_id.0).unwrap();
        state.global_progress.enabled = true;
        state.global_progress.loaded = false;
        load_global_progress(&writable, &session_id, state).unwrap();
        assert!(state.global_progress.loaded);
        state.vm.merge_verified_gallery_unlocks(&[unlock]).unwrap();
        let mut output = LegacyStepOutput {
            status: LegacyRuntimeStatus::Active,
            live: LegacyLiveOutput::default(),
            control: LegacyControlTransaction::default(),
            trace: Vec::new(),
            diagnostics: Vec::new(),
            coverage: LegacyCoverageDelta::default(),
            state_revision: 0,
        };
        store_global_progress_if_changed(&writable, &session_id, state, &mut output).unwrap();
        assert_eq!(state.global_progress.persisted_unlocks, [unlock]);
        assert_eq!(decode_global_progress(&payload).unwrap(), [unlock]);
        writable.assert_consumed();

        let snapshot = provider.test_checkpoint(&ctx, &session_id).unwrap();
        provider
            .sessions
            .get_mut(&session_id.0)
            .unwrap()
            .global_progress
            .loaded = false;
        provider
            .restore_test_checkpoint(&ctx, &session_id, &snapshot)
            .unwrap();
        assert!(provider.sessions[&session_id.0].global_progress.loaded);
        assert_eq!(
            provider.sessions[&session_id.0]
                .global_progress
                .persisted_unlocks,
            [unlock]
        );
    }

    #[test]
    fn global_progress_is_loaded_before_a_fresh_session_executes_script() {
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                (
                    "minori:/scr/unlock.sc".into(),
                    b".setglobal REN_CLEAR = 1\r\n.end\r\n".to_vec(),
                ),
                (
                    "minori:/scr/load.sc".into(),
                    b".if REN_CLEAR == 1 loaded\r\n.setglobal SUI_CLEAR = 1\r\n.label loaded\r\n.end\r\n"
                        .to_vec(),
                ),
            ]),
        });
        let writable = Arc::new(InMemoryWritableFiles::default());
        let writable_host: Arc<dyn LegacyWritableFileHostV1> = writable.clone();
        let services = || LegacyFamilyHostServicesV9 {
            vfs: Arc::clone(&vfs),
            surfaces: Arc::new(RecordingSurfaceHost::default()),
            hooks: Arc::new(UnboundHookHost),
            writable_files: Arc::clone(&writable_host),
            system_menus: Arc::new(RecordingSystemMenuHost::default()),
            confirmations: Arc::new(RecordingConfirmationHost::default()),
            system_commands: Arc::new(RecordingSystemCommandHost::default()),
            text_inputs: Arc::new(RecordingTextInputHost::default()),
        };
        let storage_options = BTreeMap::from([(
            MINORI_GLOBAL_PROGRESS_OPTION.into(),
            MINORI_WRITABLE_FILE_BINDING_ID.into(),
        )]);
        let ctx = context();

        let mut first_provider = MinoriRuntimeProvider::with_host_services(services());
        let first_session = first_provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.progress.first".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/unlock.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: storage_options.clone(),
                },
            )
            .unwrap();
        let first = first_provider
            .step(&ctx, &first_session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(first.status, LegacyRuntimeStatus::Terminal);
        assert!(writable
            .files
            .lock()
            .unwrap()
            .contains_key(MINORI_GLOBAL_PROGRESS_PATH));
        first_provider.shutdown(&ctx, &first_session).unwrap();

        let mut second_provider = MinoriRuntimeProvider::with_host_services(services());
        let second_session = second_provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.progress.second".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/load.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: storage_options,
                },
            )
            .unwrap();
        let second = second_provider
            .step(&ctx, &second_session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(second.status, LegacyRuntimeStatus::Terminal);
        assert!(second.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.gallery_unlock_count" && mutation.value == "1"
        }));
        let state = second_provider.sessions[&second_session.0].vm.state();
        assert_eq!(state.global_variables.get("REN_CLEAR"), Some(&1));
        assert_eq!(state.global_variables.get("SUI_CLEAR"), None);
        assert_eq!(state.gallery_unlocks.len(), 1);
        second_provider.shutdown(&ctx, &second_session).unwrap();
    }

    #[test]
    fn persistent_config_round_trip_is_identity_bound_and_bounded() {
        let writable = InMemoryWritableFiles::default();
        let session_id = LegacyRuntimeSessionId("session.config".into());
        let case_fingerprint = Hash256::from_sha256(b"case");
        let package_hash = Hash256::from_sha256(b"package");
        let profile_fingerprint = Hash256::from_sha256(b"profile");
        let config = MinoriConfigState {
            bgm_volume: 37,
            text_shadow: false,
            ..MinoriConfigState::default()
        };
        let envelope = MinoriConfigEnvelope {
            schema: MINORI_CONFIG_SCHEMA.into(),
            case_fingerprint,
            package_hash,
            profile_fingerprint,
            config: config.clone(),
            quick_save_cursor: 4,
        };
        let payload = encode_config(&envelope).unwrap();
        writable
            .execute(
                &session_id.0,
                astra_emu_family_api::LegacyWritableFileRequestV1::CreateDir {
                    path: MINORI_CONFIG_ROOT.into(),
                },
            )
            .unwrap();
        writable
            .execute(
                &session_id.0,
                astra_emu_family_api::LegacyWritableFileRequestV1::WriteRange {
                    path: MINORI_CONFIG_TEMPORARY_PATH.into(),
                    offset: 0,
                    bytes: payload.clone(),
                },
            )
            .unwrap();
        writable
            .execute(
                &session_id.0,
                astra_emu_family_api::LegacyWritableFileRequestV1::AtomicReplace {
                    temporary_path: MINORI_CONFIG_TEMPORARY_PATH.into(),
                    destination_path: MINORI_CONFIG_PATH.into(),
                },
            )
            .unwrap();
        assert_eq!(
            load_persistent_config(
                &writable,
                &session_id,
                case_fingerprint,
                package_hash,
                profile_fingerprint,
            )
            .unwrap(),
            (config, 4)
        );
        assert_eq!(
            load_persistent_config(
                &writable,
                &session_id,
                Hash256::from_sha256(b"other-case"),
                package_hash,
                profile_fingerprint,
            )
            .unwrap_err()
            .code(),
            "ASTRA_EMU_MINORI_CONFIG_IDENTITY"
        );
    }

    #[test]
    fn persistent_config_rejects_an_out_of_range_quick_save_cursor() {
        let writable = InMemoryWritableFiles::default();
        let session_id = LegacyRuntimeSessionId("session.cursor".into());
        let case_fingerprint = Hash256::from_sha256(b"case");
        let package_hash = Hash256::from_sha256(b"package");
        let profile_fingerprint = Hash256::from_sha256(b"profile");
        let envelope = MinoriConfigEnvelope {
            schema: MINORI_CONFIG_SCHEMA.into(),
            case_fingerprint,
            package_hash,
            profile_fingerprint,
            config: MinoriConfigState::default(),
            quick_save_cursor: MINORI_QUICK_SAVE_SLOT_COUNT,
        };
        let payload = encode_config(&envelope).unwrap();
        writable
            .execute(
                &session_id.0,
                astra_emu_family_api::LegacyWritableFileRequestV1::CreateDir {
                    path: MINORI_CONFIG_ROOT.into(),
                },
            )
            .unwrap();
        writable
            .execute(
                &session_id.0,
                astra_emu_family_api::LegacyWritableFileRequestV1::WriteRange {
                    path: MINORI_CONFIG_TEMPORARY_PATH.into(),
                    offset: 0,
                    bytes: payload,
                },
            )
            .unwrap();
        writable
            .execute(
                &session_id.0,
                astra_emu_family_api::LegacyWritableFileRequestV1::AtomicReplace {
                    temporary_path: MINORI_CONFIG_TEMPORARY_PATH.into(),
                    destination_path: MINORI_CONFIG_PATH.into(),
                },
            )
            .unwrap();
        assert_eq!(
            load_persistent_config(
                &writable,
                &session_id,
                case_fingerprint,
                package_hash,
                profile_fingerprint,
            )
            .unwrap_err()
            .code(),
            "ASTRA_EMU_MINORI_CONFIG_CURSOR"
        );
    }

    #[test]
    fn persistent_config_store_is_atomic_and_skips_unchanged_values() {
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), b".end\r\n".to_vec())]),
        }));
        let ctx = context();
        let session_id = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.progress".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let (case_fingerprint, package_hash, profile_fingerprint, config) = {
            let session = provider.sessions.get_mut(&session_id.0).unwrap();
            session.vm.begin_title_launch().unwrap();
            session.vm.open_config().unwrap();
            session
                .vm
                .apply_config_control(MinoriConfigControl::BgmVolume(37))
                .unwrap();
            session
                .vm
                .apply_config_control(MinoriConfigControl::ToggleTextShadow)
                .unwrap();
            session
                .vm
                .apply_config_control(MinoriConfigControl::Apply)
                .unwrap();
            session.config_storage_enabled = true;
            (
                session.case_fingerprint,
                session.package_hash,
                session.profile_fingerprint,
                session.vm.persistent_config().clone(),
            )
        };
        let payload = encode_config(&MinoriConfigEnvelope {
            schema: MINORI_CONFIG_SCHEMA.into(),
            case_fingerprint,
            package_hash,
            profile_fingerprint,
            config: config.clone(),
            quick_save_cursor: 0,
        })
        .unwrap();
        let writable = ScriptedWritableFiles::new([
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::CreateDir {
                    path: MINORI_CONFIG_ROOT.into(),
                },
                writable_result(true, false, 0, Vec::new(), 0),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
                    path: MINORI_CONFIG_TEMPORARY_PATH.into(),
                    length: 0,
                },
                writable_result(true, true, 0, Vec::new(), 0),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::WriteRange {
                    path: MINORI_CONFIG_TEMPORARY_PATH.into(),
                    offset: 0,
                    bytes: payload.clone(),
                },
                writable_result(
                    true,
                    true,
                    payload.len() as u64,
                    Vec::new(),
                    payload.len() as u64,
                ),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::SetLength {
                    path: MINORI_CONFIG_TEMPORARY_PATH.into(),
                    length: payload.len() as u64,
                },
                writable_result(true, true, payload.len() as u64, Vec::new(), 0),
            ),
            (
                astra_emu_family_api::LegacyWritableFileRequestV1::AtomicReplace {
                    temporary_path: MINORI_CONFIG_TEMPORARY_PATH.into(),
                    destination_path: MINORI_CONFIG_PATH.into(),
                },
                writable_result(true, true, payload.len() as u64, Vec::new(), 0),
            ),
        ]);
        {
            let session = provider.sessions.get_mut(&session_id.0).unwrap();
            store_persistent_config_if_changed(&writable, &session_id, session).unwrap();
            assert_eq!(session.config_persisted, config);
            store_persistent_config_if_changed(&writable, &session_id, session).unwrap();
        }
        writable.assert_consumed();
    }

    #[test]
    fn provider_title_launch_uses_verified_system_assets_and_restores_page_state() {
        let mut page_png = Vec::new();
        PngEncoder::new(&mut page_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let encode_rgba = |width: u32, height: u32| {
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(
                    &vec![0; usize::try_from(width * height * 4).unwrap()],
                    width,
                    height,
                    ExtendedColorType::Rgba8,
                )
                .unwrap();
            png
        };
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), b".end\r\n".to_vec()),
                ("minori:/sys/topMenu0.png".into(), page_png.clone()),
                ("minori:/sys/configBase.png".into(), page_png.clone()),
                ("minori:/sys/knob.png".into(), encode_rgba(15, 25)),
                ("minori:/sys/checkmark.png".into(), encode_rgba(21, 32)),
                ("minori:/sys/circle.png".into(), encode_rgba(74, 74)),
                (
                    "minori:/sys/BGMTest.wav".into(),
                    b"RIFF\x04\0\0\0WAVE".to_vec(),
                ),
                ("minori:/sys/saveloadBase.png".into(), page_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.title".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        let title = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(title.status, LegacyRuntimeStatus::Active);
        assert_eq!(title.live.resource_scenes.len(), 1);
        assert_eq!(title.control.blackboard.len(), 5);
        assert!(title
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.system_page" && mutation.value == "title" }));
        assert!(title.control.blackboard.iter().any(|mutation| {
            mutation.key == LEGACY_SYSTEM_UI_ACTIVE_BLACKBOARD_KEY && mutation.value == "true"
        }));
        assert!(title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.gallery_unlock_count" && mutation.value == "0"
        }));
        assert!(title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.choice_active" && mutation.value == "false"
        }));
        assert!(title
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.play_mode" && mutation.value == "normal" }));
        assert_eq!(
            title.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/topMenu0.png"
        );
        let retained_title = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert!(retained_title.live.resource_scenes.is_empty());
        assert!(retained_title.control.blackboard.is_empty());
        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        let title_revision = title.live.resource_scenes[0].value.texture_resources[0].revision;
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );

        let config = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: "arrow_down".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: "arrow_down".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: "enter".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(
            config.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/configBase.png"
        );
        assert_eq!(
            config.live.resource_scenes[0].value.texture_resources.len(),
            4
        );
        assert_eq!(config.live.resource_scenes[0].value.draws.len(), 18);
        assert_eq!(config.control.blackboard.len(), 2);
        assert_eq!(config.control.blackboard[0].value, "config");
        assert_eq!(
            config.control.blackboard[1].key,
            LEGACY_SYSTEM_UI_ACTIVE_BLACKBOARD_KEY
        );
        assert_eq!(config.control.blackboard[1].value, "true");
        assert_ne!(
            config.live.resource_scenes[0].value.texture_resources[0].revision,
            title_revision
        );
        let audio_test = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: MINORI_POINTER_X.into(),
                            pressed: false,
                            value: 750.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_Y.into(),
                            pressed: false,
                            value: 130.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_PRIMARY.into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        assert!(matches!(
            audio_test.live.audio_commands.as_slice(),
            [
                LegacySequenced {
                    value: LegacyAudioCommandV1::LoadResource {
                        encoding: LegacyAudioEncoding::Wav,
                        resource_uri,
                        ..
                    },
                    ..
                },
                LegacySequenced {
                    value: LegacyAudioCommandV1::Play { repeat: false, .. },
                    ..
                }
            ] if resource_uri == "minori:/sys/BGMTest.wav"
        ));
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: MINORI_POINTER_PRIMARY.into(),
                        pressed: false,
                        value: 0.0,
                        sequence: 1,
                    }],
                    ..step_input(5, Vec::new())
                },
            )
            .unwrap();
        let title_after_config = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(6, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(title_after_config.status, LegacyRuntimeStatus::Active);
        assert_eq!(title_after_config.control.blackboard.len(), 2);
        assert_eq!(title_after_config.control.blackboard[0].value, "title");
        assert_eq!(
            title_after_config.control.blackboard[1].key,
            LEGACY_SYSTEM_UI_ACTIVE_BLACKBOARD_KEY
        );
        assert_eq!(title_after_config.control.blackboard[1].value, "true");
        assert_eq!(
            title_after_config.live.resource_scenes[0]
                .value
                .texture_resources[0]
                .resource_uri,
            "minori:/sys/topMenu0.png"
        );

        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
        let returned_to_title = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(returned_to_title.status, LegacyRuntimeStatus::Active);
        assert_eq!(returned_to_title.control.blackboard.len(), 6);
        assert!(returned_to_title
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.system_page" && mutation.value == "title" }));
        assert!(returned_to_title.control.blackboard.iter().any(|mutation| {
            mutation.key == LEGACY_SYSTEM_UI_ACTIVE_BLACKBOARD_KEY && mutation.value == "true"
        }));
        assert!(returned_to_title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.gallery_unlock_count" && mutation.value == "0"
        }));
        assert!(returned_to_title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.choice_active" && mutation.value == "false"
        }));
        assert!(returned_to_title
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.play_mode" && mutation.value == "normal" }));
        assert!(returned_to_title.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.route_complete" && mutation.value == "true"
        }));
        assert_eq!(returned_to_title.live.resource_scenes.len(), 1);
        assert_eq!(
            returned_to_title.live.resource_scenes[0]
                .value
                .texture_resources[0]
                .resource_uri,
            "minori:/sys/topMenu0.png"
        );
    }

    #[test]
    fn config_hit_map_clamps_sliders_and_preserves_original_action_regions() {
        assert_eq!(
            config_control_at(40, 160),
            Some(MinoriConfigControl::MessageSpeedUnread(0))
        );
        assert_eq!(
            config_control_at(151, 160),
            Some(MinoriConfigControl::MessageSpeedUnread(50))
        );
        assert_eq!(
            config_control_at(259, 160),
            Some(MinoriConfigControl::MessageSpeedUnread(100))
        );
        assert_eq!(
            config_control_at(700, 130),
            Some(MinoriConfigControl::ToggleBgmMute)
        );
        assert_eq!(
            config_control_at(750, 205),
            Some(MinoriConfigControl::TestAudio(MinoriConfigAudioBus::Voice))
        );
        assert_eq!(
            config_control_at(610, 620),
            Some(MinoriConfigControl::Apply)
        );
        assert_eq!(
            config_control_at(80, 600),
            Some(MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Auto))
        );
        assert_eq!(
            config_control_at(200, 600),
            Some(MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Skip))
        );
        assert_eq!(
            config_control_at(250, 500),
            Some(MinoriConfigControl::FontPrevious)
        );
        assert_eq!(config_control_at(0, 0), None);
    }

    #[test]
    fn config_text_shadow_uses_the_existing_typed_outline_without_a_renderer_fallback() {
        let enabled = minori_message_presentation(Some((1280, 720)), true).unwrap();
        assert_eq!(
            enabled.outline,
            Some(LegacyTextOutlineV1 {
                radius: 2,
                rgba: [0, 0, 0, 192],
            })
        );
        let disabled = minori_message_presentation(Some((1280, 720)), false).unwrap();
        assert!(disabled.outline.is_none());
        assert_eq!(disabled.body, enabled.body);
        assert_eq!(disabled.speaker, enabled.speaker);
    }

    #[test]
    fn auto_menu_rebinds_the_active_message_wait_without_a_manual_advance() {
        let mut page_png = Vec::new();
        PngEncoder::new(&mut page_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let source = b".message\r\n.end\r\n";
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), source.to_vec()),
                ("minori:/sys/topMenu0.png".into(), page_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.auto-rebind".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let message = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        let token_id = match message.control.waits.as_slice() {
            [LegacyWaitRequest::Input { token_id, .. }] => token_id.clone(),
            _ => panic!("expected the initial message input wait"),
        };
        let rebound = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: MINORI_POINTER_X.into(),
                            pressed: false,
                            value: 1125.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_Y.into(),
                            pressed: false,
                            value: 577.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_PRIMARY.into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert!(matches!(
            rebound.control.waits.as_slice(),
            [LegacyWaitRequest::Time {
                token_id: rebound_token,
                milliseconds: 500,
            }] if rebound_token == &token_id
        ));
        assert_eq!(
            provider.sessions[&session.0].vm.state().system_ui.play_mode,
            MinoriPlayMode::Auto
        );
    }

    #[test]
    fn config_volume_and_mute_are_applied_at_the_shared_audio_boundary() {
        let source = b".end\r\n";
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap(),
            7,
        )
        .unwrap();
        let mut state = MinoriVm::decode_snapshot(&vm.snapshot_bytes().unwrap()).unwrap();
        state.system_ui.config.bgm_volume = 40;
        state.audio.insert(
            0,
            crate::MinoriAudioState {
                bus: "bgm".into(),
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: "minori:/bgm/test.ogg".into(),
                looped: true,
                volume_milli: 500,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        vm.restore_state(&postcard::to_allocvec(&state).unwrap())
            .unwrap();
        let (_, mapped) = map_audio_command(
            &MinoriAudioCommand::SetParams {
                sequence: 1,
                stream_id: 0,
                volume: 0.5,
                pan: 0.0,
                repeat: true,
            },
            vm.state(),
        )
        .unwrap();
        assert!(matches!(
            mapped,
            LegacyAudioCommandV1::SetParams { volume, .. } if (volume - 0.2).abs() < f32::EPSILON
        ));
        let mut state = MinoriVm::decode_snapshot(&vm.snapshot_bytes().unwrap()).unwrap();
        state.system_ui.config.bgm_muted = true;
        vm.restore_state(&postcard::to_allocvec(&state).unwrap())
            .unwrap();
        assert_eq!(effective_audio_volume(vm.state(), 0, 0.5).unwrap(), 0.0);

        state.system_ui.config.se_volume = 25;
        state.audio.insert(
            2,
            crate::MinoriAudioState {
                bus: "se2".into(),
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: "minori:/se/test.ogg".into(),
                looped: true,
                volume_milli: 800,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        vm.restore_state(&postcard::to_allocvec(&state).unwrap())
            .unwrap();
        assert_eq!(effective_audio_volume(vm.state(), 2, 0.8).unwrap(), 0.2);
    }

    #[test]
    fn provider_exposes_original_memories_order_only_for_the_verified_title_variant() {
        let mut page_png = Vec::new();
        PngEncoder::new(&mut page_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut label_png = Vec::new();
        PngEncoder::new(&mut label_png)
            .write_image(&vec![0; 160 * 80 * 4], 160, 80, ExtendedColorType::Rgba8)
            .unwrap();
        let mut note_png = Vec::new();
        PngEncoder::new(&mut note_png)
            .write_image(&vec![0; 32 * 32 * 4], 32, 32, ExtendedColorType::Rgba8)
            .unwrap();
        let mut menu_png = Vec::new();
        PngEncoder::new(&mut menu_png)
            .write_image(&vec![0; 384 * 64 * 4], 384, 64, ExtendedColorType::Rgba8)
            .unwrap();
        let mut resources = BTreeMap::from([
            ("minori:/scr/test.sc".into(), b".end\r\n".to_vec()),
            (
                "minori:/scr/fb_ren_04.sc".into(),
                b".transition 0 * 15\r\n.stage * WHITE.png 0 0\r\n.playBGM * * 5\r\n.transition 0 * 5\r\n.stage * WHITE.png 0 0\r\n.wait 100\r\n.transition 0 * 10\r\n.stage * WHITE.png 0 0\r\n.end\r\n".to_vec(),
            ),
            ("minori:/bg/WHITE.png".into(), page_png.clone()),
            ("minori:/sys/topMenu2.png".into(), page_png.clone()),
            ("minori:/sys/memories.png".into(), page_png.clone()),
            ("minori:/sys/cgmode0.png".into(), page_png.clone()),
            ("minori:/sys/cgmode0box.png".into(), page_png.clone()),
            ("minori:/sys/cgmode0menu.png".into(), label_png.clone()),
            ("minori:/sys/flash0.png".into(), page_png.clone()),
            ("minori:/sys/flash0menu.png".into(), menu_png),
            ("minori:/sys/musicPage1.png".into(), page_png),
            ("minori:/sys/musicNote.png".into(), note_png),
        ]);
        for index in 1..=12 {
            resources.insert(
                format!("minori:/sys/cgpage{index:03}.png"),
                label_png.clone(),
            );
        }
        let mut provider =
            MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader { scripts: resources }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.memories".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .sessions
            .get_mut(&session.0)
            .unwrap()
            .vm
            .merge_verified_gallery_unlocks(&[Hash256::from_sha256(b"TOHKA_CLEAR")])
            .unwrap();

        let title = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(
            title.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/topMenu2.png"
        );
        let select_page = |tick: u64, down_count: usize| LegacyStepInput {
            input_edges: (0..down_count)
                .map(|index| LegacyInputEdge {
                    control: "arrow_down".into(),
                    pressed: true,
                    value: 1.0,
                    sequence: index as u64 + 1,
                })
                .chain(std::iter::once(LegacyInputEdge {
                    control: "enter".into(),
                    pressed: true,
                    value: 1.0,
                    sequence: down_count as u64 + 1,
                }))
                .collect(),
            ..step_input(tick, Vec::new())
        };
        let memories = provider.step(&ctx, &session, select_page(2, 3)).unwrap();
        assert_eq!(
            memories.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/memories.png"
        );
        let cg = provider.step(&ctx, &session, select_page(3, 0)).unwrap();
        assert_eq!(
            cg.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/cgmode0.png"
        );
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "escape".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        let replay = provider.step(&ctx, &session, select_page(5, 1)).unwrap();
        assert_eq!(
            replay.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/flash0.png"
        );
        provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "escape".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(6, Vec::new())
                },
            )
            .unwrap();
        let bgm = provider.step(&ctx, &session, select_page(7, 2)).unwrap();
        assert_eq!(
            bgm.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/musicPage1.png"
        );

        // A replay entry launches the verified script through the same title
        // session.  The script must consume its own bounded wait and return to
        // the title page, rather than leaving the session terminal or keeping
        // the Memories page as an implicit fallback.
        let memories_again = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "escape".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(8, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(
            memories_again.live.resource_scenes[0]
                .value
                .texture_resources[0]
                .resource_uri,
            "minori:/sys/memories.png"
        );
        let title_again = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "escape".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(9, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(
            title_again.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/topMenu2.png"
        );
        let memories_reopened = provider.step(&ctx, &session, select_page(10, 3)).unwrap();
        assert_eq!(
            memories_reopened.live.resource_scenes[0]
                .value
                .texture_resources[0]
                .resource_uri,
            "minori:/sys/memories.png"
        );
        let replay_reopened = provider.step(&ctx, &session, select_page(11, 1)).unwrap();
        assert_eq!(
            replay_reopened.live.resource_scenes[0]
                .value
                .texture_resources[0]
                .resource_uri,
            "minori:/sys/flash0.png"
        );
        let started = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(12, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(started.status, LegacyRuntimeStatus::Active);
        assert_eq!(
            provider.sessions[&session.0].vm.state().script_uri,
            "minori:/scr/fb_ren_04.sc"
        );
        provider
            .step(&ctx, &session, step_input(13, Vec::new()))
            .unwrap();
        provider
            .step(&ctx, &session, step_input(14, Vec::new()))
            .unwrap();
        let waiting = provider
            .step(&ctx, &session, step_input(15, Vec::new()))
            .unwrap();
        let token_id = match waiting.control.waits.as_slice() {
            [LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            }] => {
                assert_eq!(*milliseconds, 1000);
                token_id.clone()
            }
            other => panic!("expected replay timing wait, got {other:?}"),
        };
        let returned = provider
            .step(
                &ctx,
                &session,
                step_input(
                    16,
                    vec![LegacyAwaitResult {
                        token_id,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(returned.status, LegacyRuntimeStatus::Active);
        let returned_title = provider
            .step(&ctx, &session, step_input(17, Vec::new()))
            .unwrap();
        assert_eq!(returned_title.status, LegacyRuntimeStatus::Active);
        assert!(!provider.sessions[&session.0].vm.state().terminal);
        assert_eq!(
            provider.sessions[&session.0].vm.state().system_ui.page,
            MinoriSystemPage::Title
        );
    }

    #[test]
    fn provider_title_session_naturally_chains_all_verified_routes() {
        let mut title_png = Vec::new();
        PngEncoder::new(&mut title_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut choice_png = Vec::new();
        PngEncoder::new(&mut choice_png)
            .write_image(&vec![0; 320 * 48 * 4], 320, 48, ExtendedColorType::Rgba8)
            .unwrap();
        let choice_source = b".select ren:route_ren ayame:route_ayame sui:route_sui tohka:route_tohka\r\n.label route_ren\r\n.chain REN.sc\r\n.label route_ayame\r\n.chain AYAME.sc\r\n.label route_sui\r\n.chain SUI.sc\r\n.label route_tohka\r\n.chain TOHKA.sc\r\n";
        let routes = [
            ("REN.sc", "REN_CLEAR", 0),
            ("AYAME.sc", "AYAME_CLEAR", 0),
            ("SUI.sc", "SUI_CLEAR", 1),
            ("TOHKA.sc", "TOHKA_CLEAR", 2),
        ];
        let mut resources = BTreeMap::from([
            ("minori:/scr/K06_01.sc".into(), choice_source.to_vec()),
            ("minori:/sys/topMenu0.png".into(), title_png.clone()),
            ("minori:/sys/topMenu1.png".into(), title_png.clone()),
            ("minori:/sys/topMenu2.png".into(), title_png),
            (MINORI_CHOICE_RESOURCE_URIS[0].into(), choice_png.clone()),
            (MINORI_CHOICE_RESOURCE_URIS[1].into(), choice_png.clone()),
            (MINORI_CHOICE_RESOURCE_URIS[2].into(), choice_png),
        ]);
        for (script_name, flag, _) in routes {
            resources.insert(
                format!("minori:/scr/{script_name}"),
                format!(".setGlobal {flag} = 1\r\n.end\r\n").into_bytes(),
            );
        }
        let mut provider =
            MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader { scripts: resources }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.natural-routes".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/K06_01.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();

        let title = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(title.status, LegacyRuntimeStatus::Active);
        assert_eq!(
            title.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/topMenu0.png"
        );

        let mut tick = 1;
        for (route_index, (script_name, flag, expected_variant)) in routes.into_iter().enumerate() {
            tick += 1;
            let started = provider
                .step(
                    &ctx,
                    &session,
                    LegacyStepInput {
                        input_edges: vec![LegacyInputEdge {
                            control: "enter".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 1,
                        }],
                        ..step_input(tick, Vec::new())
                    },
                )
                .unwrap_or_else(|error| {
                    panic!("route {route_index} start at tick {tick} failed: {error:?}")
                });
            assert_eq!(started.status, LegacyRuntimeStatus::Awaiting);
            assert!(started.control.waits.iter().any(|wait| {
                matches!(wait, LegacyWaitRequest::Input { keys, .. } if *keys == vec!["enter".to_owned(), "space".to_owned()])
            }));
            assert_eq!(
                provider.sessions[&session.0]
                    .vm
                    .state()
                    .choice
                    .as_ref()
                    .map(|choice| choice.option_hashes.len()),
                Some(4)
            );

            for _ in 0..route_index {
                tick += 1;
                let moved = provider
                    .step(
                        &ctx,
                        &session,
                        LegacyStepInput {
                            input_edges: vec![LegacyInputEdge {
                                control: "arrow_down".into(),
                                pressed: true,
                                value: 1.0,
                                sequence: 1,
                            }],
                            ..step_input(tick, Vec::new())
                        },
                    )
                    .unwrap();
                assert_eq!(moved.status, LegacyRuntimeStatus::Awaiting);
            }

            tick += 1;
            let chained = provider
                .step(
                    &ctx,
                    &session,
                    LegacyStepInput {
                        input_edges: vec![LegacyInputEdge {
                            control: "enter".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 1,
                        }],
                        ..step_input(tick, Vec::new())
                    },
                )
                .unwrap();
            assert_eq!(chained.status, LegacyRuntimeStatus::Active);
            assert_eq!(chained.trace[0].action.as_deref(), Some("chain"));
            assert_eq!(
                provider.sessions[&session.0].vm.state().script_uri,
                format!("minori:/scr/{script_name}")
            );

            tick += 1;
            let returned = provider
                .step(&ctx, &session, step_input(tick, Vec::new()))
                .unwrap();
            assert_eq!(returned.status, LegacyRuntimeStatus::Active);
            assert!(returned.control.blackboard.iter().any(|mutation| {
                mutation.key == "minori.route_complete" && mutation.value == "true"
            }));
            assert_eq!(
                provider.sessions[&session.0].vm.state().system_ui.page,
                MinoriSystemPage::Title
            );
            assert!(!provider.sessions[&session.0].vm.state().terminal);
            assert_eq!(
                provider.sessions[&session.0].vm.title_variant(),
                expected_variant
            );
            assert!(provider.sessions[&session.0]
                .vm
                .state()
                .global_variables
                .contains_key(flag));
            assert_eq!(
                provider.sessions[&session.0]
                    .vm
                    .state()
                    .gallery_unlocks
                    .len(),
                route_index + 1
            );
        }
    }

    #[test]
    fn provider_completes_movie_gallery_script_through_media_fence() {
        let mut page_png = Vec::new();
        PngEncoder::new(&mut page_png)
            .write_image(
                &vec![0; 1280 * 720 * 4],
                1280,
                720,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let movie_script = b".movie 9989 ed_ayame.avi 1280 720 t\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), b".end\r\n".to_vec()),
                ("minori:/scr/fb_aya_12.sc".into(), movie_script),
                ("minori:/sys/topMenu2.png".into(), page_png.clone()),
                ("minori:/sys/memories.png".into(), page_png),
                (
                    "minori:/mov/ed_ayame.avi".into(),
                    b"RIFF-verified-fixture".to_vec(),
                ),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.movie-gallery".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                        ("astra.launch_entry_explicit".into(), "false".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .sessions
            .get_mut(&session.0)
            .unwrap()
            .vm
            .merge_verified_gallery_unlocks(&[Hash256::from_sha256(b"TOHKA_CLEAR")])
            .unwrap();

        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let enter_page = |tick: u64, down_count: usize| LegacyStepInput {
            input_edges: (0..down_count)
                .map(|index| LegacyInputEdge {
                    control: "arrow_down".into(),
                    pressed: true,
                    value: 1.0,
                    sequence: index as u64 + 1,
                })
                .chain(std::iter::once(LegacyInputEdge {
                    control: "enter".into(),
                    pressed: true,
                    value: 1.0,
                    sequence: down_count as u64 + 1,
                }))
                .collect(),
            ..step_input(tick, Vec::new())
        };
        provider.step(&ctx, &session, enter_page(2, 3)).unwrap();
        let movie_page = provider.step(&ctx, &session, enter_page(3, 3)).unwrap();
        assert_eq!(
            movie_page.live.resource_scenes[0].value.texture_resources[0].resource_uri,
            "minori:/sys/memories.png"
        );
        let started = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        let (token_id, media_id) = match started.control.waits.as_slice() {
            [LegacyWaitRequest::MediaFence { token_id, media_id }] => {
                (token_id.clone(), media_id.clone())
            }
            other => panic!("expected movie media fence, got {other:?}"),
        };
        assert_eq!(started.status, LegacyRuntimeStatus::Awaiting);
        assert!(matches!(
            started.live.video.as_slice(),
            [LegacySequenced {
                value: LegacyVideoCommandV1::Play { playback_id, .. },
                ..
            }] if playback_id == &media_id
        ));
        let completed = provider
            .step(
                &ctx,
                &session,
                step_input(
                    5,
                    vec![LegacyAwaitResult {
                        token_id,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Active);
        assert!(!provider.sessions[&session.0].vm.state().terminal);
        assert_eq!(
            provider.sessions[&session.0].vm.state().system_ui.page,
            MinoriSystemPage::Title
        );
    }

    #[test]
    fn shipping_session_does_not_collect_evidence_vm_trace() {
        let script = b".end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let mut ctx = context();
        ctx.target = "windows".into();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.shipping".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([(
                        "astra.hosted_trace_profile".into(),
                        "shipping".into(),
                    )]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let shutdown = provider.shutdown(&ctx, &session).unwrap();
        assert!(shutdown.evidence_vm_trace.is_empty());
    }

    #[test]
    fn provider_tail_chains_and_restores_the_active_script_identity() {
        let entry = b".set local = 1\r\n.chain K01.sc\r\n".to_vec();
        let next = b".wait 20\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), entry),
                ("minori:/scr/K01.sc".into(), next),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.chain".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let chained = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(chained.status, LegacyRuntimeStatus::Active);
        assert_eq!(chained.trace[0].action.as_deref(), Some("chain"));

        let waiting = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
    }

    #[test]
    fn provider_resumes_a_movie_into_a_chain_and_publishes_the_next_message_wait() {
        let entry = b".movie 9989 op.avi 1280 720 t\r\n.chain K01.sc\r\n".to_vec();
        let next = b".message 1  speaker after movie\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), entry),
                ("minori:/scr/K01.sc".into(), next),
                ("minori:/mov/op.avi".into(), b"RIFFfixture".to_vec()),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.movie-chain".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let started = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let token_id = match started.control.waits.as_slice() {
            [LegacyWaitRequest::MediaFence { token_id, .. }] => token_id.clone(),
            _ => panic!("expected the movie media fence"),
        };
        let chained = provider
            .step(
                &ctx,
                &session,
                step_input(
                    2,
                    vec![LegacyAwaitResult {
                        token_id,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(chained.status, LegacyRuntimeStatus::Active);
        assert_eq!(chained.trace[0].action.as_deref(), Some("chain"));
        let resumed = provider
            .step(&ctx, &session, step_input(3, Vec::new()))
            .unwrap();
        assert_eq!(resumed.status, LegacyRuntimeStatus::Awaiting);
        assert!(matches!(
            resumed.control.waits.as_slice(),
            [LegacyWaitRequest::Input { .. }]
        ));
    }

    #[test]
    fn provider_exposes_message_plaintext_only_through_a_one_shot_lease() {
        let script =
            b".message 42  speaker hello world\r\n.message 43  speaker second\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.message".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Awaiting);
        let presentation = &output.live.text_presentations[0];
        assert_eq!(presentation.sequence, 1);
        let lease = &output.live.text[0];
        assert_eq!(lease.sequence, 2);
        assert_eq!(presentation.value.lease_id, lease.lease_id);
        assert_eq!(lease.byte_len, 11);
        assert_eq!(lease.source_ref, "minori.sc.message");
        let presentation = &presentation.value.presentation;
        assert_eq!(presentation.layout_id, "minori.message");
        assert_eq!(presentation.language, "ja-JP");
        assert_eq!(presentation.font_families, ["Noto Sans JP"]);
        assert_eq!(presentation.body.font_size, 26.0);
        assert_eq!(presentation.body.max_lines, 3);
        let text = provider
            .take_staged_text(&ctx, &session, &lease.lease_id)
            .unwrap()
            .unwrap();
        assert_eq!(text.text, "hello world");
        assert_eq!(text.speaker.as_deref(), Some("speaker"));
        assert!(text.show_advance_indicator);
        assert!(provider
            .take_staged_text(&ctx, &session, &lease.lease_id)
            .unwrap()
            .is_none());
        assert!(matches!(
            output.control.waits.as_slice(),
            [LegacyWaitRequest::Input { keys, .. }] if *keys == message_input_keys(false)
        ));
        let wait_token = match &output.control.waits[0] {
            LegacyWaitRequest::Input { token_id, .. } => token_id.clone(),
            _ => unreachable!("message output was already verified as an input wait"),
        };

        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
        let restored = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(restored.status, LegacyRuntimeStatus::Awaiting);
        assert!(restored.live.clear_text);
        assert_eq!(restored.live.text_presentations.len(), 1);
        assert_eq!(restored.live.text.len(), 1);
        assert_eq!(restored.live.text[0].source_ref, "minori.sc.message.resume");
        let restored_text = provider
            .take_staged_text(&ctx, &session, &restored.live.text[0].lease_id)
            .unwrap()
            .unwrap();
        assert_eq!(restored_text.text, "hello world");
        assert_eq!(restored_text.speaker.as_deref(), Some("speaker"));
        assert!(restored_text.show_advance_indicator);

        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
        let continued = provider
            .step(
                &ctx,
                &session,
                step_input(
                    2,
                    vec![LegacyAwaitResult {
                        token_id: wait_token,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(continued.status, LegacyRuntimeStatus::Awaiting);
        assert_eq!(continued.live.resource_scenes.len(), 1);
        assert_eq!(continued.live.text_presentations.len(), 1);
        assert_eq!(continued.live.text.len(), 1);
    }

    #[test]
    fn provider_game_menu_click_toggles_auto_without_advancing_as_message_click() {
        let script =
            b".message 1  speaker first\r\n.message 2  speaker second\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.auto".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let first = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let initial_token_id = match &first.control.waits[0] {
            LegacyWaitRequest::Input { token_id, .. } => token_id.clone(),
            _ => panic!("expected message input wait"),
        };
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![
                        LegacyInputEdge {
                            control: MINORI_POINTER_X.into(),
                            pressed: true,
                            value: 1125.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_Y.into(),
                            pressed: true,
                            value: 577.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: MINORI_POINTER_PRIMARY.into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Awaiting);
        assert!(matches!(
            output.control.waits.as_slice(),
            [LegacyWaitRequest::Time {
                token_id: rebound_token,
                milliseconds: 500,
            }] if rebound_token == &initial_token_id
        ));
        assert!(matches!(
            provider.sessions[&session.0].vm.state().wait,
            Some(MinoriWaitState::Time {
                ref token_id,
                timer_ticks: 50,
                milliseconds: 500,
            }) if token_id == &initial_token_id
        ));
        assert_eq!(
            provider.sessions[&session.0].vm.state().system_ui.play_mode,
            MinoriPlayMode::Auto
        );
        assert!(output
            .control
            .blackboard
            .iter()
            .any(|mutation| { mutation.key == "minori.play_mode" && mutation.value == "auto" }));
    }

    #[test]
    fn eligible_control_is_owned_by_the_active_host_message_wait() {
        let script = b".pragma enable_control\r\n.message 1  speaker first\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.control-rebind".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let first = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let token_id = match first.control.waits.as_slice() {
            [LegacyWaitRequest::Input { token_id, keys }] if *keys == message_input_keys(true) => {
                token_id.clone()
            }
            _ => panic!("expected message input wait"),
        };

        let pressed = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: MINORI_CONTROL_KEY.into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(
                        2,
                        vec![LegacyAwaitResult {
                            token_id,
                            status: "completed".into(),
                            payload_len: 0,
                            sequence: 1,
                        }],
                    )
                },
            )
            .unwrap();
        assert_eq!(pressed.status, LegacyRuntimeStatus::Terminal);
        assert!(pressed.control.waits.is_empty());
    }

    #[test]
    fn provider_suspends_message_wait_for_verified_backlog_wheel_navigation() {
        let script =
            b".panel 1\r\n.message 42 voice[50,-25] speaker hello world\r\n.end\r\n".to_vec();
        let mut panel_png = Vec::new();
        PngEncoder::new(&mut panel_png)
            .write_image(&vec![255; 4 * 263], 1, 263, ExtendedColorType::Rgba8)
            .unwrap();
        let mut gauge_png = Vec::new();
        PngEncoder::new(&mut gauge_png)
            .write_image(&vec![255; 18 * 144 * 4], 18, 144, ExtendedColorType::Rgba8)
            .unwrap();
        let mut ball_png = Vec::new();
        PngEncoder::new(&mut ball_png)
            .write_image(&vec![255; 14 * 14 * 4], 14, 14, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/sys/msgPanel.png".into(), panel_png),
                ("minori:/sys/backlogGauge.png".into(), gauge_png),
                ("minori:/sys/ball.png".into(), ball_png),
                ("minori:/voice/voice".into(), b"OggSfixture".to_vec()),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.backlog".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let panel = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(panel.status, LegacyRuntimeStatus::Active);
        let message = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(message.status, LegacyRuntimeStatus::Awaiting);

        let backlog = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "wheel".into(),
                        pressed: false,
                        value: -120.0,
                        sequence: 1,
                    }],
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(backlog.status, LegacyRuntimeStatus::Active);
        assert!(backlog.live.clear_text);
        let frame = &backlog.live.resource_scenes[0].value;
        assert_eq!(frame.texture_resources.len(), 3);
        assert_eq!(
            frame
                .texture_resources
                .iter()
                .map(|resource| resource.resource_uri.as_str())
                .collect::<Vec<_>>(),
            [
                "minori:/sys/msgPanel.png",
                "minori:/sys/backlogGauge.png",
                "minori:/sys/ball.png",
            ]
        );
        assert_eq!(frame.draws.last().unwrap().vertices[0].position, [2.0, 3.0]);
        assert_eq!(backlog.live.text_presentations.len(), 1);
        assert_eq!(backlog.live.text.len(), 1);
        assert_eq!(backlog.live.text[0].source_ref, "minori.sc.backlog");
        let backlog_text = provider
            .take_staged_text(&ctx, &session, &backlog.live.text[0].lease_id)
            .unwrap()
            .unwrap();
        assert_eq!(backlog_text.text, "hello world");
        assert_eq!(backlog_text.speaker.as_deref(), Some("speaker"));
        let retained = provider
            .step(&ctx, &session, step_input(4, Vec::new()))
            .unwrap();
        assert!(!retained.live.clear_text);
        assert!(retained.live.resource_scenes.is_empty());
        assert!(retained.live.text_presentations.is_empty());
        assert!(retained.live.text.is_empty());
        let wait_before_replay = provider.sessions[&session.0].vm.state().wait.clone();
        let replay = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 2,
                    }],
                    ..step_input(5, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(replay.status, LegacyRuntimeStatus::Active);
        assert_eq!(replay.live.audio_commands.len(), 3);
        assert!(matches!(
            replay.live.audio_commands.as_slice(),
            [
                LegacySequenced {
                    value: LegacyAudioCommandV1::Stop { stream_id: 4, .. },
                    ..
                },
                LegacySequenced {
                    value: LegacyAudioCommandV1::LoadResource { stream_id: 4, resource_uri, .. },
                    ..
                },
                LegacySequenced {
                    value: LegacyAudioCommandV1::Play { stream_id: 4, volume, pan, repeat: false, .. },
                    ..
                }
            ] if resource_uri == "minori:/voice/voice" && *volume == 0.5 && *pan == -0.25
        ));
        assert_eq!(
            provider.sessions[&session.0].vm.state().wait,
            wait_before_replay
        );
        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();

        let resumed = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "wheel".into(),
                        pressed: false,
                        value: 120.0,
                        sequence: 3,
                    }],
                    ..step_input(6, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(resumed.status, LegacyRuntimeStatus::Awaiting);
        assert!(resumed.live.clear_text);
        assert_eq!(resumed.live.text_presentations.len(), 1);
        assert_eq!(resumed.live.text.len(), 1);
        assert_eq!(resumed.live.text[0].source_ref, "minori.sc.message.resume");
    }

    #[test]
    fn provider_resolves_input_waits_from_canonical_key_edges() {
        let script = b".message 42  speaker hello\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.message.input".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let first = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert!(matches!(
            first.control.waits.as_slice(),
            [LegacyWaitRequest::Input { keys, .. }]
                if keys.iter().any(|key| key == "pointer.primary")
        ));
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(2, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Terminal);
        assert!(output.control.waits.is_empty());
    }

    #[test]
    fn provider_moves_and_commits_choice_from_canonical_key_edges() {
        assert_eq!(MINORI_CHOICE_RESOURCE_URIS[0], "minori:/sys/SelectBLur.png");
        let script = b".char load 100 CH.png\r\n.select first:label1 second:label2\r\n.label label1\r\n.end\r\n.label label2\r\n.end\r\n".to_vec();
        let mut choice_png = Vec::new();
        PngEncoder::new(&mut choice_png)
            .write_image(&vec![255; 320 * 48 * 4], 320, 48, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                (MINORI_CHOICE_RESOURCE_URIS[0].into(), choice_png.clone()),
                (MINORI_CHOICE_RESOURCE_URIS[1].into(), choice_png.clone()),
                (MINORI_CHOICE_RESOURCE_URIS[2].into(), choice_png.clone()),
                ("minori:/st/CH.png".into(), choice_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.choice".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let character = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(character.status, LegacyRuntimeStatus::Active);
        let first = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(first.status, LegacyRuntimeStatus::Awaiting);
        assert!(first.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.choice_active" && mutation.value == "true"
        }));
        assert!(matches!(
            first.control.waits.as_slice(),
            [LegacyWaitRequest::Input { keys, .. }] if *keys == choice_input_keys()
        ));
        let first_presentation = first
            .control
            .events
            .iter()
            .find(|event| event.event == MINORI_CHOICE_PRESENTATION_SCHEMA)
            .expect("choice presentation event");
        let expected = choice_presentation_from_parts(
            &[
                Hash256::from_sha256(b"first"),
                Hash256::from_sha256(b"second"),
            ],
            0,
            first_presentation.sequence,
        )
        .unwrap();
        assert_eq!(first_presentation, &expected);
        assert_eq!(first.live.resource_scenes.len(), 1);
        let choice_frame = &first.live.resource_scenes.last().unwrap().value;
        let texture_ids = choice_frame
            .texture_resources
            .iter()
            .map(|resource| resource.texture_id)
            .collect::<BTreeSet<_>>();
        assert_eq!(texture_ids.len(), choice_frame.texture_resources.len());
        assert!(texture_ids.contains(&(MINORI_CHARACTER_TEXTURE_BASE + 100)));
        assert!(texture_ids.contains(&MINORI_CHOICE_TEXTURE_BASE));
        assert_eq!(first.live.text_presentations.len(), 2);
        assert_eq!(first.live.text.len(), 2);
        assert!(first.live.text_presentations.iter().all(|binding| binding
            .value
            .presentation
            .body
            .horizontal_alignment
            == LegacyTextHorizontalAlignmentV1::Center));

        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
        let restored = provider
            .step(&ctx, &session, step_input(3, Vec::new()))
            .unwrap();
        assert_eq!(restored.status, LegacyRuntimeStatus::Awaiting);
        assert!(restored.live.clear_text);
        assert_eq!(restored.live.resource_scenes.len(), 2);
        assert_eq!(restored.live.text_presentations.len(), 2);
        assert_eq!(restored.live.text.len(), 2);

        let moved = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "arrow_down".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(moved.status, LegacyRuntimeStatus::Awaiting);
        let moved_presentation = moved
            .control
            .events
            .iter()
            .find(|event| event.event == MINORI_CHOICE_PRESENTATION_SCHEMA)
            .expect("updated choice presentation event");
        let expected = choice_presentation_from_parts(
            &[
                Hash256::from_sha256(b"first"),
                Hash256::from_sha256(b"second"),
            ],
            1,
            moved_presentation.sequence,
        )
        .unwrap();
        assert_eq!(moved_presentation, &expected);
        assert_eq!(moved.live.resource_scenes.len(), 1);
        assert_eq!(moved.live.text_presentations.len(), 2);
        assert_eq!(moved.live.text.len(), 2);

        let completed = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "enter".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 2,
                    }],
                    ..step_input(5, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
        assert!(completed.control.blackboard.iter().any(|mutation| {
            mutation.key == "minori.choice_active" && mutation.value == "false"
        }));
        assert!(completed.control.waits.is_empty());
        assert!(completed.live.clear_text);
    }

    #[test]
    fn provider_control_edge_enables_bounded_timer_fast_forward() {
        let script = b".pragma enable_control\r\n.wait 500\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.control.skip".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "control".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(1, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Terminal);
        assert!(output.control.waits.is_empty());
    }

    #[test]
    fn provider_control_hold_stops_only_a_script_skippable_movie() {
        let script =
            b".pragma disable_control\r\n.movie 9989 op.avi 1280 720 t\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/mov/op.avi".into(), b"RIFFfixture".to_vec()),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.movie.skip".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let started = provider
            .step(
                &ctx,
                &session,
                LegacyStepInput {
                    input_edges: vec![LegacyInputEdge {
                        control: "control".into(),
                        pressed: true,
                        value: 1.0,
                        sequence: 1,
                    }],
                    ..step_input(1, Vec::new())
                },
            )
            .unwrap();
        let (token_id, media_id) = match started.control.waits.as_slice() {
            [LegacyWaitRequest::MediaFence { token_id, media_id }] => {
                (token_id.clone(), media_id.clone())
            }
            _ => panic!("expected media fence"),
        };
        assert!(matches!(
            started.live.video.as_slice(),
            [LegacySequenced {
                value: LegacyVideoCommandV1::Play { playback_id, .. },
                ..
            }] if playback_id == &media_id
        ));

        let stopping = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(stopping.status, LegacyRuntimeStatus::Awaiting);
        assert!(matches!(
            stopping.live.video.as_slice(),
            [LegacySequenced {
                value: LegacyVideoCommandV1::Stop { playback_id },
                ..
            }] if playback_id == &media_id
        ));

        let completed = provider
            .step(
                &ctx,
                &session,
                step_input(
                    3,
                    vec![LegacyAwaitResult {
                        token_id,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
    }

    #[test]
    fn movie_restore_rejects_non_zero_continuation_without_a_v9_seek_field() {
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/mov/op.avi".into(), b"RIFFfixture".to_vec())]),
        });
        let movie = MinoriMovieState {
            media_id: "minori.movie.1".into(),
            resource_uri: "minori:/mov/op.avi".into(),
            width: 1280,
            height: 720,
            skippable: true,
            continuation_pts: 1,
            fence_id: "minori.wait.movie.1".into(),
        };
        let error =
            movie_presentation(&vfs, "mount.test", Some((1280, 720)), &movie, 1).unwrap_err();
        assert_eq!(
            error.code(),
            "ASTRA_EMU_MINORI_MEDIA_CONTINUATION_UNSUPPORTED"
        );
    }

    #[test]
    fn provider_blocks_message_without_the_verified_reference_stage() {
        let script = b".message 42 voice speaker body\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.message.invalid-stage".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        assert_eq!(
            provider
                .step(&ctx, &session, step_input(1, Vec::new()))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY"
        );
    }

    #[test]
    fn provider_validates_and_emits_bgm_through_the_shared_audio_contract() {
        let script = b".playBGM theme.ogg * * 80\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bgm/theme.ogg".into(), b"OggSfixture".to_vec()),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.bgm".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Active);
        assert_eq!(output.coverage.audio_commands, 2);
        assert_eq!(output.live.audio_commands.len(), 2);
        let commands = output
            .live
            .audio_commands
            .iter()
            .map(|command| command.value.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            commands[0],
            LegacyAudioCommandV1::LoadResource {
                stream_id: 0,
                encoding: LegacyAudioEncoding::Ogg,
                resource_uri: "minori:/bgm/theme.ogg".into(),
            }
        );
        assert_eq!(
            commands[1],
            LegacyAudioCommandV1::Play {
                stream_id: 0,
                volume: 0.8,
                pan: 0.0,
                repeat: true,
                fade_in_ms: 2,
            }
        );

        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        let terminal = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(terminal.status, LegacyRuntimeStatus::Terminal);
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
        let restored = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(restored.status, LegacyRuntimeStatus::Terminal);
        assert_eq!(restored.coverage.audio_commands, 2);
        assert_eq!(restored.live.audio_commands.len(), 2);
        assert!(matches!(
            &restored.live.audio_commands[0].value,
            LegacyAudioCommandV1::LoadResource { stream_id: 0, .. }
        ));
        assert!(matches!(
            &restored.live.audio_commands[1].value,
            LegacyAudioCommandV1::Play {
                stream_id: 0,
                volume,
                repeat: true,
                fade_in_ms: 0,
                ..
            } if *volume == 0.8
        ));
    }

    #[test]
    fn provider_emits_a_resource_bound_stage_frame_without_decoded_pixels() {
        let script = b".transition 0 * 10\r\n.stage * BLACK.png 0 0\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BLACK.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.stage".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let frame = &output.live.resource_scenes[0].value;
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(frame.texture_resources.len(), 1);
        assert_eq!(frame.draws.len(), 1);
        assert_eq!(
            frame.texture_resources[0].resource_uri,
            "minori:/bg/BLACK.png"
        );
        assert_eq!(frame.texture_resources[0].decoded_width, 1);
        assert_eq!(frame.texture_resources[0].decoded_height, 1);
    }

    #[test]
    fn provider_maps_axis_scroll_to_time_wait_and_resource_frame_updates() {
        let script =
            b".stage * BG.png 461 0\r\n.hscroll 0 -10\r\n.endscroll 0\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.axis-scroll".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let started = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(started.trace[0].action.as_deref(), Some("axis_scroll"));

        let waiting = provider
            .step(&ctx, &session, step_input(3, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let token = match &waiting.control.waits[0] {
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => {
                assert_eq!(*milliseconds, 445);
                token_id.clone()
            }
            _ => panic!("expected axis-scroll time wait"),
        };
        let frame = &waiting
            .live
            .resource_scenes
            .last()
            .expect("expected axis-scroll presentation")
            .value;
        assert_eq!(frame.draws[0].vertices[0].position, [445.0, 0.0]);

        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        let terminal = provider
            .step(
                &ctx,
                &session,
                step_input(
                    4,
                    vec![LegacyAwaitResult {
                        token_id: token,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(terminal.status, LegacyRuntimeStatus::Terminal);
    }

    #[test]
    fn provider_centers_and_bottom_anchors_verified_static_png_stands() {
        let script = b".stage * BG.png 0 0 STAND.png 727,1685\r\n.end\r\n".to_vec();
        let mut background = Vec::new();
        PngEncoder::new(&mut background)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut stand = Vec::new();
        let stand_pixels = vec![255; 1203 * 773 * 4];
        PngEncoder::new(&mut stand)
            .write_image(&stand_pixels, 1203, 773, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), background),
                ("minori:/st/STAND.png".into(), stand),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.stage-stand".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let frame = &output
            .live
            .resource_scenes
            .last()
            .expect("expected stage presentation")
            .value;
        assert_eq!(frame.texture_resources.len(), 2);
        assert_eq!(frame.draws.len(), 2);
        assert_eq!(
            frame.texture_resources[1].resource_uri,
            "minori:/st/STAND.png"
        );
        assert_eq!(frame.draws[1].vertices[0].position, [126.0, -53.0]);
        assert_eq!(frame.draws[1].vertices[3].position, [1329.0, 720.0]);

        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
    }

    #[test]
    fn provider_composes_signed_character_slots_with_native_anchor_and_orientation() {
        let script =
            b".stage * BG.png 0 0\r\n.char load -11 CH.png\r\n.char pos -11 727 0\r\n.end\r\n"
                .to_vec();
        let mut background = Vec::new();
        PngEncoder::new(&mut background)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut character = Vec::new();
        PngEncoder::new(&mut character)
            .write_image(
                &vec![255; 100 * 200 * 4],
                100,
                200,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), background),
                ("minori:/st/CH.png".into(), character),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.character".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(3, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Active);
        assert_eq!(output.trace[0].action.as_deref(), Some("character"));
        let frame = &output
            .live
            .resource_scenes
            .last()
            .expect("expected character presentation")
            .value;
        assert_eq!(frame.texture_resources.len(), 2);
        assert_eq!(frame.draws.len(), 2);
        let draw = &frame.draws[1];
        assert_eq!(draw.texture_id, 10_011);
        assert_eq!(draw.vertices[0].position, [677.0, 520.0]);
        assert_eq!(draw.vertices[3].position, [777.0, 720.0]);
        assert_eq!(draw.vertices[0].tex_coord, [1.0, 0.0]);
        assert_eq!(draw.vertices[1].tex_coord, [0.0, 0.0]);
        assert_eq!(
            draw.scissor,
            Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            })
        );

        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
    }

    #[test]
    fn provider_emits_retained_character_frames_during_blocking_transition() {
        let script =
            b".stage * BG.png 0 0\r\n.char load 11 CH.png\r\n.char trans 11 100 0\r\n.end\r\n"
                .to_vec();
        let mut background = Vec::new();
        PngEncoder::new(&mut background)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut character = Vec::new();
        PngEncoder::new(&mut character)
            .write_image(&[255; 4 * 4 * 4], 4, 4, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), background),
                ("minori:/st/CH.png".into(), character),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.character-transition".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 50_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input_with_delta(1, 50_000_000))
            .unwrap();
        provider
            .step(&ctx, &session, step_input_with_delta(2, 50_000_000))
            .unwrap();
        let started = provider
            .step(&ctx, &session, step_input_with_delta(3, 50_000_000))
            .unwrap();
        assert_eq!(started.status, LegacyRuntimeStatus::Awaiting);
        let token_id = match &started.control.waits[0] {
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => {
                assert_eq!(*milliseconds, 100);
                token_id.clone()
            }
            _ => panic!("expected transition timer"),
        };

        let midpoint = provider
            .step(&ctx, &session, step_input_with_delta(4, 50_000_000))
            .unwrap();
        assert_eq!(midpoint.status, LegacyRuntimeStatus::Awaiting);
        let draw = midpoint.live.resource_scenes[0].value.draws.last().unwrap();
        assert_eq!(draw.vertices[0].color[3], 0.5);

        let completed = provider
            .step(
                &ctx,
                &session,
                step_input_with_delta_and_await(
                    5,
                    50_000_000,
                    vec![LegacyAwaitResult {
                        token_id,
                        status: "completed".into(),
                        payload_len: 0,
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
        assert_eq!(
            provider.sessions[&session.0].vm.state().characters[&11].opacity_256,
            0
        );
    }

    #[test]
    fn provider_crossfades_both_native_character_nodes_for_inline_load() {
        let png = |rgba: [u8; 4]| {
            let mut encoded = Vec::new();
            PngEncoder::new(&mut encoded)
                .write_image(&rgba.repeat(16), 4, 4, ExtendedColorType::Rgba8)
                .unwrap();
            encoded
        };
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/st/Old.png".into(), png([255, 0, 0, 255])),
                ("minori:/st/New.png".into(), png([0, 255, 0, 255])),
            ]),
        });
        let characters = BTreeMap::from([(
            11,
            MinoriCharacterState {
                slot_id: 11,
                positive_orientation: true,
                resource_uris: vec!["minori:/st/Old.png".into()],
                anchor_position: [640, 0],
                visible: true,
                opacity_256: 128,
                transition: None,
                replacement: Some(MinoriCharacterReplacementState {
                    resource_uri: "minori:/st/New.png".into(),
                    start_opacity_256: 256,
                    target_opacity_256: 256,
                    next_opacity_256: 128,
                    duration_ms: 40,
                    elapsed_ns: 20_000_000,
                    completed: false,
                }),
                pending_stage: false,
                keep_once: false,
            },
        )]);
        let mut resources = Vec::new();
        let mut draws = Vec::new();
        append_character_contents(
            &vfs,
            "mount.test",
            &characters,
            1280,
            720,
            &mut resources,
            &mut draws,
        )
        .unwrap();
        let current = draws
            .iter()
            .find(|draw| draw.texture_id == MINORI_CHARACTER_TEXTURE_BASE + 11)
            .unwrap();
        let replacement = draws
            .iter()
            .find(|draw| draw.texture_id == MINORI_CHARACTER_REPLACEMENT_TEXTURE_BASE + 11)
            .unwrap();
        assert_eq!(current.vertices[0].color[3], 0.5);
        assert_eq!(replacement.vertices[0].color[3], 0.5);
        assert_eq!(resources.len(), 2);
    }

    #[test]
    fn provider_stage_frames_commit_new_slots_then_retire_unmarked_previous_slots() {
        let script = b".stage * BG.png 0 0\r\n.char load 11 CH1.png\r\n.char load 12 CH2.png\r\n.char keep 11\r\n.stage * BG2.png 0 0\r\n.stage * BG3.png 0 0\r\n.end\r\n"
            .to_vec();
        let png = |rgba: [u8; 4]| {
            let mut encoded = Vec::new();
            PngEncoder::new(&mut encoded)
                .write_image(&rgba, 1, 1, ExtendedColorType::Rgba8)
                .unwrap();
            encoded
        };
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), png([0, 0, 0, 255])),
                ("minori:/bg/BG2.png".into(), png([1, 1, 1, 255])),
                ("minori:/bg/BG3.png".into(), png([2, 2, 2, 255])),
                ("minori:/st/CH1.png".into(), png([3, 3, 3, 255])),
                ("minori:/st/CH2.png".into(), png([4, 4, 4, 255])),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.character-stage-retention".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        for tick in 1..=4 {
            provider
                .step(&ctx, &session, step_input(tick, Vec::new()))
                .unwrap();
        }
        let retained = provider
            .step(&ctx, &session, step_input(5, Vec::new()))
            .unwrap();
        let retained_frame = &retained.live.resource_scenes[0].value;
        assert_eq!(retained_frame.texture_resources.len(), 3);
        assert!(retained_frame
            .texture_resources
            .iter()
            .any(|resource| resource.texture_id == MINORI_CHARACTER_TEXTURE_BASE + 11));
        assert!(retained_frame
            .texture_resources
            .iter()
            .any(|resource| resource.texture_id == MINORI_CHARACTER_TEXTURE_BASE + 12));

        let discarded = provider
            .step(&ctx, &session, step_input(6, Vec::new()))
            .unwrap();
        let discarded_frame = &discarded.live.resource_scenes[0].value;
        assert_eq!(discarded_frame.texture_resources.len(), 1);
        assert!(discarded_frame
            .texture_resources
            .iter()
            .all(|resource| resource.texture_id < MINORI_CHARACTER_TEXTURE_BASE));
    }

    #[test]
    fn scroll_xf_clips_translates_and_remaps_stage_draws() {
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let mut frame = LegacyRenderResourceFrameV1 {
            width: 4,
            height: 2,
            texture_resources: Vec::new(),
            draws: vec![LegacyDrawV1 {
                texture_id: 1,
                vertices: [
                    vertex(0.0, 0.0, 0.0, 0.0),
                    vertex(4.0, 0.0, 1.0, 0.0),
                    vertex(0.0, 2.0, 0.0, 1.0),
                    vertex(4.0, 2.0, 1.0, 1.0),
                ],
                blend: LegacyBlendMode::Alpha,
                texture_filter: LegacyTextureFilter::Linear,
                scissor: None,
            }],
        };
        let scroll = crate::MinoriScrollXfState {
            start_extent: [2, 2],
            end_extent: [2, 2],
            start_offset: [1, 0],
            end_offset: [1, 0],
            duration_ms: 1000,
            easing: 0,
            elapsed_ns: 0,
            completed: false,
            visible_extent: [2, 2],
            visible_offset: [1, 0],
        };
        apply_scroll_xf_to_frame(&mut frame, &scroll).unwrap();
        assert_eq!(frame.draws.len(), 1);
        let draw = &frame.draws[0];
        assert_eq!(draw.vertices[0].position, [0.0, 0.0]);
        assert_eq!(draw.vertices[3].position, [2.0, 2.0]);
        assert_eq!(draw.vertices[0].tex_coord, [0.25, 0.0]);
        assert_eq!(draw.vertices[3].tex_coord, [0.75, 1.0]);
        assert_eq!(
            draw.scissor,
            Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            })
        );
    }

    #[test]
    fn wscroll2_validates_sync_and_wraps_far_and_near_panoramas() {
        let vertex = |x, y, u, v| LegacyVertexV1 {
            position: [x, y],
            tex_coord: [u, v],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let resource = |texture_id, uri: &str| LegacyTextureResourceV1 {
            texture_id,
            resource_uri: uri.into(),
            codec: "png".into(),
            revision: u64::from_le_bytes(
                Hash256::from_sha256(uri.as_bytes()).as_bytes()[..8]
                    .try_into()
                    .unwrap(),
            ),
            decoded_width: 3840,
            decoded_height: 720,
            decoded_format: LegacyTextureFormat::Rgba8,
        };
        let draw = |texture_id| LegacyDrawV1 {
            texture_id,
            vertices: [
                vertex(0.0, 0.0, 0.0, 0.0),
                vertex(3840.0, 0.0, 1.0, 0.0),
                vertex(0.0, 720.0, 0.0, 1.0),
                vertex(3840.0, 720.0, 1.0, 1.0),
            ],
            blend: LegacyBlendMode::Alpha,
            texture_filter: LegacyTextureFilter::Linear,
            scissor: None,
        };
        let mut frame = LegacyRenderResourceFrameV1 {
            width: 1280,
            height: 720,
            texture_resources: vec![
                resource(0, "minori:/bg/far.png"),
                resource(1, "minori:/st/near.png"),
            ],
            draws: vec![draw(0), draw(1)],
        };
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryReader {
            scripts: BTreeMap::from([(
                "minori:/st/walk.txt".into(),
                b"13\r\n16\r\n19\r\n".to_vec(),
            )]),
        });
        let scroll = crate::MinoriWScroll2State {
            sync_resource_uri: "minori:/st/walk.txt".into(),
            period_ticks: 60,
            speed_tenths: -8,
            elapsed_ns: 166_666_667,
            elapsed_ticks: 10,
            foreground_offset: -8,
            background_offset: -1,
            background_remainder: -3,
        };
        apply_wscroll2_to_frame(&vfs, "mount.test", &mut frame, &scroll).unwrap();
        frame.validate().unwrap();
        assert_eq!(frame.draws.len(), 4);
        assert_eq!(frame.draws[0].texture_id, 0);
        assert_eq!(frame.draws[0].vertices[0].position, [0.0, 0.0]);
        assert_eq!(frame.draws[0].vertices[3].position, [1.0, 720.0]);
        assert_eq!(frame.draws[1].vertices[0].position, [1.0, 0.0]);
        assert_eq!(frame.draws[1].vertices[3].position, [1280.0, 720.0]);
        assert_eq!(frame.draws[2].texture_id, 1);

        assert!(parse_wscroll2_sync(b"; comment\r\nnot-a-number\r\n").is_err());
        assert!(parse_wscroll2_sync(b"\r\n").is_err());
    }

    #[test]
    fn provider_emits_bounded_firefly_resources_and_particle_draws() {
        let script =
            b".effect Firefly Firefly_c 3 1000\r\n.wait 20\r\n.effect end\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 255, 255, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let png_revision = u64::from_le_bytes(
            Hash256::from_sha256(&png).as_bytes()[..8]
                .try_into()
                .unwrap(),
        );
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/sys/Firefly_cS.png".into(), png.clone()),
                ("minori:/sys/Firefly_cM.png".into(), png.clone()),
                ("minori:/sys/Firefly_cL.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.firefly".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        let started = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let frame = &started.live.resource_scenes[0].value;
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(frame.texture_resources.len(), 3);
        assert!(frame.draws.is_empty());
        assert_eq!(
            frame
                .texture_resources
                .iter()
                .map(|resource| resource.resource_uri.as_str())
                .collect::<Vec<_>>(),
            [
                "minori:/sys/Firefly_cS.png",
                "minori:/sys/Firefly_cM.png",
                "minori:/sys/Firefly_cL.png",
            ]
        );
        assert!(frame.texture_resources.iter().all(|resource| {
            resource.decoded_width == 1
                && resource.decoded_height == 1
                && resource.revision != 0
                && resource.revision != png_revision
        }));
        assert_eq!(
            frame
                .texture_resources
                .iter()
                .map(|resource| resource.revision)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        let waiting = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let animated = &waiting
            .live
            .resource_scenes
            .last()
            .expect("expected animated Firefly presentation")
            .value;
        assert_eq!(animated.draws.len(), 3);
        assert!(animated.draws.iter().all(|draw| {
            draw.scissor.as_ref().is_some_and(|scissor| {
                scissor.x == 0 && scissor.y == 0 && scissor.width == 1280 && scissor.height == 720
            }) && draw.blend == LegacyBlendMode::Alpha
        }));
        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
    }

    #[test]
    fn provider_composes_bounded_snow_h_as_the_secondary_slot() {
        let script = b".effect2 SnowH\r\n.wait 20\r\n.effect2 fadeout\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 255, 255, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/sys/snowS.png".into(), png.clone()),
                ("minori:/sys/snowM.png".into(), png.clone()),
                ("minori:/sys/snowL.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.snow-h".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        let started = provider
            .step(&ctx, &session, step_input_with_delta(1, 16_000_000))
            .unwrap();
        assert_eq!(started.trace[0].action.as_deref(), Some("secondary_effect"));
        let frame = &started.live.resource_scenes[0].value;
        assert_eq!(frame.texture_resources.len(), 3);
        assert!(frame.draws.is_empty());
        assert_eq!(
            frame
                .texture_resources
                .iter()
                .map(|resource| resource.resource_uri.as_str())
                .collect::<Vec<_>>(),
            [
                "minori:/sys/snowS.png",
                "minori:/sys/snowM.png",
                "minori:/sys/snowL.png",
            ]
        );

        let waiting = provider
            .step(&ctx, &session, step_input_with_delta(2, 16_000_000))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let animated = &waiting
            .live
            .resource_scenes
            .last()
            .expect("expected animated SnowH presentation")
            .value;
        assert_eq!(animated.texture_resources.len(), 3);
        assert_eq!(animated.draws.len(), 50);
        assert!(animated.draws.iter().all(|draw| {
            (600..=602).contains(&draw.texture_id)
                && draw
                    .vertices
                    .iter()
                    .all(|vertex| vertex.color[3] == 1.0 / 256.0)
                && draw.scissor
                    == Some(LegacyScissorV1 {
                        x: 0,
                        y: 0,
                        width: 1280,
                        height: 720,
                    })
        }));

        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
    }

    #[test]
    fn provider_applies_screen_shake_after_compositing_the_stage() {
        let script =
            b".stage * BG.png 0 0\r\n.shakeScreen V 10 30\r\n.wait 20\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 255, 255, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.screen-shake".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        provider
            .step(&ctx, &session, step_input_with_delta(1, 16_000_000))
            .unwrap();
        let started = provider
            .step(&ctx, &session, step_input_with_delta(2, 16_000_000))
            .unwrap();
        assert_eq!(started.trace[0].action.as_deref(), Some("screen_shake"));
        assert_eq!(
            started.live.resource_scenes[0].value.draws[0].vertices[0].position,
            [0.0, 0.0]
        );
        provider
            .step(&ctx, &session, step_input_with_delta(3, 16_000_000))
            .unwrap();
        let animated = provider
            .step(&ctx, &session, step_input_with_delta(4, 16_000_000))
            .unwrap();
        assert_eq!(animated.status, LegacyRuntimeStatus::Awaiting);
        let frame = &animated.live.resource_scenes.last().unwrap().value;
        assert_eq!(frame.draws[0].vertices[0].position, [0.0, -10.0]);
        assert_eq!(
            frame.draws[0].scissor,
            Some(LegacyScissorV1 {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            })
        );
        let snapshot = provider.test_checkpoint(&ctx, &session).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(23, 0, 0)
        );
        provider
            .restore_test_checkpoint(&ctx, &session, &snapshot)
            .unwrap();
    }

    #[test]
    fn provider_discards_a_shake_frame_when_transition_replaces_the_slot() {
        let script =
            b".stage * BG.png 0 0\r\n.shakeScreen V 10 1\r\n.transition 0 * 0\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255, 255, 255, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.screen-shake-transition".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        provider
            .step(&ctx, &session, step_input_with_delta(1, 16_000_000))
            .unwrap();
        provider
            .step(&ctx, &session, step_input_with_delta(2, 16_000_000))
            .unwrap();
        let transitioned = provider
            .step(&ctx, &session, step_input_with_delta(3, 16_000_000))
            .unwrap();
        assert_eq!(transitioned.status, LegacyRuntimeStatus::Terminal);
        assert!(transitioned.live.resource_scenes.is_empty());
    }

    #[test]
    fn provider_records_zero_resource_crossfade2_without_presentation_fallback() {
        let script = b".effect CrossFade2\r\n.wait 20\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.effect".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 20_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let cleared = provider
            .step(&ctx, &session, step_input_with_delta(1, 20_000_000))
            .unwrap();
        assert!(cleared.live.resource_scenes.is_empty());
        assert_eq!(cleared.trace[0].action.as_deref(), Some("effect_clear"));

        let waiting = provider
            .step(&ctx, &session, step_input_with_delta(2, 20_000_000))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        assert_eq!(waiting.control.waits.len(), 1);
    }

    #[test]
    fn provider_composes_verified_message_panel_over_the_visible_effect_frame() {
        let script = b".effect CrossFade2\r\n.panel 1\r\n.end\r\n".to_vec();
        let mut panel_png = Vec::new();
        PngEncoder::new(&mut panel_png)
            .write_image(&vec![255; 4 * 263], 1, 263, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/sys/msgPanel.png".into(), panel_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.panel".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 20_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input_with_delta(1, 20_000_000))
            .unwrap();
        let panel = provider
            .step(&ctx, &session, step_input_with_delta(2, 20_000_000))
            .unwrap();
        let frame = &panel.live.resource_scenes[0].value;
        assert_eq!(frame.texture_resources.len(), 1);
        assert_eq!(frame.draws.len(), 1);
        assert_eq!(
            frame.texture_resources[0].resource_uri,
            "minori:/sys/msgPanel.png"
        );
        assert_eq!(frame.texture_resources[0].texture_id, 200);
        assert_eq!(frame.texture_resources[0].decoded_height, 263);
        assert_eq!(frame.draws[0].vertices[0].position[1], 521.0);
        assert_eq!(frame.draws[0].vertices[2].position[1], 784.0);
    }

    #[test]
    fn provider_stage_presentation_retains_active_message_panel() {
        let script = b".stage * BG.png 0 0\r\n.panel 1\r\n.stage * BG.png 0 0\r\n.end\r\n".to_vec();
        let mut background_png = Vec::new();
        PngEncoder::new(&mut background_png)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut panel_png = Vec::new();
        PngEncoder::new(&mut panel_png)
            .write_image(&vec![0; 4 * 263], 1, 263, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BG.png".into(), background_png),
                ("minori:/sys/msgPanel.png".into(), panel_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.stage-panel".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();

        provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        let stage = provider
            .step(&ctx, &session, step_input(3, Vec::new()))
            .unwrap();
        let frame = &stage.live.resource_scenes[0].value;
        assert!(frame
            .texture_resources
            .iter()
            .any(|resource| resource.resource_uri == "minori:/sys/msgPanel.png"));
        let panel_draw = frame
            .draws
            .iter()
            .find(|draw| draw.texture_id == 200)
            .expect("active message panel must be drawn over a stage update");
        assert_eq!(panel_draw.vertices[0].position[1], 521.0);
        assert_eq!(panel_draw.vertices[2].position[1], 784.0);
    }

    fn context() -> LegacyRuntimeHostCtx {
        LegacyRuntimeHostCtx {
            case_id: "case.test".into(),
            package_id: "package.test".into(),
            package_hash: Hash256::from_sha256(b"package"),
            mount_set_id: "mount.test".into(),
            media_service_ids: vec!["media.test".into()],
            permission_policy_id: "policy.test".into(),
            report_sink_id: "report.test".into(),
            target: "headless-test".into(),
            profile: "test".into(),
        }
    }

    fn step_input(tick_index: u64, await_results: Vec<LegacyAwaitResult>) -> LegacyStepInput {
        step_input_with_delta_and_await(tick_index, 16_666_667, await_results)
    }

    fn step_input_with_delta(tick_index: u64, delta_ns: u64) -> LegacyStepInput {
        step_input_with_delta_and_await(tick_index, delta_ns, Vec::new())
    }

    fn step_input_with_delta_and_await(
        tick_index: u64,
        delta_ns: u64,
        await_results: Vec<LegacyAwaitResult>,
    ) -> LegacyStepInput {
        LegacyStepInput {
            tick_index,
            delta_ns,
            session_seed: 7,
            mode: LegacyReplayMode::Live,
            input_edges: Vec::new(),
            system_menu: None,
            confirmation: None,
            system_command: None,
            text_input: None,
            await_results,
            provider_results: Vec::new(),
        }
    }
}
