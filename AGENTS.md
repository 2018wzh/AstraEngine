# AstraEngine 实施宪章

## 1. 仓库定位

AstraEngine 仓库是 AstraEngine 系列的产品总入口，负责维护跨仓架构、共享契约、状态矩阵、验收口径和代理实施规则。系列仓库按 lockstep release 协同：

| 仓库 | 职责 |
| --- | --- |
| AstraEngine | EngineCore、Runtime、Asset、Media、Script、插件 ABI、公共测试框架和跨仓契约 |
| AstraVN | `.astra` canonical story、VN preset、商业 VN 基线系统、Luau policy 和发布样例 |
| AstraEditor | Qt/QML creator editor、PIE、Inspector、Graph/Timeline、Package/Release Gate UI |
| AstraEMU | 旧 VN manager、engine-native family plugin、auto probe、Trusted Luau patch/decode、文本翻译和 FilterGraph preset |
| AstraRPG | `AstraRpgRuntimeProvider`、通用 RPG runtime、AI 自主 RPG、`rpg.trpg` ruleset/profile、local-private tabletop adapter 和后续 Server/Client protocol |
| AstraPlatform | 桌面、移动、Web、实验旧主机平台壳和原生能力适配 |

实现时先更新本仓共享契约，再改子仓。不能让子仓私有设计反向污染 EngineCore 边界。

## 2. 架构硬约束

- Runtime 权威模型是 Actor/Component + StateMachine；局部 ECS 只用于可证明的热点批处理，不能作为 creator-facing 对象模型。
- Stage 1 StateMachine 保持 flat FSM；transition 可以顺序执行多个 action，但层级、并行和 pushdown stack 必须另立设计决策。
- StateMachine action 只能通过 `DeterministicActionContext` 修改 Actor/Component、Blackboard、Event、AwaitToken、PresentationCommand 和 delayed event queue。Runtime action 是 host 内注册的 typed Rust action；动态插件不再提供通用 bytes action ABI、effect envelope 或 host adapter 二次应用层。
- Runtime snapshot 必须保存 `StableIdGenerator`、完整 `EventQueue`、AwaitQueue、delayed event queue、MutationLog、StateMachine 与 typed component；不能只保存历史 trace 或在 load 后重建 sequence。实时 tick 不生成序列化 effect trace。
- Gameplay runtime provider 必须为每个 session 持有自己的 `RuntimeWorld`，产品语义经 StateMachine action 提交。跨 ABI provider 使用显式 instance create/destroy 和 session open/step/save/restore/shutdown lifecycle；Replay 只消费 transcript 中的 typed ingress、completion 与 checkpoint，不调用 live provider，也不保存 provider output payload/hash。
- Product Runtime Provider ABI v4 为每个 descriptor 固定唯一 `PresentationLane`。AstraVN 只声明 `Scene2D`，AstraEMU 只声明 `Layer2D`；同一 session 不得混用 lane。Family ABI v9 的实时 layer/audio/video/wait/control 使用 typed DTO 与 ABI-owned allocation；RGBA8/BGRA8 sRGB surface 统一 premultiplied alpha，PCM 与 surface lease 在 live 路径按所有权移动，不经 postcard、JSON、通用 payload 或 content hash envelope。Runtime component 在内存中保存 typed value 与 revision，restore 后才允许首次 typed read 懒解码，save 时才编码；Evidence digest 在提交后独立生成。VFS range 只绑定 revision、offset、length 和 bounds，result 不计算 per-read content hash。
- EngineCore 不依赖 Editor UI、MCP server、AI provider、Luau runtime、legacy VM、平台 GPU/audio handle 或具体 renderer/audio backend。
- Runtime 可使用 Tokio，但 deterministic state 不直接依赖 task completion order。任何可挂起 action 必须落成可序列化 `AwaitToken`，结果在固定 tick 边界进入有序事件队列。
- 插件采用 Rust-facing `abi_stable` 风格 ABI。插件可以加载和卸载，不支持热重载。插件 binary 必须匹配 engine version、rustc fingerprint、feature fingerprint 和 provider descriptor。
- Provider 只能通过 ServiceRegistry、ExtensionRegistry、EngineModuleSlot 暴露能力。不能跨 ABI 传递对象所有权、Actor 指针、Editor widget、GPU/audio native handle。
- `.astra` 是 AstraVN canonical story source。Graph、Timeline 和 Editor layout 只能保存作者元数据，必须能往返到同一 IR、source map 和 debug symbol。
- AstraVN Core 持有 dialogue、choice、backlog、save/load、read-state、voice replay 等权威语义；Rust 插件提供机制，Luau policy 提供表现、系统页和复杂演出策略。
- Luau 通过 `mlua` 进入 AstraVN/AstraEMU policy。默认 capability sandbox，无文件、网络或系统调用；EMU 只提供 patch/decode runtime 和 API，不负责绕过 DRM、商业保护或访问控制。AstraEMU 研究文档保留 Lua/TJS 等旧引擎事实，不作为 AstraVN policy 术语。
- Luau policy 写入、command request、query trace、diagnostic trace 和 snapshot 必须落成可序列化 state；function、thread、userdata、native handle、商业 payload 和本地路径不得进入 save/replay/package/report。
- Save 和 package 是自描述二进制容器，section payload 使用 `postcard`/serde。外部 YAML descriptor 只作为 text-first source，Cook 后不得成为 runtime 必需文件。
- `postcard` save/package section 类型必须对二进制格式稳定；除非有显式自定义 codec，不要在会进入 `postcard` 的 struct 字段上使用 `skip_serializing_if`，否则 save/load 可能只可写不可读。
- Renderer2D 后端可替换，wgpu 是默认 provider。Kira 是默认 audio mixer provider，平台设备仍由显式绑定的 WASAPI、ALSA、CoreAudio、Oboe 或 WebAudio output provider 持有。Decode profile 只能显式绑定一个 provider；桌面可通过 vcpkg 构建 FFmpeg provider，但不得作为失败后的 fallback。视觉 FilterGraph 与音频服务分离。
- Migration 8 的平台边界是 async `PlatformHostFactory`/`PlatformHostClient` 与不可序列化的 generational typed handle；平台资源由本地 event-loop executor 持有。`astra.platform_host_profile.v3` 必须分别声明 `audio_mixer`、`audio_output` 和 verified package cache 限额，v1/v2 profile 直接拒绝。capability v2 只区分 declared/available/selected，API presence 或 smoke 不能声明 provider available。Windows/Web/Android 发布必须同时提供绑定同一 build/profile/package/session 的 host conformance 与 Player automation evidence；Linux、macOS、iOS 在实现前只能返回 `PLATFORM_NOT_IMPLEMENTED`。Android GameActivity event loop 必须由 `android_main` 所在线程持有，不能把 Activity、JNI object、URI 或 native handle 放入公共契约。
- Android release 版本 pin（minSdk、compileSdk、NDK、AGP、Gradle、JDK）和 provider binding 见 `Docs/migrations/platform-host-migration.md`；shipping ABI、renderer/decode/save 唯一绑定和 audio backend 报告规则见 `Docs/contracts/game-runtime-provider.md`。Android Player 必须 interpreter-only，bundle、依赖图或运行报告出现 JIT 即 blocking。
- Migration 11 的 Headless 是 `publish = false` 的测试 host，不是第七个发布平台。`PlatformId` 和 `astra.platform_host_profile.v3` 继续只表达六个平台；Headless 使用独立 `HostKind`、`HeadlessHostProfile` 和 `HostLaunchProfile`，release API、shipping target、AstraPlayer 与 cooked profile 必须拒绝 Headless。
- Migration 11 在 Stage 2 只关闭 Windows native Headless；Linux/macOS Headless 的本机 CI、runtime 与 artifact portability evidence 延后到 Stage 6，WASM、iOS 和 Android 不支持 Headless。共享 contract 和实现不得因此硬编码 Windows 路径或放宽 shipping 隔离。
- Migration 11 实施后，`Engine/Source/Runtime` 下每个测试都必须启动并关闭 `HeadlessTestContext`，包括 parser、schema、derive 和纯数据测试。所有平台无关 Runtime/Player/full-flow 测试统一走 Headless service/client；不得长期保留直接 `HeadlessRendererProvider`、独立 meter、mock sink、ScenarioRunner 私有执行或产品语义快捷命令双轨。
- Migration 11 受控 library target 必须设置 `doctest = false`，代码示例迁到使用 Headless test macro 的 compile/unit test，不能让 Cargo doctest 绕过 session lifecycle。自定义图像/音频容差必须绑定具名人工 `astra.headless_tolerance_approval.v1`；run report 固化 checkpoint config hash，模型不能批准容差或改写旧 report。
- Migration 12 的 AstraVN shipping UI 使用 Yakui，AstraEMU Manager/overlay 使用 Slint，Editor 继续使用 Qt/QML。AstraEMU host 版本 pin（wgpu、winit）和合成约束见 `Docs/migrations/astra-ui-backend-split-migration.md`。第三方 UI 类型不得进入 Astra public contract、package、save/replay、RuntimeWorld 或 plugin ABI；provider 必须由 target/profile/package 显式唯一 binding，缺失或冲突时 blocking，不得按注册顺序或隐式 fallback 选择。
- AstraVN UI 权威分层固定为 `.astra` View/Binding/Action、Rust schema-bound read-only ViewModel、typed Luau Controller effect、Yakui layout/input/paint 与 AstraText/Scene2D。UI 只能提出 request，不能直接写 save、unlock、route cursor 或 Core state；Luau Controller state 只允许 `none/session`，不进入 save。
- Migration 12 完成时必须删除 `SystemUiModel` 固定 hit-test、公开 `compile_astra_sources`/`compile_astra_sources_with_options`、旧 `vn.compiled_story` reader 和 target v1 reader；不保留 deprecated feature、runtime migrator 或 release 双轨。AstraVN UI 只能走 `PresentScene`/Scene2D/Mesh2D，不得恢复 `PresentRgba`、bitmap 或 Headless 产品 presenter。
- UI component 使用独立 `astra-ui-plugin-abi` 和静态 typed slot，只传 bounded typed value tree；live ViewModel、action、semantic 与 paint 路径禁止 JSON/postcard bridge。禁止跨 ABI 传 Yakui node、callback、GPU/window handle。Windows dylib 必须签名并匹配 signer allowlist；Web component 必须校验 WIT/jco 输入输出 hash。panic、trap、超限、权限、timeout 或 restore 失败必须终止 UI session，不得生成替代组件或换 provider。
- workspace 工具链按 ADR 0014 迁到 `rust-toolchain.toml` stable channel，并以 lockfile 和 `astra.build_identity.v1` 固定实际 rustc/Cargo/target/feature 身份。第三方 UI/Luau/Web tool 必须先做 license、target、依赖隔离和 hash preflight，再在同一实现提交精确锁定；失败不得 vendoring 或共享 target fallback。
- Headless 产品测试只接受序列化物理输入与固定时间控制。`advance`、`choose`、`open_system`、直接 `VnPlayerCommand`、DOM/JS runtime hook 和直接状态修改都必须阻断。真实平台验收前必须用同一 build、cooked package 和 input sequence 通过 Headless 自动比较与模型审查；Headless 只形成 E2，不能替代 E3。
- Headless 正式性能门禁只接受 `astra.headless_host_profile.v3` 的精确 GPU policy、`presentation_rate_hz: 120`、clean Release build 和同 build/package/profile/input identity。Runtime 权威 tick 保持 60 Hz，120 Hz 只拆分 presentation cadence，deadline 按 frame index 的有理数时间计算，不能累加截断后的固定纳秒值。集显与独显报告必须分开；软件 adapter、timestamp query 缺失、trace 丢失/截断/回退、资源计数不闭合或预算 blocked 都立即失败。性能 trace 使用 host-owned Perfetto Trace Event writer；分析端只使用外部 `perfetto-mcp`，不得修改 `astra-mcp`，也不得把第三方 MCP、原始 trace、设备名或本地路径写入仓库。
- Headless 视频必须输出有界、逐帧 hash/PTS 校验的完整 decoded stream，不能把 first-frame decode 当作产品播放。正式审查先由 `prepare-review` 固定 required checkpoint、首尾、最大差异、失败邻近帧和完整 WAV，再由 `validate-review` 阻断缺项或覆盖自动失败；正式平台 link 必须同时校验 `astra.platform_run_identity.v1` 和真实平台 report hash。
- `astra-media-core` 只放轻量、可序列化的 Renderer2D/FilterGraph contract、headless CPU frame 和 deterministic executor；`astra-vn` dylib 可以依赖它，但不能为了演出执行把 decode/text/native media 依赖拖入 VN facade。
- Stage 2 Media + Package 的完成边界是 Desktop Native + Headless：默认验证 headless、package、asset/cook、release report 和单一 decode provider binding；六平台 native provider 接入不作为 Stage 2 完成前置。
- FFmpeg 是 optional `ffmpeg-vcpkg` feature，通过 `ffmpeg-next`/`ffmpeg-sys-next` 的 `vcpkg` crate provider 查找本机 FFmpeg。默认 workspace build 不要求本机 FFmpeg；选择 FFmpeg 的 profile 若缺少 provider 必须 blocking，不得切换到其他 decoder。
- Package/save 容器支持 `Postcard`、`Raw` 和 `Zstd` section codec。加密只通过 provider trait、`EncryptionDescriptor`、AAD/hash 和 release gate 表达；仓库不得内置发布密钥或 DRM/访问控制绕过实现。
- Project-level `package_sections` 只能引用项目内相对路径，并用 `targets`/`profiles` 明确限定写入范围。它只适合脱敏 manifest/report section；不得把商业 payload、本地绝对路径、截图、文本、音频、影片或可复原源数据作为 section 写入。
- Runtime AI 与 Editor AI 同等重要。联网 Runtime AI 可发布，但输出通过 IntentValidator 后必须固化进 save/replay，回放不重新请求 provider。
- AstraEMU 使用 Manager + AstraEngine RuntimeWorld + in-process family plugin 架构。family plugin 只注册 `LegacyRuntimeProvider` facade；auto probe、Trusted Luau、文本翻译和 FilterGraph preset 位于 Manager/RuntimeWorld 层，family plugin 不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。
- AstraEMU Family ABI 当前 hard cut 为 v9：所有 dylib lifecycle、VFS、surface、Hook 与 writable-file 调用使用显式 `StableAbi` wire DTO；v7/v8 binary、fingerprint 与旧 runtime snapshot 必须 fail-fast，不保留 compatibility shim。descriptor 只接受 `Native + MultiLayer` 与 `Ported + SingleLayer`。family 通过 Host-owned writable surface lease 直接绘制并提交 `Unchanged`、`Full` 或像素坐标 `Rects` damage；provider step 和整笔 retained Layer2D transaction 验证成功后才能公开 staged generation，失败时整批回滚。Manager 与 native CLI 的 legacy audio 统一由 PlatformHost-backed bounded worker 补水，不能再由 fixed tick 或 Slint event loop 驱动，也不能保留 Manager 私有 CPAL 链。Windows/Linux/macOS host 的 gamepad 使用独立 bounded worker + `EventLoopProxy`/host wake，生产渲染、解码和输入路径不得由 fixed polling timer 推进；无 hotplug handle 时才允许显式低频 discovery wait。
- AstraEMU Extension ABI v1 提供同步 opaque Hook lifecycle；translation companion 只定义 UTF-8 request/response。Manager、CLI 与 Headless 必须按 `(family_id, family_game_id)` 显式选择唯一 provider 与 `u32 timeout_ms`，默认 2000 ms，0 表示立即超时。Hook 必须发生在 surface acquire 前；未绑定、timeout、认证、限流、网络、协议、缺字或布局失败时，family core 保留原文并返回 typed diagnostic。正文、secret、payload 与文本 hash 不得进入日志、SQLite、report 或 package，Host 不提供翻译 overlay 或 cache。
- Family ABI v9 不提供 snapshot/save/restore、text lease、session resource presentation 或 step budget。游戏存档由 family core 通过 per-game writable root 的安全相对路径 API 管理；同一 `(family_id, family_game_id)` 只允许一个 writable session。Host 只提供 stat/list/create-dir/read-range/write-range/set-length/remove/atomic-replace，不跨 ABI 传路径或文件句柄。AstraEMU runtime provider 对共享 save/restore lifecycle 返回 unsupported。AstraEMU runtime state/text/frame/audio/route/session/input 与 RFVP live 路径不得生成语义 hash；package、plugin binary、source/archive entry、schema、build、profile 与 artifact-file 完整性 hash 保留。除 Performance E2 外，预算不作为 AstraEMU blocking policy；ABI 表示、checked arithmetic、buffer/stride、所有权、路径隔离和系统错误仍必须 fail-fast。
- AstraEMU v1 首发 family 是 FVP；固定 rfvp revision 的合法输入可观察行为、完整 syscall coverage、原生存档冷启动与脱敏 parity report 是 FVP release gate。Artemis 和其他 family 以后续 probe report 接入，不能阻塞 EngineCore、AstraVN、Editor 和六平台 gate。默认 auto-probe 顺序保持 KrKr、Artemis、BGI、Siglus、SoftPAL、FVP、Minori，显式 case profile 始终优先。
- Minori GARbro scheme 导入必须使用仓库内纯 Rust 两阶段 NRBF reader，先收集对象、metadata、library 与有符号 object id，再解析 forward reference。不得调用 .NET `BinaryFormatter`、managed helper、外部进程、启发式扫描或任何 fallback；未知 record、断裂 reference、重复 id、越界、非预期 Musica/PAZ graph 和 role/key 约束不满足都必须阻断。
- AstraEMU in-process legacy VFS 契约归 `astra-emu-family-core`，Trusted Luau、mount profile、cache、viewer、verify、extract 与 Linux FUSE 归 `astra-emu-family-support`；`astra-emu-family-api` 只保留 ABI-safe DTO，不得重新导出进程内 trait。
- 通用 VFS 命令固定为 `astra-emu-cli vfs --family <family>`，family 专用研究命令放独立 CLI。Minori 只能使用纯 Rust `MinoriPazDecryptProvider`；Luau 只注册 data-only private profile，不得参与逐 entry 解密，也不得保留 native/Luau fallback。
- AstraRPG 是后续同级 gameplay runtime provider。`AstraTRPG` 不作为独立顶层模块或 provider 落地，只能作为 AstraRPG 的 `rpg.trpg` ruleset/profile layer；package/save/report namespace 使用 `rpg.*` 和 `rpg.trpg.*`，不得新增顶层 `trpg.*`。
- CP2020 等规则书适配只能作为 local-private adapter：仓库可提交 schema、manifest、resolver skeleton、公开最小 fixture、hash、coverage 和 diagnostic，不得提交完整规则正文、表格、扫描图、职业/装备/义体完整清单或可复原 payload。


### 2.2 防回弹与精简硬约束（高性能引擎）

- 新增 crate 必须证明第二消费者或替代现有 crate；<200 行禁止独立 crate，必须并入父 crate；>600 行单文件必须拆模块。
- 推测域（Stage 7/8 RPG、AI/MCP 重媒体生成）文档保留为设计意图，不进入 \Tools/check_docs.py\ 的强制阻塞校验，仅作人类可读 draft 校验。
- 插件指纹仅 \bi_fingerprint\ blocking，\ngine_version/rustc/feature\ 降为 \warn\；工具并入 \cargo xtask\ 仅保人类可读（link + 路径泄露）。
- Runtime \Shipping\ 仅校验 \step+seed\，\HistoryChain/state_hash\ 仅 \save/replay\ 边界计算；帧内不做 \lake3/postcard/clone\。
- \workspace.members\ 预算 ~45，新增需 PR 说明替代关系；\Docs/implementation/workspace-blueprint.md\ \planned\ 行不得入主 workspace。
### 2.1 实现完备性与主路径硬约束

- 设计声明“系统”或“能力”时，不能用最小 happy path 代替完整实现。例如字体系统不能用 `font_size * 常数`、字符数切行、构造未实际使用的 `Metrics` 或“字体名包含 missing”诊断代替；必须有真实 font database/provider、glyph shaping、Unicode/script 覆盖、fallback chain、度量/换行/裁剪/省略、字体资产 hash/lifecycle、跨平台绑定、真实视觉 evidence 和 layout replay 稳定性。
- Contract/core crate 的职责可以是轻量且可序列化，但它只能证明自身 contract、schema、deterministic executor 和错误边界完整；不能把 contract、headless provider、synthetic fixture 或 facade re-export 当成真实 renderer、字体、解码器、Editor 或产品 runtime 已完成。
- 产品主路径必须与设计 owner 一致。Packaged Player 必须从 package/manifest 读取显式 provider binding，创建 provider instance/session，经 `RuntimeWorld` 和 StateMachine action 处理平台事件，再由真实 platform renderer/audio provider 执行；不得在生产 Player 中直接持有 `VnRuntime`、`VnPlayerCommand`、`HeadlessRenderer` 或用 hash/矩形变化伪造场景输出。
- Player route coverage 必须来自同一 session 的真实 Runtime/provider typed route state、terminal/choice signature和输入消费证据；state/event/presentation digest 只能由 Evidence observer 在提交后生成。外部 `expected_routes`、截图发生变化、窗口存在、route report 文件存在或 host consumed trace 单独都不能证明 route coverage 或 `player.full_playable`。
- 所有 provider 选择必须由 `ServiceRegistry`/`ExtensionRegistry` 的显式 binding 决定。不能按注册顺序、排序后的第一个 provider、隐式默认值或缺失时的任意 fallback 选择；公开 `select` API 必须和 `selected_provider` 使用同一 binding 语义，并对缺 binding、冲突和 fingerprint/capability/profile 不匹配返回 blocking diagnostic。
- `RuntimeWorld::mount_module` 必须使用 typed slot/provider binding 或 host-owned binding token，校验 provider registry、selected binding、capability、package/profile eligibility 和 fingerprint；不能接受任意字符串后无条件插入。`tick` 必须明确并校验 fixed step 的首 tick、连续 tick、恢复 tick、重复/回退 tick，以及 `delta_ns`、seed 和 replay tick 语义，非法输入必须失败而不是覆盖当前 step。
- VFS resolve 必须带 target/profile/capability/provider binding context，应用 layer/entry eligibility；重复 prefix、layer id、URI/layer/priority 冲突和未授权 overlay 必须 blocking，不能通过 `BTreeMap` 覆盖或按输入顺序取第一条静默决定结果。
- Package/save container 的 section id 必须非空、合法且唯一；builder 和 reader 都必须阻断重复 id、schema/codec/hash 冲突和同名加密/非加密竞争，不能使用 `iter().find()` 把第一条当作权威 section。
- Source-bound package 解锁只能通过平台提供的不透明用户授权目录读取安全相对路径。`astra.source_unlock_policy.v1` 必须绑定验证 manifest、读取预算、crypto provider 和 protected section 集合；key material 必须从严格匹配的源文件字节派生并仅驻留内存，禁止使用公开 source hash 直接作为密钥、记录本地路径或落盘明文缓存。protected section 缺失、明文、provider/AAD/ciphertext 不匹配均为 blocking。
- 最短 provider lifecycle、单一 fixture、headless capture、synthetic decode、minimal package、facade dylib 和静态 report 只能作为局部 contract/evidence。它们不能关闭完整产品行为、真实 Player、字体/渲染/音频、长流程、恢复、性能或 release gate。
- 实现状态必须按证据等级维护：E0 文件/类型存在，E1 局部单元或 fixture，E2 跨模块 package/provider/replay，E3 真实 Windows/Web Player 输入、host consumed trace、视觉变化、音频 meter、route 和同 run identity，E4 跨平台、规模、恢复、性能、发布包和正式 signoff。字体、渲染、媒体和 Player 产品完成至少需要 E3；没有证据不得标记 `DONE`。
- planned/reopened 模块、空目录、target path、design-only contract、fixture provider 和未加入主 workspace 的源码不能计入实现完成；新增 crate 必须同时接入 workspace、主入口、测试矩阵、observability coverage、release gate 和 manual。

## 3. 文档规则

- 中文主体，API、type、crate、command 和文件名保留英文。
- 文档结构从产品到实现：`Docs/product`、`Docs/contracts`、`Docs/modules`、`Docs/platforms`、`Docs/status`、`Docs/manual`、`Docs/references`、`Docs/adr`。
- 每个模块必须能从设计页走到 contract、public API、data format、test scenario、release gate 和 manual link。
- 设计页只写目标和契约；当前实现状态放在 `Docs/status`。
- 每完成一个实现工作项，必须同步更新 `Docs/status/implementation-plan.md`、对应 Stage 页面、测试矩阵和 coverage matrix；没有通过关联测试和报告证据，不得把状态标为 `DONE`。
- 修改页面结构时，同步更新最近的 README 或索引。
- 中文技术文档按 `humanizer-zh` 处理：去掉翻译腔、堆砌列表和空泛结尾，事实和实现状态不得拔高。
- 不写营销文案，不把 planned work 写成 implemented behavior。

## 4. 代码 Workspace、Rust 与脚本风格

- 代码 workspace 采用 UE 风格顶层分区：`Engine/` 放共享 runtime、developer tool、program 和 plugin fixture；`Editor/`、`Emulator/`、`Examples/` 作为产品与样例入口；`Docs/` 和 `Tools/` 保持顶层。
- Rust 内部仍按 crate 边界开发。每个 crate 只承担单一清晰职责，不把 Editor、AstraEMU family、AI/MCP 或平台后端私有逻辑塞回 EngineCore。
- crate 内按 Rust module 拆分，`lib.rs` 只做薄 facade 和 re-export。核心类型、调度、save、loader、runner 等实现放进独立模块；单文件接近 400-600 行时优先拆成更小模块。
- 新增 crate、移动路径或调整 UE 风格目录时，同步更新根 `Cargo.toml`、`Docs/implementation/workspace-blueprint.md`、coverage matrix、stage test matrix 和最近索引。

- Rust 采用 idiomatic Rust：`snake_case` 函数和变量，`PascalCase` 类型，`SCREAMING_SNAKE_CASE` 常量。
- 必须运行 `rustfmt` 和 `clippy`；公共 API 变更需要对应 contract 和 migration 说明。
- derive 宏可以生成 PropertySystem、serde、schema、Inspector、save/replay、MCP patch glue 和注册样板。宏必须支持 `cargo expand` 调试路径，不得生成隐藏继承、全局对象系统或不可见生命周期。
- 日志统一使用 `astra-observability`。Rust 库只发 `tracing` span/event；二进制入口负责 `init_host`、sink 生命周期和 flush。日志不得参与 deterministic state、hash、save 或 replay；machine-readable report 走 stdout，日志走 stderr、显式相对日志目录或平台 writable diagnostics 目录。
- 每条事件必须有稳定 `event` 字段，target 使用 crate/domain category。`TRACE` 记录 tick/frame/queue/provider 高频细节，`DEBUG` 记录选择、映射和状态差异，`INFO` 记录 host/session/world/package/plugin/media/VN/platform 生命周期，`WARN` 只表示允许继续的显式降级，`ERROR` 只由拥有根因或最终处置权的边界记录。不得沿调用栈重复记录同一错误。
- 日志字段只记录 step、schema、hash、diagnostic code、provider/action/plugin id、状态和计数。不得记录商业文本、payload body、secret、native handle、私有环境值、本地绝对路径或未经审计的整体 `Debug` 对象；昂贵字段必须在对应 level enabled 后计算。
- main file queue 丢弃低级别事件时必须累计 `dropped_count` 并走独立 critical WARN；WARN/ERROR 同步镜像到 critical ring/file。crash artifact 始终是 local-private 敏感数据，不得进入 package、report、Git 或自动上传。
- 跨平台脚本使用 Python，不使用 PowerShell 编写项目脚本。
- Markdown 中的命令示例使用 `bash`/`sh` 风格；不要把 PowerShell 作为项目文档的默认执行路径。
- Rust 类型是 schema 真源。YAML descriptor 和 scenario 必须配 serde 类型，并通过 `schemars` 生成 JSON Schema。
- 每个 Codex 实例必须独占一个 Git worktree。禁止多个实例在同一 worktree 中编辑、编译或测试，也禁止跨 worktree 共享 `target`、临时产物目录或仍在运行的本地服务。实例开始工作前必须确认 worktree、branch 和进程归属；发现冲突时立即停止，不能靠覆盖文件、抢占端口或复用构建产物继续执行。
- 每个实例只清理自己创建的文件、进程和 worktree。任务完成、取消或切换后，应及时停止后台进程，删除不再需要的临时 fixture、日志、报告、下载缓存和构建产物；移除 worktree 前必须确认没有未提交修改。不得删除其他实例仍在使用的 worktree、branch、`target` 或 evidence。长期不用的 worktree 应在确认无主后移除，并执行 `git worktree prune` 清理失效元数据。

## 5. 测试与验收

- 开发迭代只运行本次改动所必需的最小测试集合，不要在每次修改后重复执行全 workspace 测试。测试范围必须覆盖改动 crate、直接受影响的调用方、相关 contract/schema、回归用例和本次失败路径；公共 API、共享 contract、feature graph、workspace 配置或跨模块行为发生变化时，应按实际影响扩大范围。不得为了缩短时间跳过已知受影响的测试，也不得用局部通过代替下述提交前门禁和正式验收。

提交前至少执行：

```bash
python Tools/check_docs.py
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p astra-headless
cargo test --workspace
```

仅修改文档时至少执行：

```bash
python Tools/check_docs.py
```

该脚本同时检查文档断链、状态页覆盖矩阵和历史标记残留。

全量 clippy/test 直接使用当前独占 worktree 的 Cargo target。执行 workspace test 前必须先构建 `astra-headless`；测试框架只从当前 test executable 的 Cargo target/profile 解析同 worktree binary，不接受 `ASTRA_HEADLESS_BINARY` 或 `ASTRA_HEADLESS_BINARY_HASH` 覆盖。同一 test binary 的并行测试复用一个多 session Headless server，session lifecycle、artifact root 和可变 Runtime/media state 必须隔离；最后一个 session 经短 idle grace 后关闭 server 并删除临时产物。禁止设置指向其他 worktree 的 `CARGO_TARGET_DIR`，也禁止复用其他实例生成的动态 fixture。命令超时、局部测试通过、fixture 通过或静态 report 生成，都不能替代完整 workspace 和真实 host evidence。

统一 Headless 后端使用双向 JSONL `astra.user_input_sequence.v1`，并输出真实 PNG/WAV、artifact manifest 和 run report；旧 YAML runner 已删除，`--headless` 入口只返回显式迁移错误，不保留 alias。产品/full-flow 需要自动比较和模型审查双门禁，模型必须实际查看 required checkpoint 与音频分析结果，且不能覆盖自动失败或自行放宽容差。Release Gate 必须输出 machine-readable report。

Stage 3 AstraVN 不能用 fake smoke 代替验收。VN route scenario 必须通过 player 层输入推进 dialogue、choice、system page、save/load、replay、`complete_wait` 和 hash/assertion；Web player gate 必须由浏览器宿主读取 bundle manifest、package hash、route model 和 scenario 后输出 route report，不能只检查静态 HTML 或 bundle 文件存在。release gate 只能写可验证 manifest、coverage、diagnostic 和 hash evidence。

TsuiNoSora target 的 release gate 必须验证 `tsuinosora.reference_evidence`、`tsuinosora.asset_analysis`、`tsuinosora.conversion_manifest`、`tsuinosora.mount_policy`、`tsuinosora.modern_profile_report` 和 formal release `tsuinosora.manual_signoff`，并在缺源、缺 coverage、素材 quarantine、路径泄露或 payload 泄露时 blocking。所有 TsuiNoSora sidecar schema 字段规范、禁止字段、blocking 条件和 bundle 约束见 [TsuiNoSora Sidecar Schema Contract](Docs/contracts/tsuinosora-sidecar-schema.md)。真实源、解包产物、调试截图和中间 NativeVN 输出只能放在 ignored 私有工作区，例如 `.tmp/` 或样例本地目录。

## 6. 变更边界

- NativeVN 公开样例只保留紧凑技术验收内容。15–20 分钟、三终局、中英双语、中文全配音和正式原创资产属于 `Docs/migrations/nativevn-flagship-demo-migration.md`；该 migration 完成许可与产品验收前，不得提交 Windows SAPI/TTS 产物或把旗舰 Demo 标为 Stage 3 完成证据。

- 优先复用成熟库和已有模式，不为单一实现新增抽象。
- 任何新增 public contract 都要同时说明权限、诊断、migration、release gate 和最小测试。
- 旧 VN 兼容不能成为 NativeVN、Editor 或 EngineCore 达标前置条件。
- 不提交商业游戏 payload、未授权截图或可绕过访问控制的说明；测试报告和示例数据不得泄露私有绝对路径。
- 商业视觉参考只允许使用仓库中明确列为参考证据的文件。新的商业截图、文本、音频或影片只能写入 ignored 私有调试目录；可提交 report 只能写 hash、尺寸、区域 id、coverage、diagnostic 和 layout metric。
- Git 提交使用短祈使句，例如 `[docs] Rewrite product architecture`。
