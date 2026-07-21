use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

use astra_headless_protocol::PhysicalInput;
use astra_platform::PlatformHostClient;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone)]
pub enum ProductPackageSource {
    InMemory(Arc<[u8]>),
    VerifiedContainer(astra_package::AstraContainerReader),
    StorageVerified {
        source: Arc<dyn astra_byte_source::BoundedByteSource>,
        storage_hash: astra_core::Hash256,
    },
}

pub type ProductFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub trait ProductPerformanceObserver: Send + Sync {
    fn record_phase(&self, name: &str) -> Result<(), String>;

    /// Records product CPU work before the next presentation submission.
    fn record_sample(&self, _sample: ProductPerformanceSample) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub key: String,
    pub value_hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalAudioSnapshot {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Clone)]
pub struct ProductOpenRequest {
    pub package: ProductPackageSource,
    pub profile: String,
    pub target: String,
    pub locale: Option<String>,
    pub width: u32,
    pub height: u32,
    pub max_video_frames: u64,
    pub max_decode_output_bytes: u64,
    pub max_decoded_cache_bytes: u64,
    /// Whether the adapter must retain the full canonical mixed-audio timeline.
    /// Manifest-only behavior runs disable this while preserving mixer state,
    /// meters, completion fences, and deterministic observations.
    pub retain_audio_timeline: bool,
    /// Host-owned performance observer. Shipping sessions keep this absent so
    /// the product path does not sample clocks or allocate profiling state.
    pub performance_observer: Option<Arc<dyn ProductPerformanceObserver>>,
    /// Headless-only presentation cadence. The authoritative Runtime tick remains
    /// fixed; performance E2 may request deterministic presentation substeps.
    pub presentation_rate_hz: u32,
    pub platform: PlatformHostClient,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProductPerformanceSample {
    pub runtime_tick_ns: u64,
    pub vn_step_ns: u64,
    pub ui_layout_paint_ns: u64,
    pub ui_request_validation_ns: u64,
    pub ui_update_layout_ns: u64,
    pub ui_paint_conversion_ns: u64,
    pub ui_output_validation_ns: u64,
    pub ui_host_scene_ns: u64,
    pub ui_model_binding_ns: u64,
    pub ui_controller_ns: u64,
    pub ui_frame_model_ns: u64,
    pub ui_text_scene_ns: u64,
    pub ui_action_dispatch_ns: u64,
    pub ui_present_scene_ns: u64,
    pub media_decode_ns: u64,
    pub media_provider_decode_ns: u64,
    pub media_parse_convert_ns: u64,
    pub media_mixer_ns: u64,
    pub save_load_ns: u64,
}

#[derive(Debug, Error)]
pub enum ProductHostError {
    #[error("product adapter binding failed: {0}")]
    Binding(String),
    #[error("product adapter rejected physical input: {0}")]
    Input(String),
    #[error("product adapter output is invalid: {0}")]
    Output(String),
    #[error("product adapter shutdown failed: {0}")]
    Shutdown(String),
}

pub trait ProductAdapterFactory: Send + Sync {
    fn binding_id(&self) -> &str;
    fn open<'a>(
        &'a self,
        request: ProductOpenRequest,
    ) -> ProductFuture<'a, Result<Box<dyn ProductSession>, ProductHostError>>;
}

pub trait ProductSession {
    fn consume<'a>(
        &'a mut self,
        tick: u64,
        input: &'a PhysicalInput,
    ) -> ProductFuture<'a, Result<Vec<Observation>, ProductHostError>>;
    fn observations(&self) -> Vec<Observation>;
    fn capture_frame<'a>(
        &'a self,
    ) -> ProductFuture<'a, Result<astra_platform::CapturedFrame, ProductHostError>>;
    fn capture_audio(&self) -> Result<CanonicalAudioSnapshot, ProductHostError>;
    fn decoded_cache_bytes(&self) -> u64;
    fn take_performance_sample(&mut self) -> ProductPerformanceSample {
        ProductPerformanceSample::default()
    }
    fn shutdown<'a>(&'a mut self) -> ProductFuture<'a, Result<(), ProductHostError>>;
}

#[derive(Default)]
pub struct ProductAdapterRegistry {
    factories: BTreeMap<String, Arc<dyn ProductAdapterFactory>>,
}

impl ProductAdapterRegistry {
    pub fn register(
        &mut self,
        factory: Arc<dyn ProductAdapterFactory>,
    ) -> Result<(), ProductHostError> {
        let id = factory.binding_id().to_owned();
        if !safe_symbol(&id) {
            return Err(ProductHostError::Binding(
                "adapter binding id is unsafe".into(),
            ));
        }
        if self.factories.insert(id.clone(), factory).is_some() {
            return Err(ProductHostError::Binding(format!(
                "adapter binding {id} is duplicated"
            )));
        }
        Ok(())
    }

    pub async fn open(
        &self,
        binding: &str,
        request: ProductOpenRequest,
    ) -> Result<Box<dyn ProductSession>, ProductHostError> {
        self.factories
            .get(binding)
            .ok_or_else(|| {
                ProductHostError::Binding(format!(
                    "bound product adapter {binding} is not registered"
                ))
            })?
            .open(request)
            .await
    }
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
