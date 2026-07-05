# SoftPAL Tooling

这些命令用于本地合法安装和本地 `sena-rs` checkout 的格式验证。命令输出只用于 metadata、hash、coverage 和诊断；不要把解包资源、完整文本、反编译脚本或截图提交进 AstraEngine。

## `sena-rs` launcher

运行 SoftPAL 参考 VM：

```bash
cargo run -p pal-vm --release --bin sena -- <game-root> --nls sjis
```

常用诊断 flag：

```bash
cargo run -p pal-vm --release --bin sena -- <game-root> --nls sjis --headless --diagnostic-frames 120 --no-audio
cargo run -p pal-vm --release --bin sena -- <game-root> --nls sjis --headless --trace-script --diagnostic-auto-advance
```

如果写 PNG，只写到本地临时目录，报告里只保留 hash、尺寸和帧号。

## Asset metadata

列 ResourceManager path：

```bash
cargo run -p pal-assets -- <game-root> --nls sjis paths
```

列已加载 PAC metadata：

```bash
cargo run -p pal-assets -- <game-root> --nls sjis list-pacs
```

单资源 preview 只看前 256 byte，适合确认 header：

```bash
cargo run -p pal-assets -- <game-root> --nls sjis open Script.src
cargo run -p pal-assets -- <game-root> --nls sjis open Text.dat --decrypt
```

preview 输出不得复制进 docs，除非只保留 magic、size、hash 这类 metadata。

## PAC listing

PAC 工具支持 list-only：

```bash
cargo run -p pal-pac-unpacker -- --input <game-root>/data.pac --output <tmp> --list-only
```

AstraEngine 文档只使用 list-only 信息。需要本地调试资源时，输出目录必须在本机临时工作区或用户指定目录，不能写进 AstraEngine，也不能提交。

## Disassembly and extcall report

从单个 PAC 做本地 coverage：

```bash
cargo run -p pal-decompiler -- \
  --input-pac <game-root>/data.pac \
  --nls sjis \
  --output <tmp>/softpal.lua \
  --extcall-report <tmp>/softpal-extcalls.json
```

`softpal.lua` 是商业脚本派生物，不能进入仓库。`softpal-extcalls.json` 若要进入 report，先裁剪到 PC、category、index、name、status、arg_count、hash 和聚合计数。

只做 disassembly header/CFG：

```bash
cargo run -p pal-disassembled -- <tmp>/SCRIPT.SRC --start entry --end 0x400 --annotated
```

实际项目脚本应由 Python 包装这些命令。AstraEngine 不新增 PowerShell 项目脚本。

## AstraEMU release gate inputs

SoftPAL gate 最小输入：

```yaml
family: softpal
nls: sjis
case_root: <local-owned-game-root>
steps:
  - probe
  - boot_to_wait
  - title_input
  - adv_text
  - save_snapshot
  - load_snapshot
report:
  omit_payload: true
  include_hashes: true
  include_screenshots: false
```

Release Gate 输出 JSON。JSON 可以包含 hash、resource count、VM PC、extcall coverage、scene/audio state 和 concern，不包含商业 payload。
