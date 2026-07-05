# AstraEMU SoftPAL

本目录记录 AstraEMU 对 SoftPAL family 的兼容目标、资源格式、脚本 VM、extcall、媒体管线和验收清单。事实依据来自本机 `sena-rs` checkout 和本机 Koikake 安装目录的元数据检查；文档只保留格式、计数、hash、命名模式和实现边界，不提交商业 payload、截图、解包结果或绕过访问控制的步骤。

SoftPAL 在 AstraEMU 中只作为 compat core family。Core 持有旧引擎状态机、脚本 VM、资源 resolver 和 family 私有数据；Manager 通过 AstraEMU IPC 接收 `RuntimeEvent`、`PresentationCommand`、`AudioCommand`、`TextCaptureEvent`、trace 和 snapshot reference。SoftPAL 不能把 PAL.dll、旧 VM 指针、GPU/audio native handle 或 Editor widget 传进 EngineCore。

## 文档索引

| 页面 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | `sena-rs` crate、工具和 Koikake 资源观察 |
| [archive-format.md](archive-format.md) | ResourceManager、`ARCHIVE.DAT` 和 PAC 查找规则 |
| [pac-dat.md](pac-dat.md) | PAC、`FILE.DAT`、`TEXT.DAT`、`MEM.DAT`、`POINT.DAT` 字节布局 |
| [script-format.md](script-format.md) | `SCRIPT.SRC` / `Sv20`、operand 和控制流 |
| [script-execution.md](script-execution.md) | VM tick、wait、memory、save snapshot 和 determinism |
| [sv20-extcalls.md](sv20-extcalls.md) | extcall 编号、签名表、stack discipline 和分类 |
| [presentation-and-media.md](presentation-and-media.md) | PGD/TGA、sprite、text、audio、movie 和 render bridge |
| [runtime-core-design.md](runtime-core-design.md) | SoftPAL core 在 AstraEMU 中的最小设计 |
| [game-observations.md](game-observations.md) | Koikake 本地样本的本地结构化资源矩阵 |
| [tooling.md](tooling.md) | 可复现的安全检查命令和本地工具入口 |
| [implementation-checklist.md](implementation-checklist.md) | family 实现和 release gate 清单 |

## Family 边界

SoftPAL core 的输入是用户本地合法安装目录或只读测试 fixture。Core 可以读取 loose file 和 PAC metadata，按 `ARCHIVE.DAT` 建资源路径，解码脚本和媒体，输出 AstraEMU 的中立事件。Core 不负责提供商业资源、不保留完整文本导出、不保存未授权截图，也不提供绕过 DRM、授权、平台保护或商店检查的说明。

最小 boot 路径是：读取 `data`、从 `ARCHIVE.DAT` 加载 PAC path 列表、打开 `Script.src` / `Point.dat` / `File.dat` / `Text.dat` / `Mem.dat`、解析 `Sv20` header、用 `entry_pc` 启动 VM、在固定 tick 边界执行脚本并把等待转成可序列化 token。

## AstraEMU 接口口径

- `ProbeContent` 只报告 family、资源矩阵、hash、版本证据和缺失项。
- `LoadCase` 挂载只读 game root，初始化 ResourceCatalog、ScriptRuntime、MediaResolver 和 Diagnostics。
- `Step` 在固定预算内推进 VM，不能把 Tokio task completion order 写进 deterministic state。
- `ApplyInput` 只写入本 tick 输入边缘；脚本 wait 在下一固定边界消费。
- `SaveSnapshot` / `LoadSnapshot` 保存 VM PC、stack、memory banks、`Mem.dat` shadow、text/history 状态和 family 版本，不保存平台 handle。

## 示例

下面是文档里的抽象事件示例，不是从商业脚本复制的内容：

```text
SoftPalEvent::Text { speaker_id: 0x00001234, text_id: 0x00005678, voice_id: 0x00009ABC }
SoftPalEvent::SpriteSet { slot: 12, resource: "BK000D", x: 0, y: 0, z: 0 }
SoftPalEvent::AudioPlay { group: "bgm", slot: 0, resource: "BGM01", looped: true }
```
