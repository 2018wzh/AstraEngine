# 实施状态与下一步计划

更新：2026-10-17。本页只维护当前状态和下一步；设计见[架构](../product/architecture.md)与[重构契约](../contracts/rebuild.md)，精确版本、测试和性能数据见[性能审计](performance-upstream-audit-2026-10-12.md)。历史过程留在 Git。

## 当前结论

- RFVP、Siglus、Artemis 的 R01–R08 整改已实现并验证，适配分支已统一为 `astra` 并发布 GitHub；主仓继续固定精确 revision。
- HEAD＋单适配提交的结构已核验。RFVP/Artemis 原生功能修复尚待上游合并，严格最小钩子内容验收仍未完成。
- R09 已取得 Hosted 整改前后数据；原版性能测试按用户决定取消。正式验收使用 Sandbox，主机记录独立保留。
- `AllowVGPU=1` 已确认，但 Sandbox 在产品启动前崩溃；当前阻塞是宿主运行环境，不再是策略权限。窗口、设备听音、新槽存读档与退出仍待验收。
- CMVS、KrKr 跳过本批适配/整合；保留其既有提交、未提交成果和其他实例工作树。

## 产品状态

| 产品/模块 | 已具备 | 尚未完成 |
| --- | --- | --- |
| Engine/Runtime | 独立 EngineSession、typed 任务/保存、有界诊断与 worker/access plan 复用 | 完整产品的作用域、媒体、恢复、性能与平台组合验收 |
| AstraVN/Player | VnSession、Yakui、公共 Canvas、媒体流、typed 输入/存读档与 Luau 显式 IO | 实际授权 IO、设备音频、跨平台输入/媒体/挂起恢复及长流程 |
| Editor/Agent | GPUI、版本化事务、增量撤销、后台诊断、真实 Player 预览与 ACP/MCP | `--mcp` 持有 stdin 的关闭挂起；图/时间线/Agent 工作流、片段 seek 与三桌面验收 |
| FVP | 原 RFVP GPU/媒体/保存；输入、V1/文字修复；非阻塞 read-into、三槽背压与恢复锚点 | 上游功能修复合并；Sandbox 菜单/槽位/媒体/听音/退出，至少一结局 |
| Siglus | 原 renderer/Kira/实时时钟；单 staging Poll、三槽取消/超时与独立完成轮询 | Sandbox 菜单/槽位/代表流程、设备听音；GPU 警告及长期资源归因 |
| Artemis | 原 Vulkan/FFmpeg；PF8/AVIO 修复、显式硬件策略与成功恢复通知 | 功能 PR 合并；商业窗口/存读档、必要宿主请求与 BGM crossfade 缺口 |
| Musica/SDK | 公共文字/媒体/画布、纹理驻留；已有标题/剧情/选择/系统页/自有保存 | 完整机制整合回归、结局与听音；eden 原版互通及旧 1024×640 逻辑尺寸整合 |
| CMVS | 原格式/VM/归档、SDK 呈现与 GPU 所有权接入 | 首 advance 在 command 548 / offset 200462 返回 ACTION_UNBOUND，需先打通首 tick |
| 终之空转换 | Story IR、37 路线输入生成、Classic/Modern 私有包与部分硬件运行 | Classic/Modern 全部 37 路线及系统/演出/视听验收 |
| 平台/发行 | Windows 为主要实测环境，Linux/Android 有历史记录 | 当前整合基线四平台代表流程、macOS/Android 真机、设备音频及依赖/许可核验 |

## 下一步整改顺序

### 1. 恢复 Sandbox，再完成 R09

先恢复 Windows Sandbox 的稳定启动，再验证硬件 GPU，随后覆盖三个实际游戏的窗口、输入、正文/电影、设备音频、新槽保存/读档、恢复时钟与关闭重开。当前宿主 `26200.9711`、Sandbox `0.8.107.0` 的 `0x3B/0xC0000005` 记录见[环境限制](performance-upstream-audit-2026-10-12.md#资源与环境限制)。

剩余测量包括 GPU submit/completion、readback/depad、CPU 分配、Host upload/present、丢帧、音频欠载和测量开关开销。按相同硬件、驱动、Release、分辨率和游戏版本，以语义场景及真实时间窗口对齐，分别记录冷启动与预热。保留静态零重复回读/上传、不可变三槽 lease、有界队列和非实时 PCM 回收。

### 2. 收敛原生修复与最小适配内容

[六个独立原生 PR](performance-upstream-audit-2026-10-12.md#上游修复)尚待合并。保留已验证功能；合并后取得新 upstream HEAD、移除重复修复，重新整理并验证一个必要适配提交。当前和后续适配统一使用 `astra`，同步 gitlink、Family metadata 和 MODIFICATIONS，不以分支名代替精确 pin，不改写来源或已发布历史。

### 3. 补齐产品流程与资源边界

继续覆盖失败启动、受阻 PCM、媒体替换、关闭错误、旧 lease 与迟到结果；Siglus DXGI Live Object 警告和长期资源增长分别归因。Artemis 的 typed 窗口拒绝保留为明确错误边界，不能计作商业窗口流程通过。

### 4. 继续 Engine/VN、Editor 和四平台验收

EMU 边界收敛后继续实际 IO/媒体/保存、Editor 图/时间线/Agent 完整流程与关闭，再推进终之空路线及四平台。目标保持 Windows FVP/Musica 各一结局、终之空 Classic/Modern 全部 37 路线；其余平台验证代表流程、输入/媒体/存读档/挂起恢复/退出。VN 性能目标与产品边界以[总体架构](../product/architecture.md)和[重构契约](../contracts/rebuild.md)为准。

## 验证与数据边界

- 普通逻辑检查与真实视听验收分开；测试数量、GPU 夹具和 NullAudio 不替代产品验收。
- 每次代码修改检查受影响 crate，阶段末检查三个活动 workspace；已执行记录与跳过项见审计。文档执行 `python Tools/check_docs.py`。
- 运行时创建隔离测试副本和新存档槽；试验结束清理额外副本与测试存档，保留原有游戏、既有存档和原始运行证据。商业数据只留 ignored 区域，私有路径不入可提交文档。
- Sandbox 使用硬件 GPU；软件 adapter 和主机结果不替代本批 Sandbox 验收。当前只报告已观察结果，短时平稳不作为长期无泄露结论。

操作入口：[开发手册](../manual/development.md)、[AstraEMU 客户端](../manual/astraemu-client.md)、[Editor](../../Editor/README.md)、[NativeVN 样例](../../Examples/NativeVN/README.md)、[终之空资料](../samples/tsuinosora-modernization/README.md)。
