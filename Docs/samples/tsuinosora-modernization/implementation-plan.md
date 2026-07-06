# Implementation Plan

本页是 Stage 3 的执行蓝图，不表示仓库已经包含转换器、VFS 插件、`astra-vn` crate、可运行 TsuiNoSora Demo 或可发布 package。真实转换器、Asset analysis gate、player 自动化、package section 和 release gate 实现都留到 Stage 3 统一落地。本页只约束后续实现，避免把 planned work 写成 implemented behavior。

阶段顺序固定为：先用本地完整转换包验证 NativeVN 复刻和现代化，再实现补丁式本地挂载应用。仓库不提交原始文件、解包文件、转换后商业 payload、文本正文、截图、音频或影片。

## Planned Game Targets

Stage 3 需要两个 planned `Game` target：

| Target | Purpose | Planned automation |
| --- | --- | --- |
| `tsuinosora-internal-game` | 本地完整转换包，用于证明 AstraEngine + AstraVN 能承载真实商业 VN 项目 | classic/modern profile 都跑 headless、Windows player、Web player 全路线自动化 |
| `tsuinosora-patch-game` | 补丁式本地挂载应用，只分发应用、插件、补丁、mount policy 和脱敏 manifest | classic/modern profile 都跑 headless 和 Windows player 全路线自动化；Web patch direct-read 后置 |

`internal` target 可以读取 `local_work_root` 下的本地生成产物。`patch` target 只能通过用户本机选择的合法数据根挂载原版和可选 Remake 资源。两个 target 的 package 和 report 都不能记录本地绝对路径。

## Phase 1 Source Inventory

输入是 `original_install_root` 和 `remake_install_root`。工具只读取用户本地合法文件，输出到 `local_work_root/inventory/`，生成 `tsuinosora.source_inventory.v1`。

必须记录：

- source profile：`original_1999`、`remake_portrait_overlay`。
- logical id、resource kind、size、hash、container、coverage status。
- archive/cast/movie 计数、未知资源计数、解析诊断码。
- 无法读取或需要人工复核的资源，不记录本地路径。

Remake 数据在 Stage 3 中用于完整 modern profile 验证，第一阶段只读取立绘 overlay 候选，不默认替换背景、CG、UI、影片或音频。

## Phase 2 Reference Evidence

`Examples/TsuiNoSora/Docs/Title.png` 和 `Examples/TsuiNoSora/Docs/Game.png` 是原版表现复刻的权威视觉参考。它们只作为仓库内已有参考证据使用，不新增商业截图输出。

Stage 3 需要生成 `tsuinosora.visual_reference_report.v1`，只记录：

- reference logical id、文件名、尺寸、hash 和用途。
- comparison region id，例如 title background、title menu、dialogue viewport、frame border、text window、speaker label、text baseline。
- layout metric、区域 hash、coverage、diagnostic code 和 pass/block 状态。

`Title.png` 用于校验标题背景、菜单按钮、选中态、整体色彩和构图。`Game.png` 用于校验背景视窗、边框、文本窗、speaker/text 位置和原版画幅比例。report 不得写入新的截图 payload、文本正文、音频采样、影片帧或本地绝对路径。

## Phase 3 Unpack

本地转换工具把原版 Director/Shockwave 资源解包到 `local_work_root/unpacked/`。该目录是本地生成产物，不进入仓库。

解包阶段只做读取、解包、hash、source map 和资源候选记录。不得在这一阶段直接把素材移动到 `native-assets/`，也不得只靠文件名或目录名判断素材类型。

解包报告必须记录：

- 原始 container logical id、解包项 logical id、size、hash、format probe 和 diagnostic。
- script/cast/movie/resource 引用关系。
- 受保护、不透明或无法解析资源的 coverage status。
- 是否需要进入 Asset analysis gate。

## Phase 4 Asset Analysis Gate

解包后必须先生成 `tsuinosora.asset_analysis.v1`，通过后才能写入 `local_work_root/native-assets/`。这个 gate 的目标是防止后期把素材放错位置或错误使用素材，例如把角色图放进背景目录，或把合并差分图当成单张立绘。

分析维度包括：

- 脚本引用位置、container 来源和使用时机。
- 尺寸、透明通道、visible bounding box、边缘留白、色彩分布和重复 hash。
- 动画、atlas、切片、button hit region 或其他 metadata。
- 与 `Title.png`、`Game.png` 参考区域的布局和外观匹配结果。

分类至少包含：

| Classification | Meaning |
| --- | --- |
| `background` | 场景背景或大画幅环境图 |
| `character_sprite` | 已经可直接作为单个角色 sprite 使用的立绘 |
| `character_atlas` | 多个角色差分、表情、姿态或部件合并在同一张图中 |
| `cg` | 事件图、gallery 图或路线关键图 |
| `ui` | 通用 UI、边框、系统页装饰 |
| `text_window` | 对话框、name plate、文本窗底图 |
| `button` | 标题、系统页或菜单按钮 |
| `audio` | BGM 或非 voice 音频 |
| `voice` | 可绑定 backlog/voice replay 的语音 |
| `movie` | 影片或 movie wait 资源 |
| `font` | 字体或文本渲染资源 |
| `unknown` | 证据不足，不能进入自动重排 |

`character_atlas` 必须生成 crop/part 表、pose/expression id、anchor、layer、mouth/eye state 兼容性和 fallback。实现不能把 `character_atlas` 当作单张 `character_sprite` 使用。

低置信度或冲突分类必须进入 quarantine，并阻断 conversion report。典型阻断项包括：

- 角色图被归为 `background`。
- 背景被归为 `cg` 且缺少脚本或 gallery 证据。
- UI 边框或文本窗被当成普通背景。
- 合并差分图没有 atlas/crop 信息。
- 带透明通道的角色或 UI 图被错误展平。
- 参考截图中可见的关键 UI 或布局区域没有对应素材或命令来源。

## Phase 5 Rearrange To Native Assets

Asset analysis gate 通过后，转换器才能把解包资源重排到 `local_work_root/native-assets/`。该目录仍是本地生成产物，不进入仓库。

重排规则：

| Source kind | Native layout intent | Evidence |
| --- | --- | --- |
| script/cast text | route-scoped text table and `.astra` source map | logical id、span、hash、coverage status |
| background/cg/image | image asset set with original aspect metadata | original hash、converted hash、dimension、alpha mode、classification |
| character sprite/atlas | sprite registry、atlas crop table、pose/expression mapping | anchor、layer、crop、fallback、analysis confidence |
| UI/text window/button | system story and profile assets | region id、layout metric、reference match |
| audio/voice/sfx | AudioGraph clip registry | duration、sample rate、voice id、fence binding |
| movie | media clip registry | duration、codec class、skip policy、wait state |

转换过程必须生成 `tsuinosora.conversion_report.v1`，记录 source count、converted count、missing count、quarantine count、manual review count 和每条 route 的 coverage。

## Phase 6 NativeVN Conversion

把 Phase 5 输出转成 AstraVN planned data：

- `.astra` canonical story source。
- `CompiledStory` debug symbol、source map 和 route table。
- `VnCommandCursor` 初始位置、choice id、wait state expectation。
- Asset registry、AudioGraph、Timeline/Fence 绑定。
- classic profile 的 input map，保留左键推进和右键存档语义。
- `tsuinosora.reference_evidence`、`tsuinosora.asset_analysis`、`tsuinosora.conversion_manifest` 和 scenario refs 的 package section 计划。

转换器不能把 choice payload 先写进全局 Blackboard。choice、advance、await completion 等输入应在运行时作为 trigger event payload 交给 VN step action，再由 `VnRuntimeState` 和 command cursor 推进。

## Phase 7 Classic Profile

classic profile 是复刻基线。它使用 NativeVN 数据复现原版流程和感知体验：

- route 顺序、分支选择、文本、backlog 和 read-state。
- dialogue wait、choice wait、wait/movie/fence。
- 背景、CG、立绘、音效、voice、BGM、movie 的出现时机。
- 标题界面、对话界面、边框、文本窗和按钮布局。
- 左键继续、右键存档、save/load resume from wait、replay hash。

像素级和采样级差异作为诊断证据记录，不默认阻断。阻断项包括内容缺失、流程断裂、无法恢复、replay 不确定、visual reference 缺口、Asset analysis quarantine 未清或 evidence schema 不完整。

## Phase 8 Modern System Profile

modern profile 在 classic profile 之上启用 AstraVN 商业基线系统：

- title、save、load、quick save、quick load。
- backlog、voice replay、auto、skip、read-state、config。
- gallery、scene replay、movie replay、route chart。
- keyboard/gamepad/touch input map。
- system story 和 Luau policy presentation，不改写 Core 剧情状态。

系统页只能通过记录型 mutation、presentation、audio 和 timeline API 请求 effect。它不能直接修改 Core backlog、read-state、save/replay 或 route stack。

## Phase 9 Filter And Audio Enhancement

增强风格限定为修复增强：

- 缩放和 aspect handling，保留原始构图。
- 低分辨率图像滤镜、色彩/锐化 preset 和无损回退。
- 音频降噪、响度均衡、声道修复和 per-clip gain。
- 字幕和 UI 可读性增强。

所有增强都写入 `tsuinosora.modern_profile_report.v1`，记录 profile switch、preset id、输入 hash、输出 hash、可回退状态和人工听音/画面复核结果。

## Phase 10 Chinese Translation Patch

中文翻译作为额外 patch package 定义，不假设仓库可提交任何译文。patch 接口需要支持：

- text key 覆盖，不改变 command id 和 route graph。
- 字体、排版、ruby/注音、行宽和断行策略。
- 未覆盖文本 fallback 到原文。
- 翻译覆盖率、冲突、长度溢出和人工校对 signoff。

translation patch 关闭时，classic profile 的 route、hash 和 save/replay 行为必须回到未启用状态。

## Phase 11 Remake Portrait Overlay

Remake 版第一阶段只作为角色立绘 overlay 来源：

- 建立 old portrait logical id 到 remake portrait logical id 的映射。
- 为每条映射记录 anchor、scale、crop、layer、mouth/eye state 兼容性。
- 生成 alias/replacement review report。
- 支持逐角色、逐场景和全局关闭。

overlay 不能默认替换背景、CG、UI、影片或音频。任何替换失败都要回退到 original asset，并进入 modern profile report。

## Phase 12 VFS Direct-Read

VFS direct-read 是补丁式发布形态。发布包只包含 AstraVN story metadata、patch、plugin、filter preset、translation package、mount policy 和脱敏 manifest；用户在启动时选择 `original_install_root`，可选选择 `remake_install_root`。

VFS 插件负责：

- 读取本地原版和可选 Remake archive。
- 按 logical id 映射到 AstraEngine Asset/Media provider。
- 验证 hash 和 coverage，发现不匹配时输出诊断。
- 不绕过授权、保护或访问控制。

本地 NativeVN 转换包继续作为验证产物和回归基准，不作为公开 payload 进入仓库。Web patch direct-read 不属于 Stage 3 阻断目标。

## Phase 13 Player Automation

Stage 3 自动化需要覆盖真实玩家行为，而不只是不带窗口的 data check：

- 从 route graph 自动生成全路线 scenario。
- `tsuinosora-internal-game` 的 classic/modern profile 跑 headless、Windows player 和 Web player。
- `tsuinosora-patch-game` 的 classic/modern profile 跑 headless 和 Windows player。
- 阻断 gate 使用 engine input bridge 注入鼠标、键盘、手柄和触控事件。
- 少量系统 UI 黑盒 smoke 证明真实窗口或浏览器可以启动、显示、接收输入、播放音频并读取 package source。

自动化 report 只记录 state/event/presentation/player hash、route coverage、layout metric、区域 hash、diagnostic 和 pass/block 状态。

## Phase 14 Acceptance Evidence

Stage 3 `DONE` 需要自动证据通过，但正式 release profile 仍要求人工 signoff。

自动证据：

- `tsuinosora.source_inventory.v1`
- `tsuinosora.visual_reference_report.v1`
- `tsuinosora.asset_analysis.v1`
- `tsuinosora.conversion_report.v1`
- `tsuinosora.modern_profile_report.v1`
- `astra.player_route_report.v1`
- scenario report、coverage summary、state/event/presentation/player hash、release report。

正式发布还需要：

- `tsuinosora.manual_signoff.v1`
- 完整通关复核。
- 听音复核。
- 画面复核。
- alias/replacement review。

任一自动阻断项未清，Stage 3 不能标为 `DONE`。人工 signoff 缺失时，Stage 3 自动化可以完成，但正式 release profile 必须保持 blocked。
