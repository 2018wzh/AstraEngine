use super::*;

#[test]
fn select_preserves_source_and_resolves_each_display_label_pair() {
    let source =
        b".select first:left second:right\r\n.label left\r\n.end\r\n.label right\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let ScLineKind::Command { command } = &script.lines[0].kind else {
        panic!("expected select");
    };
    assert_eq!(
        command.control_flow,
        ScControlFlow::Choice {
            targets: vec!["left".into(), "right".into()]
        }
    );
    assert_eq!(encode_sc(&script).unwrap(), source);
}

#[test]
fn select_rejects_missing_labels_empty_options_and_excess_options() {
    for source in [
        ".select first:missing\r\n.end\r\n",
        ".select first:a malformed\r\n.label a\r\n.end\r\n",
        ".select :a\r\n.label a\r\n.end\r\n",
        ".select first:\r\n.end\r\n",
        ".select\r\n.end\r\n",
        ".select a:x b:x c:x d:x e:x\r\n.label x\r\n.end\r\n",
    ] {
        assert!(parse_sc(source.as_bytes(), &ScOpcodeCatalog::observed_musica()).is_err());
    }
}

#[test]
fn observed_cp932_source_round_trips_losslessly() {
    let source = b"; fixture\r\n.pragma entry\r\n.unknown raw operands\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    assert_eq!(script.lines.len(), 4);
    assert_eq!(encode_sc(&script).unwrap(), source);
    let census = ScCensus::from_scripts([&script]);
    assert_eq!(census.command_count, 3);
    assert_eq!(census.unknown_opcode_count, 1);
}

#[test]
fn textual_cfg_is_validated() {
    let source = b".label start\r\n.if flag == 1 done\r\n.goto start\r\n.label done\r\n.end\r\n";
    parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let invalid = b".goto missing\r\n.end\r\n";
    assert_eq!(
        parse_sc(invalid, &ScOpcodeCatalog::observed_musica()).unwrap_err(),
        ScParseError::InvalidTarget("missing".into())
    );
}

#[test]
fn duplicate_catalog_opcode_is_blocking() {
    let mut catalog = ScOpcodeCatalog::default();
    let spec = ScOpcodeSpec {
        name: "wait".into(),
        control_flow: ScControlFlowKind::Next,
    };
    catalog.insert("wait", spec.clone()).unwrap();
    assert_eq!(
        catalog.insert("WAIT", spec).unwrap_err(),
        ScParseError::DuplicateOpcode("wait".into())
    );
}

#[test]
fn operands_are_typed_without_discarding_raw_source() {
    let source = b".movie 9989 op.avi 1280 720 t\r\n.if flag == 1 done\r\n.label done\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let ScLineKind::Command { command } = &script.lines[0].kind else {
        panic!("first line must be a command");
    };
    assert_eq!(
        command.operands,
        vec![
            ScOperand::Integer { value: 9989 },
            ScOperand::Symbol {
                value: "op.avi".into()
            },
            ScOperand::Integer { value: 1280 },
            ScOperand::Integer { value: 720 },
            ScOperand::Boolean { value: true },
        ]
    );
    assert_eq!(command.raw_operands, b"9989 op.avi 1280 720 t");
    assert_eq!(encode_sc(&script).unwrap(), source);
}

#[test]
fn tokenizer_matches_the_observed_engine_space_tab_contract() {
    let tokens = tokenize_operands(b"one, two\t\"three four\" 'five'", 0).unwrap();
    assert_eq!(tokens, vec!["one,", "two", "\"three", "four\"", "'five'"]);
}

#[test]
fn tokenizer_preserves_empty_positional_operands() {
    assert_eq!(
        tokenize_operands(b"100   body", 0).unwrap(),
        vec!["100", "", "", "body"]
    );
    assert_eq!(
        tokenize_operands(b"100\t\tspeaker\tbody", 0).unwrap(),
        vec!["100", "", "speaker", "body"]
    );
    assert_eq!(tokenize_operands(b"", 0).unwrap(), Vec::<String>::new());
}
#[test]
fn chain_locations_reject_empty_or_escaping_segments() {
    use super::*;
    for target in [
        "next.sc#",
        "../next.sc#entry",
        "next.txt#entry",
        "next.sc#entry#other",
        "next.sc#/entry",
    ] {
        let source = format!(".chain {target}\r\n");
        assert!(parse_sc(source.as_bytes(), &ScOpcodeCatalog::observed_musica()).is_err());
    }
}

#[test]
fn gbk_script_round_trips_and_disassembles_without_replacement() {
    let source = "[j].message 1  speaker Japanese\r\n[e].message 2  姓名 中文剧情\r\n.end\r\n";
    let (bytes, _, errors) = encoding_rs::GBK.encode(source);
    assert!(!errors);
    let script = parse_sc_with_encoding(
        &bytes,
        &ScOpcodeCatalog::observed_musica(),
        ScriptEncoding::Gbk,
    )
    .unwrap();
    assert_eq!(encode_sc(&script).unwrap(), bytes.as_ref());
    assert!(disassemble_sc(&script).unwrap().contains("姓名 中文剧情"));
    assert_eq!(script.lines[0].language_guard, Some('j'));
    assert_eq!(script.lines[1].language_guard, Some('e'));
    let ScLineKind::Command { command } = &script.lines[1].kind else {
        panic!("message")
    };
    assert_eq!(command.tokens().unwrap(), ["2", "", "姓名", "中文剧情"]);
    for encoding in [ScriptEncoding::ShiftJis, ScriptEncoding::Gbk] {
        assert!(parse_sc_with_encoding(
            b".message 1   \x81",
            &ScOpcodeCatalog::observed_musica(),
            encoding
        )
        .is_err());
    }
}

#[test]
fn native_save_rejects_a_different_script_encoding() {
    let bytes = b".message 1   Hello\r\n.end\r\n";
    let make = |encoding| {
        crate::MusicaVm::new(
            "musica:/scr/test.sc".into(),
            astra_core::Hash256::from_sha256(bytes),
            parse_sc_with_encoding(bytes, &ScOpcodeCatalog::observed_musica(), encoding).unwrap(),
            0,
        )
        .unwrap()
    };
    let saved = make(ScriptEncoding::ShiftJis).encode_native_save().unwrap();
    assert!(make(ScriptEncoding::Gbk)
        .restore_native_save(&saved, 1)
        .is_err());
}

#[test]
fn malformed_language_guards_fail_before_execution() {
    for source in [b"[z].end".as_slice(), b"[j.end", b"[].end"] {
        assert!(parse_sc(source, &ScOpcodeCatalog::observed_musica()).is_err());
    }
    let source = b"[e].message 1   Wrong\r\n[j].message 2   Japanese\r\n.end\r\n";
    let mut vm = crate::MusicaVm::new(
        "musica:/scr/test.sc".into(),
        astra_core::Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        0,
    )
    .unwrap();
    let Some(crate::MusicaVmEvent::Message { text, .. }) = vm.step(1).unwrap() else {
        panic!("message")
    };
    assert_eq!(text, "Japanese");
}
