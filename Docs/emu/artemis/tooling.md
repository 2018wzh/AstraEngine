# Artemis Tooling

## 现有工具

仓库已有 `Tools/AstraEMU` 下的 Artemis 工具入口。当前文档只引用，不修改这些文件。

| 工具 | 用途 | 注意 |
| --- | --- | --- |
| `artemis_probe.py` | 扫描游戏根、PFS 包、扩展名、`system.ini`、movie magic | 当前 `parse_pf_archive` 会读完整 PFS；大商业包上应改为 index-only streaming |
| `artemis_pfs.py` | 列出、读取或导出 PF6/PF8 entry | 导出必须要求显式 `--out`，并走 safe path join |
| `artemis_decompile.py` | 轻量扫描 `.iet/.ast/.asb/.sli/.ipt/.tbl` 的 tag/string | 只适合 metadata，不是完整反编译器 |
| `common.py` | binary reader、magic label、PF parser、safe write、text decode | 可复用，但 PFS parser 需要避免整包读入内存 |

## pfs-rs CLI

`pfs-rs` 提供：

```text
pfs-rs list <archive>
pfs-rs extract <archive> [output]
pfs-rs create <input-dir> -o <output.pfs>
```

AstraEMU 不应依赖外部 CLI 执行 runtime lookup；CLI 只用于 fixture 生成、交叉验证和人工 probe。core 需要 Rust library 或等价内部 reader。

## Probe 输出

建议统一 JSON：

```json
{
  "family": "artemis",
  "packs": [
    {
      "name": "game.pfs",
      "format": "pf8",
      "entries": 1823,
      "index_hash": "sha256-prefix",
      "patch_rank": 0
    }
  ],
  "system_ini": {
    "platform": "WINDOWS",
    "width": 1920,
    "height": 1080,
    "charset": "UTF-8",
    "boot": "system/first.iet"
  },
  "scripts": {
    "iet": 7,
    "ast": 353,
    "asb": 3,
    "lua": 56
  },
  "movies": [
    {"name": "movie/opening.wmv", "magic": "asf"}
  ]
}
```

`index_hash`、script hash 和 media hash 可用 prefix；不要输出 payload bytes。

## Synthetic Fixtures

需要自制 fixture，不用商业样本做回归输入：

| Fixture | 内容 |
| --- | --- |
| `minimal_pf6.pfs` | PF6，两个文本 entry，无加密 |
| `minimal_pf8.pfs` | PF8，一个 `.iet`、一个 `.lua`、一个 `.ogg.sli` |
| `patch_chain` | `root.pfs`、`root.pfs.000`、`root.pfs.002`，同名文件覆盖 |
| `boot_utf8` | `system.ini` 指向 UTF-8 `.iet`，含 `calllua` 和 `wait` |
| `boot_sjis` | `system.ini` 默认 Shift_JIS，含非 ASCII path |
| `asb_probe` | 只含公开 tag/string 的 synthetic ASB-like header，用于 magic/report |

商业样本只做本地 opt-in smoke probe，不进入 CI。

## Safety

- 所有导出命令必须显式 `--out`。
- 默认只 list metadata，不 extract。
- 导出路径必须拒绝 traversal。
- 文档和 report 可包含本机绝对路径、offset、entry count 和 hash；不包含完整商业 payload、完整脚本、截图、音频或视频帧。
- 不生成补丁包覆盖商业安装目录；pack/create 只针对 synthetic fixture。

## Minimum Commands

文档改动验证：

```bash
python Tools/check_docs.py
```

工具后续验证建议：

```bash
python Tools/AstraEMU/artemis_decompile.py <fixture>/system/first.iet --json
python Tools/AstraEMU/artemis_pfs.py <fixture>/root.pfs --json
```

这些命令需要 fixture 就绪后再纳入 release gate。
