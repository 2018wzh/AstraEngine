use astra_core::{
    validate_performance_report, PerformanceBudget, PerformanceMetricBudget, PerformanceRecorder,
    PerformanceRunIdentity, PerformanceStatus, PerformanceThresholds, PerformanceUnit,
    PERFORMANCE_BUDGET_SCHEMA,
};

fn identity() -> PerformanceRunIdentity {
    PerformanceRunIdentity {
        source_revision: "a".repeat(40),
        dirty: false,
        target: "native-game".into(),
        profile: "classic".into(),
        profile_hash: format!("sha256:{}", "b".repeat(64)),
        package_hash: format!("sha256:{}", "c".repeat(64)),
        build_fingerprint: format!("sha256:{}", "d".repeat(64)),
        session_id: "session.native.1".into(),
    }
}

fn budget() -> PerformanceBudget {
    PerformanceBudget {
        schema: PERFORMANCE_BUDGET_SCHEMA.into(),
        budget_id: "windows.classic.media".into(),
        target: "native-game".into(),
        profile: "classic".into(),
        profile_hash: identity().profile_hash,
        min_run_duration_us: 100_000,
        metrics: vec![PerformanceMetricBudget {
            id: "media.tick.total_us".into(),
            unit: PerformanceUnit::Microseconds,
            min_samples: 4,
            max_samples: 8,
            thresholds: PerformanceThresholds {
                min_p50: None,
                min_p95: None,
                max_p50: Some(1_000),
                max_p95: Some(2_000),
                max_p99: Some(3_000),
                max: Some(4_000),
            },
        }],
    }
}

#[test]
fn measured_report_is_identity_bound_and_revalidated() {
    let budget = budget();
    let mut recorder = PerformanceRecorder::new(budget.clone()).unwrap();
    for value in [500, 750, 1_000, 1_500] {
        recorder.record("media.tick.total_us", value).unwrap();
    }
    let report = recorder.finalize(identity(), 150_000).unwrap();
    assert_eq!(report.status, PerformanceStatus::Pass);
    assert_eq!(report.metrics[0].p95, 1_500);
    validate_performance_report(&budget, &identity(), &report).unwrap();
}

#[test]
fn missing_samples_threshold_drift_and_identity_tamper_block() {
    let budget = budget();
    let mut recorder = PerformanceRecorder::new(budget.clone()).unwrap();
    recorder.record("media.tick.total_us", 5_000).unwrap();
    let report = recorder.finalize(identity(), 150_000).unwrap();
    assert_eq!(report.status, PerformanceStatus::Blocked);
    assert!(validate_performance_report(&budget, &identity(), &report).is_err());

    let mut valid = PerformanceRecorder::new(budget.clone()).unwrap();
    for value in [100, 200, 300, 400] {
        valid.record("media.tick.total_us", value).unwrap();
    }
    let mut report = valid.finalize(identity(), 150_000).unwrap();
    report.identity.package_hash = format!("sha256:{}", "e".repeat(64));
    assert!(validate_performance_report(&budget, &identity(), &report).is_err());
}

#[test]
fn undeclared_or_over_capacity_samples_fail_before_mutating_other_metrics() {
    let mut recorder = PerformanceRecorder::new(budget()).unwrap();
    assert!(recorder.record("media.decode.total_us", 1).is_err());
    for value in 0..8 {
        recorder.record("media.tick.total_us", value).unwrap();
    }
    assert!(recorder.record("media.tick.total_us", 9).is_err());
}
