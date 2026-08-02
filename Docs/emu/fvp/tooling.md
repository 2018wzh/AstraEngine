# FVP Tooling

## 通用 VFS

FVP 与 Minori 共用 `astra-emu-cli vfs` 和 `LegacyVfsFamilyRegistry`。FVP mount profile 必须显式列出根目录 `.bin` archive；这比 rfvp 扫描 `*.bin` 后跳过解析失败文件更严格，避免损坏包或误识别被静默隐藏。FVP 原始 archive 不需要 private patch，因此 profile 必须省略 `private_patch`；出现该字段会由 FVP factory 阻断。示例只描述格式，不是商业样本清单：

```yaml
schema: astra.emu.vfs_mount_profile.v1
profile_id: fvp-local
family_id: fvp
mount_id: fvp-main
prefix: "fvp:/"
family_options_schema: astra.emu.fvp_vfs_options.v1
family_options:
  nls: shift_jis
  archives:
    - graph.bin
    - bgm.bin
  max_entries_per_archive: 1000000
```

```sh
cargo run -p astra-emu-cli -- vfs --family fvp --game-dir <case-root> --mount-profile <profile.yaml> verify
cargo run -p astra-emu-cli -- vfs --family fvp --game-dir <case-root> --mount-profile <profile.yaml> list --uri fvp:/
cargo run -p astra-emu-cli -- vfs --family fvp --game-dir <case-root> --mount-profile <profile.yaml> stat --uri fvp:/graph/<entry>
```

mount 先有界读取 header 与 filename table，再建立规范化、大小写无关的 `fvp:/<archive-role>/<entry>` URI。为生成 manifest v2 的 source/entry hash，通用 inspection mount 以单次顺序流同时计算 archive 与各 entry hash，不会二次扫描 payload，也不会按 archive 或大 entry 分配整块内存；重叠 entry range 会阻断。`verify` 会再次复核 source identity、完整 entry stream 和首尾重复 range。`read_range` 的公共上限为 64 MiB，底层每次 `BoundedByteSource` 请求仍不超过 16 MiB；大 entry 的 `open_stream` 不整项驻留。重复 range 内容变化、revision 漂移、archive 越界、重复 role/URI/entry id 和 source audit mismatch 都是 blocking diagnostic。正常 FVP runtime 启动不走 inspection mount 的全量审计，仍通过 ABI v4 按需读取 archive metadata 和已访问资源。

Tooling is for metadata extraction, synthetic fixtures and local diagnostics. It must not become a workflow for redistributing or rewriting commercial games.

## Headless 输入与检查点

FVP Headless 只接受 `astra.user_input_sequence.v1` 的物理输入。键盘 `Enter` 映射到 RFVP 的 `Enter`/virtual `LeftClick` 状态；鼠标按键必须走 RFVP 的 mouse down/up 路径，保留 `InputSetClick` 模式和点击坐标，不能伪装成普通键盘位。菜单在淡入完成前可能忽略点击，场景应先用 pointer move 形成稳定 hover，再发送独立的 down/up 边沿。

`Shift` 和 `Control` 也按 RFVP key state 注入。`ControlMask` 只屏蔽 Ctrl，不屏蔽 Shift；默认状态不屏蔽 Ctrl。Ctrl 可加快 text reveal，但不会代替 Enter 或鼠标边沿解除输入等待。自动验收若要连续推进，仍须提交真实 down/up，并在选择页使用可核验的物理输入。

`--artifact-retention checkpoints` 仍对每个 rasterized frame 更新确定性 stream hash，但平台 recorder 不写逐帧 PNG。CLI 只保存输入序列显式声明的 checkpoint PNG，以及受策略约束的 WAV 和 manifest；`all` 才保存完整帧流。该区别只影响证据保留，不改变 fixed-step、VM、scene update 或逐帧 raster 执行。

扩展路线覆盖可显式传入 `--frame-sample-interval <N>`。VM、timer、输入、wait、scene、媒体和有序 effect 仍逐 tick 执行；Host 也会校验每个 render contract、吸收 texture update，并只在第 N 个 fixed step 执行 CPU raster/present。report 记录实际间隔，checkpoint 应放在采样 tick。这个模式只用于快速发现长流程阻断，不能作为逐帧 RGBA parity 或正式视觉 signoff；正式 RFVP 对照必须使用默认值 `1`。为避免把未呈现的 host 图形缓存伪装成可恢复状态，间隔大于 `1` 时禁止导入或导出 continuation snapshot。

长流程可用 `--snapshot-output <private-file>` 原子导出 `astra.emu.headless_resume_snapshot.v1`，再用 `--resume-snapshot <private-file>` 续跑。snapshot 绑定 CLI build、签名 family binary、game/entry identity、fixed delta、stage 尺寸、seed、fixed step、RuntimeWorld/family sections，以及 Headless 的 input/await sequence、pending completion 和 active movie identity/timeline。恢复中的 movie URI 必须是安全相对 URI，起始 tick 不能晚于 snapshot；host 会重新读取同一 session resource，并从固定 elapsed timeline 重建帧与电影音频。任一身份、section hash、tick 或边界不符都会阻断。snapshot 可能含商业运行状态，只能放在 ignored 私有目录，不得写入 package、公开 report 或 Git。

FVP 原生 WMV/ASF/MPEG 在 Headless 中使用与 Manager 相同的 bounded compatibility decoder，不要求 `ffmpeg-vcpkg`。解码结果仍由 Astra-owned Headless media executor按 fixed tick 合成，电影音频进入同一 WAV/meter evidence。MP4/M4V 不走隐式 fallback，必须由 profile 显式绑定已编译的平台 video provider。

Headless 会以 `astra_emu_headless_video_opened` 和 `astra_emu_headless_video_completed` 记录媒体生命周期；字段只包含 codec、decoded frame count、duration、audio stream 是否激活和 elapsed time，不记录资源 URI、路径或媒体内容。这样黑场检查能区分已打开/已完成视频与普通场景或输入等待，但视觉与 PTS parity 仍必须由 checkpoint 和对应 artifact evidence 判断。

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
