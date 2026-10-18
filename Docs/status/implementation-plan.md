# 实施状态与下一步计划

更新：2026-10-18。本页只维护当前状态和下一步；设计见[架构](../product/architecture.md)与[重构契约](../contracts/rebuild.md)，既有测试和性能数据见[性能审计](performance-upstream-audit-2026-10-12.md)。历史过程留在 Git。

## 当前结论

- 已实现 C ABI v2、显式 module owner、typed 配置、CPU lease 与取消/重试边界，Manager 使用原生运行路径。RFVP 的 D3D12 交付已整合并通过独立硬件验证；本轮按最新决定收口到七核心的可玩代表流程。
- 本次 Windows 交付覆盖 RFVP/FVP、Siglus、Artemis、Musica、Sena、Ethornell 和 KrKr；全部重新适配，最大化复用原生能力，最小化修改。CMVS 正在独立开发，本批不处理、不触碰其工作树。
- RFVP 先完成 CPU 完整路径，再独立实现并验收首个 GPU 交付后端；只有 Musica 实现并验证文字 Hook。资源替换 Hook 暂不做，其他核心不新增文字 Hook。
- Slint Manager 重做精炼 Material 3 界面，同窗口切换管理与游戏；迁移全部既有滤镜，提供内置 Luau 翻译扩展和本地扩展管理。
- 用户最新决定：验收只要求可玩，由用户在 Release 版本中人工手动执行；停止所有自动输入、截图、重开、关闭及性能采样，移除为自动验收增加的入口、驱动和测试钩子。编译成功不冒记人工通过。
- 既有 RFVP、Siglus、Artemis R01–R08、Hosted R09 数据和已发布 astra 分支是旧架构基线，不能算新架构验收。原生功能 PR 的未合并状态及 Sandbox 故障记录保留在审计，不再驱动本批实施顺序。

## 产品状态

| 产品/模块 | 已具备 | 尚未完成 |
| --- | --- | --- |
| Engine/Runtime | 独立 EngineSession、typed 任务/保存、有界诊断与 worker/access plan 复用 | 完整产品的作用域、媒体、恢复、性能与平台组合验收 |
| AstraVN/Player | VnSession、Yakui、公共 Canvas、媒体流、typed 输入/存读档与 Luau 显式 IO | 实际授权 IO、设备音频、跨平台输入/媒体/挂起恢复及长流程 |
| Editor/Agent | GPUI、版本化事务、增量撤销、后台诊断、真实 Player 预览与 ACP/MCP | `--mcp` 持有 stdin 的关闭挂起；图/时间线/Agent 工作流、片段 seek 与三桌面验收 |
| Manager | 原生 C ABI、驻留库、CPU/D3D12 呈现、本地 Luau；实窗完成同窗口剧情输入、返回/重开、窄屏导航、明暗主题及四类滤镜，修复 Anime4K 移植错误并通过参考像素回归 | 前台物理输入、听音与性能验收；当前实窗证据为自有窗口消息级交互 |
| FVP | 历史开发已覆盖连续正文、三次选择、菜单存读档、OP、最小化恢复与重开；自动驱动已移除，Release DLL 已构建 | Release 人工验收待用户执行；Debug 低帧率未完成归因，停止自动采样。字体分发授权未核验 |
| Siglus | 已完成 Rewrite 小鳥结局、ED staff roll、自动回标题、同驻留 DLL 重开；通用空栈暂停 latch 修复单独归因并整理入新原生基线 | 前台输入、听音、长期资源归因及其他平台未完成 |
| Artemis | 原 Vulkan/FFmpeg、新 ABI；解码取消、restore-close 音频竞态和真实窗口查询修复保留，自动验收驱动已清理，Release DLL 已构建 | 人工 Release 验收待用户执行；此前只取得通用 `ASTRA_EMU_SESSION_FAILED`，具体原因未归因，不再自动重开复现 |
| Musica/SDK | 真实标题/重开、连续对白 Hook、错误重试、存读档、网络取消；音频快进背压与 MOV 类型密码、流式落盘已修，OP 与影片中保存恢复通过 | OP 后复合图像参数阻塞；确认来源 `00610272` 也没有对应解析，不丢参数或另猜语义。选择项 Hook 实玩与逻辑尺寸收敛仍有缺口 |
| Sena | 真实简中标题、连续剧情/CG、菜单、存读档、关闭重开；新会话读档后保存修复已整合，原生 1920×1080 与设备初始化通过 | 后续目的地流程 `0xCDFF8 → 0xCD6E4` 循环已在固定 native `2d28936c` 独立实窗复现；另有 EV417/MIX 错误，不外推完整兼容 |
| Ethornell/BGI | 用户明确确认人工验收完毕；原生等价 cursor 错误策略修复保留，Release DLL 已构建 | 不再重复自动验收；许可覆盖待澄清，仅本地交付，不 public push |
| KrKr | 原生 C ABI、完整画布 Alpha、首错保留与 WindowEx overlay owner 初始化修复保留 | 自动验收驱动及钩子已移除；清理后 Release 产物供用户人工验收，不把历史开发结果冒充本次人工通过 |
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

真实验证同步网络等待、错误暂停、重试无重复副作用、独立原生取消、结束会话和读档上下文失效。RFVP 与其他核心只做本轮可玩代表流程，不新增文字 Hook，也不要求通关结局。资源替换 Hook 与在线扩展市场不在本批范围。

### 4. RFVP 首个 GPU 交付后端

复用同一 Runtime，以版本化图形扩展协商共享图像和双向同步，优先保留核心自己的 Device。验证滤镜、resize、配置代次、旧 lease、失败关闭和资源回收。用相同场景和 R/O 对比 CPU 路径，不能用 CPU 成绩代表 GPU 性能。

### 5. 其余核心重新适配与可玩验收

Siglus、Artemis、Musica、Sena、Ethornell、KrKr 全部按新 ABI 重新适配；共享契约稳定后可并行独立任务，不复制 VM、renderer、decoder、调度或保存实现。每个核心至少实现一种真实可用的 Surface，不要求同时实现 CPU/GPU。

Sena/Ethornell 继承指定上游的实际支持范围；KrKr 继续使用既有 Kirikiroid2-web 和已有插件能力。KrKr 必须验证同进程关闭重开，旧 child/IPC 隔离不算完成；必要通用生命周期修复与 Astra 适配分别提交。

七个参与核心均交付 Release 运行包，由用户手动进入剧情、连续交互、操作基本菜单与存读档、退出重开；不继续自动追结局或采样。BGI 按用户确认记录为人工通过。既有缺陷和未确认项保留，不改脚本、不伪造 Host 返回值，也不通过自动化操作用户窗口。

### 6. 整合与后续平台

fork/master 承载独立原生改进，astra 相对该基线仅一个必要适配提交，主仓固定精确 pin。已授权推送本次工作分支与核心 fork，并在验收后合入主线；不改写来源或已发布历史，不自动发布二进制 Release。既有原生 PR 保留独立归因，不能为满足单提交外观混入适配。

本次 Windows 产品验收完成后再逐项推进 Linux/macOS/Android，不能以平台编译结果替代实际运行。Engine/VN、Editor 与终之空保持既有目标，终之空 Classic/Modern 仍需全部 37 路线；不将无关产品重写混入本批 EMU 任务。

## 验证与数据边界

- 普通逻辑检查与真实视听验收分开；测试数量、GPU 夹具和 NullAudio 不替代产品验收。
- 每次代码修改检查受影响 crate，阶段末检查三个活动 workspace；已执行记录与跳过项见审计。文档执行 `python Tools/check_docs.py`。
- 运行时创建隔离测试副本和新存档槽；试验结束清理额外副本与测试存档，保留原有游戏、既有存档和原始运行证据。商业数据只留 ignored 区域，私有路径不入可提交文档。
- 主机硬件 GPU/音频是人工运行环境；不再使用脚本式 headless、模拟输入、自动截图和定时关闭作验收。用户确认的手动误关闭按人为中断记录，不归因核心或计作结局。
- RFVP 低帧率的既有证据保留：运行配置为 Debug+CPU、滤镜关闭；短 CPU 采样指向 Manager 主线程饱和，完整帧时归因未完成。后续交付 Release 供用户观察，不继续自动采样或对照，不把窗口 FPS 当 VM Tick。

清理前的开发证据包括 ABI/日志边界、Manager 同窗口输入与四类滤镜、Musica Hook 重试/读档上下文/真实 TLS 等待取消、RFVP 原生 UI/电影/选择及其他核心的代表流程。原始日志和已知问题保留；这些记录不冒充清理后的 Release 人工通过。自动验收入口、硬件/路线驱动、公开假核心、额外截图/输入脚本和测试访问钩子已删除，普通纯逻辑测试保留；本次清理后不再运行自动验收。

共享改动已通过 root workspace 的 `cargo check --workspace --all-targets --features astra-media/ffmpeg-vcpkg`，以及 Editor 独立 workspace 的全 target check。Emulator 按受影响包及独占 Family 构建验证，CMVS 未编译；编译结果不替代运行证据。保留依赖 `binrw` 和 `proc-macro-error2` 的未来 Rust 兼容警告。

操作入口：[开发手册](../manual/development.md)、[AstraEMU 客户端](../manual/astraemu-client.md)、[Editor](../../Editor/README.md)、[NativeVN 样例](../../Examples/NativeVN/README.md)、[终之空资料](../samples/tsuinosora-modernization/README.md)。
