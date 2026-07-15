use std::collections::VecDeque;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    UiPerformanceSample, UiValidationError, MAX_DRAW_CALLS, MAX_TEXTURE_BYTES,
    MAX_VERTICES_PER_FRAME,
};

pub const UI_PERFORMANCE_WINDOW: usize = 240;
pub const UI_PERFORMANCE_MIN_SAMPLES: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiPerformanceBudget {
    pub update_layout_p95_ns: u64,
    pub paint_conversion_p95_ns: u64,
    pub max_draw_calls: u32,
    pub max_vertices: u32,
    pub max_active_texture_bytes: u64,
    pub stable_frame_max_texture_upload_bytes: u64,
}

impl UiPerformanceBudget {
    pub fn production() -> Self {
        Self {
            update_layout_p95_ns: 2_000_000,
            paint_conversion_p95_ns: 1_000_000,
            max_draw_calls: MAX_DRAW_CALLS as u32,
            max_vertices: MAX_VERTICES_PER_FRAME as u32,
            max_active_texture_bytes: MAX_TEXTURE_BYTES as u64,
            stable_frame_max_texture_upload_bytes: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiPerformanceReport {
    pub schema: String,
    pub sample_count: u32,
    pub update_layout_p95_ns: u64,
    pub paint_conversion_p95_ns: u64,
    pub peak_draw_calls: u32,
    pub peak_vertices: u32,
    pub peak_active_texture_bytes: u64,
    pub stable_texture_upload_violations: u32,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UiPerformanceGate {
    budget: UiPerformanceBudget,
    samples: VecDeque<UiPerformanceSample>,
    stable_texture_upload_violations: u32,
}

impl UiPerformanceGate {
    pub fn new(budget: UiPerformanceBudget) -> Self {
        Self {
            budget,
            samples: VecDeque::new(),
            stable_texture_upload_violations: 0,
        }
    }

    pub fn record(
        &mut self,
        sample: UiPerformanceSample,
        stable_frame: bool,
    ) -> Result<(), UiValidationError> {
        if sample.draw_calls > self.budget.max_draw_calls
            || sample.vertices > self.budget.max_vertices
            || sample.active_texture_bytes > self.budget.max_active_texture_bytes
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_PERFORMANCE_HARD_LIMIT",
                "UI frame exceeds a hard draw, vertex, or texture budget",
            ));
        }
        if stable_frame
            && sample.texture_update_bytes > self.budget.stable_frame_max_texture_upload_bytes
        {
            self.stable_texture_upload_violations =
                self.stable_texture_upload_violations.saturating_add(1);
        }
        if self.samples.len() == UI_PERFORMANCE_WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        Ok(())
    }

    pub fn report(&self) -> UiPerformanceReport {
        let update = self
            .samples
            .iter()
            .map(|sample| sample.update_layout_ns)
            .collect::<Vec<_>>();
        let paint = self
            .samples
            .iter()
            .map(|sample| sample.paint_conversion_ns)
            .collect::<Vec<_>>();
        let update_p95 = percentile_95(update);
        let paint_p95 = percentile_95(paint);
        let mut diagnostics = Vec::new();
        if self.samples.len() < UI_PERFORMANCE_MIN_SAMPLES {
            diagnostics.push("ASTRA_UI_PERFORMANCE_SAMPLE_COUNT".to_string());
        }
        if update_p95 > self.budget.update_layout_p95_ns {
            diagnostics.push("ASTRA_UI_PERFORMANCE_UPDATE_P95".to_string());
        }
        if paint_p95 > self.budget.paint_conversion_p95_ns {
            diagnostics.push("ASTRA_UI_PERFORMANCE_PAINT_P95".to_string());
        }
        if self.stable_texture_upload_violations != 0 {
            diagnostics.push("ASTRA_UI_PERFORMANCE_STABLE_UPLOAD".to_string());
        }
        UiPerformanceReport {
            schema: "astra.ui_performance_report.v1".to_string(),
            sample_count: self.samples.len() as u32,
            update_layout_p95_ns: update_p95,
            paint_conversion_p95_ns: paint_p95,
            peak_draw_calls: self
                .samples
                .iter()
                .map(|sample| sample.draw_calls)
                .max()
                .unwrap_or(0),
            peak_vertices: self
                .samples
                .iter()
                .map(|sample| sample.vertices)
                .max()
                .unwrap_or(0),
            peak_active_texture_bytes: self
                .samples
                .iter()
                .map(|sample| sample.active_texture_bytes)
                .max()
                .unwrap_or(0),
            stable_texture_upload_violations: self.stable_texture_upload_violations,
            status: if diagnostics.is_empty() {
                "pass"
            } else {
                "blocking"
            }
            .to_string(),
            diagnostics,
        }
    }
}

fn percentile_95(mut values: Vec<u64>) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let rank = (values.len() * 95).div_ceil(100).saturating_sub(1);
    values[rank]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(update_layout_ns: u64, paint_conversion_ns: u64) -> UiPerformanceSample {
        UiPerformanceSample {
            update_layout_ns,
            paint_conversion_ns,
            texture_update_bytes: 0,
            draw_calls: 1,
            vertices: 4,
            active_texture_bytes: 4,
            instantiated_nodes: 1,
        }
    }

    #[astra_headless_test::test]
    fn production_gate_computes_blocking_p95_without_hiding_stable_uploads() {
        let mut gate = UiPerformanceGate::new(UiPerformanceBudget::production());
        for _ in 0..29 {
            gate.record(sample(1_000_000, 500_000), true).unwrap();
        }
        let mut slow = sample(3_000_000, 2_000_000);
        slow.texture_update_bytes = 1;
        gate.record(slow, true).unwrap();
        let report = gate.report();
        assert_eq!(report.status, "blocking");
        assert!(report
            .diagnostics
            .contains(&"ASTRA_UI_PERFORMANCE_STABLE_UPLOAD".to_string()));
    }
}
