use super::*;

#[test]
fn rejects_bad_magic() {
    let error = parse_ps2a(&[0; HEADER_BYTES]).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_PS2A_MAGIC");
}

#[test]
fn rejects_truncated_lzss_literal() {
    let mut source = vec![0; HEADER_BYTES + 1];
    source[..4].copy_from_slice(PS2A_MAGIC);
    source[4..8].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
    source[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
    source[0x28..0x2c].copy_from_slice(&1u32.to_le_bytes());
    source[HEADER_BYTES] = 1;
    let error = parse_ps2a(&source).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_PS2A_LZSS");
}

#[test]
fn censes_only_bounded_string_starts() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x0120_0201u32.to_le_bytes());
    bytecode.extend_from_slice(&0u32.to_le_bytes());
    bytecode.extend_from_slice(&0x0120_0201u32.to_le_bytes());
    bytecode.extend_from_slice(&1u32.to_le_bytes());
    let script = CmvsScript {
        schema: "test".into(),
        header: Ps2aHeader {
            header_length: HEADER_BYTES as u32,
            program_key: 0,
            name_index_count: 0,
            bytecode_size: u32::try_from(bytecode.len()).unwrap(),
            metadata_size: 0,
            name_index_size: 0,
            initial_pc: 0,
            compressed_size: 1,
            uncompressed_size: 1,
        },
        name_index: Vec::new(),
        bytecode_offset: HEADER_BYTES as u32,
        bytecode,
        data_segment: Vec::new(),
        string_pool: vec![b'x'],
        strings: vec![CmvsScriptString {
            offset: (HEADER_BYTES + 16) as u32,
            byte_length: 1,
            text_hash: [0; 32],
        }],
        decoded_size: 0,
    };
    let census = census_ps2a_string_references(&script).unwrap();
    assert_eq!(census.candidate_count, 2);
    assert_eq!(census.valid_string_start_count, 1);
    assert_eq!(census.invalid_offset_count, 1);
}

fn control_script(bytecode: Vec<u8>, name_index: Vec<u32>) -> CmvsScript {
    CmvsScript {
        schema: "test".into(),
        header: Ps2aHeader {
            header_length: HEADER_BYTES as u32,
            program_key: 0,
            name_index_count: u32::try_from(name_index.len()).unwrap(),
            bytecode_size: u32::try_from(bytecode.len()).unwrap(),
            metadata_size: 0,
            name_index_size: 0,
            initial_pc: 0,
            compressed_size: 1,
            uncompressed_size: 1,
        },
        name_index,
        bytecode_offset: HEADER_BYTES as u32,
        bytecode,
        string_pool: Vec::new(),
        data_segment: Vec::new(),
        strings: Vec::new(),
        decoded_size: 0,
    }
}

#[test]
fn installing_a_script_frame_replaces_all_lookup_tables_and_preserves_failed_loads() {
    use crate::CmvsPs2aVmState;
    use astra_core::Hash256;
    let mut vm = CmvsPs2aVmState::new(0);
    let mut script = control_script(vec![0; 4], vec![0, 2]);
    script.header.initial_pc = 2;
    script.data_segment = vec![7, 0, 0, 0];
    let hash = Hash256::from_sha256(b"first");
    vm.install_script_frame(1, "cmvs:/scripts/first.ps3", hash, &script)
        .unwrap();
    assert_eq!(vm.program_counter, 2);
    assert_eq!(vm.script_frames[&1].script_hash, hash);
    assert_eq!(vm.script_name_indices[&1], vec![0, 2]);
    assert_eq!(vm.script_data_segment_words[&1][&0], 7);
    let before = vm.clone();
    assert_eq!(
        vm.install_script_frame(4, "cmvs:/scripts/second.ps3", hash, &script)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_SCRIPT_FRAME"
    );
    assert_eq!(vm, before);
    assert!(vm
        .install_script_frame(1, "cmvs:/../second.ps3", hash, &script)
        .is_err());
    assert_eq!(vm, before);
    script.header.initial_pc = 0;
    script.name_index.clear();
    script.data_segment.clear();
    vm.frame_string_lengths
        .insert(1, std::collections::BTreeMap::from([(9, 3)]));
    vm.install_script_frame(
        1,
        "cmvs:/scripts/second.ps3",
        Hash256::from_sha256(b"second"),
        &script,
    )
    .unwrap();
    assert_eq!(vm.program_counter, 0);
    assert!(vm.script_name_indices[&1].is_empty());
    assert!(vm.frame_string_lengths[&1].is_empty());
    assert!(!vm.script_data_segment_words.contains_key(&1));
    assert_eq!(vm.script_frames[&1].script_uri, "cmvs:/scripts/second.ps3");
}

#[test]
fn decodes_proven_absolute_branch() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x400u16.to_le_bytes());
    bytecode.extend_from_slice(&0u32.to_le_bytes());
    let instruction =
        decode_ps2a_control_instruction(&control_script(bytecode, vec![]), 0).unwrap();
    assert_eq!(
        instruction,
        CmvsPs2aControlInstruction::AbsoluteJump {
            span: CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 6,
            },
            target: 0,
        }
    );
}

#[test]
fn decodes_jump_with_opaque_field_without_assigning_semantics() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x407u16.to_le_bytes());
    bytecode.extend_from_slice(&9u32.to_le_bytes());
    bytecode.extend_from_slice(&0u32.to_le_bytes());
    let instruction =
        decode_ps2a_control_instruction(&control_script(bytecode, vec![]), 0).unwrap();
    assert!(matches!(
        instruction,
        CmvsPs2aControlInstruction::JumpWithOpaqueField {
            opaque_field: 9,
            target: 0,
            ..
        }
    ));
}

#[test]
fn resolves_name_index_control_target() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x410u16.to_le_bytes());
    bytecode.extend_from_slice(&0u16.to_le_bytes());
    bytecode.extend_from_slice(&0u16.to_le_bytes());
    let instruction =
        decode_ps2a_control_instruction(&control_script(bytecode, vec![4]), 0).unwrap();
    assert!(matches!(
        instruction,
        CmvsPs2aControlInstruction::NameIndexCall { target: 4, .. }
    ));
}

#[test]
fn resolves_only_a_validated_private_string_start() {
    let mut script = control_script(Vec::new(), vec![]);
    script.string_pool = b"abc".to_vec();
    script.strings = vec![CmvsScriptString {
        offset: HEADER_BYTES as u32,
        byte_length: 3,
        text_hash: Sha256::digest(b"abc").into(),
    }];
    assert_eq!(script.resolve_private_string_relative(0).unwrap(), "abc");
    let error = script.resolve_private_string_relative(1).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_PS2A_STRING_OFFSET");
    let error = script
        .resolve_private_tag_zero_string(0x4000_0000)
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_PS2A_STRING_TAG");
}

#[test]
fn rejects_unknown_control_opcode_without_skipping() {
    let script = control_script(0x2000u16.to_le_bytes().to_vec(), vec![]);
    let error = decode_ps2a_control_instruction(&script, 0).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_PS2A_OPCODE");
}

#[test]
fn decodes_proven_value_expression_tokens() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x201u16.to_le_bytes());
    bytecode.extend_from_slice(&0x120u16.to_le_bytes());
    bytecode.extend_from_slice(&42u32.to_le_bytes());
    bytecode.extend_from_slice(&0x160u16.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    let expression = decode_ps2a_value_expression(&control_script(bytecode, vec![]), 0).unwrap();
    assert_eq!(expression.span.byte_length, 12);
    assert_eq!(expression.tokens.len(), 2);
    assert!(matches!(
        expression.tokens[0],
        CmvsPs2aValueExpressionToken::Value {
            opcode: 0x120,
            payload: 42,
            ..
        }
    ));
    assert!(matches!(
        expression.tokens[1],
        CmvsPs2aValueExpressionToken::Operator { opcode: 0x160, .. }
    ));
    let error = private_string_reference_from_value_expression(&expression).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_PS2A_STRING_EXPRESSION");
}

#[test]
fn recognizes_only_the_proven_tag_zero_string_expression() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x201u16.to_le_bytes());
    bytecode.extend_from_slice(&0x120u16.to_le_bytes());
    bytecode.extend_from_slice(&42u32.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    let expression = decode_ps2a_value_expression(&control_script(bytecode, vec![]), 0).unwrap();
    assert_eq!(
        private_string_reference_from_value_expression(&expression).unwrap(),
        CmvsPs2aPrivateStringReference {
            relative_offset: 42
        }
    );
}

#[test]
fn decodes_nested_value_expression() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x201u16.to_le_bytes());
    bytecode.extend_from_slice(&0x121u16.to_le_bytes());
    bytecode.extend_from_slice(&0x200u16.to_le_bytes());
    bytecode.extend_from_slice(&0x120u16.to_le_bytes());
    bytecode.extend_from_slice(&7u32.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    let expression = decode_ps2a_value_expression(&control_script(bytecode, vec![]), 0).unwrap();
    assert_eq!(expression.tokens.len(), 1);
    assert!(matches!(
        expression.tokens[0],
        CmvsPs2aValueExpressionToken::NestedStackExpression { .. }
    ));
}

#[test]
fn decodes_nested_stack_expression() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x200u16.to_le_bytes());
    bytecode.extend_from_slice(&0x101u16.to_le_bytes());
    bytecode.extend_from_slice(&0x200u16.to_le_bytes());
    bytecode.extend_from_slice(&0x120u16.to_le_bytes());
    bytecode.extend_from_slice(&7u32.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    let expression = decode_ps2a_stack_expression(&control_script(bytecode, vec![]), 0).unwrap();
    assert_eq!(expression.span.byte_length, 16);
    assert_eq!(expression.nodes.len(), 1);
    assert!(matches!(
        expression.nodes[0],
        CmvsPs2aStackExpressionNode::Prefix {
            opcode: 0x101,
            ref operands,
            ..
        } if operands.len() == 1
    ));
}

#[test]
fn decodes_numeric_expression_value_and_operator() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x202u16.to_le_bytes());
    bytecode.extend_from_slice(&0x12cu16.to_le_bytes());
    bytecode.extend_from_slice(&7u32.to_le_bytes());
    bytecode.extend_from_slice(&0x160u16.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    let expression = decode_ps2a_numeric_expression(&control_script(bytecode, vec![]), 0).unwrap();
    assert_eq!(expression.span.byte_length, 12);
    assert!(matches!(
        expression.nodes.as_slice(),
        [
            CmvsPs2aNumericExpressionNode::Value {
                opcode: 0x12c,
                payload: 7,
                ..
            },
            CmvsPs2aNumericExpressionNode::Operator { opcode: 0x160, .. }
        ]
    ));
}

#[test]
fn decodes_numeric_expression_stack_prefix() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x202u16.to_le_bytes());
    bytecode.extend_from_slice(&0x130u16.to_le_bytes());
    bytecode.extend_from_slice(&9u32.to_le_bytes());
    bytecode.extend_from_slice(&0x200u16.to_le_bytes());
    bytecode.extend_from_slice(&0x120u16.to_le_bytes());
    bytecode.extend_from_slice(&7u32.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    let expression = decode_ps2a_numeric_expression(&control_script(bytecode, vec![]), 0).unwrap();
    assert!(matches!(
        expression.nodes.as_slice(),
        [CmvsPs2aNumericExpressionNode::Prefix {
            opcode: 0x130,
            payload: Some(9),
            index: None,
            operands,
            ..
        }] if operands.len() == 1
    ));
}

#[test]
fn blocks_truncated_stack_expression_value() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x200u16.to_le_bytes());
    bytecode.extend_from_slice(&0x120u16.to_le_bytes());
    let error = decode_ps2a_stack_expression(&control_script(bytecode, vec![]), 0).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_PS2A_CONTROL");
}

#[test]
fn frames_command_without_executing_it() {
    let script = control_script(0x2007u16.to_le_bytes().to_vec(), vec![]);
    let frame = frame_ps2a_instruction(&script, 0).unwrap();
    assert_eq!(
        frame,
        CmvsPs2aInstructionFrame::Command {
            span: CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: 7,
        }
    );
}

#[test]
fn frames_numeric_expression_without_evaluating_it() {
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(&0x202u16.to_le_bytes());
    bytecode.extend_from_slice(&0x12au16.to_le_bytes());
    bytecode.extend_from_slice(&1u32.to_le_bytes());
    bytecode.extend_from_slice(&0x20fu16.to_le_bytes());
    let frame = frame_ps2a_instruction(&control_script(bytecode, vec![]), 0).unwrap();
    assert!(matches!(
        frame,
        CmvsPs2aInstructionFrame::NumericExpression(CmvsPs2aNumericExpression {
            nodes,
            ..
        }) if nodes.len() == 1
    ));
}
