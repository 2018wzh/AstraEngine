# 实施状态与下一步计划

更新：2026-10-18。本页只维护当前状态和下一步；设计见[架构](../product/architecture.md)与[重构契约](../contracts/rebuild.md)，既有测试和性能数据见[性能审计](performance-upstream-audit-2026-10-12.md)。历史过程留在 Git。

## 当前结论

- 已实现 C ABI v2、显式 module owner、typed 配置、CPU lease 与取消/重试边界，Manager 使用原生运行路径。RFVP 的 D3D12 交付已整合并通过独立硬件验证；七核心结局与完整产品视听验收尚未全部完成。
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
| Manager | 原生 C ABI、驻留库、CPU/D3D12 呈现、本地 Luau；实窗完成同窗口剧情输入、返回/重开、窄屏导航、明暗主题及四类滤镜，修复 Anime4K 移植错误并通过参考像素回归 | 前台物理输入、听音与性能验收；当前实窗证据为自有窗口消息级交互 |
| FVP | 原生 App/VM/媒体/保存、完整 UI CPU 帧；CPU 与 D3D12 的输入、缩略图、fence、旧 lease 和重开硬件验证 | 商业首帧前请求 WINDOWED，被无窗口 Host 拒绝；需实际窗口继续，不能归因上游；字体分发授权未核验 |
| Siglus | 已完成 Rewrite 小鳥结局、ED staff roll、自动回标题、同驻留 DLL 重开；通用空栈暂停 latch 修复单独归因并整理入新原生基线 | 前台输入、听音、长期资源归因及其他平台未完成 |
| Artemis | 原 Vulkan/FFmpeg、新 ABI、17 项普通与 4 项硬件检查；修复解码取消、restore-close 音频竞态和真实窗口状态查询，实窗正常关闭 | 原生失焦暂停使锁屏环境下剧情不推进；正文、商业保存及结局需要真实焦点，不新增后台开关绕过 |
| Musica/SDK | 真实标题/重开、8 声道设备、对白 Hook/错误重试/存读档/网络取消；音频快进背压与 MOV 类型密码、流式落盘已修，真实 OP 播放及影片中保存恢复通过 | OP 后复合图像参数阻塞；确认来源 `00610272` 也没有对应解析，不丢参数或另猜语义。选择项 Hook 实玩、逻辑尺寸收敛、听音及结局待完成 |
| Sena | ABI v2、原生 GPU/设备音频 owner；5 项普通检查，修复准确 Steam Koikake 身份识别 | 真实 probe 已通过，open 要求窗口 Host 而拒绝 headless；无可达帧、商业代表流程或结局，不扩大为所有 SoftPAL |
| Ethornell | 本地 ABI v2 构建，修复格式标识；真实 descriptor/probe、Vulkan 与字体初始化通过 | 首次 VM tick 前请求 window.title，无窗口 Host 拒绝；许可证覆盖待澄清，适配不推送；结局未完成 |
| KrKr | 原生 C ABI，无 child/IPC；完整画布 Alpha 语义、首错保留；真实 Manager 公开画面、输入、两轮重开及 System.exit 通过 | 商业 startup 请求“corrupted execution file”提示后被 headless 拒绝；原插件/商业流程和结局待验收，不归因上游 |
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

已执行：共享 ABI 7 项与 observability 9 项检查及 Clippy；Manager 77 项普通检查、1 项独立参考像素 GPU 回归、真实独立 DLL smoke、自有窗口操作。实窗覆盖缩放、锐化、Anime4K、文件选择器加载/保存外部 HLSL、坏源错误及旧滤镜保留；缩放后的真实 NewGame 输入到达正文，同窗口返回后再次打开标题，最终退出 0。Musica/SDK 整合回归 237 项，随后音频 27 项、归档 10 项、视频 worker/PCM 11 项针对性检查均通过。真实 Musica VM 在 TLS ServerHello 等待中收到原生取消，连接结束、Manager 正常退出，临时测试凭据已删除；不以该用例宣称远端翻译质量。CLI 安装/启用/配置/禁用、语义 Hook 错误重试及存读档后新上下文均有实际运行证据。Artemis 整合树的 17 项普通检查与 Clippy 通过；4 项硬件结果来自独占验收树。各核心商业阻塞与硬件结果保留在修改说明和 ignored 原始输出中。CPAL ALSA 依赖对齐后的 WSL 编译与枚举已通过，不代表 Linux 播放验收。

操作入口：[开发手册](../manual/development.md)、[AstraEMU 客户端](../manual/astraemu-client.md)、[Editor](../../Editor/README.md)、[NativeVN 样例](../../Examples/NativeVN/README.md)、[终之空资料](../samples/tsuinosora-modernization/README.md)。
