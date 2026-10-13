use astra_headless_protocol::{ProductPerformanceObserver, ProductPerformanceSample};

use super::*;

impl ProductPerformanceObserver for ProductPerformanceRecorder {
    fn record_phase(&self, name: &str) -> Result<(), String> {
        self.record_memory_snapshot(name)
    }

    fn record_sample(&self, sample: ProductPerformanceSample) -> Result<(), String> {
        self.record_product_sample_inner(sample)
    }

    fn finish_presentation_sample(&self) -> Result<(), String> {
        self.finish_presentation_scope()
    }
}
