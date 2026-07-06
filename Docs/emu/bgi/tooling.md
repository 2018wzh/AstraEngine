# BGI Tooling

BGI 工具链分为参考工具、AstraEMU probe 和验收报告。参考工具用于理解格式；AstraEMU 自身仍要有可复现的 parser、decoder 和 machine-readable report。

## 参考命令

以下命令只在参考仓库中使用，不构成 AstraEngine 的长期 CLI contract：

```bash
cargo run -p ethornell-cli -- arc-list <game-root>
cargo run -p ethornell-cli -- script-info <game-root> <archive> <entry>
cargo run -p ethornell-cli -- vm-trace <game-root> <program>
cargo run -p ethornell-cli -- image-info <game-root> <archive> <entry>
cargo run -p ethornell-cli -- audio-info <game-root> <archive> <entry>
```

BGITool 和 FuckGalEngine 工具主要用于交叉验证 header、offset 和 opcode，不应引入它们的文本替换输出格式、patch hook 或商业 payload 操作。

## AstraEMU probe 命令

当前仓库提供 Python 研究脚本：

```bash
python Tools/AstraEMU/bgi_probe.py <game-root> --json
python Tools/AstraEMU/bgi_arc.py <archive.arc> --json
python Tools/AstraEMU/bgi_arc.py <archive.arc> --decode-dsc --out <extract-dir>
python Tools/AstraEMU/bgi_bcs.py <script.bcs> --json
python Tools/AstraEMU/bgi_bp.py <script._bp> --json
python Tools/AstraEMU/bgi_dsc.py <payload.dsc> --out <decoded.bin> --json
```

报告中保存：

- game root 或 archive path
- entry name
- archive format
- entry index
- `absolute_offset`
- raw size
- decoded kind
- decoded size
- magic
- short hash
- diagnostics

## Safe probe 规则

- 默认只读取 header、entry table 和 payload 前 64 bytes。
- script smoke 可 decode 单个 entry，但只输出 header、opcode count、offset 和 hash，不输出完整文本。
- image smoke 可输出尺寸、bpp、format 和 decode status，不写出图片。
- audio smoke 可输出 codec、frequency、channels、duration estimate 和 header bytes 摘要，不写出音频。
- movie smoke 只输出 magic、size、container guess 和是否需要平台 decoder。
- 所有工具拒绝 path traversal。输出允许包含本地路径、offset、size、hash 和短 header；不输出完整商业 payload。

## Fixture 策略

自动化测试不使用商业 payload。需要覆盖格式时，创建小型自造 fixture：

- `PackFile`：1 个 entry，name 为 ASCII，payload 为小字节串。
- `BURIKO ARC20`：1 个 entry，tail fields 覆盖全零和非零两种。
- `DSC`：用公开可控数据生成小 payload，断言 roundtrip。
- `BCS`：自造 magic、`header_size=36`、1 个 `main@0`、`push_dword`、`ret` 和短 string。
- `BP`：自造 `header_size=16`、`instruction_size`、`push_byte`、`ret`。
- `BurikoWaveBox`：自造 `bw  ` header，payload 可是极短 RIFF/Ogg header mock。

商业样本只作为手动 case report 输入，不进入 repo。

## Report schema 草案

```json
{
  "case_id": "sakura-no-uta-local",
  "tool_version": "0.1",
  "archives": [
    {
      "path": "data01100.arc",
      "format": "BurikoArc20",
      "entry_count": 5,
      "data_base": "0x290",
      "entries": [
        {
          "name": "00_op_01",
          "index": 0,
          "relative_offset": "0x0",
          "absolute_offset": "0x290",
          "raw_size": 34372,
          "decoded_kind": "BcsV1",
          "decoded_size": 114699,
          "diagnostics": []
        }
      ]
    }
  ]
}
```

JSON 中的数字字段可以同时保留 decimal 和 hex string；release gate 比较时应选择一种 canonical form，避免跨语言格式差异。
