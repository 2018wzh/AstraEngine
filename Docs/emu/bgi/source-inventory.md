# BGI Source Inventory

本文列出 BGI core 文档采用的事实来源。源码仅作为格式和行为参考，不能直接把翻译工具、hook 代码或商业 payload 流程搬入 AstraEMU runtime。

## 参考源码

| 路径 | 可采用事实 | 不采用内容 |
| --- | --- | --- |
| `ethornell-archive` | `PackFile`、`BURIKO ARC20`、`DSC FORMAT 1.00` header 和安全输出路径规则。 | CLI 的本地输出布局不构成 AstraEMU package contract。 |
| `ethornell-script` | `ScriptFormat` 检测、BCS dword opcode、BP bytecode、call catalog。 | 反编译文本输出格式不作为 runtime API。 |
| `ethornell-vm` | BP VM memory 常量、dispatch trait、yield/stop reason、BCS shadow string 处理。 | demo VM 的 UI 行为不作为 AstraEMU Manager 行为。 |
| `ethornell-image` | `CompressedBG___`、raw BGI image、RGBA 输出规则。 | viewer 交互和调试 UI。 |
| `ethornell-audio` | `BurikoWaveBox` header、Ogg/RIFF payload 探测。 | 本机播放命令。 |
| BGI 工具研究代码 | BCS header 结构、dword instruction table、BP header、CBG/DSC 解码交叉验证。 | 文本替换工作流和编码猜测策略。 |
| 历史 BGI 格式线索 | `BURIKO ARC20` entry layout、BCS text offset 扫描、翻译工具对 header 的处理。 | patch/hook 代码、商业访问控制相关步骤。 |

## 游戏样本

| 本地根目录 | 用途 |
| --- | --- |
| `<bgi-packfile-case>` | `PackFile` 为主的早期/中文发行形态，`._bp` 和 compact scenario payload。 |
| `<bgi-modern-case>` | 混合 `PackFile` 与 `BURIKO ARC20`，大量 BCS scenario、CBG 和 `BurikoWaveBox`。 |
| `<bgi-15th-case>` | `BURIKO ARC20` 全量样本，BCS scenario、MPEG、`BF_Movie____` 和大量 voice box。 |

## 可固化为 core 规则的事实

- archive entry name 使用 CP932/Shift_JIS 风格的 NUL padded byte string。读取时先保留原始 bytes，再生成 normalized resource id。
- `PackFile` 与 `BURIKO ARC20` 的 entry offset 都是相对 data block 的偏移，absolute offset 为 `data_base + relative_offset`。
- 多数 script 和 system resource 在 archive 内先以 `DSC FORMAT 1.00` 包裹，core 必须先 decode payload，再检测 BP、BCS、image、audio 或 movie。
- 现代 scenario payload 通常是 `BurikoCompiledScriptVer1.00`。system program 通常是 `._bp`，但 archive entry 原始 bytes 也常以 DSC magic 开头。
- 文本、选择、声音和图像 command 必须转成 trace/event，不复制原始商业文本块。测试 fixture 只能使用自造小型 payload。

## 明确排除

- 不记录完整商业 script 文本、图像、音频、影片或解密后 payload。
- 不记录 patch、hook、访问控制绕过、DRM 规避或商业保护处理步骤。
- 不把翻译工具里的 GBK 重编码规则当作 runtime 默认行为。runtime 以原发行 payload 的 CP932/Shift_JIS 兼容读取为主，遇到本地化版本时通过 case profile 明确声明。
