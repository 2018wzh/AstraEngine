# AstraEngine 模块能力完备度审查 Migration

**状态：`IN_PROGRESS`**

本文是一次面向当前代码的实现完整度审查结果和后续迁移总纲。审查目标不是确认“能编译”或“能跑一个样例”，而是确认设计声明的能力是否在真实 owner/provider、主运行路径、跨平台边界、错误处理、生命周期、测试和 release gate 中闭合。

结论先行：当前仓库已经形成较完整的 EngineCore、Package/Asset、字体排版、Windows Media、AstraVN runtime provider 和平台 host 骨架，但还不能宣称达到完整的 UE 级引擎完备度。最紧迫的基础设施缺口不再是字体算法本身，而是分散的 headless/fixture/Scenario 路径尚未收束成完整测试 host，导致 E1/E2 证据仍可能绕开真实 Player service lifecycle。Editor、AI/MCP、AstraEMU 和 AstraRPG 没有产品实现，继续按 design gap 管理。`DONE` 只能保留在职责边界和证据标准均满足的工作项上。

### 2026-07-14 执行范围

本轮先修复根 workspace 中已有代码，不新增 Editor、AI/MCP、AstraEMU、AstraRPG、网络或新发布平台。Web 产品实现暂缓，但 Web 缺口仍保留为 completion blocker，不会改写成已完成。NativeVN 旗舰内容和 TsuiNoSora 商业迁移不在本轮关闭范围；它们依赖的通用 Runtime、Package、Media、Headless、Player 和 Windows 基础设施仍属于本轮。

| Issue | 当前状态 | 本轮处理 |
| --- | --- | --- |
| `P0-002` | `PARTIALLY_RESOLVED` | 继续完成通用 stage/frame/await/media 主链；NativeVN 产品 E3 暂不关闭 |
| `P0-003` | `DEFERRED_SCOPE` | route truth contract 保持 blocking；NativeVN/TsuiNoSora full-route evidence 暂缓 |
| `P0-004` | `IN_PROGRESS` | typed Headless launch/profile contract 已完成，完整 Headless host 是当前最高优先级 |
| `P1-001` | `PARTIALLY_RESOLVED` | shared E2 与 Windows glyph 子链已完成；Web 和完整产品 scene 仍开放 |
| `P1-002`、`P1-003`、`P2-001` 新模块项 | `DESIGN_GAP` | 不新增产品模块，只维持状态与 release 阻断 |
| `P1-004`、`P1-011` | `DEFERRED_SCOPE` | 通用 Player 基础设施继续修复；NativeVN full behavior/evidence 暂缓 |
| `P1-005`、`P2-002` | `IN_PROGRESS` | 完整 Headless、Media 生命周期、长流程、恢复和真实 artifact 是当前修补主线 |
| `P1-006` | `GUARD_ACTIVE` | planned/reopened 状态继续由文档和 gate 约束 |
| `P1-007` 至 `P1-010`、`P1-012` | `RESOLVED` | 保留回归测试，不重复建设第二套 authority |

执行顺序固定为：先关闭 `P0-004` 的 Headless 基础设施，再补 `P1-005/P2-002` 的媒体、资源和长流程矩阵，然后整合非 Web 的 save/timeline/audio 与 Product Stage/Windows 主路径。最后回填 Stage 2/3 状态；被暂缓的 Web、NativeVN 和 TsuiNoSora 证据仍保持开放。

## 1. 审查口径

### 1.1 完备度判定

每个能力必须同时满足以下条件才能进入“实现完成”候选：

1. 设计页、contract、public API 和实际 owner/provider 的职责一致。
2. 主路径确实调用该实现，而不是只存在独立 fixture、headless provider 或 report builder。
3. 正常、空输入、边界输入、异常输入、取消、重复调用、资源释放和恢复路径均有明确行为。
4. 关键结果是真实状态、真实输入、真实视觉/音频输出、真实 package/save/replay 或真实 host evidence，而不是存在性检查或固定结构。
5. 错误不会被静默转换为成功；允许的 fallback 必须由 profile 显式声明，并在报告中记录实际选择。
6. 测试覆盖语义结果和负向门禁，release gate 不得把较弱证据升级成产品完成。

轻量 contract/core 模块按职责边界判定：它必须把契约、序列化模型、确定性 executor 和错误边界做完整；字体排版、真实渲染、平台解码等产品能力则必须在实际 owner/provider 和 Player/Editor 路径闭合，不能因为 contract crate 存在而视为完成。

### 1.2 问题分类

| 分类 | 判定含义 |
| --- | --- |
| `MINIMAL_IMPLEMENTATION` | 只覆盖最小 happy path，未达到设计声明的能力深度 |
| `FAKE_IMPLEMENTATION` | 返回固定值、合成结果或未执行声明语义 |
| `SMOKE_ONLY` | 只能证明启动、接口存在、单一 capability 或基础 fixture |
| `FIXTURE_ONLY` | 只在测试 fixture/synthetic path 存在，未接入产品路径 |
| `CONTRACT_ONLY` | contract 已存在，但实际 provider/owner 尚未闭合 |
| `BYPASS` | 绕过 RuntimeWorld、权限、Provider、Player 输入、Package 或 Release Gate |
| `SILENT_FAILURE` | 错误被吞掉、伪装成成功或隐式降级 |
| `UNWIRED_MAIN_PATH` | 实现存在但主 binary、Player、Editor 或 release path 未调用 |
| `DESIGN_GAP` | 当前代码没有覆盖已确定的设计要求 |
| `STATUS_MISMATCH` | 状态页或模块文档对当前实现的描述过高或过低 |
| `UE_CAPABILITY_GAP` | 为达到 UE 级产品完备度需要新增、但当前设计尚未定义的能力 |

### 1.3 证据等级

| 等级 | 允许证明的内容 |
| --- | --- |
| E0 | 文件、类型、trait、manifest 或 report 存在；不能证明行为完成 |
| E1 | 单元测试或 headless fixture 证明局部语义 |
| E2 | package/cook/provider/replay 链路证明跨模块语义，但仍不代表真实 host 可玩 |
| E3 | Windows/Web 真实 Player 输入、宿主消费 trace、视觉变化、音频 meter、route 和同 run identity 全部闭合 |
| E4 | 产品级跨平台、长流程、恢复、性能、资源规模、发布包和正式人工 signoff 均有证据 |

字体、渲染、媒体和 Player 的产品完成不能低于 E3；Editor、AI/MCP、AstraEMU、AstraRPG 的正式发布能力不能以 E0/E1 代替其设计规定的 evidence。

## 2. 当前实现 inventory

| 模块/能力域 | 当前真实形态 | 当前结论 |
| --- | --- | --- |
| EngineCore Runtime | `astra-runtime`、`astra-core`、`astra-engine` facade、StateMachine、snapshot/replay 和 action provider 已进入 workspace | 基础 runtime 具备 E1/E2；需继续核对长流程、故障恢复、压力和 provider 组合，不能由 native smoke 推断 UE 完备 |
| Package/Asset/Cook | `astra-package`、`astra-asset`、`astra-cook` 和 release validator 已完成 section/schema/VFS authority、typed dependency graph、内容缓存、bounded concurrency、取消和原子提交 | 本轮生产加固已覆盖核心冲突矩阵；真实 Headless/Player package source、长流程和规模 evidence 继续归入 `P0-004/P2-002` |
| Media contract | `astra-media-core` 提供 Renderer2D/FilterGraph contract、headless CPU executor；这是符合轻量 core 边界的实现 | core 本身不能替代真实 renderer/provider；硬件 surface、设备恢复和完整视觉输出仍由平台/provider 负责 |
| Text/Font | verified Package/VFS font database、`cosmic-text` shaping、Swash raster、multiscript fallback、layout replay、Windows hardware glyph atlas/golden 和 Player retained glyph stream 已落地 | 固定宽度假实现已删除；shared E2 和 Windows E3 子证据成立，Web 与完整 product scene 仍阻断 `P1-001` |
| Platform/Headless | Windows wgpu/WMF/WASAPI 主链已有真实 owner；Migration 11 已接入 `astra-platform-headless`、物理输入 JSONL、PNG/WAV artifact、统一测试 context 与 review/preflight gate | 隔离 workspace、FFmpeg feature job、Windows/Linux/macOS matrix 和正式 identity-linked evidence 尚未闭合，继续构成 `P0-004` |
| Web host | WebGPU/WebCodecs/WebAudio/OPFS 等实现存在，但正式用户手势、恢复、完整 Player 和同 run evidence 未闭合 | 本轮暂缓实现，所有 Web completion 条件继续保持 blocking |
| AstraVN | 多 crate、runtime provider、policy、presentation 和 package/save 代码存在；script frontend 与 live Player 仍重开/进行中 | 已实现范围不能升级为完整 VN 产品；frontend 和真实 Player 是主要闭口 |
| Editor | `Editor/Source/.gitkeep` 是当前唯一 tracked 文件 | P1 `CONTRACT_ONLY`、`UNWIRED_MAIN_PATH`；Stage 4 不应被当作实现 |
| AI/MCP | `astra-ai` 源码存在，但不在根 workspace；Copilot 和 TrustedSession 有明确未完成路径 | P1 `UNWIRED_MAIN_PATH`、`FAKE_IMPLEMENTATION`；Stage 4 必须保持 reopened |
| AstraEMU | `AstraEMU/Source/.gitkeep` 是当前唯一 tracked 文件 | P2 `DESIGN_GAP`；Stage 5 设计存在但没有实现对象 |
| AstraRPG | 当前只有设计、contract、stage 和 migration 目标，没有对应 workspace crate | P2 `DESIGN_GAP`；Stage 7/8 必须保持 planned/spec-ready |
| UE 能力域 | runtime、package、media contract、provider ABI 和基础 platform host 已有骨架；Editor、完整字体系统、网络、资产规模工具链、完整恢复/性能闭环不足 | P1/P2 `UE_CAPABILITY_GAP`，需要按 owner 拆分后再设计和实现 |

## 3. 已确认问题与证据

### P1-001：TextLayout 是固定宽度模型，不是完整字体系统

**分类：** `MINIMAL_IMPLEMENTATION`, `SMOKE_ONLY`, `STATUS_MISMATCH`

**修复状态：** `PARTIALLY_RESOLVED`。固定字符宽度、按 `char` 切行、字体名模拟 missing font 和未使用 `Metrics` 的旧实现已经删除。`Engine/Source/Runtime/astra-media/src/text_layout/` 现在持有 target/profile-bound package font database、真实 cosmic-text shaping、Swash raster、UTF-8 cluster/source mapping、actual face/hash/fallback decision、baseline/advance、ruby、BiDi、wrap/clip/ellipsis、bounded cache、动态字体事务替换和 renderer glyph resource owner。`astra-media-core` 的 glyph contract同时支持 Alpha8/RGBA，并保证失败 command stream 不提交 resource mutation。

**现有证据：** `Engine/Source/Runtime/astra-media/tests/text_layout.rs` 使用仓库内 OFL 字体覆盖 `astra.font_manifest.v1 → verified package → VFS resolve context → font section bytes → provider` 主链、Latin、组合字符、ruby、RTL paragraph、空输入、wrap/clip/ellipsis、hash/face/fallback/direction 负向路径、字体替换 cache invalidation、真实 CPU glyph capture，以及 layout transcript 的 snapshot/restore、uninterrupted continuation、provider-free replay、request/provider/font/payload drift 和容量回滚；`Engine/Source/Runtime/astra-media-core/tests/scene_compositor.rs` 覆盖 Alpha8/RGBA glyph、引用资源绘制和失败回滚。该证据达到 shared E2，但不能外推为 Windows 产品视觉 E3。

**2026-07-13 新增证据：** `Engine/Fixtures/PublicDomainFonts/manifest.json` 固定同一 `google/fonts` revision、OFL license、byte size 和 SHA-256；Noto Sans SC、Noto Sans Arabic、Noto Emoji 与 Poppins 会进入真实 `astra.font_manifest.v1`、Package section 和 VFS resolve 主链。`text_layout.rs` 现覆盖 CJK/假名/ruby、Arabic RTL/组合字符、emoji variation/ZWJ cluster、实际 fallback family、glyph bitmap 和 layout hash。该回归同时暴露并修复了把三通道 subpixel mask 当四通道 RGBA、零 advance 组合 glyph 被误判损坏的问题。

**2026-07-13 Windows 子项：** `WgpuGlyphAtlasRenderer` 已把 renderer-ready glyph command 接到真实 hardware wgpu atlas、vertex/scissor draw、surface present 和 GPU texture readback。`text_surface.rs` 用固定 revision/hash/OFL 的 CJK/假名、Arabic、emoji 字体生成 layout，校验 layout hash、GPU capture hash、变化像素、重复 upload 事务回滚、资源 release、loss/rebuild 后同图，并明确拒绝非文本 command。逻辑 glyph resource 只在 present 成功后提交；测试注入只证明 loss 后 retained-resource rebuild，不伪装成物理 GPU 移除。

**2026-07-13 Player 子项：** `PlayerHostCommand::PresentScene` 和 `PlatformCommandSink` 已直接转发 renderer-ready glyph/texture/sprite/rect command，不携带 CPU raster frame。Windows product-path test 从真实 Package/VFS font sections 创建 provider，经 Player command executor 到 hardware GPU capture，并生成 `astra.player_presentation_report.v1`。bundled VN 的 bitmap glyph、`HeadlessRenderer` 和 `PresentRgba` 路径已删除；dialogue、choice、system page 会解析 package localization，使用 Cook 生成的 font manifest完成 shaping，再进入 retained GPU stream。缺 locale/key/font coverage、重复 localization key、未绑定 presentation command 和未释放资源都会 blocking。Release product evidence API 会把报告与 capability、conformance、automation 的 package/profile/build/session/renderer identity 逐项对齐；缺报告、headless provider、空画面或 identity drift 均 blocking。

**剩余缺口：** shared layout save/replay continuation、Windows retained atlas/golden、Player command/release consumer与 bundled VN text/system 子路径已闭合；WebGPU consumer、bundled VN camera/timeline/video/audio 和完整 SceneCommand GPU renderer 仍未闭合。P1-001 保持开放，不能标记 `RESOLVED`。

**迁移要求：**

1. 在 text owner 中引入明确的 `FontProvider`/`FontFace`/`FontCoverage`/`ShapedRun` 边界；`astra-media-core` 只保留可序列化 contract。
2. 使用真实 `cosmic-text` font database 和 buffer shaping，禁止用字符数或固定比例替代 glyph advance。
3. 将字体资产通过 Asset VFS/package 绑定，记录字体 family、face、subset、hash、license 和 fallback chain；缺字体必须按 profile blocking 或显式 warning。
4. 增加 Latin、CJK、日文 ruby、阿拉伯/RTL、组合字符、emoji、长词、换行、裁剪、省略、空字符串、超宽 glyph 和字体缺失负向场景。
5. 增加 headless layout hash、真实 renderer glyph capture、跨平台 golden region、字体变更 hash 和 save/replay continuation 证据。

**完成条件：** E2 证明真实 shaping/layout；E3 证明 Windows/Web Player 或 Editor 的真实视觉输出；release gate 能识别缺字体、未声明 fallback、字体 hash 不匹配和不同 provider 产生的 layout drift。

### P1-002：AI 源码存在但不在 workspace，且 Copilot 返回空 hint

**分类：** `FAKE_IMPLEMENTATION`, `UNWIRED_MAIN_PATH`, `STATUS_MISMATCH`

**证据：** `Engine/Source/Developer/astra-ai/src/editor_copilot.rs:168` 的 `generate_inline_hint` 忽略 context，返回空内容 hint；`Engine/Source/Developer/astra-ai/src/trusted_session.rs:142` 的 `apply_write` 只移除 review 并返回 `Applied`，没有执行 patch、scope 校验、checkpoint、文件事务或 release recheck。根 `Cargo.toml` 没有 `Engine/Source/Developer/astra-ai` workspace member；当前 `cargo metadata --no-deps` 观察到 45 个 workspace package，`astra-ai` 不在其中，根 workspace 不能把 `cargo test -p astra-ai` 当作正常验证路径。

**缺口：** provider profile、真实请求、超时/取消、审计、用户确认、文件范围约束、原子写入、undo/redo、语义校验、save/replay 和 release gate 没有闭合。该源码不能作为 Stage 4 已实现证据。

**迁移要求：** 先决定 crate 是否进入 workspace；再实现 `AiProvider` 调用、typed request/response、provider unavailable error、review identity、scope-bound patch application、atomic transaction、undo checkpoint、post-write validation、audit trace 和 provider-free committed output。空 hint 和未执行写入必须改为显式 unavailable/blocked，而不是成功返回。

### P1-003：Editor 只有设计目标，没有实现主路径

**分类：** `CONTRACT_ONLY`, `UNWIRED_MAIN_PATH`, `DESIGN_GAP`

**证据：** `Editor/Source/.gitkeep` 是当前唯一 tracked Editor 文件。`Docs/implementation/workspace-blueprint.md:17` 明确写为 Stage 4 not implemented；`Docs/status/stages/stage-4-editor-ai-mcp.md:13` 及后续 target paths 都是 planned target。

**缺口：** 没有 Project Wizard、Qt/QML shell、PIE、Inspector、Graph/Timeline、Plugin Manager、Release Gate Panel、runtime provider switching 或真实字体资源加载。Editor 字体策略文档中声明的捆绑 Noto Sans SC 与 `QFontDatabase::addApplicationFont` 没有仓库资产或实现路径支撑。

**迁移要求：** 先建立 Editor target 和 bridge crate，再接 `RuntimeEditorMetadata`、provider/profile selector、source round-trip、undo/redo、PIE session、package/release panel。Editor 发行字体必须有实际资源、构建复制、hash manifest、fallback coverage 和跨平台截图证据。

### P1-004：真实 Player 仍未证明主状态推进

**分类：** `UNWIRED_MAIN_PATH`, `SMOKE_ONLY`

**证据：** `Docs/status/implementation-plan.md:151-152` 将 NativeVN Game target 和 live player automation 保持 `IN_PROGRESS`，明确写出需要真实 player input 推进可见 VN state，并要求视觉 hash 变化、音频 meter、host evidence 和 route evidence 同 run。`Docs/contracts/release-gate.md:57` 明确禁止 `VnPlayerCommand`、`--route-scenario` 自推进、DOM click、JS callback 和 route report 冒充 playable evidence。

**缺口：** 当前 automation contract、report validator 和 synthetic/internal route tests 只能证明门禁和部分 host plumbing；不能证明真实 package 被 Player 消费后，输入经过平台宿主进入 RuntimeWorld，状态推进并产生可见 scene/audio/route 变化。

**迁移要求：** 绑定同一 commit、build fingerprint、profile hash、package hash、session id 和 evidence manifest；Windows 使用真实 window focus、SendInput、player consumed trace、renderer region 变化、WASAPI meter 和 route；Web 使用真实 Chrome/Edge、CDP input、canvas/screenshot 变化、WebAudio meter 和 route。任何只完成 report 生成的路径保持 blocked。

### P1-005：Media 已有 provider 形状，但文本、渲染和 fallback 深度不能由 headless fixture 代表

**分类：** `CONTRACT_ONLY`, `FIXTURE_ONLY`, `SMOKE_ONLY`

**证据：** `Engine/Source/Runtime/astra-media-core/src/renderer2d.rs:109` 提供 `HeadlessRendererProvider`；`Engine/Source/Runtime/astra-media/tests/headless_capture.rs` 和 `filter_graph.rs` 主要验证 CPU/headless frame/hash。`Engine/Source/Runtime/astra-media/tests/decode_provider.rs` 同时使用 `SyntheticPlatformDecodeProvider` 和公开媒体 fixture。`astra-package` 只保留显式命名的 `PackageBuildRequest::fixture` 供局部测试；`astra-cli` 产品 package 路径已改为从已验证 target 构造显式 wgpu/runtime/VFS policy，不再调用 fixture constructor。

**结论边界：** 这些实现作为 contract、headless、fixture 和负向 gate 是合理的；它们不能证明真实 surface、设备丢失恢复、真实音频输出、长视频同步、字体 glyph 绘制或发布 package 的产品完备度。文档已经规定 minimal package 不能冒充 release input，后续审查必须继续保持这一边界。

**迁移要求：** 为每个 provider 增加 owner、selected/available evidence、真实资源生命周期、device/context loss recovery、bounded queue、取消和 release profile；把 headless/fixture 证据与 E3/E4 产品证据分离，不允许 gate 复用弱证据。

**2026-07-13 修补进度：** 已删除 descriptor-only `WgpuRendererProvider`、graph-hash audio meter、Kira facade 和 Windows 中 `cfg(any())` 遮蔽的旧 renderer；`WgpuPresentationCore` 增加 ordered frame、malformed rollback、resize、context/device recovery policy 与 retained upload rebuild；FilterGraph 阻断 unknown/no-op/target bypass；DecodeRegistry 改为 exact provider/target/profile binding，WMF 保留 diagnostic/sequence rollback。optional `ffmpeg-vcpkg` 已执行真实 MP3/MP4 timestamped stream，覆盖目标设备 resample、seek generation、EOS drain、取消、packet hash、终段 trimming、live byte budget 和单 packet backpressure。Windows profile 只接受精确 `[wmf, ffmpeg]` 软件 fallback 声明；`WindowsNativeMediaSession` 已把 stream 接到 audio-master scheduler、WASAPI 和 wgpu，真实测试覆盖 pause/resume、seek、视觉 capture、非静音 meter、可注入 device-loss recovery、失败清理和 product-profile-bound measured performance report。`astra.performance_budget.v1`/`astra.performance_report.v1` 阻断 sample/identity/budget drift，WASAPI underflow 改按 callback 计数；`astra-release` 已增加 clean checkout 下 budget/report/capability/conformance/Player/package 同 run identity consumer。Windows font golden 已由 P1-001 的 hardware glyph 子路径闭合；GPU FilterGraph、真实 Player 生成的 performance artifact 和正式 reference threshold pass 仍开放，因此 P1-005 保持未关闭。

### P1-006：Workspace 与状态页必须持续阻断未实现 Stage 4/5/7

**分类：** `STATUS_MISMATCH` 风险，当前部分已由文档正确标记为 planned/reopened

**证据：** `Docs/implementation/workspace-blueprint.md:17-18`、`Docs/status/stages/stage-4-editor-ai-mcp.md:3`、`Docs/status/stages/stage-5-astra-emu.md:1-9` 和 `Docs/status/stages/stage-7-astra-rpg.md:130` 均明确这些模块尚未实现或处于 spec-ready/reopened；`AstraEMU/Source/.gitkeep`、`Editor/Source/.gitkeep` 也支持该结论。

**迁移要求：** 后续新增代码时必须先加入真实 workspace/target、测试矩阵和 release gate，再改变状态。设计 target path、fixture provider、facade re-export、synthetic report 和 planned contract 均不得计入实现完成数量。

### P2-001：UE 级能力域仍缺少产品级闭环

**分类：** `UE_CAPABILITY_GAP`, `DESIGN_GAP`

当前可确认的能力域缺口包括：

| UE 能力域 | AstraEngine 当前状态 | 后续闭合条件 |
| --- | --- | --- |
| 字体/文本 | 已有 verified package font database、真实 multiscript shaping/raster、fallback、glyph resource、layout identity、bounded provider-free replay E2，以及 Windows Player command/release hardware glyph E3 子证据 | 补 WebGPU glyph consumer、bundled VN 主路径和完整跨端 E3 |
| Editor | 只有设计和 metadata contract | 可创建项目、编辑、PIE、调试、撤销、打包和 release review |
| 资产规模 | 有 VFS URI、package section、hash 和 cook audit | DDC/cache、增量 cook、依赖图、并发、取消、恢复、超大资产和发布包验证 |
| 渲染 | 有 headless CPU contract、wgpu provider 代码和部分 host | 资源生命周期、材质/纹理管理、surface/device recovery、GPU budget、真实视觉验收 |
| 媒体 | exact provider binding、Symphonia/WMF/optional FFmpeg 真实 one-shot decode、AudioGraph v2 与 Windows WASAPI 输出已落地 | 长视频/音频 session、同步、seek、暂停恢复、设备变化、真实产品输出和 profile gate |
| 输入/平台 | Windows/Web host plumbing 存在，完整 Player route 未闭合 | 真实宿主输入、焦点、resize、IME/gamepad、save、audio、decode、恢复和同 run evidence |
| 网络 | Stage 8 只定义 planned RPG protocol | 权威 session、seat、同步、断线恢复、审计、provider-free replay 和安全边界 |
| 调试/工具链 | tracing、crash、CLI、scenario 基础存在 | 长流程 traceability、profiling、memory/resource inspector、capture/replay、artifact retention |

这些条目不是把所有 UE 功能未经设计地塞进 EngineCore，而是要求在后续产品设计中明确 owner、contract、scope、platform policy、evidence 和 release gate。

## 3.1 第二轮主路径深审新增问题

### P0-002：Windows bundled Player 绕过 NativeVnRuntimeProvider、RuntimeWorld 和真实渲染

**分类：** `BYPASS`, `FAKE_IMPLEMENTATION`, `UNWIRED_MAIN_PATH`

**2026-07-13 复核：** `NativeVnHostCommandSource` 已改为读取 `PackageReader` 保留的 `ValidatedRuntimeProviderSelection`，按 binding 的 provider id、target、profile、binding hash 和完整 descriptor 创建 instance，执行 prepare/probe/open，并在失败时清理 instance/session；request context、linked descriptor 或 provider report identity 漂移都会 blocking。runtime output再经 package localization/font shaping转换为 `PresentScene`，由 Windows WGPU执行；旧 `VnRuntime` owner、`HeadlessRenderer`、bitmap glyph 与 `PresentRgba` 已删除。camera、timeline、video、audio command 仍以 `ASTRA_PLAYER_PRESENTATION_UNSUPPORTED` 在 package open 阶段阻断，因此本项缩小为“完整 presentation/audio command stream 未闭合”，不能标记 `RESOLVED`。

**剩余视觉/音频问题：** dialogue、choice、system page、背景 texture/sprite 已进入 retained scene 和真实 Windows GPU capture，但 camera、timeline、video、voice、BGM/SE、AudioGraph 与 FilterGraph尚未接入同一 bundled session。当前 fail-fast preflight 会阻止包含这些 command 的 package 启动；这是诚实的未完成状态，不能用 text 子路径的 capture 外推完整 VN 演出。

**影响：** 当前 Windows bundled Player 已证明 packaged Game 的 provider/RuntimeWorld/text/texture/GPU 子链，但尚不能证明 advanced presentation、真实音频、等待/取消和完整 route 可玩。若仅凭该子链生成 `player.full_playable`，仍会构成证据拔高。

**迁移要求：** 删除 Player 主路径中的 direct `VnRuntime`/`VnPlayerCommand` 快捷实现；Player 必须从 package 读取 provider binding，创建 provider instance/session，由 RuntimeWorld 的 `astra.vn.step` action 消费平台事件，输出可序列化 presentation/audio/effect，再由真实 platform renderer/audio provider 执行。headless renderer 只能保留为明确标记的 unit/headless evidence，不能作为 bundled Player renderer。

### P0-003：live automation 用外部 expected route 标签和像素变化生成 route coverage

**分类：** `BYPASS`, `SMOKE_ONLY`, `FAKE_IMPLEMENTATION`

**证据：** `Engine/Source/Programs/astra-player/src/lib.rs:418-425` 把调用方传入的 `expected_routes` 逐项变成事件标签；`lib.rs:426-465` 对每个标签发送同一个空格键，只要前后截图 hash 不同就把该外部标签加入 `route_coverage`；`lib.rs:495-499` 只检查该标签对应的输入是否有 consumed trace，并没有从 Player/RuntimeWorld 读取真实 route id、terminal、choice 或 state transition。

**影响：** 任意视觉动画、计时器、窗口变化或错误状态变化都可能让外部标签被记录为 route coverage。输入 consumed trace 只证明宿主消费了按键，不证明该按键推进了指定剧情路线。

**迁移要求：** route coverage 必须来自同一 Player session 的 Runtime/provider route report、terminal/choice state hash 和真实输入序列；`expected_routes` 只能作为待验证期望值，不能成为 report 的事实来源。每个 route 必须关联实际 terminal/choice signature、state/event/presentation hash 和 package/profile/session identity。缺真实 route evidence 必须 blocking。

### P0-004：Headless、Scenario、renderer/audio fixture 和 Player 测试仍是分散双轨

**分类：** `BYPASS`, `FIXTURE_ONLY`, `UNWIRED_MAIN_PATH`, `UE_CAPABILITY_GAP`

**原始证据：** `ScenarioRunner`、`HeadlessRendererProvider`、CPU scene compositor、AudioGraph meter、`PlatformCommandSink` 和 Player automation contract 分别存在，但没有共同的 host/session/resource owner。部分测试直接调用 provider、mock sink 或产品语义命令，不能证明相同 package、输入和 command stream 能通过完整 `PlatformHostClient` 生命周期。Headless 也没有独立身份，容易被静态 hash、synthetic frame 或 fixture report 错误外推成产品证据。

**影响：** Runtime、Media、Player 和 full-flow 测试无法共享统一的窗口/surface、音频、decode、save、package、输入、事件、completion 和 zero-leak shutdown 语义。真实平台验收缺少强制 preflight，失败时也难以区分产品逻辑、媒体输出和平台后端问题。

**2026-07-14 contract 进度：** `astra-platform` 已新增 `HostKind`、`HostLaunchProfile` 和 `astra.headless_host_profile.v1`。`PlatformId` 继续只表示六个发布平台；Headless profile显式绑定 build/package SHA-256、renderer/text/audio/decode/save/package providers、`astra.user_input_sequence.v1`、file/stdio 权限、artifact retention/checkpoint/容量和 host limits。`PlatformHostFactory::start` 现在接收 typed launch profile，native factory在 `host.start` 拒绝 Headless variant。短 hash、未知 schema、缺 provider、空 transport、重复 checkpoint 和零预算均返回 `InvalidProfile`。该证据只关闭 `S2-HEADLESS-CONTRACT-01`，不能外推为完整 Headless host。

**剩余迁移要求：**

1. 建立 `publish = false` 的 `astra-platform-headless`，实现完整 `PlatformHostClient` service、generational handle、ordered event/completion、bounded queue 和 zero-leak shutdown。
2. renderer、TextLayout、FilterGraph、AudioGraph 和 decode 继续由 Media owner 提供；Headless 组合显式 provider并输出真实 PNG 与 PCM S16LE WAV，不接受颜色块、静态 meter 或 synthetic decode。
3. 新增双向 JSONL 物理输入协议，只允许 keyboard/IME/pointer/touch/gamepad、固定时间、await/checkpoint 和 shutdown。`advance`、`choose`、`open_system`、直接 `VnPlayerCommand` 和 Runtime mutation 必须 blocking。
4. artifact manifest/run report只记录相对路径、hash、尺寸、时长、sequence、checkpoint 和 provider identity；超出数量、字节、帧或时长预算立即失败，不能截断后生成 pass。
5. 所有平台无关 Runtime/Player/full-flow 测试收束到 `HeadlessTestContext`。真实 Windows/Web acceptance 必须先通过同 build/package/input 的 Headless preflight；Headless 最高只计 E2。

**完成条件：** `S2-HEADLESS-HOST/MEDIA/INPUT/ARTIFACT/CLI/TEST-MIGRATION/REVIEW/PREFLIGHT` 全部有代码和负向测试；workspace inventory 不再发现长期直连 headless provider、独立 meter、mock sink、Scenario 私有执行或语义快捷命令；真实平台 gate 会在 preflight identity 不匹配、artifact/review 缺失或 Headless blocked 时拒绝启动。

### P1-007：Runtime API 与设计 contract 漂移，module mount 和 tick 输入没有完整约束

**分类：** `STATUS_MISMATCH`, `BYPASS`, `SILENT_FAILURE`, `DESIGN_GAP`

**证据：** `Docs/implementation/runtime-api.md:31-50` 设计 `mount_module(&mut self, slot: EngineModuleSlot, provider: ProviderRef) -> Result<(), RuntimeError>`；实际 `Engine/Source/Runtime/astra-runtime/src/world.rs:154-159` 接收任意字符串并无条件插入，返回 `()`，不校验 slot/provider 是否已注册、是否 selected、是否 packaged eligible、capability/fingerprint 是否匹配，也不阻止同一 slot 被替换。`world.rs:475-484` 直接接受任意 `fixed_step`，覆盖当前 step，并只把 `delta_ns` 和 seed 用于日志/输入对象；没有发现单调递增、重复 step、回退 step、delta policy 或 input seed 与 session seed 的 blocking 校验。

**影响：** 低层 Runtime API 可以绕过 PluginRegistrar 和 provider policy 自行挂载任意 provider 字符串；乱序 tick 可能改变 delayed event、await drain、stable id step 和 replay 语义。已有测试证明正常顺序下的确定性，不证明非法 tick 序列会被拒绝。

**迁移要求：** 引入 typed `EngineModuleSlot`/provider reference 或 host-owned binding token；mount 必须返回错误并验证 registry selection、capability、package/profile eligibility 和 fingerprint。tick 必须明确允许的首 tick、连续 tick、恢复 tick、重复 tick 和 replay tick，非法序列必须返回稳定 diagnostic；`delta_ns`、seed 和 fixed step 的语义必须写入 contract 并有负向测试。

**2026-07-13 修补进度：** Runtime 已改用 `EngineModuleSlot + ValidatedModuleBinding`，显式阻断未选择、非 packaged、slot/context mismatch 和重复挂载；binding context 已固化 package、target、profile、capability、engine version 与 rustc/feature/ABI fingerprint。`RuntimeWorld::tick` 只接收 typed `TickRequest`，ordered player input、Await completion、live/recorded provider output 在同一事务提交；live、restore continuation、replay mode 被严格隔离，旧的 tick 外 input/await/provider output 公开旁路已删除。tick 阻断 ingress 乱序、重复/回退/跳步、非法 delta、seed mismatch 和缺 required slot，任一失败恢复 tick 前 snapshot；整个 replay transcript 失败也恢复 replay 调用前 world。Product runtime ABI 同步补齐 `delta_ns`、`session_seed` 和 `RuntimeStepMode`；`ProductRuntimeHost` 在 provider 调用前阻断 drift 与 live-provider replay，并用 `RuntimeRestoreReport.restored_fixed_step/session_seed` 恢复连续 tick authority。NativeVN save 主路径已删除拆分 state authority，唯一 `runtime.world`/`astra.runtime.save_blob.v2` section 保存完整 RuntimeSnapshot；损坏 nested container 不修改现有 world。`runtime.world` 为 `2.0.0`、replay transcript 为 `v2`，旧布局稳定拒绝。`tick_contract.rs`、`save_replay.rs`、`product_runtime_host.rs` 与 NativeVN provider/FFI tests 覆盖负向路径。Runtime 与 product provider tick/save contract 缺口已关闭；平台 frame tick 与 typed presentation director 仍计入 P0-002/P1-011，不由本项外推完成。

**2026-07-13 typed presentation 修补进度：** Standard presentation 不再把 command 名和 `BTreeMap<String, String>` 作为 Runtime IR。编译器会生成 `StageCommand` v2、`FixedScalar`、typed audio/movie/timeline/effect；timeline 必须提供有序 keyframe，blocking join 必须提供 fence，movie wait 必须同时提供 fence 和 fallback。扩展命令改为 `ExtensionCommandDescriptor`，provider、schema、字段类型和 required 状态缺一不可。`task`、`fence`、`command`、`bind_setting`、`source` 等没有完整语义的伪 standard command 已从 registry 删除。Runtime output 的 presentation/audio schema 升为 v2，Player 遇到未实现 typed command 或未绑定 extension provider 会在 package open 阶段阻断。该修补关闭 raw IR 和静默忽略问题；preset policy、StageModel product director、平台 frame tick、camera/timeline/video/audio 执行与同 run evidence 仍开放，因此 P0-002 继续保持未关闭。

**2026-07-13 presentation policy 修补进度：** `vn.presentation_provider_manifest` 已升级为 v2，并把 `classic`、`modern`、`advanced-vn` 的 preset、filter、fallback、layer/timeline/effect budget 写入经过验证的 package section。Package loader、Player 和 Release Gate 复用同一 validator；重复或未知 id、preset/command 不匹配、profile 越权、filter/fallback 断链、预算越界和旧 v1 schema 都会在 provider session 创建前阻断。Player 不再依赖源码内隐式 preset 默认值。该子项关闭 preset policy binding 缺口，但不代表 preset 已由 frame tick 执行；typed product director、平台 timer、camera/timeline/video/audio command stream 与真实证据仍开放。

**2026-07-13 product director 检查点：** `ProductStageDirector` 已成为 NativeVN Player 的 typed stage state owner。它以 fixed-point 和 profile budget 管理 layer/entity/camera、tween、timeline、shake、movie/effect intent、frame identity 与 snapshot/restore，所有 apply/tick/restore 失败都保持原状态。Player 已从 director state 生成 package texture-backed background/sprite stream，并执行 safe-area clip、camera translation/zoom、opacity 和资源生命周期。当前只保存为可恢复检查点：平台 event loop 尚未送入固定 frame tick，timeline fence 尚未回注 Runtime await，movie/audio/effect、非 normal blend、camera rotation 和完整 Windows/Web E3 仍 blocking；P0-002/P1-011 不关闭。

### P1-008：ExtensionRegistry 的公开 `select()` 忽略显式 binding

**分类：** `BYPASS`, `STATUS_MISMATCH`

**证据：** `Engine/Source/Runtime/astra-plugin/src/registry.rs:96-100` 的 `ExtensionRegistry::select` 只返回按 `(slot, provider_id)` 排序后的第一个 provider；它不读取 `ServiceRegistry` 的显式 binding。相邻的 `PluginRegistrar::selected_provider` 在 `registry.rs:155-161` 才按 service binding 选择正确 provider。现有测试调用的是 `selected_provider`，没有覆盖公开 `ExtensionRegistry::select` 在多个 provider 和显式 binding 下的行为。

**影响：** 新调用方若使用较直观的 `extensions.select(slot)`，provider 选择会依赖排序而不是 project/package binding，违反“加载顺序不能改变语义”的硬约束。

**迁移要求：** 删除或私有化无 binding 语义的 `select()`；公开选择 API 必须要求 binding context 并返回 selected provider 或 blocking conflict。新增两个 provider、显式选择第二个 provider、重新排序注册顺序和缺 binding 的负向测试，并让 release gate 使用同一选择实现。

**2026-07-13 关闭证据：** 无 binding context 的 `ExtensionRegistry::select()` 已删除，provider 注册不再创建隐式默认 binding；`PluginRegistrar::bind_provider` 是唯一公开选择入口。`astra-plugin-abi` 现提供 `astra.plugin_extension_registry.v2`、`astra.provider_policy.v2`、带 canonical hash 的 `ProviderBinding` 和共享 validator，Package builder/reader、scenario runner、Release Gate 与 VFS provider gate 使用同一语义。注册 API 已 fallible，非法或重复 provider 不产生部分写入，loader 注册失败会回滚。两种注册顺序、缺失/重复 binding、hash、capability、fingerprint、package/target/profile drift 和未 packaged provider 的负向测试均已落地。P1-008 关闭。

### P1-009：VFS resolve 没有 target/profile eligibility，且 layer 冲突可能被静默覆盖

**分类：** `BYPASS`, `SILENT_FAILURE`, `DESIGN_GAP`

**证据：** `Engine/Source/Runtime/astra-asset/src/vfs.rs:303-337` 的 `VfsManifest::resolve` 只按 URI 和 layer priority 选择 entry，API 没有 target/profile 参数，也没有检查 `VfsLayerDescriptor.targets/profiles` 或 entry eligibility。`vfs.rs:235-255` 将重复 prefix 和 layer id 放入 `BTreeMap` 时覆盖前值，没有 blocking diagnostic；相同 URI、相同 priority 的多个 entry 也没有唯一性校验。Release gate 在 `astra-release` 中补做了部分 prefix/provider/package 检查，但 Runtime/Editor/工具直接使用 `resolve` 时没有同等边界。

**影响：** 非当前 target/profile 的资源可能被解析；同 priority overlay 或重复 layer 的最终选择依赖输入顺序，破坏 UE 风格 mount graph 的显式覆盖语义。

**迁移要求：** 增加 `ResolveContext { target, profile, capability, provider_binding }`，由 resolve 统一过滤 eligibility；validate 必须阻断重复 prefix、layer id、URI/layer/priority 冲突、缺 provider registration 和非法 overlay base。Release gate、runtime reader、Editor preview 和 local mount 必须共用该解析规则。

**2026-07-13 关闭证据：** `VfsManifest::resolve` 强制接收 `ResolveContext`，校验 target/profile eligibility、prefix provider binding 与 capability；manifest validation 阻断重复 prefix/layer/URI-layer/whiteout、entry-layer prefix mismatch、range overflow和非法 overlay base，resolve 阻断缺候选与同 priority 多权威候选。Package builder/reader 现同时验证 mount graph、当前 package target/profile 的唯一 resolve、VFS prefix 显式 binding 与 backend capability；Release Gate 复用同一 `VfsManifest` validator，backend capability 真源移入 `VfsBackendKind::required_provider_capability`。P1-009 关闭。

### P1-010：Astra package container 接受重复 section id，读取时取第一条

**分类：** `SILENT_FAILURE`, `BYPASS`, `DESIGN_GAP`

**证据：** `Engine/Source/Runtime/astra-package/src/container.rs:218-249` 的 `AstraContainerBuilder::add_section` 只追加 section；`container.rs:301-303` 的 `section_entry` 使用 `iter().find`；`container.rs:520-610` 验证 section count、bounds 和 hash，但没有验证 section id 唯一性。重复 id 因此可以同时存在，读取结果由 table 顺序决定。

**影响：** package/save section 的 schema、hash、provider policy 或 compiled story 可能出现同名竞争，调用方只读取第一条而不产生 diagnostic，破坏自描述容器和 release gate 的唯一权威来源。

**迁移要求：** builder 在写入时阻断空 id、非法 schema 和重复 id；reader 在读取 table 时阻断重复 id，并增加重复 id、同 hash/不同 payload、不同 schema、加密/未加密混合重复 section 的负向 fixture。Release validator 必须把该 diagnostic 作为 package blocking。

**2026-07-13 修补进度：** container builder 与 reader 已同时校验非空 safe section id/schema、唯一 id、section count/table/decoded-size 上限、migration range、alignment、header/table overlap、section overlap、bounds、stored/decoded hash 和 encryption AAD；Zstd decoded length 使用 checked conversion。reader 私有恶意 table fixture覆盖 duplicate authority 与 range overlap。`PackageBuilder` 生成 `astra.schema_registry.v2` 的 section-id/schema/version 精确映射，`PackageReader` 在进入 release/runtime 前验证 required sections、package identity 以及 registry 双向闭合，旧 v1/unknown/mismatch schema 直接拒绝。Release validator 使用同一 `PackageReader`，P1-010 已关闭。

### P1-011：NativeVN release behavior evidence 仍是最短 smoke，不是完整产品行为

**分类：** `SMOKE_ONLY`, `FIXTURE_ONLY`

**证据：** `Engine/Source/Developer/astra-release/src/lib.rs:928-1010` 的 `native_vn_behavioral_evidence` 从 package 解码 story，只执行 `launch_default` 一步，记录 state/event/presentation hash，随后 save/restore/shutdown；它没有推进 dialogue、choice、system page、voice replay、movie wait、timeline join/cancel、replay 或真实 Player input。`Docs/contracts/game-runtime-provider.md` 允许 provider conformance 使用最短 step，但这只能证明 provider lifecycle，不等价于 Stage 3 产品完整度。

**迁移要求：** 保留该检查作为 `runtime_provider.native_vn` 的最小生命周期 gate，同时新增独立的 full behavior gate：从 package/scenario 派生真实 input，覆盖 dialogue、choice、system、save/load、replay、await、audio/timeline effect 和 route terminal；该 gate 必须由 Player/host evidence 补齐，不能由 provider direct call 或 headless report 替代。

**2026-07-11 修补进展：** NativeVN provider save 已增加 `vn.runtime_world`，不再只保存 VN/Policy component；恢复前交叉校验完整 `RuntimeSnapshot` 与 component sections。Player 侧已有带整体 hash 的 save envelope、tamper/session mismatch 阻断和 `complete_wait` provider 路径；Windows F5/F9 已走平台原子 save transaction，写入或提交失败会执行 abort，load 后重新提交保存时的已校验 presentation frame。media completion callback、多 route session、Web 等价路径与 Windows/Web E3 evidence 尚未完成，因此 P1-011 仍保持开放。

NativeVN timeline task 现已通过 descriptor 声明的 `astra.vn.timeline_task.v1` effect envelope 返回 ProductRuntimeHost；此前仅写入 `RuntimeWorld.effects`、Player 不可见的 `UNWIRED_MAIN_PATH` 已消除。Windows/Web 已通过共享产品 media owner 执行 scheduler 与 join/cancel completion；真实浏览器和 E3 evidence 仍未完成。

Windows/Web Player 已接入同一 bounded timeline scheduler：start/cancel/deadline 使用单调 host clock，重复 ID、容量溢出、非法 duration/symbol、未知 cancel 和时钟回退均 blocking；completion/cancel 保留原 fence，并由共享 `NativeVnProductMediaHost` 经 ProductRuntimeHost `complete_wait` 回到固定 tick。正式 E3 evidence 仍未完成。

NativeVN Player 已能从 package 的 catalog/VFS 唯一映射读取 encoded voice/audio，执行 bounded read、entry hash 和 MP3/Ogg/FLAC/WAV signature 校验；WMF `pcm_s16le` output 的截断、sample budget、sample rate、channel 和 frame alignment 也已有负向测试。Windows/Web 产品主链现共享 `NativeVnProductMediaHost`，由其内部 `NativeVnProductAudioHost` 把 Runtime audio output 送入同一持久 sample mixer；该 owner 覆盖平台 preferred format 协商、bounded sinc resampling、可证明的 mono/stereo mapping、loop、bus fade、ordered pause/resume/stop、completion、queue query/backpressure、underflow blocking、时长感知 drain 和 close，`voice_end` 不再来自伪 callback。Web 还要求真实 input 后 `AudioContext.resume()`，并已接 timeline、wait completion、F5/F9 save/load 和 Runtime consumed trace。设备热切换恢复、真实浏览器/CDP run 和同 run E3 evidence 仍未闭合，因此不能关闭真实音频缺口。

Web bundle 过去把可任意替换的 loader 当作产品入口，并且没有打包 loader 所 import 的 wasm-bindgen glue，真实 bundle 会在模块解析阶段失败，fixture loader 却能让静态 browser test 通过。现已删除 `--web-player-loader` 与 `--web-audio-worklet` 输入，改由 `astra-cli` 嵌入并写出同版本 canonical host scripts；调用方必须显式提供匹配的 `--web-player-wasm` 与 `--web-player-glue`。bundle 构建使用 staging directory 原子提交，wasm 通过 `wasmparser` 完整校验，glue 缺固定 wasm-bindgen marker 或包含 route/DOM input bypass marker时 blocking，失败不会留下可发布目录。该修补关闭了 bundle 形态的 `BYPASS`/`SMOKE_ONLY`，但不能替代真实 CDP/E3 run。

Web target 的真实编译曾先后被 Luau C++ VM 与 `abi_stable` dynamic loader 阻断，两者都来自 contract 与 native executor/loader 未分层。现已把 `astra-policy` 的 DTO/schema、budget、`VnPolicyState`、mutation/query/command/trace/snapshot contract 从 mlua executor 分离；`astra-plugin-abi` 的 provider DTO 也与 `ffi` RootModule 分离，`astra-plugin` 的 in-process host 与 `dynamic-abi` loader 分离。`astra-vn-package`、save、runtime provider 和 Web Player 只启用 portable contract/in-process feature，native 默认仍覆盖完整 FFI lifecycle。当前 `cargo check -p astra-player-web --target wasm32-unknown-unknown` 与 `wasm-pack build ... --target web` 已真实通过，Web graph 不再包含 mlua、`abi_stable` 或 `libloading`。这关闭了 compile-time `UNWIRED_MAIN_PATH`，但真实浏览器 CDP、视觉、音频和 route 同 run evidence 仍未完成。

Web Player 现新增由 Rust 产品主链直接发出的 `astra.player_web_live_evidence.v1` console envelope。package 验证记录 target/profile/package hash；每次真实 keyboard/pointer 经 RuntimeWorld 消费后记录 provider/session、player sequence、fixed step、state/event/presentation hash、terminal route、pending choice 和最近平台 audio meter。audio owner 同时保留 query/drain meter，避免 driver 根据音频文件或静态 report 估算。该 envelope 是 CDP driver 的输入证据，不是 E3 本身；没有 screenshot region drift、CDP dispatch、平台 meter 和完整 route 时仍然 blocking。

`astra-player` 已增加真实 WebSocket CDP protocol owner：稳定 request sequence、command error、timeout、runtime exception、unsupported message 和 duplicate response 都会 fail fast；mouse/keyboard dispatch、固定 launch/canvas geometry query、PNG screenshot capture 与 runtime-owned evidence 解析不再依赖测试里的 `--dump-dom`。当前 transport 及伪 CDP peer 负向边界已测试，浏览器进程/HTTP lifecycle、scenario route 编排和同 run report 聚合仍在后续检查点完成前保持 `IN_PROGRESS`。

### P2-002：容器和 VFS 的局部测试没有覆盖冲突矩阵

**分类：** `SMOKE_ONLY`, `FIXTURE_ONLY`

**证据：** 定向测试 `cargo test -p astra-plugin -p astra-runtime -p astra-package -p astra-asset -p astra-vn-runtime-provider -p astra-player-vn` 全部通过，但现有测试主要覆盖正常 provider registration、单一 package roundtrip、单一 VFS overlay、正常 Runtime tick/save/replay 和单个 NativeVN flow；没有覆盖 `ExtensionRegistry::select`、重复 section id、duplicate layer/prefix、target/profile filtering 或乱序 Runtime tick。

**迁移要求：** 在实现修补前先补齐这些负向测试，确保现有绿色测试不会掩盖冲突输入。测试必须断言稳定 diagnostic code、无部分提交、无状态变化、无错误 report 生成和 package 不可发布。

**2026-07-13 修补进度：** Runtime tick/replay、Plugin binding/lifecycle、VFS graph/context、package builder/reader 冲突矩阵、恶意 table、required schema registry 和 shared release reader 已补齐；`scenario.refs.v2` 把 bundle path 与 package section authority 分离并绑定 hash/size。Cook 已补 sidecar typed dependency、唯一无环 graph、processor registry、source/version identity、持久内容 cache、显式 node/byte/concurrency limits、取消、panic containment、128-node/8-MiB 规模测试和 CLI staging/swap/rollback。P2-002 的 Runtime/Media 长流程、设备恢复和资源释放矩阵仍开放，暂不关闭。

### P1-012：workspace verification 不能可靠地区分当前 checkout 与其他 worktree 的构建产物

**分类：** `SILENT_FAILURE`, `SMOKE_ONLY`

**修复状态：** `RESOLVED`。`Tools/run_cargo_isolated.py` 现将 checkout state、workspace manifest、Cargo.lock、Rust toolchain 与 feature/target/profile fingerprint 绑定到独立 target root，并写出 `astra.build_identity.v1`；`astra_plugin::dylib_path` 与 nested fixture build 共同遵循 `CARGO_TARGET_DIR`。identity mismatch、无效 report 和 Cargo 失败均阻断，artifact evidence 只记录相对 path、role、hash 与 byte size。回归由 `T-S1-BUILD-IDENTITY-01` 和隔离 workspace test 覆盖。

**原始触发证据：** 早期审查直接执行共享 target 的 `cargo test --workspace` 时，logging 测试加载了 fingerprint 不匹配的动态 fixture，随后 observability coverage 又加载了成员数量过期的测试二进制。该现象证明按文件存在性复用其他 checkout artifact 会产生假失败或假通过。当前 `Tools/run_cargo_isolated.py` 已绑定 checkout、manifest、lock、toolchain 和 feature identity；2026-07-14 的隔离 workspace test 通过，原始触发条件已进入回归保护。

**原始影响：** 修复前的测试命令可能把其他 worktree 的测试二进制、动态插件或 manifest 常量混入当前 checkout，导致假失败或假通过。隔离 runner 已把这种身份不确定性改为 blocking diagnostic；后续只能通过该入口生成 workspace/release evidence。

**迁移要求：** 每个 worktree/checkout 使用包含 workspace manifest hash、Rust toolchain fingerprint 和 feature fingerprint 的独立 target/artifact root；动态 fixture 必须在当前 build fingerprint 不匹配时强制重建，不能只按 DLL 文件存在判断。测试报告必须记录 checkout identity、workspace manifest hash、artifact path role、binary hash 和 dependency lock hash；不匹配时 blocking，不允许继续执行并生成产品 evidence。

## 4. 按优先级的迁移路线

### P0：发布证据与状态安全边界

1. 先关闭 `P0-004`：typed contract 之后依次实现 full host、Media 组合、物理输入、PNG/WAV artifact、Developer CLI、测试收束、review 和 preflight。
2. 保持所有无法达到 E3 的 Player、字体视觉、真实音频和真实平台能力为 blocking/in-progress；Headless 最高只计 E2。
3. 在 release validator 中继续区分 E0/E1/E2 fixture evidence 与 E3/E4 product evidence，并阻断 Headless schema/provider/developer artifact 进入 shipping package。
4. 对空成功、静态 report、直接命令、自推进 route、未执行写入和未声明 fallback 增加负向测试。
5. 把同一 build/package/input/profile/session identity continuity 作为 Headless preflight 与真实平台验收的强制条件。

### P1：已存在实现的能力深度补齐

1. 以 Headless service/client 为统一入口，补齐 Media/Renderer/Decode/Audio provider 的生命周期、恢复、长流程、真实输出和资源释放矩阵。
2. 将现有 Package/Asset/Cook authority、缓存、取消和原子提交接入 Headless package source 与 Game-only consumption，补大输入和恢复测试。
3. 整合非 Web 的 Runtime await、Product Stage fixed frame、timeline fence、save/load 和 persistent audio，消除 Player 产品路径与测试路径双轨。
4. 保留字体 shared E2 与 Windows glyph E3 子证据；完整 SceneCommand、GPU FilterGraph、正式 performance artifact 和 Web consumer 未通过前不关闭 `P1-001/P1-005`。
5. AI/Editor/AstraEMU/AstraRPG 没有现有产品实现，本轮只维持 fail-closed 状态，不用 facade、fixture 或空返回补齐它们。

### P2：未实现模块与新增 UE 能力设计

1. Editor：target、bridge、QML shell、PIE、Inspector、Graph/Timeline、Plugin Manager、Release UI。
2. AI/MCP：ModelBundle/VFS、Context Pack、provider profiles、runtime memory、MCP capability、trusted write 和 release gate。
3. AstraEMU：Manager、RuntimeWorld bridge、family plugin、VFS、scheduler、probe、文本和 Artemis gate。
4. AstraRPG：shared policy、runtime provider、AI Town、`rpg.trpg`、local-private adapter 和 Stage 8 protocol。
5. UE 能力域：分别形成架构决策，不把新增能力伪装成现有实现缺陷，也不把设计目标写成完成证据。

### P3：可观测性与文档治理

1. 每个能力维护 owner/provider、入口、状态、证据等级和 release check id。
2. 日志记录 provider selection、fallback、resource lifecycle、diagnostic code、hash 和计数，不能记录 payload 或本地路径。
3. 每个完成项同步更新 `Docs/status/implementation-plan.md`、对应 stage、coverage matrix、stage test matrix、release gate 和 manual。
4. 对 fixture、synthetic、headless 和 local-private 证据明确标注用途和禁止外推的范围。

当前实施顺序以 `P0-004` 为首。`P1-007` 至 `P1-010` 已关闭，不再建立第二套 Runtime/provider/package/VFS authority。Headless 基础设施闭合后处理 `P1-005/P2-002`，再整合非 Web 的 Product Stage、save/timeline/audio 与 Windows 主路径。Web、NativeVN full behavior 和 TsuiNoSora 商业证据按本轮范围暂缓，但继续保留 blocking 状态。

## 5. 后续修补任务格式

每个后续 implementation task 必须包含：

| 字段 | 要求 |
| --- | --- |
| Owner | 唯一 crate/provider/program 和责任边界 |
| Contract | public API、schema、权限、错误和 migration 兼容性 |
| Main path | 从输入到 RuntimeWorld/Player/Editor/package 的真实调用链 |
| Failure | 空、边界、异常、取消、重复、资源丢失和 provider unavailable 行为 |
| Evidence | 测试级别 E0-E4、报告字段、hash、session/package identity |
| Release gate | blocking 条件、允许的显式 warning 和禁止的弱证据 |
| Status | 只有完成全部证据后才能从 planned/in-progress 改为 DONE |
| Observability | 稳定 event、诊断码、provider/fallback、计数和资源生命周期字段 |

## 6. 验收矩阵

### 字体/文本

- 真实字体装载、family/face/coverage/hash 和 package/VFS 绑定。
- Latin、CJK、日文 ruby、RTL、组合字符、emoji、长文本、空文本、缺字体和不支持 glyph。
- shaping 后 glyph advance、kerning、line break、wrap、clip、ellipsis、baseline 和 ruby placement。
- Windows/Web/Editor 真实 capture、同一输入的 layout hash 和 visual region evidence。
- save/load/replay 后文本状态、voice reference 和 layout identity 保持一致。

### Runtime/Package/Provider

- deterministic tick、Await/Delayed/Event queue、snapshot/restore、stable id、MutationLog、effect trace 和 provider-free replay。
- package section bounds/hash/codec、VFS URI、cook artifact、增量/取消/恢复和 Game-only 消费。
- ABI fingerprint、instance create/destroy、session lifecycle、权限、卸载、错误归属和禁止跨 ABI 对象所有权。

### Media/Platform/Player

- Headless 使用独立 `HostKind`/profile，完整执行 surface、audio、decode、save、package、input、event、completion 和 shutdown；它不进入 `PlatformId` 或 shipping dependency graph。
- `astra.user_input_sequence.v1` 只接受物理输入与固定时间控制；语义快捷命令、直接 callback 和 Runtime mutation 必须在 schema/adapter 边界阻断。
- PNG/WAV artifact 来自真实 SceneCommand/AudioGraph/decode 输出，执行容量门禁、自动比较和 required checkpoint review。
- 真实 surface、GPU/CPU provider、device/context loss、纹理/音频资源释放和 bounded queue。
- WMF/WebCodecs/Audio provider 的真实输入输出、seek/pause/resume、错误和 profile-bound fallback。
- Windows/Web 真实 Player 输入、host consumed trace、视觉变化、音频 meter、route completion 和同 run report。

### Editor/AI/EMU/RPG

- planned/reopened 模块不得用 facade、fixture、report 或空实现提升状态。
- 每个正式模块必须有 workspace target、主入口、最小真实场景、负向 gate、save/replay、审计和文档链。

## 7. 本次审查命令与结果

2026-07-14 在 checkout-bound identity 下重新验证当前分支：

- `cargo check --workspace --all-targets` 通过，证明 `HostLaunchProfile` API 已同步到 native factories、Player 和全部当前测试 target。
- `cargo test -p astra-platform` 通过；新增 `headless_launch_profile` 的 3 个测试覆盖六平台枚举保持不变、Headless identity/provider/input/artifact limits 和 native factory variant rejection。
- `python Tools/run_cargo_isolated.py test --workspace` 通过；动态 fixture 与 workspace test 使用同一 checkout-bound target identity。该结果替代早期审查中共享 target 导致的 44/45 crate 假失败记录。
- `python Tools/check_observability.py` 通过，当前 45 个 workspace crate 均有 classification。
- `python Tools/check_docs.py`、`cargo fmt --check` 和 `git diff --check` 通过。
- 完整 `python Tools/run_cargo_isolated.py clippy --workspace --all-targets -- -D warnings` 已在独立构建身份下通过。

这些结果证明 typed Headless launch/profile contract 和当前 workspace 没有回归，不证明 `astra-platform-headless`、PNG/WAV、JSONL runner、统一 `HeadlessTestContext` 或 E3 已完成。原始审查中暴露的 shared-target 污染、固定宽度 TextLayout、provider selection、Runtime tick、VFS 和 package authority 问题均已按各自关闭证据修复；仍开放的问题以本页顶部状态表为准。

## 8. 完成总 migration 的门槛

在后续修补完成前，以下结论必须保持：

- EngineCore、Package、Media contract、AstraVN 已实现部分可以继续独立演进，但不能宣称整个引擎达到 UE 级完备。
- `P0-004` 只有 typed launch/profile contract 完成；完整 Headless host、物理输入、PNG/WAV、CLI、统一测试 context、review 和 preflight 全部闭合前，Stage 2 继续保持 `IN_PROGRESS`。
- TextLayout 已删除固定宽度实现，建立 shared E2 与 Windows Player command/release hardware glyph E3 子证据，但仍不能标记为完整字体产品系统；WebGPU 与完整 product SceneCommand 主路径继续阻断 `P1-001`。Web 本轮暂缓不等于通过。
- Editor、AI/MCP、AstraEMU、AstraRPG 不能从 planned/reopened 改为 DONE，直到存在真实 workspace target、主路径和对应 release evidence。
- Windows/Web Player、NativeVN full behavior 和 TsuiNoSora full playable gate 继续阻断于真实状态推进、完整路线和同 run host evidence；本轮不关闭 NativeVN/TsuiNoSora 产品项。
- 每一项弱证据都必须在报告中保留其真实等级和用途，不能通过改名或重新包装规避验收。

文档校验命令：

```bash
python Tools/check_docs.py
git diff --check
```
