use astra_core::{
    PerformanceBudget, PerformanceMetricBudget, PerformanceThresholds, PerformanceUnit,
    PERFORMANCE_BUDGET_SCHEMA,
};
use astra_platform::{PlatformError, PlatformErrorCode, PlatformHostProfile, PlatformId};

pub const WINDOWS_MEDIA_PERFORMANCE_BUDGET_ID: &str = "windows.native_media.v1";

pub fn windows_media_performance_budget(
    profile: &PlatformHostProfile,
    product_profile: &str,
) -> Result<PerformanceBudget, PlatformError> {
    if profile.platform != PlatformId::Windows
        || product_profile.is_empty()
        || !product_profile
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "media.performance_budget",
            "native Windows media budget requires a Windows host and safe product profile",
        ));
    }
    Ok(PerformanceBudget {
        schema: PERFORMANCE_BUDGET_SCHEMA.into(),
        budget_id: WINDOWS_MEDIA_PERFORMANCE_BUDGET_ID.into(),
        target: profile.target.clone(),
        profile: product_profile.to_string(),
        profile_hash: profile.hash()?,
        min_run_duration_us: 1_000_000,
        metrics: vec![
            upper_latency("media.open.total_us", 1, 1, 5_000_000, 10_000_000),
            upper_latency("media.tick.total_us", 30, 120_000, 33_334, 250_000),
            upper_latency("media.tick.audio_status_us", 30, 120_000, 10_000, 100_000),
            upper_latency("media.tick.scheduler_us", 30, 120_000, 4_000, 50_000),
            upper_latency("media.tick.present_us", 1, 120_000, 20_000, 100_000),
            upper_latency("media.tick.pump_us", 30, 120_000, 20_000, 150_000),
            upper_count("media.queue.audio_packets", 30, 120_000, 64),
            upper_count("media.queue.video_frames", 30, 120_000, 64),
            upper_count("media.resources.audio_bytes", 30, 120_000, 32 * 1024 * 1024),
            upper_count(
                "media.resources.video_bytes",
                30,
                120_000,
                256 * 1024 * 1024,
            ),
            upper_count("media.audio.underflows", 1, 1, 8),
            upper_count("media.video.dropped_frames", 1, 1, 120),
            upper_count("media.audio.recoveries", 1, 1, 8),
        ],
    })
}

fn upper_latency(
    id: &str,
    min_samples: usize,
    max_samples: usize,
    max_p95: u64,
    max: u64,
) -> PerformanceMetricBudget {
    PerformanceMetricBudget {
        id: id.into(),
        unit: PerformanceUnit::Microseconds,
        min_samples,
        max_samples,
        thresholds: PerformanceThresholds {
            min_p50: None,
            min_p95: None,
            max_p50: Some(max_p95),
            max_p95: Some(max_p95),
            max_p99: Some(max),
            max: Some(max),
        },
    }
}

fn upper_count(
    id: &str,
    min_samples: usize,
    max_samples: usize,
    max: u64,
) -> PerformanceMetricBudget {
    PerformanceMetricBudget {
        id: id.into(),
        unit: if id.contains("bytes") {
            PerformanceUnit::Bytes
        } else {
            PerformanceUnit::Count
        },
        min_samples,
        max_samples,
        thresholds: PerformanceThresholds {
            min_p50: None,
            min_p95: None,
            max_p50: Some(max),
            max_p95: Some(max),
            max_p99: Some(max),
            max: Some(max),
        },
    }
}
