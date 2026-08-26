use std::collections::{BTreeMap, BTreeSet};

use astra_core::is_safe_symbol;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BlendMode, FilterGraph, MediaError, RectI, TextureFilter2D, Transform2D};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct Layer2DId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct Surface2DId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct Layer2DRole(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Surface2DFormat {
    Rgba8SrgbPremultiplied,
    Bgra8SrgbPremultiplied,
}

impl Surface2DFormat {
    pub const fn bytes_per_pixel(self) -> u32 {
        4
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Layer2DDamage {
    Unchanged,
    Full,
    Rects(Vec<RectI>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WritableSurface2DRef {
    pub surface_id: Surface2DId,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: Surface2DFormat,
    pub damage: Layer2DDamage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextureResource2DRef {
    pub resource_id: String,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Layer2DContent {
    WritableSurface(WritableSurface2DRef),
    TextureResource(TextureResource2DRef),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Layer2DState {
    pub id: Layer2DId,
    pub role: Layer2DRole,
    pub z_index: i32,
    pub content: Layer2DContent,
    pub transform: Transform2D,
    pub clip: Option<RectI>,
    pub opacity: f32,
    pub texture_filter: TextureFilter2D,
    pub blend: BlendMode,
    pub filter_graph: Option<FilterGraph>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "operation", content = "value")]
pub enum Layer2DOperation {
    Create(Layer2DState),
    Update(Layer2DState),
    Destroy(Layer2DId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Layer2DTransaction {
    pub sequence: u64,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub operations: Vec<Layer2DOperation>,
}

#[derive(Debug, Clone, Default)]
pub struct RetainedLayer2DState {
    layers: BTreeMap<Layer2DId, Layer2DState>,
    surface_generations: BTreeMap<Surface2DId, u64>,
    last_sequence: Option<u64>,
}

impl RetainedLayer2DState {
    pub fn apply(
        &mut self,
        transaction: &Layer2DTransaction,
    ) -> Result<Vec<Layer2DState>, MediaError> {
        if transaction.viewport_width == 0 || transaction.viewport_height == 0 {
            return Err(MediaError::message("Layer2D viewport must be non-zero"));
        }
        if self
            .last_sequence
            .is_some_and(|sequence| transaction.sequence <= sequence)
        {
            return Err(MediaError::message(
                "Layer2D transaction sequence must increase",
            ));
        }
        let mut next = self.clone();
        let mut touched = BTreeSet::new();
        for operation in &transaction.operations {
            let id = match operation {
                Layer2DOperation::Create(layer) | Layer2DOperation::Update(layer) => &layer.id,
                Layer2DOperation::Destroy(id) => id,
            };
            validate_symbol("layer_id", &id.0)?;
            if !touched.insert(id.clone()) {
                return Err(MediaError::message(
                    "Layer2D transaction touches one layer more than once",
                ));
            }
            match operation {
                Layer2DOperation::Create(layer) => {
                    if next.layers.contains_key(&layer.id) {
                        return Err(MediaError::message("Layer2D create duplicates a layer"));
                    }
                    validate_layer(
                        layer,
                        &next.surface_generations,
                        transaction.viewport_width,
                        transaction.viewport_height,
                    )?;
                    remember_surface_generation(layer, &mut next.surface_generations);
                    next.layers.insert(layer.id.clone(), layer.clone());
                }
                Layer2DOperation::Update(layer) => {
                    if !next.layers.contains_key(&layer.id) {
                        return Err(MediaError::message(
                            "Layer2D update references an unknown layer",
                        ));
                    }
                    validate_layer(
                        layer,
                        &next.surface_generations,
                        transaction.viewport_width,
                        transaction.viewport_height,
                    )?;
                    remember_surface_generation(layer, &mut next.surface_generations);
                    next.layers.insert(layer.id.clone(), layer.clone());
                }
                Layer2DOperation::Destroy(id) => {
                    if next.layers.remove(id).is_none() {
                        return Err(MediaError::message(
                            "Layer2D destroy references an unknown layer",
                        ));
                    }
                }
            }
        }
        let mut ordered = next.layers.values().cloned().collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            left.z_index
                .cmp(&right.z_index)
                .then_with(|| left.id.cmp(&right.id))
        });
        next.last_sequence = Some(transaction.sequence);
        *self = next;
        Ok(ordered)
    }

    pub fn layers(&self) -> &BTreeMap<Layer2DId, Layer2DState> {
        &self.layers
    }
}

fn validate_layer(
    layer: &Layer2DState,
    generations: &BTreeMap<Surface2DId, u64>,
    viewport_width: u32,
    viewport_height: u32,
) -> Result<(), MediaError> {
    validate_symbol("layer_role", &layer.role.0)?;
    if !layer.opacity.is_finite() || !(0.0..=1.0).contains(&layer.opacity) {
        return Err(MediaError::message(
            "Layer2D opacity must be finite and in 0..=1",
        ));
    }
    for value in [
        layer.transform.m11,
        layer.transform.m12,
        layer.transform.m21,
        layer.transform.m22,
        layer.transform.tx,
        layer.transform.ty,
    ] {
        if !value.is_finite() {
            return Err(MediaError::message("Layer2D transform must be finite"));
        }
    }
    if let Some(clip) = layer.clip {
        validate_rect("clip", clip, viewport_width, viewport_height)?;
    }
    match &layer.content {
        Layer2DContent::WritableSurface(surface) => validate_surface(surface, generations),
        Layer2DContent::TextureResource(resource) => {
            validate_symbol("resource_id", &resource.resource_id)?;
            if resource.width == 0 || resource.height == 0 {
                return Err(MediaError::message(
                    "Layer2D texture dimensions must be non-zero",
                ));
            }
            Ok(())
        }
    }
}

fn validate_surface(
    surface: &WritableSurface2DRef,
    generations: &BTreeMap<Surface2DId, u64>,
) -> Result<(), MediaError> {
    validate_symbol("surface_id", &surface.surface_id.0)?;
    if surface.width == 0 || surface.height == 0 {
        return Err(MediaError::message(
            "Layer2D surface dimensions must be non-zero",
        ));
    }
    let minimum_stride = surface
        .width
        .checked_mul(surface.format.bytes_per_pixel())
        .ok_or_else(|| MediaError::message("Layer2D surface stride overflow"))?;
    if surface.stride < minimum_stride {
        return Err(MediaError::message(
            "Layer2D surface stride is smaller than one row",
        ));
    }
    if generations
        .get(&surface.surface_id)
        .is_some_and(|generation| surface.generation < *generation)
    {
        return Err(MediaError::message(
            "Layer2D surface generation moved backwards",
        ));
    }
    match &surface.damage {
        Layer2DDamage::Unchanged | Layer2DDamage::Full => {}
        Layer2DDamage::Rects(rects) => {
            if rects.is_empty() {
                return Err(MediaError::message("Layer2D rect damage must not be empty"));
            }
            for rect in rects {
                validate_rect("damage", *rect, surface.width, surface.height)?;
            }
        }
    }
    Ok(())
}

fn validate_rect(field: &str, rect: RectI, width: u32, height: u32) -> Result<(), MediaError> {
    let right = i64::from(rect.x)
        .checked_add(i64::from(rect.width))
        .ok_or_else(|| MediaError::message(format!("Layer2D {field} x overflow")))?;
    let bottom = i64::from(rect.y)
        .checked_add(i64::from(rect.height))
        .ok_or_else(|| MediaError::message(format!("Layer2D {field} y overflow")))?;
    if rect.x < 0
        || rect.y < 0
        || rect.width == 0
        || rect.height == 0
        || right > i64::from(width)
        || bottom > i64::from(height)
    {
        return Err(MediaError::message(format!(
            "Layer2D {field} is outside its coordinate space"
        )));
    }
    Ok(())
}

fn remember_surface_generation(layer: &Layer2DState, generations: &mut BTreeMap<Surface2DId, u64>) {
    if let Layer2DContent::WritableSurface(surface) = &layer.content {
        generations.insert(surface.surface_id.clone(), surface.generation);
    }
}

fn validate_symbol(field: &str, value: &str) -> Result<(), MediaError> {
    if !is_safe_symbol(value) {
        return Err(MediaError::message(format!("Layer2D {field} is invalid")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(generation: u64, damage: Layer2DDamage) -> Layer2DState {
        Layer2DState {
            id: Layer2DId("content".into()),
            role: Layer2DRole("content".into()),
            z_index: 0,
            content: Layer2DContent::WritableSurface(WritableSurface2DRef {
                surface_id: Surface2DId("surface.content".into()),
                generation,
                width: 4,
                height: 4,
                stride: 16,
                format: Surface2DFormat::Rgba8SrgbPremultiplied,
                damage,
            }),
            transform: Transform2D::IDENTITY,
            clip: None,
            opacity: 1.0,
            texture_filter: TextureFilter2D::Linear,
            blend: BlendMode::Alpha,
            filter_graph: None,
        }
    }

    #[test]
    fn transaction_is_atomic_and_generations_never_move_backwards() {
        let mut retained = RetainedLayer2DState::default();
        retained
            .apply(&Layer2DTransaction {
                sequence: 1,
                viewport_width: 4,
                viewport_height: 4,
                operations: vec![Layer2DOperation::Create(layer(2, Layer2DDamage::Full))],
            })
            .unwrap();
        let error = retained
            .apply(&Layer2DTransaction {
                sequence: 2,
                viewport_width: 4,
                viewport_height: 4,
                operations: vec![Layer2DOperation::Update(layer(1, Layer2DDamage::Unchanged))],
            })
            .unwrap_err();
        assert!(error.to_string().contains("moved backwards"));
        assert_eq!(
            retained.layers()[&Layer2DId("content".into())],
            layer(2, Layer2DDamage::Full)
        );
    }

    #[test]
    fn explicit_rect_damage_rejects_empty_and_out_of_bounds_ranges() {
        let mut retained = RetainedLayer2DState::default();
        for damage in [
            Layer2DDamage::Rects(Vec::new()),
            Layer2DDamage::Rects(vec![RectI::new(3, 3, 2, 2)]),
        ] {
            assert!(retained
                .apply(&Layer2DTransaction {
                    sequence: 1,
                    viewport_width: 4,
                    viewport_height: 4,
                    operations: vec![Layer2DOperation::Create(layer(1, damage))],
                })
                .is_err());
        }
    }

    #[test]
    fn ordering_is_z_then_stable_id_and_sequence_is_monotonic() {
        let mut retained = RetainedLayer2DState::default();
        let mut upper = layer(1, Layer2DDamage::Full);
        upper.id = Layer2DId("zeta".into());
        upper.z_index = 3;
        let mut alpha = layer(1, Layer2DDamage::Full);
        alpha.id = Layer2DId("alpha".into());
        alpha.content = Layer2DContent::TextureResource(TextureResource2DRef {
            resource_id: "texture.alpha".into(),
            generation: 1,
            width: 4,
            height: 4,
        });
        alpha.z_index = 3;
        let ordered = retained
            .apply(&Layer2DTransaction {
                sequence: 4,
                viewport_width: 4,
                viewport_height: 4,
                operations: vec![
                    Layer2DOperation::Create(upper),
                    Layer2DOperation::Create(alpha),
                ],
            })
            .unwrap();
        assert_eq!(ordered[0].id, Layer2DId("alpha".into()));
        assert!(retained
            .apply(&Layer2DTransaction {
                sequence: 4,
                viewport_width: 4,
                viewport_height: 4,
                operations: Vec::new(),
            })
            .is_err());
    }
}
