use std::{collections::BTreeSet, time::Instant};

use astra_core::PerformanceBudget;
use astra_headless_protocol::ProductPerformanceSample;
use astra_platform::PlatformError;
use astra_platform_headless::{HeadlessGpuFrameSample, HeadlessPerformanceObserver};

use super::*;

pub(crate) fn validate_product_budget(budget: &PerformanceBudget) -> Result<(), String> {
    let actual = budget
        .metrics
        .iter()
        .map(|metric| metric.id.as_str())
        .collect::<BTreeSet<_>>();
    let required = crate::performance_e2::PRODUCT_METRICS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let stress_thresholds = budget
        .metrics
        .iter()
        .all(|metric| metric.thresholds == crate::performance_e2::metric_thresholds(&metric.id));
    let route_thresholds = budget.metrics.iter().all(|metric| {
        metric.thresholds == crate::performance_e2::route_metric_thresholds(&metric.id)
    });
    if actual != required
        || budget
            .metrics
            .iter()
            .any(|metric| metric.unit != crate::performance_e2::metric_unit(&metric.id))
        || (!stress_thresholds && !route_thresholds)
    {
        return Err("ASTRA_PERFORMANCE_PRODUCT_BUDGET_INVALID".into());
    }
    Ok(())
}

impl HeadlessPerformanceObserver for ProductPerformanceRecorder {
    fn pace_gpu_frame(&self, sequence: u64) -> Result<(), PlatformError> {
        let deadline = {
            let mut guard = self
                .state
                .lock()
                .map_err(|_| performance_error("recorder mutex poisoned"))?;
            let state = guard
                .as_mut()
                .ok_or_else(|| performance_error("recorder already finished"))?;
            if !state.armed {
                return Ok(());
            }
            if state.gpu_measurement_cutoff.is_some() {
                return Ok(());
            }
            state.first_gpu_sequence.get_or_insert(sequence);
            state.last_paced_gpu_sequence = Some(sequence);
            state.paced_frame_count = state
                .paced_frame_count
                .checked_add(1)
                .ok_or_else(|| performance_error("paced frame count overflowed"))?;
            let deadline = next_frame_deadline(
                state.next_frame_deadline,
                Instant::now(),
                state.presentation_rate_hz,
            )
            .map_err(performance_error)?;
            state.next_frame_deadline = Some(deadline);
            deadline
        };
        pace_until(deadline);
        Ok(())
    }

    fn bind_gpu_frame(&self, sequence: u64) -> Result<Option<u64>, PlatformError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| performance_error("recorder mutex poisoned"))?;
        let state = guard
            .as_mut()
            .ok_or_else(|| performance_error("recorder already finished"))?;
        if !state.armed
            || state.gpu_measurement_cutoff.is_some()
            || state
                .first_gpu_sequence
                .is_none_or(|first| sequence < first)
        {
            return Ok(None);
        }
        // Time-driven presentation can originate in the media executor without
        // a ProductPerformanceSample that is marked as scene-producing. Its
        // decode/mix work remains an independently traced product phase; the
        // GPU frame still owns scene construction and submission time. Never
        // consume a later UI sample on behalf of that media-only frame.
        if state.active_input_flow.is_none() {
            return Err(performance_error(
                "GPU frame submitted outside an input flow",
            ));
        }
        let product_sample = state
            .pending_product_sample
            .take()
            .unwrap_or(BoundProductSample {
                cpu_ns: 0,
                phases: ProductPerformanceSample::default(),
            });
        if state
            .product_sample_by_gpu_sequence
            .insert(sequence, product_sample)
            .is_some()
        {
            return Err(performance_error("duplicate GPU sequence CPU binding"));
        }
        if state.product_sample_by_gpu_sequence.len() > 1_024 {
            return Err(performance_error("GPU CPU correlation capacity exceeded"));
        }
        Ok(state
            .active_input_flow
            .as_mut()
            .filter(|flow| !flow.gpu_seen)
            .map(|flow| {
                flow.gpu_seen = true;
                flow.sequence
            }))
    }

    fn record_gpu_frame(&self, sample: HeadlessGpuFrameSample) -> Result<(), PlatformError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| performance_error("recorder mutex poisoned"))?;
        let state = guard
            .as_mut()
            .ok_or_else(|| performance_error("recorder already finished"))?;
        if !state.armed
            || state
                .first_gpu_sequence
                .is_none_or(|first| sample.sequence < first)
            || state
                .gpu_measurement_cutoff
                .is_some_and(|cutoff| sample.sequence > cutoff)
        {
            return Ok(());
        }
        let product_sample = state
            .product_sample_by_gpu_sequence
            .remove(&sample.sequence)
            .ok_or_else(|| performance_error("GPU frame has no submit-time CPU binding"))?;
        self.record_gpu_frame_sample(state, sample, product_sample)
    }
}
