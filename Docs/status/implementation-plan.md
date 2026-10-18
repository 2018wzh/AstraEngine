# 实施状态与下一步计划

更新：2026-10-18。本页只维护当前状态和下一步；设计见[架构](../product/architecture.md)与[重构契约](../contracts/rebuild.md)，既有测试和性能数据见[性能审计](performance-upstream-audit-2026-10-12.md)。历史过程留在 Git。

## 当前结论

- 已实现 C ABI v2、显式 module owner、typed 配置、CPU lease 与取消/重试边界，Manager 使用原生运行路径。RFVP 的 D3D12 交付已整合并通过独立硬件验证；本轮按最新决定收口到七核心的可玩代表流程。
- 本次 Windows 交付覆盖 RFVP/FVP、Siglus、Artemis、Musica、Sena、Ethornell 和 KrKr。Musica/CMVS 属于自有引擎，可直接嵌入适配能力并与 SDK 深度集成；其余外部核心保持薄适配、最大化复用原生实现。CMVS 仍由独立任务开发，本批不触碰其工作树。
- RFVP 先完成 CPU 完整路径，再独立实现并验收首个 GPU 交付后端；只有 Musica 实现并验证文字 Hook。资源替换 Hook 暂不做，其他核心不新增文字 Hook。
- Slint Manager 重做精炼 Material 3 界面，同窗口切换管理与游戏；迁移全部既有滤镜，提供内置 Luau 翻译扩展和本地扩展管理。
- 用户最新决定：验收只要求可玩，由用户在 Release 版本中人工手动执行；停止所有自动输入、截图、重开、关闭及性能采样，移除为自动验收增加的入口、驱动和测试钩子。编译成功不冒记人工通过。
- Manager 已提供“引擎公共设置”，分离滤镜前原生渲染分辨率与模拟器专属选项，映射 RFVP/Musica/Sena 现有参数，其余核心明确显示当前不可调整。Musica 默认 `title` 菜单启动，显式 `direct` 保留；运行时仍只读取 `keys.v2` 的 `key.toml`，其他格式转换只在开发 CLI。引擎/模拟器分离、按游戏选择、光标与输入适配修复保留，产品仍由用户人工 Release 验收。
- 既有 RFVP、Siglus、Artemis R01–R08、Hosted R09 数据和已发布 astra 分支是旧架构基线，不能算新架构验收。原生功能 PR 的未合并状态及 Sandbox 故障记录保留在审计，不再驱动本批实施顺序。

## 产品状态

| 产品/模块 | 已具备 | 尚未完成 |
| --- | --- | --- |
| Engine/Runtime | 独立 EngineSession、typed 任务/保存、有界诊断与 worker/access plan 复用 | 完整产品的作用域、媒体、恢复、性能与平台组合验收 |
| AstraVN/Player | VnSession、Yakui、公共 Canvas、媒体流、typed 输入/存读档与 Luau 显式 IO | 实际授权 IO、设备音频、跨平台输入/媒体/挂起恢复及长流程 |
| Editor/Agent | GPUI、版本化事务、增量撤销、后台诊断、真实 Player 预览与 ACP/MCP | `--mcp` 持有 stdin 的关闭挂起；图/时间线/Agent 工作流、片段 seek 与三桌面验收 |
| Manager | 同窗口原生会话、CPU/D3D12、Luau 与既有滤镜；公共原生分辨率与专属参数共用配置范围及保存/重置；按游戏保存模拟器选择，schema 4→5 保留资料库；Host 统一光标显隐与完整有界错误 | 新分组、渲染配置与窗口行为仍待用户在 Release 中人工验证，不以逻辑检查代替实窗 |
| FVP | 历史开发已覆盖连续正文、三次选择、菜单存读档、OP、最小化恢复与重开；自动驱动已移除，Release DLL 已构建 | Release 人工验收待用户执行；Debug 低帧率未完成归因，停止自动采样。字体分发授权未核验 |
| Siglus | 正常提交文本、翻页/标点/数字小键盘沿用原生输入路径，保留错误 cause chain；9 项纯回归通过、Release DLL 已构建；既有长流程见审计 | 旧日志只保留 `ASTRA_EMU_SIGLUS_ARGUMENT`，不能确定当次触发输入；本轮人工 Release 复测、听音及其他平台未完成 |
| Artemis | 原 Vulkan/FFmpeg 与既有生命周期修复保留；Host 独占光标显隐并在 Slint 分发后同步；18 项核心纯回归通过、Release DLL 已构建 | 光标修复的人工 Release 验收待用户执行；不再自动重开复现旧通用失败 |
| Musica/SDK | 默认从原生 Title 菜单启动；运行时只读完整 `key.toml`，旧格式转换仅在开发 CLI；既有 key/PAZ 与 CLI 纯回归保留，新的核心 Release 已构建且实际 descriptor 默认值已核对 | 默认 Title 与渲染配置的人工复测待用户执行；OP 后复合图像参数、选择项 Hook 实玩与逻辑尺寸收敛的既有缺口保留，不猜测脚本语义 |
| SoftPAL / Sena | 引擎标识为 `softpal`，模拟器标识保持 `sena-native-koikake`；既有 Koikake 范围不变；5 项纯回归通过、Release DLL 已构建 | 既有目的地 `0xCDFF8 → 0xCD6E4` 循环及 EV417/MIX 问题保留，不外推完整 SoftPAL 兼容 |
| Ethornell/BGI | 用户明确确认人工验收完毕；原生等价 cursor 策略修复保留，Release DLL 已构建；源码已按本次授权发布到 fork 的 astra | 不再重复自动验收；许可覆盖仍待澄清，二进制只作本地交付 |
| KrKr | 原生 C ABI、完整画布 Alpha、首错保留与 WindowEx overlay owner 初始化修复保留 | 自动验收驱动及钩子已移除；清理后 Release 产物供用户人工验收，不把历史开发结果冒充本次人工通过 |
| CMVS | 独立开发中的原格式/VM/归档与 SDK 接入 | 本批不修改、整合或验收，保留来源成果 |
| 终之空转换 | Story IR、37 路线输入生成、Classic/Modern 私有包与部分硬件运行 | Classic/Modern 全部 37 路线及系统/演出/视听验收 |
| 平台/发行 | Windows 为主要实测环境，Linux/Android 有历史记录 | 当前整合基线四平台代表流程、macOS/Android 真机、设备音频及依赖/许可核验 |

## 实施顺序

### 1. 共享契约与隔离任务

主线程维护共享 `astra-emu-abi` 与整合工作树，从 `9b34d2d32` 接续，稳定发布分支为主仓 `astra`。本轮从现有稳定成果建立独占 detached 工作树，分别整合 Manager、Musica 和外部适配；不覆盖其他实例的工作区或建立新临时分支。CMVS 不修改、不编译、不验收。

委派工具没有模型选择参数。本轮问题修复、Release 构建和 key-only 收口代理的受信任 runtime context 均确认 `openai-codex/gpt-6.1-sol`。此前实施曾使用已授权的 `gpt-6-astra` fallback；一次旧只读 scout 为非 Codex，未修改代码且已停止使用该路由。

### 2. RFVP CPU 完整路径与 Manager

复用原生 App 和运行层，直接导出统一 C ABI；完整离屏合成原生游戏 UI 后交付 CPU lease。完成核心 R 配置、自由 resize、输入映射、原生音频、存读档、暂停恢复和关闭重开。CPU 第一里程碑不等待 GPU，不标为无回读。

Manager 在 Slint 中重做精炼 Material 3 界面，保留游戏库/配置/管理功能，同一窗口进入和退出游戏视图。迁移缩放、锐化、Anime4K 与 Magpie format 4 外部 HLSL，在真实产品中验证滤镜、错误显示、核心配置和游戏输入，不能只用静态截图验收。

### 3. Musica 文字 Hook 闭环

本次仅 Musica 接入对白、说话人和选择项 Hook。固定扩展线程持有共享串行 Luau VM，提供内置翻译扩展和本地扩展安装、启停与配置，复用现有服务配置和 secret store。

真实验证同步网络等待、错误暂停、重试无重复副作用、独立原生取消、结束会话和读档上下文失效。RFVP 与其他核心只做本轮可玩代表流程，不新增文字 Hook，也不要求通关结局。资源替换 Hook 与在线扩展市场不在本批范围。

### 4. RFVP 首个 GPU 交付后端

复用同一 Runtime，以版本化图形扩展协商共享图像和双向同步，优先保留核心自己的 Device。验证滤镜、resize、配置代次、旧 lease、失败关闭和资源回收。用相同场景和 R/O 对比 CPU 路径，不能用 CPU 成绩代表 GPU 性能。

### 5. 其余核心重新适配与可玩验收

Siglus、Artemis、Sena、Ethornell、KrKr 按新 ABI 做薄适配，不复制或替换其 VM、renderer、decoder、调度、音频与存档。Musica/CMVS 允许将适配直接融入自有引擎与 SDK，删除多余包装和转发；这是设计授权，不代表深集成重构已经完成。所有核心仍遵守统一 ABI、资源生命周期和取消边界，每个核心至少提供一种实际可用的 Surface。

Sena/Ethornell 继承指定上游的实际支持范围；KrKr 继续使用既有 Kirikiroid2-web 和已有插件能力。KrKr 必须验证同进程关闭重开，旧 child/IPC 隔离不算完成；必要通用生命周期修复与 Astra 适配分别提交。

七个参与核心均交付 Release 运行包，由用户手动进入剧情、连续交互、操作基本菜单与存读档、退出重开；不继续自动追结局或采样。BGI 按用户确认记录为人工通过。既有缺陷和未确认项保留，不改脚本、不伪造 Host 返回值，也不通过自动化操作用户窗口。

### 6. 整合与后续平台

核心适配统一推送各 fork 的 `astra`，主仓固定精确 pin，主仓工作成果也保存在 `astra`；没有合入或改动其他实例的 `master`。用户本次明确批准带 lease 的强推覆盖，并清理本轮本地及远端临时分支；旧引用先存本地 Git bundle，其他任务分支与 CMVS 保留。该授权不等于公开发布二进制 Release，也不改变人工验收要求。

本次 Windows 产品验收完成后再逐项推进 Linux/macOS/Android，不能以平台编译结果替代实际运行。Engine/VN、Editor 与终之空保持既有目标，终之空 Classic/Modern 仍需全部 37 路线；不将无关产品重写混入本批 EMU 任务。

## 验证与数据边界

- 普通逻辑检查与真实视听验收分开；测试数量、GPU 夹具和 NullAudio 不替代产品验收。
- 每次代码修改检查受影响 crate，阶段末检查三个活动 workspace；已执行记录与跳过项见审计。文档执行 `python Tools/check_docs.py`。
- 运行时创建隔离测试副本和新存档槽；试验结束清理额外副本与测试存档，保留原有游戏、既有存档和原始运行证据。商业数据只留 ignored 区域，私有路径不入可提交文档。
- 主机硬件 GPU/音频是人工运行环境；不再使用脚本式 headless、模拟输入、自动截图和定时关闭作验收。用户确认的手动误关闭按人为中断记录，不归因核心或计作结局。
- RFVP 低帧率的既有证据保留：运行配置为 Debug+CPU、滤镜关闭；短 CPU 采样指向 Manager 主线程饱和，完整帧时归因未完成。后续交付 Release 供用户观察，不继续自动采样或对照，不把窗口 FPS 当 VM Tick。

清理前的开发证据包括 ABI/日志边界、Manager 同窗口输入与四类滤镜、Musica Hook 重试/读档上下文/真实 TLS 等待取消、RFVP 原生 UI/电影/选择及其他核心的代表流程。原始日志和已知问题保留；这些记录不冒充清理后的 Release 人工通过。自动验收入口、硬件/路线驱动、公开假核心、额外截图/输入脚本和测试访问钩子已删除，普通纯逻辑测试保留；本次清理后不再运行自动验收。

上一版 key-only 七核心本地 Release 包保留。本轮已重建 Manager 与 Musica，并组装包含公共分辨率设置和 Title 默认值的新包，其余六个核心 DLL 复用，不重做 BGI 游戏验收。58 个 PE 均为 x64，七个核心均导出统一 query，静态依赖闭包无缺项；运行包仅包含一个 Manager executable，依赖、Resources 与许可完整保留，不纳入开发转换 CLI 或自动验收驱动。RFVP 字体及 Ethornell 许可未澄清，不公开发布二进制 Release。

前一轮共享改动已通过 root workspace 的 `cargo check --workspace --all-targets --features astra-media/ffmpeg-vcpkg`，以及 Editor 独立 workspace 的全 target check。本轮聚焦受影响的 Emulator 包及独占核心构建，未改动 Engine/Editor 的共享接口，CMVS 未编译；编译结果不替代运行证据。保留依赖 `binrw` 和 `proc-macro-error2` 的未来 Rust 兼容警告。

前一轮 key-only 的 97 项定向纯逻辑回归通过：资料库 7、Manager 识别/选择 10 与配置 2、指针 5、Musica key 10 与 PAZ 11、开发 CLI 20、Siglus 9、Artemis 18、Sena 5。检查使用临时合成数据，不触碰用户游戏、原 key 或实际资料库；未启动游戏窗口。

前一轮实际执行运行包的 Manager `--help`，以及开发 CLI 三个转换子命令的 `--help`；公开合成 JSON 与 keys.v1 各完成一次真实转换，验证 keys.v2 TOML、显式 `false`、算法参数与可选项保留，重复输出明确拒绝且源文件哈希不变。

本轮 16 项定向纯逻辑检查通过：公共/专属配置与作用域 5、识别与模拟器选择 10、Musica 标题状态 1。覆盖配置混合保存、重开继承、非法边界不覆盖、模拟器草稿隔离及已有标题语义；未运行 GPU/游戏夹具。实际查询新 Musica Release 的 C ABI descriptor，默认 `launch_mode=title`、显式 `direct` 选项保留，未调用 open/frame；新包中的 Manager `--help` 通过。未操作用户游戏窗口、资料库、密钥或存档；新分组、实际渲染尺寸和默认 Title 的实窗验收由用户手动执行。

操作入口：[开发手册](../manual/development.md)、[AstraEMU 客户端](../manual/astraemu-client.md)、[Editor](../../Editor/README.md)、[NativeVN 样例](../../Examples/NativeVN/README.md)、[终之空资料](../samples/tsuinosora-modernization/README.md)。
