use astra_media::{FilterGraph, FilterNode, FilterParam, FilterTarget, FilterValidator};

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
