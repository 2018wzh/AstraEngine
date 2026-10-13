use astra_core::{Hash256, PerformanceBudget};
use astra_platform::{validate_headless_performance_profile, HeadlessHostProfile};
use astra_platform_common::WgpuOffscreenRenderer;

use super::*;

use astra_platform::{
    GpuAdapterPolicy, GpuBackendPolicy, GpuDeviceTypePolicy, HeadlessArtifactRetention,
    HeadlessReadbackPolicy, HeadlessRenderPolicy,
};

#[test]
fn prepared_budget_uses_fixed_production_thresholds() {
    let hash = Hash256::from_sha256(b"fixture").to_string();
    let mut profile =
        HeadlessHostProfile::reference("windows-x64", "performance.fixture", hash.clone(), hash);
    profile.providers.renderer = "wgpu_offscreen".into();
    profile.presentation_rate_hz = astra_platform::HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ;
    profile.gpu_adapter = Some(GpuAdapterPolicy {
        backend: GpuBackendPolicy::Dx12,
        device_type: GpuDeviceTypePolicy::Integrated,
        require_timestamp_query: true,
        adapter_identity_hash: None,
    });
    profile.render_policy = HeadlessRenderPolicy::All;
    profile.readback_policy = HeadlessReadbackPolicy::CheckpointsOnly;
    profile.artifacts.retention = HeadlessArtifactRetention::ManifestOnly;
    let temp = tempfile::tempdir().unwrap();
    let profile_path = temp.path().join("profile.json");
    let budget_path = temp.path().join("budget.json");
    write_json(&profile_path, &profile).unwrap();
    prepare_budget(
        &profile_path,
        &budget_path,
        "headless.performance.fixture".into(),
        72_000,
        72_000,
        600_000_000,
        PerformanceBudgetKind::RendererStress,
    )
    .unwrap();
    let budget: PerformanceBudget = read_json(&budget_path, "BUDGET").unwrap();
    assert_eq!(budget.metrics.len(), REQUIRED_METRICS.len());
    assert_eq!(
        budget
            .metrics
            .iter()
            .find(|metric| metric.id == "frame.end_to_end_ns")
            .unwrap()
            .thresholds
            .max_p99,
        Some(FRAME_PERIOD_NS)
    );
}

#[test]
fn scene2d_animation_changes_output_without_mutating_draw_geometry() {
    let (_, mut frame) = scene2d_workload();
    let commands = frame.commands.clone();
    let clear = frame.clear_rgba;
    animate(&mut frame, 7).unwrap();
    assert_ne!(frame.clear_rgba, clear);
    assert_eq!(frame.commands, commands);
}

#[test]
fn product_budget_uses_product_frame_metrics_without_synthetic_overhead_samples() {
    let hash = Hash256::from_sha256(b"fixture").to_string();
    let mut profile =
        HeadlessHostProfile::reference("windows-x64", "performance.fixture", hash.clone(), hash);
    profile.providers.renderer = "wgpu_offscreen".into();
    profile.presentation_rate_hz = astra_platform::HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ;
    profile.gpu_adapter = Some(GpuAdapterPolicy {
        backend: GpuBackendPolicy::Dx12,
        device_type: GpuDeviceTypePolicy::Integrated,
        require_timestamp_query: true,
        adapter_identity_hash: None,
    });
    profile.render_policy = HeadlessRenderPolicy::All;
    profile.readback_policy = HeadlessReadbackPolicy::CheckpointsOnly;
    profile.artifacts.retention = HeadlessArtifactRetention::ManifestOnly;
    let temp = tempfile::tempdir().unwrap();
    let profile_path = temp.path().join("profile.json");
    let budget_path = temp.path().join("budget.json");
    write_json(&profile_path, &profile).unwrap();
    prepare_budget(
        &profile_path,
        &budget_path,
        "headless.performance.product.fixture".into(),
        445,
        445,
        1,
        PerformanceBudgetKind::ProductRoute,
    )
    .unwrap();
    let budget: PerformanceBudget = read_json(&budget_path, "BUDGET").unwrap();
    assert!(budget
        .metrics
        .iter()
        .all(|metric| !metric.id.starts_with("profiler.")));
    assert!(budget
        .metrics
        .iter()
        .all(|metric| metric.min_samples == 445));
}

#[test]
fn prepared_performance_profile_requires_timestamped_hardware_gpu() {
    let hash = Hash256::from_sha256(b"fixture").to_string();
    let profile =
        HeadlessHostProfile::reference("windows-x64", "performance.fixture", hash.clone(), hash);
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.json");
    let output = temp.path().join("output.json");
    let mut legacy_profile = profile.clone();
    legacy_profile.schema = "astra.headless_host_profile.v2".into();
    write_json(&input, &legacy_profile).unwrap();
    let error = prepare_profile(
        &input,
        &output,
        PerformanceGpuBackend::Dx12,
        PerformanceGpuDeviceType::Integrated,
        None,
    )
    .unwrap_err();
    assert!(error.contains("ASTRA_HEADLESS_PERFORMANCE_PROFILE_SCHEMA"));
    write_json(&input, &profile).unwrap();
    prepare_profile(
        &input,
        &output,
        PerformanceGpuBackend::Dx12,
        PerformanceGpuDeviceType::Integrated,
        None,
    )
    .unwrap();
    let prepared: HeadlessHostProfile = read_json(&output, "PROFILE").unwrap();
    let policy = validate_headless_performance_profile(&prepared).unwrap();
    assert!(policy.require_timestamp_query);
    assert!(matches!(
        policy.device_type,
        GpuDeviceTypePolicy::Integrated
    ));
}

#[tokio::test]
#[ignore = "requires the integrated DX12 performance adapter"]
async fn stable_scene_submit_has_zero_heap_allocation_p95() {
    let policy = GpuAdapterPolicy {
        backend: GpuBackendPolicy::Dx12,
        device_type: GpuDeviceTypePolicy::Integrated,
        require_timestamp_query: true,
        adapter_identity_hash: None,
    };
    let mut renderer = WgpuOffscreenRenderer::new_with_policy(
        &astra_platform_common::native_wgpu_instance().unwrap(),
        &policy,
    )
    .await
    .unwrap();
    let (initialization, mut frame) = scene2d_workload();
    renderer.submit_frame_profiled(&initialization).unwrap();
    let mut allocations = Vec::with_capacity(240);
    for sequence in 0..240 {
        animate(&mut frame, sequence + 1).unwrap();
        let pending = renderer.submit_frame_timestamped(&frame).unwrap();
        renderer.resolve_profiled_submission(pending).unwrap();
        allocations.push(renderer.performance_counters().engine_allocation_bytes);
    }
    allocations.sort_unstable();
    assert_eq!(percentile(&allocations, 95), 0, "{allocations:?}");
}

#[tokio::test]
#[ignore = "requires the integrated DX12 performance adapter"]
async fn timestamp_and_trace_profiling_stays_within_overhead_budget() {
    let policy = GpuAdapterPolicy {
        backend: GpuBackendPolicy::Dx12,
        device_type: GpuDeviceTypePolicy::Integrated,
        require_timestamp_query: true,
        adapter_identity_hash: None,
    };
    let mut renderer = WgpuOffscreenRenderer::new_with_policy(
        &astra_platform_common::native_wgpu_instance().unwrap(),
        &policy,
    )
    .await
    .unwrap();
    let (initialization, mut frame) = scene2d_workload();
    renderer.submit_frame_profiled(&initialization).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (overhead_ppm, working_set_bytes) = validate_profiler_overhead(
        &mut renderer,
        &mut frame,
        &temp.path().join("overhead.json"),
    )
    .unwrap();
    assert!(overhead_ppm <= MAX_PROFILER_OVERHEAD_PPM, "{overhead_ppm}");
    assert!(working_set_bytes <= MAX_PROFILER_WORKING_SET_BYTES);
}
