# 实施状态与下一步计划

更新：2026-10-19。本页只维护当前状态和下一步；设计见[架构](../product/architecture.md)与[重构契约](../contracts/rebuild.md)，既有测试和性能数据见[性能审计](performance-upstream-audit-2026-10-12.md)。历史过程留在 Git。

## 当前结论

- 已实现 C ABI v2、显式 module owner、typed 配置、CPU lease 与取消/重试边界，Manager 使用原生运行路径。RFVP 的 D3D12 交付已整合并通过独立硬件验证；本轮按最新决定收口到七核心的可玩代表流程。
- 本次 Windows 交付覆盖 RFVP/FVP、Siglus、Artemis、Musica、Sena、Ethornell 和 KrKr。Musica/CMVS 属于自有引擎，可直接嵌入适配能力并与 SDK 深度集成；其余外部核心保持薄适配、最大化复用原生实现。CMVS 仍由独立任务开发，本批不触碰其工作树。
- RFVP 先完成 CPU 完整路径，再独立实现并验收首个 GPU 交付后端；只有 Musica 实现并验证文字 Hook。资源替换 Hook 暂不做，其他核心不新增文字 Hook。
- Slint Manager 重做精炼 Material 3 界面，同窗口切换管理与游戏；迁移全部既有滤镜，提供内置 Luau 翻译扩展和本地扩展管理。
- 用户最新决定：验收只要求可玩，由用户在 Release 版本中人工手动执行；停止所有自动输入、截图、重开、关闭及性能采样，移除为自动验收增加的入口、驱动和测试钩子。编译成功不冒记人工通过。
- Manager 已提供“引擎公共设置”，分离滤镜前原生渲染分辨率与模拟器专属选项，映射 RFVP/Musica/Sena 现有参数，其余核心明确显示当前不可调整。Musica 默认 `title` 菜单启动，显式 `direct` 保留；运行时仍只读取 `keys.v2` 的 `key.toml`，其他格式转换只在开发 CLI。引擎/模拟器分离、按游戏选择、光标与输入适配修复保留，产品仍由用户人工 Release 验收。
- 启动优化改为手动刷新：先显示缓存资料库，不自动加载核心或识别游戏；“刷新核心”只异步加载核心，“扫描游戏”与“添加游戏”才刷新资料库，核心就绪不隐式变更游戏绑定。手动扫描按变化复用有效识别提示，每次启动重新校验所选核心。按需封面、批量元数据/统计及惰性扩展/滤镜资源保留；只向既有日志添加阶段标记，不恢复自动验收或性能采样。实窗效果待用户手动观察。
- 当前 Beta2 将文件转换收口到唯一 `astra-emu-manager` 可执行文件：无参数为 GUI，命令行为 `patch list` / `patch apply`；GUI 直接使用共享 `astra-emu-patch` 库。转换仍由用户显式发起，CLI 不启动 GUI、核心或游戏；原生取消清理并 join 后返回 130。Beta1 双程序包保持历史冻结，新构建和打包不再提供独立补丁 EXE。
- 既有 RFVP、Siglus、Artemis R01–R08、Hosted R09 数据和已发布 astra 分支是旧架构基线，不能算新架构验收。原生功能 PR 的未合并状态及 Sandbox 故障记录保留在审计，不再驱动本批实施顺序。

## 产品状态

| 产品/模块 | 已具备 | 尚未完成 |
| --- | --- | --- |
| Engine/Runtime | 独立 EngineSession、typed 任务/保存、有界诊断与 worker/access plan 复用 | 完整产品的作用域、媒体、恢复、性能与平台组合验收 |
| AstraVN/Player | VnSession、Yakui、公共 Canvas、媒体流、typed 输入/存读档与 Luau 显式 IO | 实际授权 IO、设备音频、跨平台输入/媒体/挂起恢复及长流程 |
| Editor/Agent | GPUI、版本化事务、增量撤销、后台诊断、真实 Player 预览与 ACP/MCP | `--mcp` 持有 stdin 的关闭挂起；图/时间线/Agent 工作流、片段 seek 与三桌面验收 |
| Manager | 同窗口原生会话、CPU/D3D12、Luau 与既有滤镜；公共渲染配置与按游戏模拟器选择；缓存资料库首窗、异步核心/扫描、持久识别提示、按需封面与批量快照、惰性扩展及滤镜资源；Host 统一光标与有界错误 | 启动、惰性资源、新分组、渲染配置与窗口行为仍待用户在 Release 中人工验证，不以逻辑检查代替实窗 |
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

### 1. 共享接口与任务拆分

主线程维护共享 `astra-emu-abi` 与整合工作树，从 `9b34d2d32` 接续。主仓成果合入 `master`；外部引擎适配保留在各 fork 的 `astra`，由主仓固定精确 pin。独立模块按接口明确逐步实施，不等待全部设计冻结；协作与检查按[开发手册](../manual/development.md)执行。主线整合使用独占临时工作树，保留其他实例的工作区及未提交内容。CMVS 不修改、不编译、不验收。

委派工具没有模型选择参数。本轮问题修复、Release 构建和 key-only 收口代理的受信任 runtime context 均确认 `openai-codex/gpt-6.1-sol`。此前实施曾使用已授权的 `gpt-6-astra` fallback；一次旧只读 scout 为非 Codex，未修改代码且已停止使用该路由。

启动优化的四个子任务及两项手动刷新调整均由已核实的 `openai-codex/gpt-6.1-sol` 执行，分别使用独占 detached 工作树；子代理不运行构建，主线程整合后统一检查。未改变原生核心 pin、ABI 或 CMVS。

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

按用户最新授权，主仓已验证成果整合到远端 `master`，外部核心适配保留在各 fork 的 `astra`，主仓固定精确 pin；不覆盖另一实例已检出的本地 `master` 及其未提交内容。此前批准的核心强推使用 lease，旧引用已存 Git bundle；只清理本任务的临时引用与整合工作树，其他任务分支与 CMVS 保留。主线整合不等于公开发布二进制 Release，也不计作人工游戏验收通过。

本次 Windows 产品验收完成后再逐项推进 Linux/macOS/Android，不能以平台编译结果替代实际运行。Engine/VN、Editor 与终之空保持既有目标，终之空 Classic/Modern 仍需全部 37 路线；不将无关产品重写混入本批 EMU 任务。

## 验证与数据边界

- 普通逻辑检查与真实视听验收分开；测试数量、GPU 夹具和 NullAudio 不替代产品验收。
- 按改动范围运行相关检查，跨 workspace 改动及交付时扩大验证；日常操作见[开发手册](../manual/development.md)，已有结果见审计。
- 运行时创建隔离测试副本和新存档槽；试验结束清理额外副本与测试存档，保留原有游戏、既有存档和原始运行证据。商业数据只留 ignored 区域，私有路径不入可提交文档。
- 主机硬件 GPU/音频是人工运行环境；不再使用脚本式 headless、模拟输入、自动截图和定时关闭作验收。用户确认的手动误关闭按人为中断记录，不归因核心或计作结局。
- RFVP 低帧率的既有证据保留：运行配置为 Debug+CPU、滤镜关闭；短 CPU 采样指向 Manager 主线程饱和，完整帧时归因未完成。后续交付 Release 供用户观察，不继续自动采样或对照，不把窗口 FPS 当 VM Tick。

清理前的开发证据包括 ABI/日志边界、Manager 同窗口输入与四类滤镜、Musica Hook 重试/读档上下文/真实 TLS 等待取消、RFVP 原生 UI/电影/选择及其他核心的代表流程。原始日志和已知问题保留；这些记录不冒充清理后的 Release 人工通过。自动验收入口、硬件/路线驱动、公开假核心、额外截图/输入脚本和测试访问钩子已删除，普通纯逻辑测试保留；本次清理后不再运行自动验收。

前一轮已保留 key-only 包，并重建 Manager 与 Musica，组装包含公共分辨率设置和 Title 默认值的七核心本地 Release；其余六个核心 DLL 复用，不重做 BGI 游戏验收。该包的 58 个 PE 均为 x64，七个核心均导出统一 query，静态依赖闭包无缺项；仅包含一个 Manager executable，依赖、Resources 与许可完整保留，不纳入开发转换 CLI 或自动验收驱动。RFVP 字体及 Ethornell 许可未澄清，不公开发布二进制 Release。

前一轮共享改动已通过 root workspace 的 `cargo check --workspace --all-targets --features astra-media/ffmpeg-vcpkg`，以及 Editor 独立 workspace 的全 target check。本轮聚焦受影响的 Emulator 包及独占核心构建，未改动 Engine/Editor 的共享接口，CMVS 未编译；编译结果不替代运行证据。保留依赖 `binrw` 和 `proc-macro-error2` 的未来 Rust 兼容警告。

前一轮 key-only 的 97 项定向纯逻辑回归通过：资料库 7、Manager 识别/选择 10 与配置 2、指针 5、Musica key 10 与 PAZ 11、开发 CLI 20、Siglus 9、Artemis 18、Sena 5。检查使用临时合成数据，不触碰用户游戏、原 key 或实际资料库；未启动游戏窗口。

前一轮实际执行运行包的 Manager `--help`，以及开发 CLI 三个转换子命令的 `--help`；公开合成 JSON 与 keys.v1 各完成一次真实转换，验证 keys.v2 TOML、显式 `false`、算法参数与可选项保留，重复输出明确拒绝且源文件哈希不变。

前一轮公共分辨率/Title 的 16 项定向纯逻辑检查通过：公共/专属配置与作用域 5、识别与模拟器选择 10、Musica 标题状态 1。覆盖配置混合保存、重开继承、非法边界不覆盖、模拟器草稿隔离及已有标题语义；未运行 GPU/游戏夹具。实际查询该 Musica Release 的 C ABI descriptor，默认 `launch_mode=title`、显式 `direct` 选项保留，未调用 open/frame；运行包中的 Manager `--help` 通过。未操作用户游戏窗口、资料库、密钥或存档；新分组、实际渲染尺寸和默认 Title 的实窗验收由用户手动执行。

本轮启动与手动刷新改动通过 115 项纯逻辑回归：Manager 库 20、应用 47、资料库/Core 40、Slint 模型/封面 8。覆盖手动核心加载不变更游戏绑定、显式扫描后才发现游戏、缓存失效/偏好/重复 ID、选中核心的启动前校验、批量统计、封面过期任务和惰性滤镜状态。另执行公开 API 命令，真实创建并重开临时 SQLite，验证统计快照与非标准尺寸的惰性滤镜描述；指定不存在的 DXC 路径仍能完成元数据准备，未创建窗口、GPU 或游戏会话。文档检查 181 项通过；固定编译器实际报告 `1.8.2502.8`。

三个受影响 Manager crate 的全 target Clippy（`--no-deps -D warnings`）及 locked Release 构建通过。新手动刷新运行包仅重建 Manager，七核心 DLL 保持前一版哈希；随包 52 个运行 DLL 包含固定 DXC/dxil，60 个 PE 均为 x64，七个核心 query 导出及静态依赖闭包已核对。实际运行新包 Manager `--help` 通过，PATH 仅保留 Windows 系统目录；未打开 GUI 或游戏。六个代理工作树已在 Git bundle 备份后删除，主整合工作树、旧运行包和其他任务保留。首窗实际耗时、交互、听音与滤镜仍由用户人工 Release 验收。

本次主线整合保留 `master` 的开发流程修改，并合入 `astra@fa6c80a70`；三处冲突均为文档，运行逻辑未改，仅按工作区 Rust 2021 配置统一 Manager 格式。三个工作区实际 locked Cargo metadata 解析、文档及受影响包格式检查通过；同实现的 115 项既有回归可执行文件复跑和既有 Release `--help` 通过，属于缓存产物复核，不记为重新编译。六个引擎 fork 的 `astra` 与既定源码提交一致，音频依赖保留版本分支；本地已检出的 `master`、其未提交内容及 CMVS 工作树均不覆盖。

2026-10-19 本轮离线补丁任务先保留并验证既有 Manager/UI/会话菜单源码。RFVP、Siglus 已按父仓 gitlink 恢复，并初始化固定 Kira 嵌套依赖；完整 `cargo metadata --locked` 解析 1166 个包。源码及索引已备份到仓库外，授权的私有逆向临时目录已移出并核对全部文件哈希，CMVS 未改动。

该基线的 ABI、Manager/Core/Slint 共 139 项纯逻辑检查和 Musica 原生边界 4 项检查通过。补齐快速读档的文字上下文失效、核心更新失败保留旧文件、Bangumi 草稿隔离、旧外观数据迁移、key 选择卡片与搜索焦点；删除仅锁定界面文案/按键映射的测试。另以实际公开 API 程序创建并重开临时 SQLite，验证旧外观迁移且游戏记录不变。新增会话菜单 C 头已用固定 cbindgen 0.29.2 生成。尚未操作真实 Manager 界面或商业游戏，不能记为新增 UI/游戏人工通过。

受影响五个 crate 的全 target Clippy（`--no-deps -D warnings`）通过；按当前编译器建议调整等价的有界字节分块后，Musica 的 10 项 key/profile 纯回归通过。实际生成并运行 MSVC x64 C 消费程序，核对会话菜单结构布局；实际重建 Manager，`--help` 通过。文档链接与数据边界检查 181 项通过。这些检查未启动商业游戏或操作真实 Manager 窗口。

独立共享库 `astra-emu-patch` 已实现只读输入、私有目录事务、新 VM 的 Luau API 和 SHA256 收据。52 项公开行为检查及该 crate 的全 target Clippy 通过；另运行真实 API 程序，读取合成 x64 PE overlay 的四个 zlib/CRC 块，修补两段文本和两张 BMP，逐字节匹配独立期望、验证完整副本及收据/profile，原始输入不变。取消、既有输出、链接逃逸、解压边界和小数偏移均有回归。当前 mlua 0.10.5 的 epsilon 数值分类会误写极小数和负零，已以保留许可的源码固定依赖回补；其既有 23 项生命周期写法警告仍是第三方警告，没有通过关闭产品检查隐藏。

固定 mlua 回补后重新编译并执行 Manager/Core/Slint 的 128 项消费者检查，全部通过；转换库最终 52 项检查和独立 API 烟测也再次通过。未启用 Artemis Lua 5.1 的合并构建，未改变文字 Hook 的生命周期或串行队列。

## 本地化：原生优先的当前状态

本轮按 18 个已安装样本逐项判断，其中 13 个有已确认的本地化候选，另 5 个尚缺已确认译文。优先复用或补齐核心的语言、字体、归档、脚本及媒体机制；只有旧封装/DLL 中无法直接消费的数据才离线转换为标准原生资源和 typed 配置。没有“13 个脚本”的配额。转换器 Luau 只在用户主动转换时运行，原生会话不加载转换脚本，也不新增运行时 Lua 格式或转换器依赖；Artemis 自身 Lua 与 Musica 文字 Hook 的 Luau 生命周期保持原有边界。

### 18 样本矩阵

“原生”表示直接使用核心格式/选项；“必要转换”表示旧封装确实需要离线处理；“阻塞”表示尚缺等价接入的必要证据。路径分类与人工验收分开，下面的解析、字体及转换结果均不计作完整游戏或本轮 Release 人工通过。

| # | 样本 | 当前路径 | 已核实的实际样本证据 | 未闭合效果 / 精确先决条件 |
| --- | --- | --- | --- | --- |
| 1 | Artemis 终之空 2025 | 原生 | `extra_archive` 挂载独立 PFS；117 项、56 个 UTF-8 AST；2936 个字体 scalar 无缺字 | 完整 AST 引用/VM 分支与实玩待验；保留原基包，无需重写 PFS |
| 2 | Artemis Demo | 阻塞 | 原生读取已有 cn/en/tw 资源，不虚构语言 ABI | 缺 `gamestart_trial.ast`、`bt_the` 的实际 backing，以及 `init.trial_script` 生产者和纹理激活语义；不能用隐藏按钮或忽略缺图代替 |
| 3 | Sena Koikake | 原生 | `text_language=cn` 选择原生第 1 列；47221 条记录，选择错误/替换字符均为 0；3255 个 scalar 经实际 Source Han CN 字体验证无缺字/零墨迹 | 本轮中文流程、菜单和视听人工验收待执行；不外推完整 SoftPAL 兼容 |
| 4 | Musica eden | 必要转换 | 原生解析 23 个 SC；9 个中文姓名与原生 V2 结果逐项一致，原生字体 CPU 验证通过 | 转换仅处理实际姓名资源，PAZ 保持原样；正文、字体选择及媒体的人工流程待验 |
| 5 | Musica ef first | 必要转换 | 原生解析 75 个 SC；10 个中文姓名与原生 V2 结果逐项一致，原生字体 CPU 验证通过；eden/ef 各自 8 个 PAZ 原样复制 | EXE 中相关中文为 Win32 About/对话框内容；实际源缺 `r18_cn`，不生成虚构资源；游戏人工流程待验 |
| 6 | Musica 永不落幕的前奏诗（Yorino） | 阻塞（原生回看消费者缺口已定位） | 原始/转换后各 102 个 SC 及 40 个 PNG 原生读取通过；44 个姓名、28358 条对白完整保留。零基行 28–42 已全部匹配到原版回看页生成的说话人标题，27/43 也已定位隐藏姓名的条件生产与显示链 | 原生回看页仍用替换前的说话人生成计数标题，未生产原版标题字节身份或经过 names-first 查找；须补齐原生回看/隐藏姓名消费者并验证，继续不登记 |
| 7 | Musica 夏空的英仙座（Perseus） | 必要转换 | 106 个 STELPACK 成员、2384 个 CRC16 块、18 个归档 bank；126 个输出，89 个 SC 原生解析通过；30 个 PNG 的全部 RGBA 与独立期望哈希相符；5 条 ASS 共 83 个正时长事件、249 次原生渲染；2630 个正文汉字经 Musica 内置中文字体逐字实际光栅化，均有墨迹 | 随附两字体是字幕子集，已撤掉正文自定义字体配置，仅供 ASS；实际媒体转换与字幕 GPU 路径、用户人工流程分别验收 |
| 8 | FVP 樱花萌放 | 必要转换 | 101 个实际输出；57053 个原 HCB 字符串偏移经原生解析器身份/最终哈希核对；71 个 hzc1 纹理/帧验证通过；4 条实际 ASS 共 2182 个正时长事件、6546 次渲染，随附字体无缺字 | 原字体选择、对白/媒体 UI 与用户人工流程待验；不能只复制 HCB 或最终显示字符串来代替脚本、资源路由和字幕效果 |
| 9 | Siglus Rewrite PLUS | 阻塞（原生 CHS 机制已接入） | 原版/中文成对加载实际覆盖 1074 个 Gameexe 项、160 个 scene、245740 个字符串；12 项语言/G01/影片/存档隔离检查通过 | 5 处机器码效果尚不明：前 2 处位于受保护高熵区，后 3 处缺原始 backing；还缺实际字体来源。须先还原效果，不能把 RVA 写补丁搬进 Rust |
| 10 | BGI 素晴日 HD | 阻塞 | Enigma 目录恢复 233 个 payload、231 个唯一资源；54 个 ARC 归属已定位，另有 2 个 BCS/BMP 未归属 | 需确定当前构建 `fcxzzd.arc` 消费者、字体注册/选择与新资源优先级，并解决未归属项；旧版字体消费者不能充当当前构建证明 |
| 11 | BGI 樱之诗 | 阻塞 | 已定位私有描述流及 handler；原生 BGI 的 GBK 显示与每 runtime 显式字体能力已接入，602 项检查通过（另 2 项既有忽略） | 缺描述流/handler 所需 live register 的生产者，以及 `data_0a`、`data_0b`、`fcxzzd.int` 的专用消费者；不能散放资源并假定覆盖 |
| 12 | KrKr LLJ | 原生资源路径，格式/字体分支待闭合 | 实际原生读取 138 个 SCN、69636 条主记录及 182 个 KBAD PBD；旧回退字体的 3575 个 scalar 已渲染。此前 119 个引用实为两个 `.ks` 目标各出现 50/69 次，两个目标均有唯一归档成员 | 另一个 PBD 是 `TJS/ns0`，当前 `Dictionary.loadStruct` 只接受 KBAD；须补原生格式路线并核验实际自动路径/目标标签、分支状态及 Windows 真正选定字体的字形，不能把旧回退字体结果当作系统字体通过 |
| 13 | KrKr 千恋万花 Steam | 阻塞 | 原生命名 producer 匹配 47495/47495；14 组碰撞、18 个额外项已识别；启动/根 loader 与两个 Steam 脚本完成静态恢复；20 条本地化记录中 4 条含 58 个简体汉字且无假名 | 需解明生成 key 的 EAX `10001138`/`10001145`、2 处歧义/异常及完整中文列选择；实际插件接口须有真实消费者，禁止伪造 Steam 插件返回 |
| 14 | Artemis 10th | 缺已确认译文 | 安装已确认 | 先取得与该版本对应、可合法读取的译文载荷；编码选项不产生译文 |
| 15 | BGI 樱之诗春之雪 | 缺已确认译文 | 安装已确认 | 先确认该版本的译文载荷及接入来源 |
| 16 | BGI 素晴日 15th | 缺已确认译文 | 安装已确认 | 先确认该版本的译文载荷；HD 补丁不自动视为兼容 |
| 17 | Musica ef latter | 缺已确认译文 | 安装已确认 | 先确认 latter 对应译文，不能沿用 first 的完成结论 |
| 18 | Anemoi | 缺已确认译文 | 安装已确认 | 先确认对应译文载荷与语言激活来源 |

### 工具、构建与验收边界

统一入口迁移前，Manager 与共享库 `astra-emu-patch` 的显式转换、取消、独立输出导入及 typed profile 保存逻辑已经实现。当时独立 CLI 的 12 项真实二进制检查通过；Windows Ctrl+C 实际子进程返回 130、已 join，且无输出/残留 staging。Manager 134 项检查（库 20、应用 57、Core 45、UI 12）通过，真实临时 SQLite 的事务配置写入与重开验证通过。这些是迁移前证据，不作为 Beta2 统一入口的检查结果。实际 Manager GUI 尚未观察，逻辑与 SQLite 结果不替代按钮操作、进度显示或实窗导入验收。

共享 ASS 使用实际 libass `0.17.4#1`、static-MD，11 项单元/集成/编译失败检查及 Clippy 通过；公开中文字体的暂停、结束、seek、resize 路径和 SDK 3 项原生字幕生命周期检查已验证。Musica 背景 Opaque 保留 A=0 时的 RGB，前景语义不变；末尾选择分隔符与 `@stage` 已有原生回归。最终已知 90 个原输入哈希仍匹配；5 个必要转换输出的 10/10/101/55/126 项逐文件收据哈希均通过。字体字符映射不能代替轮廓：夏空两份字幕子集不再误用作正文，正文走已验证的内置中文字体。

整合回归曾定位两个真实缺陷：Eden 导出仍校验旧游戏身份，已与原生会话共用包含本地化/loose/font/subtitle 的身份计算；影片在一帧内释放并重传同一纹理 ID，已改为双槽，字幕 mask/轨道另用内容 revision/serial。修复后的 41 项公开 GPU 回归通过（1 项既有忽略）；增加实际 ASS 红/绿像素切换的影片回归也通过暂停、存读档和结束。两项原生色彩检查及夏空五部实际影片首帧确认 swscale 使用 BT.601 limited 的既有 SDR 呈现，未按分辨率猜测源标签；商业 VM、游戏 GUI 和设备声音未执行。

### 历史 checkpoint 与冻结 Beta1

统一入口迁移前，七个核心、Manager 和当时的独立转换 CLI 均已完成该轮独立 Windows x64 Release 构建：Artemis 保留 Vulkan/FFmpeg；KrKr 保留 FFmpeg 3.3.9、插件及 layerExImage；RFVP 使用 `native-dylib-export` 导出原生 query。本机验证目录 `AstraEMU-native-first-checkpoint-20261019` 包含 61 个 x64 PE、52 个运行 DLL、7 个 query 导出，799 条静态导入边无缺项；仅保留四个已登记必要转换及其共享模块。包内 Manager `--help`、CLI 跨工作目录列举及真实公开 PE 转换均通过，转换字节与独立期望一致、公开输入哈希未变。打包器 23 项检查通过。这是历史本机验证包，不是当前双程序打包建议，也不是 13 款本地化全部完成或人工通过的发布。

冻结历史包 `0.1.0-beta.1` 已按提交 `455c94284519ce4121deedc8767cc66a9badfe4f` 构建为本机 Windows x64 私测包。Manager 与当时独立转换 CLI 均重新编译，`--version` 均实际返回 Beta1 版本；七个核心复用上一验证包的相同哈希 Release DLL，未冒记为重新编译。包内 61 个 PE、7 个 query 导出、799 条导入边、真实公开转换及 324 个 ZIP 文件的逐项哈希通过。ZIP 名为 `AstraEMU-0.1.0-beta.1-win64.zip`，SHA-256 为 `8b6b63529ee43c94e169f2cc73602769ad0d859689259f1dd4fda069e150c97e`。该双程序包和原目录不原地替换；当前单程序用法见[Beta2 私测说明](../manual/astraemu-client.md#010-beta2-私测)。原游戏、旧存档及七个候选的阻塞不因 Beta 名称而改变。

Beta1 的 Manager/独立 CLI 全 target Clippy（`--no-deps -D warnings`）及 CLI 12 项实际进程回归通过；当时仅改产品 prerelease 版本和标准 `--version`，没有修改核心 ABI、原生读取或资料库 schema。Beta1 文件固定为上述提交；后续只读分析结果另行记录，不原地替换测试包。

### 0.1.0-beta.2 统一入口

Beta2 使用唯一的 `astra-emu-manager`：补丁 CLI 已迁入 `patch` 子命令，GUI 直接调用共享 `astra-emu-patch` 库；独立程序 crate、打包参数及旧命令 shim 均已移除。Manager 已完成实际 Release 重建，根版本输出为 `0.1.0-beta.2`，原 `extension-*` 命令名保留；迁移后的 12 项真实进程回归、Manager 库/应用 77 项回归、Release 全 target Clippy 及打包器 24 项行为检查通过。在独立隐藏测试控制台发送真实 Ctrl-C，纯 Luau 循环和 2 GiB 原生复制的子进程均在非空 staging 后返回 130，工作线程回收且无输出/残留，公开源哈希不变；未操作用户控制台或游戏窗口。七核心沿用冻结 Beta1 的相同哈希 DLL，仅重建 Manager；整包结果以本批 `release-manifest.json` 与 `SHA256SUMS` 为准。字体、Ethornell 和静态 FriBidi 许可限制仍禁止公开二进制发布，实际 GUI/游戏验收仍由用户手动执行。

### Beta1 后的只读分析

前奏诗的十五条姓名（零基行 28–42）已通过原版拷贝的生产者与绘制调用链定位：`0x4A1140` 从原始 CLog 说话人构造独立回看标题，全部生成字节与有序 ACR 身份匹配；34/35 保留原有重复与 first-record 语义。27/43 也分别定位到隐藏姓名的条件生产者。原生 `runtime/message.rs` 在会话替换之前保存回看文本，`scene.rs` 的回看分支直接拼接计数与旧说话人，未走该标题身份和 names-first 查找。后续应在引擎中保留原始身份、生产并翻译回看/隐藏姓名，不增加旧封装脚本或删除记录；本轮只分析，Beta 及目录登记保持不变。

LLJ 旧 helper 把所有以 `.scn/.ks` 结尾的字符串都计入 119 次，但只对照 `.scn` 集合；它们实际是 26 个 PBD 内重复的两个 `.ks` storage 值，均有唯一有效成员，因此“匹配 0”不能作为缺文件证据。唯一非 KBAD 成员为 49418 字节的 `TJS/ns0`，经静态参数追踪传入 `Scripts.loadDataPack`，再调用只接受 KBAD 的 native `loadStruct`。按[公开 NS0 格式参考](https://raw.githubusercontent.com/lifegpc/msg-tool/6e64a859500729a97238e1e42ab04607805bf055/src/scripts/kirikiri/tjs_ns0.rs)完成只读内存遍历：一个平坦字典、566 个唯一键及字符串值，567 次类型校验、严格 UTF-16LE、字段边界、末尾校验和及精确 EOF 全部通过，未压缩、未加密。这是有效数据缺原生格式消费路线，不是文件损坏；后续应补有界 native 支持，而非增加 Lua 解包或执行商业脚本绕过。实际自动路径、标签和动态分支仍未运行。

LLJ 的 `force_default_font=true`、空 `default_font` 来自游戏配置，旧 helper 没有伪造该开关，但显式测试的是另一回退字体。Windows 字体文件优先级静态上先到系统 SimHei 候选，实际 native 枚举/选择及其 3575 字形尚未验证；不能沿用回退字体的通过数作结论。本轮未启动商业 VM、游戏窗口或音频，也未修改原输入和冻结 Beta。


BGI 既有用户人工验收结论保持不变，不重复操作用户窗口；这项旧验收不是本轮两款 BGI 汉化迁移的新证明。所有本轮游戏人工验收与长流程、听音均按实际结果另记。

### 完整性、隐私与发布

18 个安装均已确认。14 个样本有 90 个已知原文件哈希基线，当前 90 匹配、0 不匹配、0 缺失；另 4 个样本没有该基线。全部 18 个样本都缺任务开始前的完整目录及逐文件存档基线，因此不能据此声称全部输入/存档未变或最初零写入。原路径曾生成 IDA sidecar，后来移至私有位置；事故后的 EXE 分析改用副本，这不构成事故前的零写入证明。此前子代理 keymap、父线程私有帮助文本检索，以及本轮文档检索中的私有安装路径曾进入工具输出；相关私有路径/正文未写入 Git 或运行包，本文只记录事故类型。

本轮外部核心的已完成原生改动已发布到各自 fork 的 `astra`，主仓固定精确源码；Ethornell 仍依既有许可边界保留外部源码，不因此新增 submodule。源码发布、私有本机二进制与公开二进制分发是不同交付：RFVP 内嵌字体、Ethornell 许可仍待核验，静态 FriBidi 的 LGPL 对应源码及重链接义务也未完成核验，当前不能公开发布二进制 Release。转换输出、商业资源/译文、字体、密钥及旧存档不进入 Git 或公开包。操作方式见[客户端本地化说明](../manual/astraemu-client.md#本地化与原生选项)。


操作入口：[开发手册](../manual/development.md)、[AstraEMU 客户端](../manual/astraemu-client.md)、[Editor](../../Editor/README.md)、[NativeVN 样例](../../Examples/NativeVN/README.md)、[终之空资料](../samples/tsuinosora-modernization/README.md)。
