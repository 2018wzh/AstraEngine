use astra_core::Hash256;
use astra_ui_core::{UiThemeManifest, UiThemeValue};
use astra_vn_script::{
    compile_astra_project, format_astra_source, parse_astra_source, CompileAstraProjectOptions,
    ExtensionCommandDescriptor, ExtensionFieldContract, ExtensionFieldKind, FormatOptions,
    ScriptLanguageService, SemanticPass, SyntaxKind, SEMANTIC_PASS_ORDER,
};
use std::collections::BTreeMap;

const SOURCE: &str = "# leading\nstory main #@id story.main\n\nstate prologue #@id state.prologue\n  scene room #@id scene.room\n    text key:\"line hello\" speaker:hero #@id line.hello\n";

#[astra_headless_test::test]
fn lossless_parse_preserves_comments_blank_lines_quotes_and_spans() {
    let parsed = parse_astra_source("story.astra", SOURCE);
    assert_eq!(parsed.cst.text(), SOURCE);
    let kinds = parsed
        .cst
        .descendants()
        .map(|node| node.kind())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&SyntaxKind::Story));
    assert!(kinds.contains(&SyntaxKind::State));
    assert!(kinds.contains(&SyntaxKind::Scene));
    assert!(kinds.contains(&SyntaxKind::Command));
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let text = parsed
        .ast
        .commands()
        .find(|command| command.keyword() == "text")
        .unwrap();
    assert_eq!(text.attribute("key").unwrap().value(), "line hello");
    assert_eq!(text.source_id(), Some("line.hello"));
    assert_eq!(
        u32::from(text.keyword_span().start),
        SOURCE.find("text").unwrap() as u32
    );
}

#[astra_headless_test::test]
fn semantic_pass_order_is_stable_and_language_service_exposes_navigation() {
    assert_eq!(
        SEMANTIC_PASS_ORDER,
        [
            SemanticPass::Symbols,
            SemanticPass::Routes,
            SemanticPass::Variables,
            SemanticPass::Commands,
            SemanticPass::SystemStories,
            SemanticPass::CompiledStory,
        ]
    );
    let source = "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    jump target:state.start #@id jump.loop\n";
    let service = ScriptLanguageService::new("service.astra", source);
    assert!(service.definition("state.start").is_some());
    assert_eq!(service.references("state.start").len(), 1);
    assert!(service
        .semantic_tokens()
        .iter()
        .any(|token| token.kind == "attribute"));
}

#[astra_headless_test::test]
fn semantic_hash_ignores_trivia_and_implicit_command_source_spans() {
    let compact = "story main\nstate start\n  scene room\n    text key:hello\n";
    let with_trivia =
        "# comment\nstory   main\n\nstate start\n  scene room\n    text   key:hello # comment\n";
    let left = compile_astra_project(
        [astra_vn_script::AstraSource::story("story.astra", compact)],
        Default::default(),
    )
    .unwrap();
    let right = compile_astra_project(
        [astra_vn_script::AstraSource::story(
            "story.astra",
            with_trivia,
        )],
        Default::default(),
    )
    .unwrap();
    assert_eq!(left.story_hash, right.story_hash);
    assert_ne!(left.source_map.hash, right.source_map.hash);
}

#[astra_headless_test::test]
fn select_items_are_a_typed_widget_property_not_a_repeat_binding() {
    let story = "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one #@id line.one\n";
    let ui = concat!(
        "ui_bind surface:message view:ui.config controller:config policy:standard ",
        "theme:theme.classic #@id bind.config\n",
        "ui_view ui.config model:astra.vn.ui_model.config.v1 ",
        "theme:theme.classic #@id ui.config\n",
        "  screen id:root\n",
        "    select id:locale value:$model.locale items:$model.available_locales\n",
        "      on change -> vn.set_config key:display.language value:$event.value\n",
    );

    let compiled = compile_astra_project(
        [
            astra_vn_script::AstraSource::story("story.astra", story),
            astra_vn_script::AstraSource::ui("ui.astra", ui),
        ],
        CompileAstraProjectOptions::default()
            .with_ui_theme(test_theme())
            .with_ui_controller_source("config", test_controller_source()),
    )
    .expect("select items should compile without repeat item_key");
    let select = &compiled.ui_blueprints.views["ui.config"].root.children[0];
    assert!(select.repeat.is_none());
    assert!(select.properties.contains_key("items"));
}

fn test_theme() -> UiThemeManifest {
    let mut theme = UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "theme.classic".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        content_hash: Hash256::from_sha256(&[]),
    };
    theme.content_hash = theme.compute_hash().expect("theme hash");
    theme
}

fn test_controller_source() -> &'static str {
    r#"
astra.ui.controller.register("config", {
  schema = "astra.vn.ui_controller.v1",
  view = "ui.config",
  model_schema = "astra.vn.ui_model.config.v1",
  snapshot = "none",
}, {
  on_action = function(_, _, action)
    return { astra.ui.effect.forward(action) }
  end,
})
"#
}

#[astra_headless_test::test]
fn unknown_command_is_editable_but_requires_explicit_compile_binding() {
    let source = "story main\nstate start\n  scene room\n    studio_fx intensity:2 #@id fx.1\n";
    let parsed = parse_astra_source("unknown.astra", source);
    assert!(parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_UNKNOWN_COMMAND"));

    let error = compile_astra_project(
        [astra_vn_script::AstraSource::story("unknown.astra", source)],
        CompileAstraProjectOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_VN_COMMAND_UNBOUND");

    let compiled = compile_astra_project(
        [astra_vn_script::AstraSource::story("unknown.astra", source)],
        CompileAstraProjectOptions::default().bind_extension(ExtensionCommandDescriptor {
            command: "studio_fx".to_string(),
            provider_id: "studio.presentation".to_string(),
            schema: "studio.presentation.fx.v1".to_string(),
            fields: BTreeMap::from([(
                "intensity".to_string(),
                ExtensionFieldContract {
                    kind: ExtensionFieldKind::Integer,
                    required: true,
                },
            )]),
        }),
    )
    .unwrap();
    assert_eq!(compiled.schema, "astra.vn.compiled_project.v1");
}

#[astra_headless_test::test]
fn standard_audio_control_is_bound_without_an_extension_bypass() {
    let source = "story main\nstate start\n  scene room\n    audio action:pause target:bgm.main #@id audio.pause\n";
    let parsed = parse_astra_source("audio.astra", source);
    assert!(!parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_UNKNOWN_COMMAND"));

    let compiled = compile_astra_project(
        [astra_vn_script::AstraSource::story("audio.astra", source)],
        Default::default(),
    )
    .unwrap();
    assert_eq!(compiled.schema, "astra.vn.compiled_project.v1");
}

#[astra_headless_test::test]
fn formatter_is_idempotent_and_preserves_semantics() {
    let formatted = format_astra_source("story.astra", SOURCE, FormatOptions::default()).unwrap();
    assert_eq!(
        format_astra_source("story.astra", &formatted, FormatOptions::default()).unwrap(),
        formatted
    );
    assert_eq!(
        format_astra_source(
            "story.astra",
            &(SOURCE.to_string() + "\n"),
            FormatOptions::default()
        )
        .unwrap(),
        formatted
    );
    let service = ScriptLanguageService::new("story.astra", &formatted);
    assert!(service
        .symbols()
        .iter()
        .any(|symbol| symbol.id == "line.hello"));
    assert!(service
        .hover(formatted.find("text").unwrap() as u32 + 1)
        .is_some());
}
