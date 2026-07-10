#[test]
fn linux_probe_is_explicitly_unavailable_until_stage6() {
    let report = astra_platform_linux::probe(Some("nativevn-game"));
    assert!(report.renderer.available.is_empty());
    assert!(report.renderer.selected.is_none());
    assert_eq!(report.diagnostics[0].code, "ASTRA_PLATFORM_NOT_IMPLEMENTED");
    let _factory = astra_platform_linux::factory();
}
