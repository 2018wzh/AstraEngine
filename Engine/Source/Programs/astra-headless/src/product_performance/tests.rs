use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use astra_core::{PerformanceBudget, PerformanceRunIdentity};
use astra_headless_protocol::ProductPerformanceSample;
use astra_platform_headless::{HeadlessGpuFrameSample, HeadlessPerformanceObserver};

use super::*;

use astra_core::{Hash256, PerformanceMetricBudget, PerformanceUnit, PERFORMANCE_BUDGET_SCHEMA};

#[test]
fn frame_deadlines_do_not_accumulate_fractional_rate_drift() {
    assert_eq!(
        frame_deadline_offset(120, 120).unwrap(),
        Duration::from_secs(1)
    );
    assert_eq!(
        frame_deadline_offset(72_000, 120).unwrap(),
        Duration::from_secs(600)
    );
    assert!(frame_deadline_offset(1, 0).is_err());
}

#[test]
fn frame_pacing_drops_elapsed_slots_instead_of_catching_up_after_idle() {
    let started = Instant::now();
    let first = next_frame_deadline(None, started, 120).unwrap();
    assert_eq!(first, started);

    let second_now = started + Duration::from_millis(1);
    let second = next_frame_deadline(Some(first), second_now, 120).unwrap();
    assert_eq!(second, started + Duration::from_nanos(8_333_333));

    let resumed = started + Duration::from_secs(30);
    let after_authored_wait = next_frame_deadline(Some(second), resumed, 120).unwrap();
    assert_eq!(after_authored_wait, resumed);
    assert_eq!(
        next_frame_deadline(Some(after_authored_wait), resumed, 120).unwrap(),
        resumed + Duration::from_nanos(8_333_333)
    );
}

#[test]
fn trace_counters_emit_changes_and_bounded_heartbeats() {
    let previous = TraceCounterSample {
        value: 42,
        paced_frame: 10,
    };
    assert!(should_emit_trace_counter(None, 10, 42, true));
    assert!(!should_emit_trace_counter(Some(previous), 11, 42, true));
    assert!(should_emit_trace_counter(Some(previous), 11, 43, true));
    assert!(!should_emit_trace_counter(Some(previous), 11, 43, false));
    assert!(should_emit_trace_counter(
        Some(previous),
        10 + TRACE_COUNTER_HEARTBEAT_FRAMES,
        42,
        false,
    ));
}

#[test]
fn trace_phase_tracks_are_stable_and_distinct() {
    let names = [
        "physical_input.consume",
        "package.storage_verify",
        "package.table_open",
        "package.source_unlock",
        "product.open",
        "checkpoint.encode_write",
        "runtime.tick_action",
        "vn.step",
        "ui.layout_paint",
        "ui.host_scene",
        "vn.runtime_render",
        "media.decode_mix",
        "media.mixer",
        "save_load",
        "frame.cpu",
        "frame.end_to_end",
        "scene.build",
        "scene.validation",
        "wgpu.prepare_submit",
        "scene.commands",
        "scene.atlas",
        "scene.geometry",
        "scene.vertex_upload",
        "scene.render_submit",
        "scene.render_encode",
        "scene.queue_submit",
        "atlas.upload",
        "scene.pass",
        "filter.pass",
    ];
    let tracks = names.into_iter().map(trace_track).collect::<BTreeSet<_>>();
    assert_eq!(tracks.len(), names.len());
    assert!(tracks
        .iter()
        .all(|track| ![TRACE_TRACK_INPUT_FLOW, TRACE_TRACK_GPU_FLOW].contains(track)));
}

#[test]
fn detailed_trace_phase_sampling_is_fixed_and_includes_each_cadence_boundary() {
    assert!(!trace_detailed_phase(1));
    assert!(!trace_detailed_phase(
        TRACE_DETAILED_PHASE_SAMPLE_INTERVAL - 1
    ));
    assert!(trace_detailed_phase(TRACE_DETAILED_PHASE_SAMPLE_INTERVAL));
    assert!(trace_detailed_phase(
        TRACE_DETAILED_PHASE_SAMPLE_INTERVAL * 2
    ));
}

#[test]
fn deterministic_measurement_duration_covers_all_120hz_intervals() {
    assert_eq!(
        measurement_duration_us(
            72_000,
            astra_platform::HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ
        )
        .unwrap(),
        600_000_000
    );
    assert_eq!(
        measurement_duration_us(1, astra_platform::HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ)
            .unwrap(),
        8_334
    );
}

#[test]
fn product_recorder_writes_identity_bound_report_and_trace_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let profile_hash = hash(b"profile");
    let metrics = [
        ("frame.cpu_ns", PerformanceUnit::Nanoseconds),
        ("frame.gpu_ns", PerformanceUnit::Nanoseconds),
        ("frame.end_to_end_ns", PerformanceUnit::Nanoseconds),
        ("deadline.miss_count", PerformanceUnit::Count),
        ("memory.working_set_bytes", PerformanceUnit::Bytes),
        ("memory.private_bytes", PerformanceUnit::Bytes),
        ("memory.growth_bytes", PerformanceUnit::Bytes),
        ("gpu.resource_bytes", PerformanceUnit::Bytes),
        ("gpu.atlas_bytes", PerformanceUnit::Bytes),
        ("cache.decoded_bytes", PerformanceUnit::Bytes),
        ("gpu.upload_bytes", PerformanceUnit::Bytes),
        ("gpu.readback_bytes", PerformanceUnit::Bytes),
        ("renderer.draw_calls", PerformanceUnit::Count),
        ("renderer.queue_submissions", PerformanceUnit::Count),
        ("renderer.pipeline_count", PerformanceUnit::Count),
        ("heap.allocation_bytes", PerformanceUnit::Bytes),
        ("heap.allocation_count", PerformanceUnit::Count),
    ]
    .into_iter()
    .map(|(id, unit)| PerformanceMetricBudget {
        id: id.into(),
        unit,
        min_samples: 1,
        max_samples: 1,
        thresholds: crate::performance_e2::metric_thresholds(id),
    })
    .collect();
    let recorder = ProductPerformanceRecorder::create(
        PerformanceBudget {
            schema: PERFORMANCE_BUDGET_SCHEMA.into(),
            budget_id: "product.performance.test".into(),
            target: "nativevn.test".into(),
            profile: "classic".into(),
            profile_hash: profile_hash.clone(),
            min_run_duration_us: 1,
            metrics,
        },
        &temp.path().join("trace.json"),
        1,
        astra_platform::HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ,
        1,
    )
    .unwrap();
    recorder.begin_input_flow(1).unwrap();
    let mut sample = HeadlessGpuFrameSample {
        sequence: 1,
        input_flow_id: Some(1),
        scene_build_ns: 10,
        scene_digest_ns: 3,
        scene_validation_ns: 4,
        scene_pending_ns: 3,
        cpu_submit_ns: 20,
        gpu_duration_ns: 30,
        scene_cpu_ns: 15,
        filter_cpu_ns: 5,
        scene_command_cpu_ns: 3,
        scene_atlas_cpu_ns: 0,
        scene_geometry_cpu_ns: 4,
        scene_vertex_upload_cpu_ns: 3,
        scene_render_encode_cpu_ns: 2,
        scene_queue_submit_cpu_ns: 3,
        scene_render_submit_cpu_ns: 5,
        atlas_upload_gpu_ns: 0,
        scene_gpu_ns: 20,
        filter_gpu_ns: 10,
        gpu_resource_bytes: 1024,
        atlas_bytes: 512,
        upload_bytes: 0,
        readback_bytes: 0,
        draw_calls: 1,
        queue_submissions: 1,
        pipeline_count: 1,
        heap_allocation_bytes: 0,
        heap_allocation_count: 0,
        command_allocation_bytes: 0,
        atlas_allocation_bytes: 0,
        geometry_allocation_bytes: 0,
    };
    let product_sample = ProductPerformanceSample {
        bind_to_next_presentation: true,
        runtime_tick_ns: 5,
        vn_step_ns: 2,
        ui_layout_paint_ns: 40,
        ui_request_validation_ns: 1,
        ui_input_routing_ns: 1,
        ui_tree_build_ns: 2,
        ui_layout_finalize_ns: 3,
        ui_semantics_ns: 1,
        ui_update_layout_ns: 10,
        ui_paint_conversion_ns: 5,
        ui_output_validation_ns: 1,
        ui_host_scene_ns: 25,
        ui_model_binding_ns: 4,
        ui_controller_ns: 3,
        ui_frame_model_ns: 2,
        ui_text_scene_ns: 1,
        ui_text_layout_ns: 1,
        ui_text_resource_ns: 1,
        ui_text_compose_ns: 1,
        ui_action_dispatch_ns: 1,
        ui_present_scene_ns: 1,
        ui_runtime_host_step_ns: 1,
        ui_runtime_output_decode_ns: 1,
        ui_runtime_render_ns: 1,
        vn_stage_prepare_ns: 1,
        vn_stage_scene_ns: 1,
        vn_scene_compose_ns: 1,
        vn_stage_texture_ns: 1,
        vn_stage_command_ns: 1,
        vn_stage_lifecycle_ns: 1,
        media_decode_ns: 6,
        media_prewarm_ns: 1,
        media_provider_decode_ns: 3,
        media_parse_convert_ns: 2,
        media_mixer_ns: 1,
        media_audio_query_ns: 1,
        media_audio_render_ns: 1,
        media_audio_submit_ns: 1,
        media_audio_completion_ns: 1,
        save_load_ns: 7,
    };
    // The production observer receives measured durations after the work
    // completes. Keep this synthetic sample inside the active flow's real
    // elapsed interval so the streamed timestamp contract remains valid.
    std::thread::sleep(Duration::from_millis(1));
    recorder
        .record_product_sample(
            1,
            ProductPerformanceSample {
                bind_to_next_presentation: false,
                runtime_tick_ns: 100_000,
                media_decode_ns: 100_000,
                ..ProductPerformanceSample::default()
            },
        )
        .unwrap();
    recorder.record_product_sample(1, product_sample).unwrap();
    recorder.pace_gpu_frame(1).unwrap();
    assert_eq!(recorder.bind_gpu_frame(1).unwrap(), Some(1));
    recorder.record_gpu_frame(sample).unwrap();
    recorder.finish_presentation_scope().unwrap();
    recorder.end_input_flow(1).unwrap();

    recorder.begin_input_flow(2).unwrap();
    recorder
        .record_product_sample(
            2,
            ProductPerformanceSample {
                bind_to_next_presentation: true,
                runtime_tick_ns: 9_000_000,
                ..ProductPerformanceSample::default()
            },
        )
        .unwrap();
    recorder.finish_presentation_scope().unwrap();
    recorder.end_input_flow(2).unwrap();
    {
        let guard = recorder.state.lock().unwrap();
        let state = guard.as_ref().unwrap();
        assert!(state.pending_product_sample.is_none());
        assert_eq!(state.skipped_presentation_samples, 1);
    }

    recorder.begin_input_flow(3).unwrap();
    sample.sequence = 2;
    sample.input_flow_id = Some(3);
    recorder.record_product_sample(3, product_sample).unwrap();
    recorder.pace_gpu_frame(2).unwrap();
    assert_eq!(recorder.bind_gpu_frame(2).unwrap(), Some(3));
    recorder.record_gpu_frame(sample).unwrap();
    recorder.finish_presentation_scope().unwrap();
    recorder.end_input_flow(3).unwrap();
    {
        let guard = recorder.state.lock().unwrap();
        let state = guard.as_ref().unwrap();
        assert_eq!(state.measurement_frame_count, 1);
        assert_eq!(state.gpu_measurement_cutoff, Some(2));
    }
    // The product may still have physical inputs after the budget closes.
    // Shutdown must preserve the exact completed measurement instead of
    // treating that normal path as a duplicate-stop error.
    recorder.stop_gpu_measurement().unwrap();
    recorder
        .finish(
            PerformanceRunIdentity {
                source_revision: "a".repeat(40),
                dirty: false,
                target: "nativevn.test".into(),
                profile: "classic".into(),
                profile_hash,
                package_hash: hash(b"package"),
                build_fingerprint: hash(b"build"),
                session_id: "product.performance.test".into(),
            },
            "product.performance.test",
            hash(b"adapter"),
            hash(b"driver"),
            &temp.path().join("report.json"),
            &temp.path().join("trace-manifest.json"),
        )
        .unwrap();
    assert!(temp.path().join("trace.json").is_file());
    assert!(temp.path().join("report.json").is_file());
    assert!(temp.path().join("trace-manifest.json").is_file());
    let report: astra_core::PerformanceReport =
        serde_json::from_slice(&std::fs::read(temp.path().join("report.json")).unwrap()).unwrap();
    let cpu = report
        .metrics
        .iter()
        .find(|metric| metric.id == "frame.cpu_ns")
        .unwrap();
    assert_eq!(cpu.p95, 88);
}

fn hash(bytes: &[u8]) -> String {
    Hash256::from_sha256(bytes).to_string()
}
