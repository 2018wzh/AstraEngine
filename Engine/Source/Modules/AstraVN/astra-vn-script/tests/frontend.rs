use astra_vn_script::{
    compile_astra_sources, compile_astra_sources_with_options, format_astra_source,
    parse_astra_source, CompileAstraOptions, FormatOptions, ScriptLanguageService, SemanticPass,
    SyntaxKind, SEMANTIC_PASS_ORDER,
};

const SOURCE: &str = "# leading\nstory main #@id story.main\n\nstate prologue #@id state.prologue\n  scene room #@id scene.room\n    text key:\"line hello\" speaker:hero #@id line.hello\n";

#[test]
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

#[test]
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

#[test]
fn semantic_hash_ignores_trivia_and_implicit_command_source_spans() {
    let compact = "story main\nstate start\n  scene room\n    text key:hello\n";
    let with_trivia =
        "# comment\nstory   main\n\nstate start\n  scene room\n    text   key:hello # comment\n";
    let left = compile_astra_sources([("story.astra", compact).into()]).unwrap();
    let right = compile_astra_sources([("story.astra", with_trivia).into()]).unwrap();
    assert_eq!(left.story_hash, right.story_hash);
    assert_ne!(left.source_map.hash, right.source_map.hash);
}

#[test]
fn unknown_command_is_editable_but_requires_explicit_compile_binding() {
    let source = "story main\nstate start\n  scene room\n    studio_fx intensity:2 #@id fx.1\n";
    let parsed = parse_astra_source("unknown.astra", source);
    assert!(parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_UNKNOWN_COMMAND"));

    let error = compile_astra_sources_with_options(
        [("unknown.astra", source)],
        CompileAstraOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_VN_COMMAND_UNBOUND");

    let compiled = compile_astra_sources_with_options(
        [("unknown.astra", source)],
        CompileAstraOptions::default().bind_extension("studio_fx", "studio.presentation"),
    )
    .unwrap();
    assert_eq!(compiled.schema, "astra.vn.compiled_story");
}

#[test]
fn standard_audio_control_is_bound_without_an_extension_bypass() {
    let source = "story main\nstate start\n  scene room\n    audio action:pause target:bgm.main #@id audio.pause\n";
    let parsed = parse_astra_source("audio.astra", source);
    assert!(!parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_UNKNOWN_COMMAND"));

    let compiled = compile_astra_sources([("audio.astra", source).into()]).unwrap();
    assert_eq!(compiled.schema, "astra.vn.compiled_story");
}

#[test]
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
