# 实施状态与下一步计划

更新：2026-10-18。本页只维护当前状态和下一步；设计见[架构](../product/architecture.md)与[重构契约](../contracts/rebuild.md)，既有测试和性能数据见[性能审计](performance-upstream-audit-2026-10-12.md)。历史过程留在 Git。

## 当前结论

- 已实现 `astra-emu-abi` C 函数表、typed 配置校验、会话/输出/帧代次、独立 CPU lease 和可选诊断桥，并运行真实 C→Rust DLL 边界 smoke。RFVP、Manager 与其他核心的产品调用链仍在迁移；GPU 交接及七核心验收尚未完成。
- 本次 Windows 交付覆盖 RFVP/FVP、Siglus、Artemis、Musica、Sena、Ethornell 和 KrKr；全部重新适配，最大化复用原生能力，最小化修改。CMVS 正在独立开发，本批不处理、不触碰其工作树。
- RFVP 先完成 CPU 完整路径，再独立实现并验收首个 GPU 交付后端；只有 Musica 实现并验证文字 Hook。资源替换 Hook 暂不做，其他核心不新增文字 Hook。
- Slint Manager 重做精炼 Material 3 界面，同窗口切换管理与游戏；迁移全部既有滤镜，提供内置 Luau 翻译扩展和本地扩展管理。
- Windows 正式验收使用主机硬件，可以先 headless 测试；Sandbox 不再是前置条件。七个参与核心各要求至少一个结局，确认的上游缺陷仅跳过受阻项，不记为通过。
- 既有 RFVP、Siglus、Artemis R01–R08、Hosted R09 数据和已发布 astra 分支是旧架构基线，不能算新架构验收。原生功能 PR 的未合并状态及 Sandbox 故障记录保留在审计，不再驱动本批实施顺序。

## 产品状态

| 产品/模块 | 已具备 | 尚未完成 |
| --- | --- | --- |
| Engine/Runtime | 独立 EngineSession、typed 任务/保存、有界诊断与 worker/access plan 复用 | 完整产品的作用域、媒体、恢复、性能与平台组合验收 |
| AstraVN/Player | VnSession、Yakui、公共 Canvas、媒体流、typed 输入/存读档与 Luau 显式 IO | 实际授权 IO、设备音频、跨平台输入/媒体/挂起恢复及长流程 |
| Editor/Agent | GPUI、版本化事务、增量撤销、后台诊断、真实 Player 预览与 ACP/MCP | `--mcp` 持有 stdin 的关闭挂起；图/时间线/Agent 工作流、片段 seek 与三桌面验收 |
| FVP | 原 RFVP GPU/媒体/保存；输入、V1/文字修复；非阻塞 read-into、三槽背压与恢复锚点 | 原生 Embedded、新 C ABI、完整 UI CPU 帧、R 配置、GPU 交接与结局验收 |
| Siglus | 原 renderer/Kira/实时时钟；单 staging Poll、三槽取消/超时与独立完成轮询 | 新 ABI 最小适配、原生流程与结局；GPU 警告及长期资源归因 |
| Artemis | 原 Vulkan/FFmpeg；PF8/AVIO 修复、显式硬件策略与成功恢复通知 | 新 ABI 最小适配、商业窗口/媒体/存读档与结局；既有上游缺陷单独归因 |
| Musica/SDK | 公共文字/媒体/画布、纹理驻留；已有标题/剧情/选择/系统页/自有保存 | 新 ABI、文字 Hook 与共享 Luau 闭环、完整机制整合、逻辑尺寸收敛与结局 |
| Sena | 已指定 sena-rs 原生来源，上游 README 以 Koikake 为目标 | 嵌入能力核对、新 ABI/Surface 最小适配、代表流程及结局 |
| Ethornell | 已指定 ethornell-rs 的 BGI 原生来源 | 嵌入能力与许可核对、新 ABI/Surface 最小适配、代表流程及结局 |
| KrKr | 既有 Kirikiroid2-web 原生适配与子进程传输成果 | 同进程重新适配、生命周期/关闭重开、原插件能力复用与结局 |
| CMVS | 独立开发中的原格式/VM/归档与 SDK 接入 | 本批不修改、整合或验收，保留来源成果 |
| 终之空转换 | Story IR、37 路线输入生成、Classic/Modern 私有包与部分硬件运行 | Classic/Modern 全部 37 路线及系统/演出/视听验收 |
| 平台/发行 | Windows 为主要实测环境，Linux/Android 有历史记录 | 当前整合基线四平台代表流程、macOS/Android 真机、设备音频及依赖/许可核验 |

## 实施顺序

### 1. 共享契约与隔离任务

主线程维护共享 `astra-emu-abi` 与整合工作树 `codex/emu-native-implementation-20261018`，从 `9b34d2d32` 接续。独立 Codex 任务分别负责 RFVP、Manager UI、Manager 运行层、Musica、Siglus、Artemis、Sena、Ethornell 和 KrKr，独占工作树、分支和构建产物；共享边界另由 Codex 只读检查。CMVS 不修改、不编译、不验收。

委派工具没有模型选择参数。上述实施代理的受信任 runtime context 均报告 `openai-codex/gpt-6-astra`，按已授权的 Codex fallback 执行，未宣称使用 `gpt-6.1-sol`。一次只读 scout 报告非 Codex 模型，未修改代码，停止使用该路由。

### 2. RFVP CPU 完整路径与 Manager

复用原生 App 和运行层，直接导出统一 C ABI；完整离屏合成原生游戏 UI 后交付 CPU lease。完成核心 R 配置、自由 resize、输入映射、原生音频、存读档、暂停恢复和关闭重开。CPU 第一里程碑不等待 GPU，不标为无回读。

Manager 在 Slint 中重做精炼 Material 3 界面，保留游戏库/配置/管理功能，同一窗口进入和退出游戏视图。迁移缩放、锐化、Anime4K 与 Magpie format 4 外部 HLSL，在真实产品中验证滤镜、错误显示、核心配置和游戏输入，不能只用静态截图验收。

### 3. Musica 文字 Hook 闭环

本次仅 Musica 接入对白、说话人和选择项 Hook。固定扩展线程持有共享串行 Luau VM，提供内置翻译扩展和本地扩展安装、启停与配置，复用现有服务配置和 secret store。

真实验证同步网络等待、错误暂停、重试无重复副作用、独立原生取消、结束会话和读档上下文失效。RFVP 与其他核心仍做原生运行和结局验收，但不做本次文字 Hook 实现或闭环验证。资源替换 Hook 与在线扩展市场不在本批范围。

### 4. RFVP 首个 GPU 交付后端

复用同一 Runtime，以版本化图形扩展协商共享图像和双向同步，优先保留核心自己的 Device。验证滤镜、resize、配置代次、旧 lease、失败关闭和资源回收。用相同场景和 R/O 对比 CPU 路径，不能用 CPU 成绩代表 GPU 性能。

### 5. 其余核心重新适配与结局

Siglus、Artemis、Musica、Sena、Ethornell、KrKr 全部按新 ABI 重新适配；共享契约稳定后可并行独立任务，不复制 VM、renderer、decoder、调度或保存实现。每个核心至少实现一种真实可用的 Surface，不要求同时实现 CPU/GPU。

Sena/Ethornell 继承指定上游的实际支持范围；KrKr 继续使用既有 Kirikiroid2-web 和已有插件能力。KrKr 必须验证同进程关闭重开，旧 child/IPC 隔离不算完成；必要通用生命周期修复与 Astra 适配分别提交。

RFVP、Siglus、Artemis、Musica、Sena、Ethornell、KrKr 各至少验证一个结局，并覆盖启动、连续剧情、选择、游戏 UI、视频/设备音频、存读档、暂停恢复、输出配置和关闭重开。确认上游问题须在相同固定版本的独立上游路径复现；记录具体证据，仅跳过受阻项，继续其他可达路线/游戏。没有完成结局的核心标记上游阻塞或未完成，不能标通过。

### 6. 整合与后续平台

fork/master 承载独立原生改进，astra 相对该基线仅一个必要适配提交，主仓固定精确 pin。已授权推送本次工作分支与核心 fork，并在验收后合入主线；不改写来源或已发布历史，不自动发布二进制 Release。既有原生 PR 保留独立归因，不能为满足单提交外观混入适配。

本次 Windows 产品验收完成后再逐项推进 Linux/macOS/Android，不能以平台编译结果替代实际运行。Engine/VN、Editor 与终之空保持既有目标，终之空 Classic/Modern 仍需全部 37 路线；不将无关产品重写混入本批 EMU 任务。

## 验证与数据边界

- 普通逻辑检查与真实视听验收分开；测试数量、GPU 夹具和 NullAudio 不替代产品验收。
- 每次代码修改检查受影响 crate，阶段末检查三个活动 workspace；已执行记录与跳过项见审计。文档执行 `python Tools/check_docs.py`。
- 运行时创建隔离测试副本和新存档槽；试验结束清理额外副本与测试存档，保留原有游戏、既有存档和原始运行证据。商业数据只留 ignored 区域，私有路径不入可提交文档。
- 主机硬件 GPU/音频为本批 Windows 正式验收环境；headless 先覆盖自动化可达流程，再运行实际窗口、输入、滤镜、原生菜单和设备音频。软件 adapter、NullAudio、首帧和公开夹具不能替代商业流程及结局。
- 性能记录包含实际 R/O、帧交付方式、回读/上传字节、CPU/GPU 时间、帧池等待和显示延迟；按相同硬件、驱动、Release、游戏版本及语义场景分别记录冷启动与预热。短时平稳不作为长期无泄露结论。

本轮已执行 `astra-emu-abi --features diagnostic-bridge` 的 6 项测试及 all-targets Clippy（`-D warnings`）。真实 C 程序通过生成的头文件调用 Rust DLL，验证 typed 配置、原分配 CPU 像素、跨线程 release、旧会话代次拒绝后的释放、文字输入模态回调与 open 前取消查询；该 smoke 不代表游戏或 GPU 验收。Slint 页面改动已整合，实窗交互、完整产品构建和各核心结局尚未验收。

操作入口：[开发手册](../manual/development.md)、[AstraEMU 客户端](../manual/astraemu-client.md)、[Editor](../../Editor/README.md)、[NativeVN 样例](../../Examples/NativeVN/README.md)、[终之空资料](../samples/tsuinosora-modernization/README.md)。
