# BGI Implementation Checklist

本清单描述 AstraEMU BGI core 的建议实现顺序和验收口径，不表示当前仓库已经完成这些项。

| Area | 状态 | 交付内容 | 验收 |
| --- | --- | --- | --- |
| Archive index | 未开始 | `PackFile`、`BURIKO ARC20` reader，entry bounds check，name normalization。 | 自造 fixture 通过；三组本地样本 entry count 和 data base 与观测一致。 |
| DSC decode | 未开始 | `DSC FORMAT 1.00` header、Huffman/LZ decode、size 校验。 | 小 fixture roundtrip；样本 script entry decoded magic 可识别。 |
| Script probe | 未开始 | BCS、BP、headerless scenario 检测顺序。 | `00_op_01` 识别为 BCS，`ipl._bp` 识别为 BP。 |
| BCS parser | 未开始 | header、namespace、sub table、dword command、string ref、code end。 | 样本 `header_size`、`body_start`、sub count、opcode histogram 匹配。 |
| BP parser | 未开始 | BP header、byte opcode、relative string、dispatch opcode。 | 三个 `ipl._bp` 样本 header 和 code range 匹配。 |
| VM memory | 未开始 | memory、stack、PC、program table、source map。 | 自造 BP 能 push/call/ret；unknown opcode 有 diagnostic。 |
| Host dispatch | 未开始 | System/Graph/Sound/User group、arg extraction、return value。 | 已知 call name 和 arg count 可追踪；unknown dispatch 不崩溃。 |
| Await token | 未开始 | time/input/animation/audio/movie token。 | token 只在固定 tick 边界完成；replay 顺序稳定。 |
| Presentation | 未开始 | layer model、sprite classification、transition patch。 | graph command 可生成 deterministic patch。 |
| Image decode | 未开始 | CBG probe、raw image probe、RGBA 输出。 | fixture decode；样本可输出 width/height/bpp。 |
| Audio decode | 未开始 | `BurikoWaveBox` unwrap、Ogg/RIFF probe。 | 样本 frequency/channels/header len 读取正确。 |
| Movie probe | 未开始 | MPEG start code、`BF_Movie____` diagnostic。 | movie entry 不误判为 script/image。 |
| Snapshot | 未开始 | VM、presentation、media、await queue section。 | snapshot/replay hash 稳定。 |
| Report | 未开始 | machine-readable probe report。 | 不含私有绝对路径，不含商业 payload。 |
| Docs index | 受限 | 当前只新增 `Docs/emu/bgi/**`。 | 等该目录外 ownership 开放后，再接入上级 README/索引。 |

## 实现顺序

1. 先做 archive index 和 safe path normalization。
2. 加 DSC decode，并把 decoded-kind probe 接到 index report。
3. 实现 BCS/BP parser，只输出 source map、opcode histogram 和 diagnostics。
4. 实现首批 VM：BP push/load/store/call/ret、BCS command iteration、dispatch trace。
5. 接 System/Graph/Sound/User dispatch skeleton，稳定 unknown 行为。
6. 接 presentation/media probe，不急于完整渲染。
7. 加 snapshot/replay，并把 await token 放入固定 tick 队列。
8. 最后接 release gate：fixture tests、metadata smoke tests、report schema 校验。

## Release gate 标准

- `python Tools\check_docs.py` 通过。
- archive fixture tests 覆盖两种 archive 格式。
- DSC fixture tests 覆盖 literal 和 back reference。
- BCS fixture tests 覆盖 header、sub table、string offset、`ret`。
- BP fixture tests 覆盖 header、relative string、dispatch opcode。
- 三个本地 game case 的 probe report 能生成 metadata，且 report 不含私有绝对路径。
- unknown magic、unknown opcode、unknown dispatch 都有 source span 和 diagnostic。
