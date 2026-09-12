use super::*;

#[test]
fn tick_counter_overflow_is_rejected_before_either_counter_changes() {
    let mut director = ProductStageDirector::new(
        VnPresentationProviderManifest::standard(),
        "advanced-vn",
        StageViewport {
            width: 1280,
            height: 720,
        },
    )
    .unwrap();
    for (frame_index, elapsed_ns, code) in [
        (u64::MAX, 0, "ASTRA_VN_STAGE_FRAME_OVERFLOW"),
        (7, u64::MAX, "ASTRA_VN_STAGE_TIME_OVERFLOW"),
    ] {
        director.state.frame_index = frame_index;
        director.state.elapsed_ns = elapsed_ns;
        let before = director.snapshot().unwrap();
        assert_eq!(director.tick(1).unwrap_err().code(), code);
        assert_eq!(director.snapshot().unwrap(), before);
        assert!(!director.is_failed());
    }
}
