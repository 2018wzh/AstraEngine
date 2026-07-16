use astra_vn_script::{compile_astra_project, AstraSource};

#[astra_headless_test::test]
fn system_story_states_do_not_pollute_gameplay_route_graph() {
    let project = compile_astra_project(
        [AstraSource::story(
            "route-graph.astra",
            r#"
story main #@id story.main
state start #@id state.main.start
  scene room #@id scene.main.room
    jump -> ending.done #@id command.main.finish

story system #@id story.system
state save #@id state.system.save
  scene menu #@id scene.system.save
    system_page kind:save policy:astra.policy.standard #@id page.save
"#,
        )],
        Default::default(),
    )
    .unwrap();

    let nodes = project
        .story
        .route_graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.terminal))
        .collect::<Vec<_>>();
    assert_eq!(
        nodes,
        vec![("ending.done", true), ("state.main.start", false)]
    );
    assert!(project
        .story
        .route_graph
        .edges
        .iter()
        .all(|edge| edge.from != "state.system.save"));
}
