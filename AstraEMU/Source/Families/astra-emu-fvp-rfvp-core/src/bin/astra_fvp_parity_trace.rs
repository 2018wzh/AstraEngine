use std::sync::Arc;

use anyhow::Result;
use rfvp::script::{
    context::{Context, ContextSnapshotV1, ThreadState},
    opcode::Opcode,
    parser::{Nls, Parser, Syscall},
    Table, Variant, VmSyscall,
};
use serde_json::{json, Value};

fn main() -> Result<()> {
    let trace = json!({
        "schema": "astra.emu.fvp.reference_trace.v1",
        "opcode_table": opcode_table()?,
        "parser": parser_trace()?,
        "variant": variant_trace(),
        "context": context_trace()?,
    });
    println!("{}", serde_json::to_string_pretty(&trace)?);
    Ok(())
}

fn opcode_table() -> Result<Vec<Value>> {
    (0_i32..=0x27)
        .map(|raw| {
            let opcode = Opcode::try_from(raw)
                .map_err(|_| anyhow::anyhow!("defined opcode failed to decode: {raw}"))?;
            Ok(json!({"raw": raw, "name": opcode.to_string()}))
        })
        .collect()
}

fn parser_trace() -> Result<Value> {
    let mut hcb = Vec::new();
    hcb.extend_from_slice(&4_u32.to_le_bytes());
    hcb.extend_from_slice(&4_u32.to_le_bytes());
    hcb.extend_from_slice(&2_u16.to_le_bytes());
    hcb.extend_from_slice(&3_u16.to_le_bytes());
    hcb.push(8);
    hcb.push(9);
    hcb.push(4);
    hcb.extend_from_slice(b"FVP\0");
    hcb.extend_from_slice(&1_u16.to_le_bytes());
    hcb.push(2);
    hcb.push(5);
    hcb.extend_from_slice(b"Echo\0");
    hcb.extend_from_slice(&0_u16.to_le_bytes());

    let parser = Parser::from_bytes(hcb, Nls::UTF8)?;
    let syscall = parser
        .get_syscall(0)
        .ok_or_else(|| anyhow::anyhow!("synthetic syscall missing"))?;
    Ok(json!({
        "entry_point": parser.get_entry_point(),
        "sys_desc_offset": parser.get_sys_desc_offset(),
        "non_volatile_globals": parser.get_non_volatile_global_count(),
        "volatile_globals": parser.get_volatile_global_count(),
        "game_mode": parser.get_game_mode(),
        "game_mode_reserved": parser.get_game_mode_reserved(),
        "screen": parser.get_screen_size(),
        "title": parser.get_title(),
        "syscall": {"args": syscall.args, "name": syscall.name},
        "custom_syscalls": parser.get_custom_syscall_count(),
    }))
}

fn variant_trace() -> Value {
    let binary = |mut left: Variant, right: Variant, operation: fn(&mut Variant, &Variant)| {
        operation(&mut left, &right);
        variant_value(&left)
    };
    let mut table = Table::new();
    table.push(Variant::Int(10));
    table.insert(3, Variant::String("three".into()));
    table.insert(3, Variant::String("updated".into()));
    json!({
        "truth": [
            Variant::Nil.canbe_true(),
            Variant::True.canbe_true(),
            Variant::Int(0).canbe_true(),
            Variant::String(String::new()).canbe_true()
        ],
        "add_int": binary(Variant::Int(40), Variant::Int(2), Variant::vadd),
        "add_string": binary(Variant::String("astra".into()), Variant::Int(7), Variant::vadd),
        "div_zero": binary(Variant::Int(9), Variant::Int(0), Variant::vdiv),
        "mod_overflow": binary(Variant::Int(i32::MIN), Variant::Int(-1), Variant::vmod),
        "equal_cross_numeric": binary(Variant::Int(2), Variant::Float(2.0), Variant::equal),
        "greater_cross_numeric": binary(Variant::Int(3), Variant::Float(2.5), Variant::greater),
        "table": {
            "zero": table.get(0).map(variant_value),
            "three": table.get(3).map(variant_value),
            "missing": table.get(2).map(variant_value),
        }
    })
}

#[derive(Default)]
struct RecordingSyscall {
    name: Option<String>,
    args: Vec<Variant>,
}

impl VmSyscall for RecordingSyscall {
    fn do_syscall(&mut self, name: &str, args: Vec<Variant>) -> Result<Variant> {
        self.name = Some(name.to_owned());
        self.args = args;
        Ok(Variant::Int(73))
    }
}

fn context_trace() -> Result<Value> {
    let mut syscall = RecordingSyscall::default();

    let mut arithmetic = Context::new(0, 7);
    let mut arithmetic_code = bytecode_parser(
        [
            vec![0x0a],
            40_i32.to_le_bytes().to_vec(),
            vec![0x0c, 2, 0x1a, 0x19],
        ]
        .concat(),
    );
    for _ in 0..4 {
        arithmetic.dispatch_opcode(&mut syscall, &mut arithmetic_code)?;
    }

    let mut call = Context::new(0, 9);
    let mut call_code = bytecode_parser(vec![
        0x02, 8, 0, 0, 0, 0, 0, 0, // call 8
        0x01, 0, 0, // init_stack
        0x0c, 42,   // push_i8
        0x05, // retv
    ]);
    for _ in 0..4 {
        call.dispatch_opcode(&mut syscall, &mut call_code)?;
    }

    let mut syscall_context = Context::new(0, 11);
    let mut syscall_code = bytecode_parser(vec![0x0c, 11, 0x0c, 22, 0x03, 9, 0]);
    syscall_code.syscalls.insert(
        9,
        Syscall {
            args: 2,
            name: "record".into(),
        },
    );
    let mut recorded = RecordingSyscall::default();
    for _ in 0..3 {
        syscall_context.dispatch_opcode(&mut recorded, &mut syscall_code)?;
    }

    let mut waiting = Context::new(17, 12);
    waiting.set_waiting_time(250);
    waiting.set_sleeping_time(90);
    waiting.set_status(ThreadState::CONTEXT_STATUS_WAIT | ThreadState::CONTEXT_STATUS_TEXT);

    Ok(json!({
        "arithmetic": snapshot_value(&arithmetic.capture_snapshot_v1()),
        "call_retv": snapshot_value(&call.capture_snapshot_v1()),
        "syscall": {
            "snapshot": snapshot_value(&syscall_context.capture_snapshot_v1()),
            "name": recorded.name,
            "args": recorded.args.iter().map(variant_value).collect::<Vec<_>>(),
        },
        "waiting": snapshot_value(&waiting.capture_snapshot_v1()),
    }))
}

fn bytecode_parser(bytes: Vec<u8>) -> Parser {
    let len = bytes.len();
    let mut parser = Parser::default();
    parser.buffer = Arc::new(bytes);
    parser.sys_desc_offset = len as u32;
    parser
}

fn snapshot_value(snapshot: &ContextSnapshotV1) -> Value {
    let visible = snapshot
        .stack
        .iter()
        .take(
            snapshot
                .cur_stack_base
                .max(snapshot.cur_stack_pos)
                .saturating_add(3)
                .min(12),
        )
        .map(variant_value)
        .collect::<Vec<_>>();
    json!({
        "id": snapshot.id,
        "cursor": snapshot.cursor,
        "stack_pos": snapshot.cur_stack_pos,
        "stack_base": snapshot.cur_stack_base,
        "start_addr": snapshot.start_addr,
        "return_value": variant_value(&snapshot.return_value),
        "state_bits": snapshot.state_bits,
        "wait_ms": snapshot.wait_ms,
        "sleep_ms": snapshot.sleep_ms,
        "should_exit": snapshot.should_exit,
        "should_break": snapshot.should_break,
        "stack_prefix": visible,
    })
}

fn variant_value(value: &Variant) -> Value {
    if value.is_saved_stack_info() {
        return json!({"type": "saved_stack_info"});
    }
    match value {
        Variant::Nil => json!({"type": "nil"}),
        Variant::True => json!({"type": "true"}),
        Variant::Int(value) => json!({"type": "int", "value": value}),
        Variant::Float(value) => json!({"type": "float", "bits": value.to_bits()}),
        Variant::String(value) => json!({"type": "string", "value": value}),
        Variant::ConstString(value, marker) => {
            json!({"type": "const_string", "value": value, "marker": marker})
        }
        Variant::Table(_) => json!({"type": "table"}),
        _ => unreachable!("saved stack info handled above"),
    }
}
