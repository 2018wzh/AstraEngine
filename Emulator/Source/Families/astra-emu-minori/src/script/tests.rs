use super::*;

#[test]
fn select_preserves_source_and_resolves_each_display_label_pair() {
    let source =
        b".select first:left second:right\r\n.label left\r\n.end\r\n.label right\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
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
        assert!(parse_sc(source.as_bytes(), &ScOpcodeCatalog::observed_minori()).is_err());
    }
}

#[test]
fn observed_cp932_source_round_trips_losslessly() {
    let source = b"; fixture\r\n.pragma entry\r\n.unknown raw operands\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
    assert_eq!(script.lines.len(), 4);
    assert_eq!(encode_sc(&script).unwrap(), source);
    let census = ScCensus::from_scripts([&script]);
    assert_eq!(census.command_count, 3);
    assert_eq!(census.unknown_opcode_count, 1);
}

#[test]
fn textual_cfg_is_validated() {
    let source = b".label start\r\n.if flag == 1 done\r\n.goto start\r\n.label done\r\n.end\r\n";
    parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
    let invalid = b".goto missing\r\n.end\r\n";
    assert_eq!(
        parse_sc(invalid, &ScOpcodeCatalog::observed_minori()).unwrap_err(),
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
    let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
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
        assert!(parse_sc(source.as_bytes(), &ScOpcodeCatalog::observed_minori()).is_err());
    }
}
