use astra_media::{
    CpuFilterExecutor, DrawCommand, FilterGraph, FilterNode, FilterParam, FilterTarget,
    FilterValidator, HeadlessRendererProvider, RenderTargetFormat, Renderer2DProvider,
    RendererCreateRequest,
};

#[test]
fn filter_graph_validates_typed_nodes_and_fallback_diagnostics() {
    let graph = FilterGraph {
        schema: "astra.filter_graph.v1".to_string(),
        nodes: vec![FilterNode {
            id: "bloom_main".to_string(),
            kind: "astra.filter.bloom".to_string(),
            input: FilterTarget::Final,
            output: FilterTarget::Final,
            params: [("intensity".to_string(), FilterParam::Float(0.35))]
                .into_iter()
                .collect(),
            deterministic: true,
            allow_cpu_fallback: true,
        }],
    };
    let report = FilterValidator.validate(&graph);
    assert!(report.blocking_diagnostics().is_empty());
    assert!(report
        .diagnostics
        .iter()
        .any(|diag| diag.code == "ASTRA_FILTER_CPU_FALLBACK"));

    let mut invalid = graph;
    invalid.nodes[0].params.insert(
        "intensity".to_string(),
        FilterParam::Text("bad".to_string()),
    );
    let report = FilterValidator.validate(&invalid);
    assert!(report
        .blocking_diagnostics()
        .iter()
        .any(|diag| diag.code == "ASTRA_FILTER_PARAM_TYPE"));
}

#[test]
fn cpu_filter_executor_runs_deterministic_filter_graph_on_real_frame() {
    let provider = HeadlessRendererProvider;
    let mut renderer = provider
        .create(RendererCreateRequest {
            width: 8,
            height: 8,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "classic".to_string(),
        })
        .unwrap();
    let frame = renderer
        .capture_frame(&[
            DrawCommand::clear([0, 0, 0, 255]),
            DrawCommand::rect("hero", 2, 2, 2, 2, [10, 20, 30, 255]),
        ])
        .unwrap();
    let graph = FilterGraph {
        schema: "astra.filter_graph.v1".to_string(),
        nodes: vec![FilterNode {
            id: "bloom_main".to_string(),
            kind: "astra.filter.bloom".to_string(),
            input: FilterTarget::Final,
            output: FilterTarget::Final,
            params: [("intensity".to_string(), FilterParam::Float(0.25))]
                .into_iter()
                .collect(),
            deterministic: true,
            allow_cpu_fallback: true,
        }],
    };

    let (first, first_report) = CpuFilterExecutor.execute(&graph, frame.clone()).unwrap();
    let (second, second_report) = CpuFilterExecutor.execute(&graph, frame).unwrap();

    assert_eq!(first.hash, second.hash);
    assert_ne!(first_report.input_hash, first_report.output_hash);
    assert_eq!(first_report.output_hash, second_report.output_hash);
    assert_eq!(first_report.executed_nodes[0].id, "bloom_main");
    let hero_pixel = ((2 * 8 + 2) * 4) as usize;
    assert_eq!(&first.bytes[hero_pixel..hero_pixel + 4], &[73, 83, 93, 255]);
}
