# FVP Tooling

Tooling is for metadata extraction, synthetic fixtures and local diagnostics. It must not become a workflow for redistributing or rewriting commercial games.

## rfvp tools

| Tool | Input | Output | AstraEMU use |
| --- | --- | --- | --- |
| `disassembler` | `.hcb` | project dir with `config.yaml`, `disassembly.yaml`, `project.toml` | inspect syscall table and bytecode locally |
| `assembler` | disassembler project dir | `.hcb` | round-trip synthetic fixtures only |
| `hcb2lua_decompiler` | `.hcb` | first-pass Lua | inspect control flow locally |
| `lua2hcb` | constrained Lua-like source + YAML meta | `.hcb` | build tiny public fixtures |
| `nvsg_pack` | PNG/NVSG texture | `hzc1`/NVSG or PNG | create public graph fixtures |

## Disassembly boundary

`disassembler` creates a project layout:

```text
project/
  config.yaml
  disassembly.yaml
  project.toml
```

For commercial samples, only these fields may be copied into AstraEMU reports:

- HCB size and hash prefix.
- Header fields.
- Syscall id/name/argc.
- Opcode address and mnemonic.
- String length, encoding and hash prefix.

Do not copy full `push_string` text, branch-local story context or reassembled commercial scripts.

## Compiler contract

`lua2hcb` does not accept full Lua. It accepts a fixed contract:

- Required `function main() ... end`.
- Top-level `global` and `volatile` declarations only before functions.
- Simple function calls, `__ret`, `Sx` temporaries and explicit table access.
- `if`, `elseif`, `else`, `while`, `break`, and limited return forms.
- No closures, modules, metatables, coroutines, `for`, `repeat`, `goto`, varargs or general Lua standard library use.

This is useful for public fixtures. Example fixture shape:

```lua
global boot_flag
volatile current_voice

function main()
    S0 = 1
    boot_flag = S0
    __ret = AudioState(S0)
    S1 = __ret
    return
end
```

The matching YAML carries NLS, `game_mode`, `game_title` and syscall descriptors. Global counts should be derived from source, not authored by hand.

## Local probe commands

These command shapes are acceptable for local diagnostics:

```bash
cargo run -p disassembler -- --input <game-root>/Sakura.hcb --output <work-dir> --nls sjis
cargo run -p hcb2lua_decompiler -- --input <game-root>/Sakura.hcb --output <work-dir>/script.lua --lang sjis
cargo run -p nvsg_pack -- inspect <fixture>.nvsg
```

For AstraEMU checked-in tests, replace `<game-root>` with generated fixtures under the test data folder. Commercial files stay outside the repository.

## AstraEMU Python scripts

当前仓库提供这些研究入口：

```bash
python Tools/AstraEMU/fvp_probe.py <game-root> --json
python Tools/AstraEMU/fvp_hcb.py <game-root>/Sakura.hcb --json
python Tools/AstraEMU/fvp_bin.py <game-root>/bgm.bin --json
python Tools/AstraEMU/fvp_bin.py <game-root>/bgm.bin --out <extract-dir>
```

No tool should accept or emit decrypt keys, executable patch bytes or bypass instructions.
