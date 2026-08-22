use abi_stable::{
    std_types::{ROption, RString, RVec},
    StableAbi,
};
use astra_byte_source::{FfiOwnedByteBuffer, OwnedByteBuffer};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{FfiLegacyResult, LegacyProviderError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacySurfaceFormatV9 {
    Rgba8SrgbPremultiplied,
    Bgra8SrgbPremultiplied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyDamageRectV9 {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind", content = "rects")]
pub enum LegacySurfaceDamageV9 {
    Unchanged,
    Full,
    Rects(Vec<LegacyDamageRectV9>),
}

#[derive(Debug, PartialEq)]
pub struct LegacySurfaceLeaseV9 {
    pub lease_id: String,
    pub surface_id: String,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: LegacySurfaceFormatV9,
    pub pixels: OwnedByteBuffer,
}

#[derive(Debug, PartialEq)]
pub struct LegacySurfaceCommitV9 {
    pub lease: LegacySurfaceLeaseV9,
    pub damage: LegacySurfaceDamageV9,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyLayerTransformV9 {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub tx: f32,
    pub ty: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyLayerBlendV9 {
    Opaque,
    Alpha,
    Add,
    Multiply,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyLayerFilterV9 {
    Nearest,
    Linear,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyLayerStateV9 {
    pub layer_id: String,
    pub role: String,
    pub z_index: i32,
    pub surface_id: String,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: LegacySurfaceFormatV9,
    pub damage: LegacySurfaceDamageV9,
    pub transform: LegacyLayerTransformV9,
    pub clip: Option<LegacyDamageRectV9>,
    pub opacity: f32,
    pub texture_filter: LegacyLayerFilterV9,
    pub blend: LegacyLayerBlendV9,
    /// Explicit binding to a host-registered typed FilterGraph. The Family ABI
    /// never transports JSON, postcard, shader source, or an implicit preset.
    pub filter_graph_binding: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "operation", content = "layer")]
pub enum LegacyLayerOperationV9 {
    Create(LegacyLayerStateV9),
    Update(LegacyLayerStateV9),
    Destroy { layer_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyLayerTransactionV9 {
    pub sequence: u64,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub operations: Vec<LegacyLayerOperationV9>,
}

impl LegacyLayerTransactionV9 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        if self.viewport_width == 0 || self.viewport_height == 0 {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_LAYER_VIEWPORT",
                "layer viewport must be non-zero",
            ));
        }
        let mut touched = std::collections::BTreeSet::new();
        for operation in &self.operations {
            let (layer_id, layer) = match operation {
                LegacyLayerOperationV9::Create(layer) | LegacyLayerOperationV9::Update(layer) => {
                    (&layer.layer_id, Some(layer))
                }
                LegacyLayerOperationV9::Destroy { layer_id } => (layer_id, None),
            };
            validate_symbol_v9("layer_id", layer_id)?;
            if !touched.insert(layer_id.as_str()) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_LAYER_DUPLICATE_OPERATION",
                    "one transaction may touch a layer only once",
                ));
            }
            let Some(layer) = layer else { continue };
            validate_symbol_v9("layer_role", &layer.role)?;
            validate_symbol_v9("surface_id", &layer.surface_id)?;
            if layer.width == 0
                || layer.height == 0
                || layer.stride
                    < layer.width.checked_mul(4).ok_or_else(|| {
                        LegacyProviderError::invalid(
                            "ASTRA_EMU_LAYER_STRIDE",
                            "layer row size overflow",
                        )
                    })?
                || !layer.opacity.is_finite()
                || !(0.0..=1.0).contains(&layer.opacity)
                || [
                    layer.transform.m11,
                    layer.transform.m12,
                    layer.transform.m21,
                    layer.transform.m22,
                    layer.transform.tx,
                    layer.transform.ty,
                ]
                .into_iter()
                .any(|value| !value.is_finite())
            {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_LAYER_STATE",
                    "layer dimensions, stride, opacity, or transform are invalid",
                ));
            }
            validate_damage_v9(&layer.damage, layer.width, layer.height)?;
            if let Some(binding) = &layer.filter_graph_binding {
                validate_symbol_v9("filter_graph_binding", binding)?;
            }
        }
        Ok(())
    }
}

fn validate_damage_v9(
    damage: &LegacySurfaceDamageV9,
    width: u32,
    height: u32,
) -> Result<(), LegacyProviderError> {
    let LegacySurfaceDamageV9::Rects(rects) = damage else {
        return Ok(());
    };
    if rects.is_empty() {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_LAYER_DAMAGE",
            "rect damage must contain at least one rectangle",
        ));
    }
    for rect in rects {
        if rect.width == 0
            || rect.height == 0
            || rect
                .x
                .checked_add(rect.width)
                .is_none_or(|right| right > width)
            || rect
                .y
                .checked_add(rect.height)
                .is_none_or(|bottom| bottom > height)
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_LAYER_DAMAGE",
                "damage rectangle is outside its surface",
            ));
        }
    }
    Ok(())
}

fn validate_symbol_v9(field: &str, value: &str) -> Result<(), LegacyProviderError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_LAYER_SYMBOL",
            format!("{field} is not a valid stable symbol"),
        ));
    }
    Ok(())
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, StableAbi)]
pub struct FfiLayerTransformV9 {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub tx: f32,
    pub ty: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLayerBlendV9 {
    Opaque,
    Alpha,
    Add,
    Multiply,
    Screen,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiLayerFilterV9 {
    Nearest,
    Linear,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct FfiLayerStateV9 {
    pub layer_id: RString,
    pub role: RString,
    pub z_index: i32,
    pub surface_id: RString,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: FfiSurfaceFormatV9,
    pub damage: FfiSurfaceDamageV9,
    pub transform: FfiLayerTransformV9,
    pub clip: ROption<FfiDamageRectV9>,
    pub opacity: f32,
    pub texture_filter: FfiLayerFilterV9,
    pub blend: FfiLayerBlendV9,
    pub filter_graph_binding: ROption<RString>,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub enum FfiLayerOperationV9 {
    Create(FfiLayerStateV9),
    Update(FfiLayerStateV9),
    Destroy { layer_id: RString },
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi)]
pub struct FfiLayerTransactionV9 {
    pub sequence: u64,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub operations: RVec<FfiLayerOperationV9>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyHookInvocationV1 {
    pub session_id: String,
    pub invocation_id: String,
    pub family_id: String,
    pub family_game_id: String,
    pub hook_id: String,
    pub timeout_ms: u32,
    pub payload: OwnedByteBuffer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyHookStatusV1 {
    Completed,
    Unbound,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyHookResultV1 {
    pub status: LegacyHookStatusV1,
    pub payload: OwnedByteBuffer,
    pub diagnostics: Vec<crate::LegacyDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub enum LegacyWritableFileRequestV1 {
    Stat {
        path: String,
    },
    List {
        path: String,
    },
    CreateDir {
        path: String,
    },
    ReadRange {
        path: String,
        offset: u64,
        length: u64,
    },
    WriteRange {
        path: String,
        offset: u64,
        bytes: Vec<u8>,
    },
    SetLength {
        path: String,
        length: u64,
    },
    Remove {
        path: String,
    },
    AtomicReplace {
        temporary_path: String,
        destination_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyWritableFileEntryV1 {
    pub name: String,
    pub is_file: bool,
    pub length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyWritableFileResultV1 {
    pub exists: bool,
    pub is_file: bool,
    pub length: u64,
    pub entries: Vec<LegacyWritableFileEntryV1>,
    pub bytes: OwnedByteBuffer,
    pub written: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiSurfaceFormatV9 {
    Rgba8SrgbPremultiplied,
    Bgra8SrgbPremultiplied,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FfiDamageRectV9 {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub enum FfiSurfaceDamageV9 {
    Unchanged,
    Full,
    Rects(RVec<FfiDamageRectV9>),
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiSurfaceLeaseV9 {
    pub lease_id: RString,
    pub surface_id: RString,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: FfiSurfaceFormatV9,
    pub pixels: FfiOwnedByteBuffer,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiAcquireSurfaceCallV9 {
    pub host_token: RString,
    pub session_id: RString,
    pub fixed_step: u64,
    pub surface_id: RString,
    pub width: u32,
    pub height: u32,
    pub format: FfiSurfaceFormatV9,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiCommitSurfaceCallV9 {
    pub host_token: RString,
    pub session_id: RString,
    pub fixed_step: u64,
    pub lease: FfiSurfaceLeaseV9,
    pub damage: FfiSurfaceDamageV9,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiHookInvocationV1 {
    pub host_token: RString,
    pub session_id: RString,
    pub invocation_id: RString,
    pub family_id: RString,
    pub family_game_id: RString,
    pub hook_id: RString,
    pub timeout_ms: u32,
    pub payload: FfiOwnedByteBuffer,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FfiHookStatusV1 {
    Completed,
    Unbound,
    TimedOut,
    Failed,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiHookResultV1 {
    pub status: FfiHookStatusV1,
    pub payload: FfiOwnedByteBuffer,
    pub diagnostics: RVec<crate::FfiDiagnostic>,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub enum FfiWritableFileRequestV1 {
    Stat {
        path: RString,
    },
    List {
        path: RString,
    },
    CreateDir {
        path: RString,
    },
    ReadRange {
        path: RString,
        offset: u64,
        length: u64,
    },
    WriteRange {
        path: RString,
        offset: u64,
        bytes: FfiOwnedByteBuffer,
    },
    SetLength {
        path: RString,
        length: u64,
    },
    Remove {
        path: RString,
    },
    AtomicReplace {
        temporary_path: RString,
        destination_path: RString,
    },
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiWritableFileCallV1 {
    pub host_token: RString,
    pub session_id: RString,
    pub request: FfiWritableFileRequestV1,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, StableAbi)]
pub struct FfiWritableFileEntryV1 {
    pub name: RString,
    pub is_file: bool,
    pub length: u64,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct FfiWritableFileResultV1 {
    pub exists: bool,
    pub is_file: bool,
    pub length: u64,
    pub entries: RVec<FfiWritableFileEntryV1>,
    pub bytes: FfiOwnedByteBuffer,
    pub written: u64,
}

pub type FfiAcquireSurfaceV9 =
    extern "C" fn(FfiAcquireSurfaceCallV9) -> FfiLegacyResult<FfiSurfaceLeaseV9>;
pub type FfiCommitSurfaceV9 = extern "C" fn(FfiCommitSurfaceCallV9) -> FfiLegacyResult<()>;
pub type FfiInvokeHookV1 = extern "C" fn(FfiHookInvocationV1) -> FfiLegacyResult<FfiHookResultV1>;
pub type FfiWritableFileV1 =
    extern "C" fn(FfiWritableFileCallV1) -> FfiLegacyResult<FfiWritableFileResultV1>;

impl LegacySurfaceLeaseV9 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        validate_symbol_v9("surface_lease_id", &self.lease_id)?;
        validate_symbol_v9("surface_id", &self.surface_id)?;
        let row = self.width.checked_mul(4).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_SURFACE_STRIDE", "surface row overflow")
        })?;
        let expected = u64::from(self.stride)
            .checked_mul(u64::from(self.height))
            .ok_or_else(|| {
                LegacyProviderError::invalid("ASTRA_EMU_SURFACE_SIZE", "surface size overflow")
            })?;
        if self.width == 0
            || self.height == 0
            || self.generation == 0
            || self.stride < row
            || u64::try_from(self.pixels.len()).ok() != Some(expected)
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_SURFACE_SIZE",
                "surface lease dimensions, stride, and byte length disagree",
            ));
        }
        Ok(())
    }
}

impl LegacySurfaceCommitV9 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        self.lease.validate()?;
        validate_damage_v9(&self.damage, self.lease.width, self.lease.height)
    }
}

impl LegacyWritableFileRequestV1 {
    pub fn validate(&self) -> Result<(), LegacyProviderError> {
        match self {
            Self::Stat { path }
            | Self::List { path }
            | Self::CreateDir { path }
            | Self::Remove { path } => validate_relative_writable_path(path),
            Self::ReadRange {
                path,
                offset,
                length,
            } => {
                validate_relative_writable_path(path)?;
                offset.checked_add(*length).ok_or_else(|| {
                    LegacyProviderError::invalid(
                        "ASTRA_EMU_WRITABLE_RANGE",
                        "read range overflows u64",
                    )
                })?;
                Ok(())
            }
            Self::WriteRange {
                path,
                offset,
                bytes,
            } => {
                validate_relative_writable_path(path)?;
                let length = u64::try_from(bytes.len()).map_err(|_| {
                    LegacyProviderError::invalid(
                        "ASTRA_EMU_WRITABLE_RANGE",
                        "write byte length does not fit u64",
                    )
                })?;
                offset.checked_add(length).ok_or_else(|| {
                    LegacyProviderError::invalid(
                        "ASTRA_EMU_WRITABLE_RANGE",
                        "write range overflows u64",
                    )
                })?;
                Ok(())
            }
            Self::SetLength { path, .. } => validate_relative_writable_path(path),
            Self::AtomicReplace {
                temporary_path,
                destination_path,
            } => {
                validate_relative_writable_path(temporary_path)?;
                validate_relative_writable_path(destination_path)?;
                if temporary_path == destination_path {
                    return Err(LegacyProviderError::invalid(
                        "ASTRA_EMU_WRITABLE_ATOMIC_REPLACE",
                        "temporary and destination paths must differ",
                    ));
                }
                Ok(())
            }
        }
    }
}

pub fn validate_relative_writable_path(path: &str) -> Result<(), LegacyProviderError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains(':')
        || path
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_WRITABLE_PATH",
            "writable file paths must be normalized safe relative paths",
        ));
    }
    Ok(())
}

pub fn ffi_optional_string(value: Option<String>) -> ROption<RString> {
    value.map(Into::into).into()
}

fn ffi_surface_format(value: LegacySurfaceFormatV9) -> FfiSurfaceFormatV9 {
    match value {
        LegacySurfaceFormatV9::Rgba8SrgbPremultiplied => FfiSurfaceFormatV9::Rgba8SrgbPremultiplied,
        LegacySurfaceFormatV9::Bgra8SrgbPremultiplied => FfiSurfaceFormatV9::Bgra8SrgbPremultiplied,
    }
}

fn legacy_surface_format(value: FfiSurfaceFormatV9) -> LegacySurfaceFormatV9 {
    match value {
        FfiSurfaceFormatV9::Rgba8SrgbPremultiplied => LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
        FfiSurfaceFormatV9::Bgra8SrgbPremultiplied => LegacySurfaceFormatV9::Bgra8SrgbPremultiplied,
    }
}

fn ffi_damage_rect(value: LegacyDamageRectV9) -> FfiDamageRectV9 {
    FfiDamageRectV9 {
        x: value.x,
        y: value.y,
        width: value.width,
        height: value.height,
    }
}

fn legacy_damage_rect(value: FfiDamageRectV9) -> LegacyDamageRectV9 {
    LegacyDamageRectV9 {
        x: value.x,
        y: value.y,
        width: value.width,
        height: value.height,
    }
}

fn ffi_damage(value: LegacySurfaceDamageV9) -> FfiSurfaceDamageV9 {
    match value {
        LegacySurfaceDamageV9::Unchanged => FfiSurfaceDamageV9::Unchanged,
        LegacySurfaceDamageV9::Full => FfiSurfaceDamageV9::Full,
        LegacySurfaceDamageV9::Rects(rects) => FfiSurfaceDamageV9::Rects(
            rects
                .into_iter()
                .map(ffi_damage_rect)
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}

fn legacy_damage(value: FfiSurfaceDamageV9) -> LegacySurfaceDamageV9 {
    match value {
        FfiSurfaceDamageV9::Unchanged => LegacySurfaceDamageV9::Unchanged,
        FfiSurfaceDamageV9::Full => LegacySurfaceDamageV9::Full,
        FfiSurfaceDamageV9::Rects(rects) => {
            LegacySurfaceDamageV9::Rects(rects.into_iter().map(legacy_damage_rect).collect())
        }
    }
}

fn ffi_layer_state(value: LegacyLayerStateV9) -> FfiLayerStateV9 {
    FfiLayerStateV9 {
        layer_id: value.layer_id.into(),
        role: value.role.into(),
        z_index: value.z_index,
        surface_id: value.surface_id.into(),
        generation: value.generation,
        width: value.width,
        height: value.height,
        stride: value.stride,
        format: ffi_surface_format(value.format),
        damage: ffi_damage(value.damage),
        transform: FfiLayerTransformV9 {
            m11: value.transform.m11,
            m12: value.transform.m12,
            m21: value.transform.m21,
            m22: value.transform.m22,
            tx: value.transform.tx,
            ty: value.transform.ty,
        },
        clip: value.clip.map(ffi_damage_rect).into(),
        opacity: value.opacity,
        texture_filter: match value.texture_filter {
            LegacyLayerFilterV9::Nearest => FfiLayerFilterV9::Nearest,
            LegacyLayerFilterV9::Linear => FfiLayerFilterV9::Linear,
        },
        blend: match value.blend {
            LegacyLayerBlendV9::Opaque => FfiLayerBlendV9::Opaque,
            LegacyLayerBlendV9::Alpha => FfiLayerBlendV9::Alpha,
            LegacyLayerBlendV9::Add => FfiLayerBlendV9::Add,
            LegacyLayerBlendV9::Multiply => FfiLayerBlendV9::Multiply,
            LegacyLayerBlendV9::Screen => FfiLayerBlendV9::Screen,
        },
        filter_graph_binding: value.filter_graph_binding.map(Into::into).into(),
    }
}

fn legacy_layer_state(value: FfiLayerStateV9) -> LegacyLayerStateV9 {
    LegacyLayerStateV9 {
        layer_id: value.layer_id.to_string(),
        role: value.role.to_string(),
        z_index: value.z_index,
        surface_id: value.surface_id.to_string(),
        generation: value.generation,
        width: value.width,
        height: value.height,
        stride: value.stride,
        format: legacy_surface_format(value.format),
        damage: legacy_damage(value.damage),
        transform: LegacyLayerTransformV9 {
            m11: value.transform.m11,
            m12: value.transform.m12,
            m21: value.transform.m21,
            m22: value.transform.m22,
            tx: value.transform.tx,
            ty: value.transform.ty,
        },
        clip: value.clip.into_option().map(legacy_damage_rect),
        opacity: value.opacity,
        texture_filter: match value.texture_filter {
            FfiLayerFilterV9::Nearest => LegacyLayerFilterV9::Nearest,
            FfiLayerFilterV9::Linear => LegacyLayerFilterV9::Linear,
        },
        blend: match value.blend {
            FfiLayerBlendV9::Opaque => LegacyLayerBlendV9::Opaque,
            FfiLayerBlendV9::Alpha => LegacyLayerBlendV9::Alpha,
            FfiLayerBlendV9::Add => LegacyLayerBlendV9::Add,
            FfiLayerBlendV9::Multiply => LegacyLayerBlendV9::Multiply,
            FfiLayerBlendV9::Screen => LegacyLayerBlendV9::Screen,
        },
        filter_graph_binding: value
            .filter_graph_binding
            .into_option()
            .map(|value| value.to_string()),
    }
}

pub(crate) fn ffi_layer_transaction(value: LegacyLayerTransactionV9) -> FfiLayerTransactionV9 {
    FfiLayerTransactionV9 {
        sequence: value.sequence,
        viewport_width: value.viewport_width,
        viewport_height: value.viewport_height,
        operations: value
            .operations
            .into_iter()
            .map(|operation| match operation {
                LegacyLayerOperationV9::Create(layer) => {
                    FfiLayerOperationV9::Create(ffi_layer_state(layer))
                }
                LegacyLayerOperationV9::Update(layer) => {
                    FfiLayerOperationV9::Update(ffi_layer_state(layer))
                }
                LegacyLayerOperationV9::Destroy { layer_id } => FfiLayerOperationV9::Destroy {
                    layer_id: layer_id.into(),
                },
            })
            .collect::<Vec<_>>()
            .into(),
    }
}

pub(crate) fn legacy_layer_transaction(value: FfiLayerTransactionV9) -> LegacyLayerTransactionV9 {
    LegacyLayerTransactionV9 {
        sequence: value.sequence,
        viewport_width: value.viewport_width,
        viewport_height: value.viewport_height,
        operations: value
            .operations
            .into_iter()
            .map(|operation| match operation {
                FfiLayerOperationV9::Create(layer) => {
                    LegacyLayerOperationV9::Create(legacy_layer_state(layer))
                }
                FfiLayerOperationV9::Update(layer) => {
                    LegacyLayerOperationV9::Update(legacy_layer_state(layer))
                }
                FfiLayerOperationV9::Destroy { layer_id } => LegacyLayerOperationV9::Destroy {
                    layer_id: layer_id.to_string(),
                },
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(damage: LegacySurfaceDamageV9) -> LegacyLayerStateV9 {
        LegacyLayerStateV9 {
            layer_id: "main".into(),
            role: "content".into(),
            z_index: 0,
            surface_id: "surface.main".into(),
            generation: 1,
            width: 4,
            height: 4,
            stride: 16,
            format: LegacySurfaceFormatV9::Bgra8SrgbPremultiplied,
            damage,
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
            texture_filter: LegacyLayerFilterV9::Nearest,
            blend: LegacyLayerBlendV9::Alpha,
            filter_graph_binding: Some("filter.main".into()),
        }
    }

    #[test]
    fn layer_transaction_round_trips_across_v9_wire() {
        let transaction = LegacyLayerTransactionV9 {
            sequence: 9,
            viewport_width: 800,
            viewport_height: 600,
            operations: vec![LegacyLayerOperationV9::Create(layer(
                LegacySurfaceDamageV9::Rects(vec![LegacyDamageRectV9 {
                    x: 1,
                    y: 1,
                    width: 2,
                    height: 2,
                }]),
            ))],
        };
        transaction.validate().unwrap();
        let decoded = legacy_layer_transaction(ffi_layer_transaction(transaction.clone()));
        assert_eq!(decoded, transaction);
    }

    #[test]
    fn surface_validation_checks_exact_owned_buffer_and_damage() {
        let lease = LegacySurfaceLeaseV9 {
            lease_id: "lease.main".into(),
            surface_id: "surface.main".into(),
            generation: 1,
            width: 4,
            height: 4,
            stride: 16,
            format: LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            pixels: vec![0; 64].into(),
        };
        LegacySurfaceCommitV9 {
            lease,
            damage: LegacySurfaceDamageV9::Full,
        }
        .validate()
        .unwrap();

        let invalid = LegacySurfaceLeaseV9 {
            lease_id: "lease.main".into(),
            surface_id: "surface.main".into(),
            generation: 1,
            width: 4,
            height: 4,
            stride: 16,
            format: LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            pixels: vec![0; 63].into(),
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn writable_file_requests_reject_escape_and_overflow() {
        for path in ["../save.dat", "/save.dat", "C:/save.dat", "save//slot.dat"] {
            assert!(LegacyWritableFileRequestV1::Stat { path: path.into() }
                .validate()
                .is_err());
        }
        assert!(LegacyWritableFileRequestV1::ReadRange {
            path: "save/slot.dat".into(),
            offset: u64::MAX,
            length: 1,
        }
        .validate()
        .is_err());
        LegacyWritableFileRequestV1::AtomicReplace {
            temporary_path: "save/slot.tmp".into(),
            destination_path: "save/slot.dat".into(),
        }
        .validate()
        .unwrap();
    }
}
