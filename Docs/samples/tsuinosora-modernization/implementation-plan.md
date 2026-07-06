# Implementation Plan

本计划是实现蓝图，不表示仓库已经包含转换器、VFS 插件或可运行 TsuiNoSora Demo。阶段顺序固定为先 NativeVN 转换验收，再规划补丁包加 VFS direct-read。

## Phase 1 Source Inventory

输入是 `original_install_root` 和可选 `remake_install_root`。工具只读取用户本地合法文件，输出到 `local_work_root/inventory/`，生成 `tsuinosora.source_inventory.v1`。

必须记录：

- source profile：`original_1999`、`remake_portrait_overlay`。
- logical id、resource kind、size、hash、container、coverage status。
- archive/cast/movie 计数、未知资源计数、解析诊断码。
- 无法读取或需要人工复核的资源，不记录本地路径。

## Phase 2 Unpack And Rearrange

本地转换工具把原版 Director/Shockwave 资源解包到 `local_work_root/unpacked/`，再重排到 `local_work_root/native-assets/`。这两个目录都是本地生成产物，不进入仓库。

重排规则：

| Source kind | Native layout intent | Evidence |
| --- | --- | --- |
| script/cast text | route-scoped text table and `.astra` source map | logical id、span、hash、coverage status |
| image/cg/background | image asset set with original aspect metadata | original hash、converted hash、dimension、alpha mode |
| audio/voice/sfx | AudioGraph clip registry | duration、sample rate、voice id、fence binding |
| movie | media clip registry | duration、codec class、skip policy、wait state |
| system resource | system story and profile assets | command id、source span、manual review |

转换过程必须生成 `tsuinosora.conversion_report.v1`，记录 source count、converted count、missing count、manual review count 和每条 route 的 coverage。

## Phase 3 NativeVN Conversion

把 Phase 2 输出转成 AstraVN planned data：

- `.astra` canonical story source。
- `CompiledStory` debug symbol、source map 和 route table。
- `VnCommandCursor` 初始位置、choice id、wait state expectation。
- Asset registry、AudioGraph、Timeline/Fence 绑定。
- classic profile 的 input map，保留左键推进和右键存档语义。

转换器不能把 choice payload 先写进全局 Blackboard。choice、advance、await completion 等输入应在运行时作为 trigger event payload 交给 VN step action，再由 `VnRuntimeState` 和 command cursor 推进。

## Phase 4 Classic Profile

classic profile 是验收基线。它使用 NativeVN 数据复现原版流程和感知体验：

- route 顺序、分支选择、文本、backlog 和 read-state。
- dialogue wait、choice wait、wait/movie/fence。
- 背景、CG、立绘、音效、voice、BGM、movie 的出现时机。
- 左键继续、右键存档、save/load resume from wait、replay hash。

像素级和采样级差异作为诊断证据记录，不默认阻断。阻断项只包括内容缺失、流程断裂、无法恢复、replay 不确定、人工 signoff 缺失或 evidence schema 不完整。

## Phase 5 Modern System Profile

modern profile 在 classic profile 之上启用 AstraVN 商业基线系统：

- title、save、load、quick save、quick load。
- backlog、voice replay、auto、skip、read-state、config。
- gallery、scene replay、movie replay、route chart。
- keyboard/gamepad/touch input map。
- system story 和 Luau policy presentation，不改写 Core 剧情状态。

系统页只能通过记录型 mutation、presentation、audio 和 timeline API 请求 effect。它不能直接修改 Core backlog、read-state、save/replay 或 route stack。

## Phase 6 Filter And Audio Enhancement

增强风格限定为修复增强：

- 缩放和 aspect handling，保留原始构图。
- 低分辨率图像滤镜、色彩/锐化 preset 和无损回退。
- 音频降噪、响度均衡、声道修复和 per-clip gain。
- 字幕和 UI 可读性增强。

所有增强都写入 `tsuinosora.modern_profile_report.v1`，记录 profile switch、preset id、输入 hash、输出 hash、可回退状态和人工听音/画面复核结果。

## Phase 7 Chinese Translation Patch

中文翻译作为额外 patch package 定义，不假设仓库可提交任何译文。patch 接口需要支持：

- text key 覆盖，不改变 command id 和 route graph。
- 字体、排版、ruby/注音、行宽和断行策略。
- 未覆盖文本的 fallback 到原文。
- 翻译覆盖率、冲突、长度溢出和人工校对 signoff。

translation patch 关闭时，classic profile 的 route、hash 和 save/replay 行为必须回到未启用状态。

## Phase 8 Remake Portrait Overlay

Remake 版第一阶段只作为可选角色立绘 overlay：

- 建立 old portrait logical id 到 remake portrait logical id 的映射。
- 为每条映射记录 anchor、scale、crop、layer、mouth/eye state 兼容性。
- 生成 alias/replacement review report。
- 支持逐角色、逐场景和全局关闭。

overlay 不能默认替换背景、CG、UI、影片或音频。任何替换失败都要回退到 original asset，不阻断 classic profile。

## Phase 9 VFS Direct-Read

VFS direct-read 是后续发布形态。发布包只包含 AstraVN story metadata、patch、plugin、filter preset、translation package 和脱敏 manifest；用户在启动时选择 `original_install_root`，可选选择 `remake_install_root`。

VFS 插件负责：

- 读取本地原版和可选 Remake archive。
- 按 logical id 映射到 AstraEngine Asset/Media provider。
- 验证 hash 和 coverage，发现不匹配时输出诊断。
- 不绕过授权、保护或访问控制。

本地 NativeVN 转换包继续作为验证产物和回归基准，不作为公开 payload 进入仓库。

## Phase 10 Acceptance Evidence

Demo 达成验收需要同时满足自动证据和人工证据：

- `tsuinosora.source_inventory.v1`
- `tsuinosora.conversion_report.v1`
- `tsuinosora.modern_profile_report.v1`
- `tsuinosora.manual_signoff.v1`
- scenario report、coverage summary、state/event/presentation hash、release report。

任一自动阻断项未清或人工 signoff 缺失，都不能声明 Demo 验收完成。
