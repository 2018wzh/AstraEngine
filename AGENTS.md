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
- StateMachine action 只能通过 `DeterministicActionContext` 修改 Actor/Component、Blackboard、Event、AwaitToken、PresentationCommand 和 delayed event queue。插件 action 必须返回可序列化 effect list，由 host adapter 应用。
- Runtime snapshot 必须保存 `StableIdGenerator`、完整 `EventQueue`、AwaitQueue、delayed event queue、MutationLog 和序列化 effect trace；不能只保存历史 trace 或在 load 后重建 sequence。
- Gameplay runtime provider 必须为每个 session 持有自己的 `RuntimeWorld`，产品语义经 StateMachine action 提交。跨 ABI provider 使用显式 instance create/destroy 和 session open/step/save/restore/shutdown lifecycle；Replay 只消费已校验 hash 的 recorded provider output，不调用 live provider。
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
- Renderer2D 后端可替换，wgpu 是默认 provider。平台解码优先，桌面可通过 vcpkg 接 FFmpeg fallback。视觉 FilterGraph 和 AudioGraph 分离。
- Migration 8 的平台边界是 async `PlatformHostFactory`/`PlatformHostClient` 与不可序列化的 generational typed handle；平台资源由本地 event-loop executor 持有。`astra.platform_host_profile.v2` 必须声明 verified package cache 限额，Player 只可显式迁移 v1 profile；capability v2 只区分 declared/available/selected，API presence 或 smoke 不能声明 provider available。Windows/Web/Android 发布必须同时提供绑定同一 build/profile/package/session 的 host conformance 与 Player automation evidence；Linux、macOS、iOS 在实现前只能返回 `PLATFORM_NOT_IMPLEMENTED`。Android GameActivity event loop 必须由 `android_main` 所在线程持有，不能把 Activity、JNI object、URI 或 native handle 放入公共契约。
- Android release 固定 minSdk 28、compileSdk/targetSdk 36、Build Tools 36.0.0、NDK 30.0.15729638、AGP 9.3.0、Gradle 9.5.0 和 JDK 17；shipping 只含 `arm64-v8a`，`x86_64` 仅用于 emulator。renderer/decode/save 必须分别唯一绑定 `wgpu_vulkan`、`mediacodec`、`android_app_storage`；音频实际 backend 必须报告 `oboe_aaudio` 或显式 compatibility profile 下的 `oboe_opensl_es`，禁止静默切换。Android Player 必须 interpreter-only，bundle、依赖图或运行报告出现 JIT 即 blocking。
- Migration 11 的 Headless 是 `publish = false` 的测试 host，不是第七个发布平台。`PlatformId` 和 `astra.platform_host_profile.v2` 继续只表达六个平台；Headless 使用独立 `HostKind`、`HeadlessHostProfile` 和 `HostLaunchProfile`，release API、shipping target、AstraPlayer 与 cooked profile 必须拒绝 Headless。
- Migration 11 在 Stage 2 只关闭 Windows native Headless；Linux/macOS Headless 的本机 CI、runtime 与 artifact portability evidence 延后到 Stage 6，WASM、iOS 和 Android 不支持 Headless。共享 contract 和实现不得因此硬编码 Windows 路径或放宽 shipping 隔离。
- Migration 11 实施后，`Engine/Source/Runtime` 下每个测试都必须启动并关闭 `HeadlessTestContext`，包括 parser、schema、derive 和纯数据测试。所有平台无关 Runtime/Player/full-flow 测试统一走 Headless service/client；不得长期保留直接 `HeadlessRendererProvider`、独立 meter、mock sink、ScenarioRunner 私有执行或产品语义快捷命令双轨。
- Migration 11 受控 library target 必须设置 `doctest = false`，代码示例迁到使用 Headless test macro 的 compile/unit test，不能让 Cargo doctest 绕过 session lifecycle。自定义图像/音频容差必须绑定具名人工 `astra.headless_tolerance_approval.v1`；run report 固化 checkpoint config hash，模型不能批准容差或改写旧 report。
- Migration 12 的 AstraVN shipping UI 使用 Yakui，AstraEMU Manager/overlay 使用 Slint 1.17.1，Editor 继续使用 Qt/QML。AstraEMU host 精确绑定 wgpu 29.0.4 与 winit 0.30，由 Slint host 持有窗口、事件循环、surface 和同一套 wgpu `Device`/`Queue`；游戏 underlay 与 Slint overlay 不得通过 CPU 整帧回读或跨设备纹理复制合成。第三方 UI 类型不得进入 Astra public contract、package、save/replay、RuntimeWorld 或 plugin ABI；provider 必须由 target/profile/package 显式唯一 binding，缺失或冲突时 blocking，不得按注册顺序或隐式 fallback 选择。
- AstraVN UI 权威分层固定为 `.astra` View/Binding/Action、Rust schema-bound read-only ViewModel、typed Luau Controller effect、Yakui layout/input/paint 与 AstraText/Scene2D。UI 只能提出 request，不能直接写 save、unlock、route cursor 或 Core state；Luau Controller state 只允许 `none/session`，不进入 save。
- Migration 12 完成时必须删除 `SystemUiModel` 固定 hit-test、公开 `compile_astra_sources`/`compile_astra_sources_with_options`、旧 `vn.compiled_story` reader 和 target v1 reader；不保留 deprecated feature、runtime migrator 或 release 双轨。AstraVN UI 只能走 `PresentScene`/Scene2D/Mesh2D，不得恢复 `PresentRgba`、bitmap 或 Headless 产品 presenter。
- UI component 使用独立 `astra-ui-plugin-abi` 和静态 typed slot，只传 bounded serialized DTO；禁止跨 ABI 传 Yakui node、callback、GPU/window handle。Windows dylib 必须签名并匹配 signer allowlist；Web component 必须校验 WIT/jco 输入输出 hash。panic、trap、超限、权限、timeout 或 restore 失败必须终止 UI session，不得生成替代组件或换 provider。
- workspace 工具链按 ADR 0014 迁到 `rust-toolchain.toml` stable channel，并以 lockfile 和 `astra.build_identity.v1` 固定实际 rustc/Cargo/target/feature 身份。第三方 UI/Luau/Web tool 必须先做 license、target、依赖隔离和 hash preflight，再在同一实现提交精确锁定；失败不得 vendoring 或共享 target fallback。
- Headless 产品测试只接受序列化物理输入与固定时间控制。`advance`、`choose`、`open_system`、直接 `VnPlayerCommand`、DOM/JS runtime hook 和直接状态修改都必须阻断。真实平台验收前必须用同一 build、cooked package 和 input sequence 通过 Headless 自动比较与模型审查；Headless 只形成 E2，不能替代 E3。
- Headless 视频必须输出有界、逐帧 hash/PTS 校验的完整 decoded stream，不能把 first-frame decode 当作产品播放。正式审查先由 `prepare-review` 固定 required checkpoint、首尾、最大差异、失败邻近帧和完整 WAV，再由 `validate-review` 阻断缺项或覆盖自动失败；正式平台 link 必须同时校验 `astra.platform_run_identity.v1` 和真实平台 report hash。
- `astra-media-core` 只放轻量、可序列化的 Renderer2D/FilterGraph contract、headless CPU frame 和 deterministic executor；`astra-vn` dylib 可以依赖它，但不能为了演出执行把 decode/text/native media 依赖拖入 VN facade。
- Stage 2 Media + Package 的完成边界是 Desktop Native + Headless：默认验证 headless、package、asset/cook、release report 和 profile-bound fallback policy；六平台 native provider 接入不作为 Stage 2 完成前置。
- FFmpeg 是 optional `ffmpeg-vcpkg` feature，通过 `ffmpeg-next`/`ffmpeg-sys-next` 的 `vcpkg` crate provider 查找本机 FFmpeg。默认 workspace build 不要求本机 FFmpeg；release profile 必须明确把缺失 FFmpeg 判为 warning 还是 blocking，不能静默 fallback。
- Package/save 容器支持 `Postcard`、`Raw` 和 `Zstd` section codec。加密只通过 provider trait、`EncryptionDescriptor`、AAD/hash 和 release gate 表达；仓库不得内置发布密钥或 DRM/访问控制绕过实现。
- Project-level `package_sections` 只能引用项目内相对路径，并用 `targets`/`profiles` 明确限定写入范围。它只适合脱敏 manifest/report section；不得把商业 payload、本地绝对路径、截图、文本、音频、影片或可复原源数据作为 section 写入。
- Runtime AI 与 Editor AI 同等重要。联网 Runtime AI 可发布，但输出通过 IntentValidator 后必须固化进 save/replay，回放不重新请求 provider。
- AstraEMU 使用 Manager + AstraEngine RuntimeWorld + in-process family plugin 架构。family plugin 只注册 `LegacyRuntimeProvider` facade；auto probe、Trusted Luau、文本翻译和 FilterGraph preset 位于 Manager/RuntimeWorld 层，family plugin 不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。
- AstraEMU v1 首发 family 是 FVP；固定 rfvp revision 的合法输入可观察行为、完整 syscall coverage、snapshot/replay 与脱敏 parity report 是 FVP release gate。Artemis 和其他 family 以后续 probe report 接入，不能阻塞 EngineCore、AstraVN、Editor 和六平台 gate。默认 auto-probe 顺序保持 KrKr、Artemis、BGI、Siglus、SoftPAL、FVP、Minori，显式 case profile 始终优先。
- AstraRPG 是后续同级 gameplay runtime provider。`AstraTRPG` 不作为独立顶层模块或 provider 落地，只能作为 AstraRPG 的 `rpg.trpg` ruleset/profile layer；package/save/report namespace 使用 `rpg.*` 和 `rpg.trpg.*`，不得新增顶层 `trpg.*`。
- CP2020 等规则书适配只能作为 local-private adapter：仓库可提交 schema、manifest、resolver skeleton、公开最小 fixture、hash、coverage 和 diagnostic，不得提交完整规则正文、表格、扫描图、职业/装备/义体完整清单或可复原 payload。

### 2.1 实现完备性与主路径硬约束

- 设计声明“系统”或“能力”时，不能用最小 happy path 代替完整实现。例如字体系统不能用 `font_size * 常数`、字符数切行、构造未实际使用的 `Metrics` 或“字体名包含 missing”诊断代替；必须有真实 font database/provider、glyph shaping、Unicode/script 覆盖、fallback chain、度量/换行/裁剪/省略、字体资产 hash/lifecycle、跨平台绑定、真实视觉 evidence 和 layout replay 稳定性。
- Contract/core crate 的职责可以是轻量且可序列化，但它只能证明自身 contract、schema、deterministic executor 和错误边界完整；不能把 contract、headless provider、synthetic fixture 或 facade re-export 当成真实 renderer、字体、解码器、Editor 或产品 runtime 已完成。
- 产品主路径必须与设计 owner 一致。Packaged Player 必须从 package/manifest 读取显式 provider binding，创建 provider instance/session，经 `RuntimeWorld` 和 StateMachine action 处理平台事件，再由真实 platform renderer/audio provider 执行；不得在生产 Player 中直接持有 `VnRuntime`、`VnPlayerCommand`、`HeadlessRenderer` 或用 hash/矩形变化伪造场景输出。
- Player route coverage 必须来自同一 session 的真实 Runtime/provider route state、terminal/choice signature、state/event/presentation hash 和输入消费证据。外部 `expected_routes`、截图发生变化、窗口存在、route report 文件存在或 host consumed trace 单独都不能证明 route coverage 或 `player.full_playable`。
- 所有 provider 选择必须由 `ServiceRegistry`/`ExtensionRegistry` 的显式 binding 决定。不能按注册顺序、排序后的第一个 provider、隐式默认值或缺失时的任意 fallback 选择；公开 `select` API 必须和 `selected_provider` 使用同一 binding 语义，并对缺 binding、冲突和 fingerprint/capability/profile 不匹配返回 blocking diagnostic。
- `RuntimeWorld::mount_module` 必须使用 typed slot/provider binding 或 host-owned binding token，校验 provider registry、selected binding、capability、package/profile eligibility 和 fingerprint；不能接受任意字符串后无条件插入。`tick` 必须明确并校验 fixed step 的首 tick、连续 tick、恢复 tick、重复/回退 tick，以及 `delta_ns`、seed 和 replay tick 语义，非法输入必须失败而不是覆盖当前 step。
- VFS resolve 必须带 target/profile/capability/provider binding context，应用 layer/entry eligibility；重复 prefix、layer id、URI/layer/priority 冲突和未授权 overlay 必须 blocking，不能通过 `BTreeMap` 覆盖或按输入顺序取第一条静默决定结果。
- Package/save container 的 section id 必须非空、合法且唯一；builder 和 reader 都必须阻断重复 id、schema/codec/hash 冲突和同名加密/非加密竞争，不能使用 `iter().find()` 把第一条当作权威 section。
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

- 代码 workspace 采用 UE 风格顶层分区：`Engine/` 放共享 runtime、developer tool、program 和 plugin fixture；`Editor/`、`AstraEMU/`、`Examples/` 作为产品与样例入口；`Docs/` 和 `Tools/` 保持顶层。
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
- 多 worktree/checkout 的验证必须使用独立且可识别的 target/artifact root，至少绑定 workspace manifest hash、Rust toolchain fingerprint、feature fingerprint、Cargo.lock hash 和当前 commit。动态 fixture binary 不能只按文件存在判断是否新鲜，必须校验或重建；artifact identity 不匹配时必须 blocking。

## 5. 测试与验收

- 开发迭代只运行本次改动所必需的最小测试集合，不要在每次修改后重复执行全 workspace 测试。测试范围必须覆盖改动 crate、直接受影响的调用方、相关 contract/schema、回归用例和本次失败路径；公共 API、共享 contract、feature graph、workspace 配置或跨模块行为发生变化时，应按实际影响扩大范围。不得为了缩短时间跳过已知受影响的测试，也不得用局部通过代替下述提交前门禁和正式验收。

提交前至少执行：

```bash
python Tools/check_docs.py
cargo fmt --check
python Tools/run_cargo_isolated.py clippy --workspace --all-targets -- -D warnings
python Tools/run_cargo_isolated.py test --workspace
```

仅修改文档时至少执行：

```bash
python Tools/check_docs.py
```

该脚本同时检查文档断链、状态页覆盖矩阵和历史标记残留。

全量 clippy/test 必须通过 `Tools/run_cargo_isolated.py` 在当前 checkout 的新鲜构建身份下执行。该入口把 commit/dirty state、workspace manifest、Cargo.lock、Rust toolchain 和 feature/target/profile 参数绑定到独立 target root，并写出不含绝对路径的 `astra.build_identity.v1`；动态 fixture 必须从同一 `CARGO_TARGET_DIR` 构建和加载。identity mismatch、无效 report 或命令失败必须 blocking，不能退回共享 `target/debug`。命令超时、局部测试通过、fixture 通过或静态 report 生成，都不能替代完整 workspace 和真实 host evidence。

统一 Headless 后端使用双向 JSONL `astra.user_input_sequence.v1`，并输出真实 PNG/WAV、artifact manifest 和 run report；旧 YAML runner 已删除，`--headless` 入口只返回显式迁移错误，不保留 alias。产品/full-flow 需要自动比较和模型审查双门禁，模型必须实际查看 required checkpoint 与音频分析结果，且不能覆盖自动失败或自行放宽容差。Release Gate 必须输出 machine-readable report。

Stage 3 AstraVN 不能用 fake smoke 代替验收。VN route scenario 必须通过 player 层输入推进 dialogue、choice、system page、save/load、replay、`complete_wait` 和 hash/assertion；Web player gate 必须由浏览器宿主读取 bundle manifest、package hash、route model 和 scenario 后输出 route report，不能只检查静态 HTML 或 bundle 文件存在。`vn.standard_commands`、`vn.presentation_provider`、`vn.advanced_presentation` 等 release gate 只能写可验证 manifest、coverage、diagnostic 和 hash evidence。Advanced profile 必须用 opt-in package 与 scenario 证明多层 stage、camera、video、timeline join/cancel、fallback、voice sync 和 effect budget，不得影响普通 `classic`/`modern` gate。TsuiNoSora target 的 release gate 必须验证 `tsuinosora.reference_evidence`、`tsuinosora.asset_analysis`、`tsuinosora.conversion_manifest`、`tsuinosora.mount_policy`、`tsuinosora.modern_profile_report` 和 formal release `tsuinosora.manual_signoff`，并在缺源、缺 coverage、素材 quarantine、路径泄露或 payload 泄露时 blocking；`extract-readable` 只能复制合法可直接读取的 sidecar 资源，解析未加密 RIFF/RIFX container 的 `imap`/`mmap` resource map、`KEY*`/`CAS*` cast map、`Lctx`/`Lnam`/`Lscr` Lingo map 和受限 chunk 表，并抽取带公开文件签名的 embedded payload；`XFIR` 只允许 verified exact wrapper 中的 RIFF/RIFX payload 进入同一 reader，wrapper size 必须覆盖整个文件，opaque、压缩、尾随未验证 bytes 或 source/hash 断裂的 Shockwave 容器必须 blocking，不能退回线性扫描或伪装成 RIFF/RIFX；从 Director cast map 到 `tsuinosora.cast_source_map_report.v1` 的派生只能通过 resource id、FourCC、container entry id 和 extracted payload hash 证明，不得靠文件名猜测或写入素材 payload；手写或外部 reader 产出的 cast sidecar 如果声明 `source_hash`，必须匹配 actual extracted source asset，mismatch 必须 blocking；`tsuinosora.route_graph.v1` 只能记录 sanitized route id、terminal id、choice id 和 coverage，不得写正文、bytecode、payload 或不安全 symbol；`tsuinosora.director_lingo_map.v1` 只能记录 Lingo resource id、entry id、size、hash、`Lnam` entry count、text-extractable 标志和 bytecode-reader 需求，不能写 Lingo names、脚本文本或 bytecode；`tsuinosora.script_source_map.v1` 只能作为外部 reader 或 `extract-readable` 对可读或短 binary-header wrapped mapped `Lscr` 生成的内部 reader 脱敏 sidecar，字段限于 reader identity/hash、source relative path/hash、line、route id、terminal id、choice id、coverage、Lscr resource id 和 Lscr payload hash，route source 必须匹配同一 sidecar 中声明的 source，route `source_hash` 必须等于该 source 的 `sha256`，route line 必须在声明 source line_count 内，且声明 source hash 必须匹配现有 report-relative source 文件；source 指向含 unsupported `Lscr` bytecode 的 `director_lingo_map.json` 时，route 还必须声明匹配的 `script_resource_id` 和 `script_payload_sha256`；严禁写 `text`、`script_text`、`source_text`、`content`、`payload`、`bytecode` 或本地路径；它不得伪装成完整 Director/Shockwave cast/source-map reader，遇到未解析、截断、不可读、source/hash/resource 断裂或 coverage 不可证明的 container 必须 blocking；`tsuinosora.director_resource_map.v1`（含 `free_resource_count`，free mmap entry 不作为 payload/tag evidence）、`tsuinosora.director_cast_map.v1`、`tsuinosora.director_lingo_map.v1`、`tsuinosora.cast_source_map_report.v1`、`tsuinosora.script_source_map.v1`、`tsuinosora.script_source_map_report.v1` 和 `tsuinosora.route_graph_report.v1` 只能记录 resource id、member id、route id、terminal id、choice id、reader id/hash/output_contract、source relative path、container entry id、line、hash 和 diagnostic，不得写商业脚本文本或素材 payload；同一 route 同时来自抽出的 `.ls` 文本和 reader sidecar 时，`tsuinosora.script_source_map_report.v1` 必须优先保留 reader source-map evidence 和脱敏 reader identity/hash，避免重复 coverage；NativeVN package input 必须把 route graph/source map 中的 sanitized choice id 保留到 `.astra` option key 和 scenario `player_input choose`，不能替换成虚构单选；从 `tsuinosora.cast_source_map_report.v1` 的 route-bound member 到 `native-assets/` 的映射必须通过 source hash、converted hash、classification 和 route id 生成 `mount_assets`，不能靠文件名猜测或让 local-gate 丢失由 report 派生的 choice/mount evidence；Asset analysis 必须先记录脚本引用、container alias、尺寸、透明通道、visible bbox、edge padding、颜色分布、重复 hash、atlas crop/part、reference match 和分类冲突，再允许重排到 `native-assets/`；`tsuinosora.native_asset_rearrange_report.v1` 和 `tsuinosora.conversion_report.v1` 只能记录 source/native 相对路径、classification、hash、byte size、coverage 和 diagnostic，rearrange 失败时 conversion 必须 blocking。真实源、解包产物、调试截图和中间 NativeVN 输出只能放在 ignored 私有工作区，例如 `.tmp/` 或样例本地目录。
补充：`tsuinosora.director_lingo_map.v1` 的 `Lctx` 资源也只能输出 entry count 和 table hash；不得输出 context payload。
补充：`Lctx` payload size 必须按 32-bit entry 对齐；未对齐时 `tsuinosora.director_lingo_map.v1` 必须 blocking，不能作为 source-map reader 前置证据。
补充：当前内置 `Lnam` preflight 只接受 null-terminated sanitized name table；未终止或无法证明边界的 `Lnam` 必须 blocking，不能输出或猜测 Lingo name。
补充：`tsuinosora.director_cast_map.v1` 遇到同一个 `CASt` resource 被多个 `CAS*` library/slot 绑定时必须 blocking，不能静默保留第一条映射作为 route/source-map evidence。
补充：`tsuinosora.cast_map.v1` 和外部 `tsuinosora.director_cast_map.v1` sidecar 不得包含 `text`、`script_text`、`source_text`、`content`、`payload` 或 `bytecode` 字段；`tsuinosora.cast_source_map_report.v1` 遇到这些字段必须 blocking，且 diagnostic 只能记录字段路径。
补充：`CASt` payload 只有在显式声明 `tsuinosora.director_cast_member_metadata.v1` 时才能作为脱敏 metadata 读取，允许字段限于 kind、route id、command id、anchor、bounds、character atlas part/crop/pose/expression/layer/fallback/state compatibility 和 metadata hash；这些字段必须继续传入 `tsuinosora.cast_source_map_report.v1`，不得输出 cast payload、正文、bytecode 或本地路径。
补充：`tsuinosora.director_cast_member_metadata.v1` 的 anchor 必须是数值 `x`/`y`，bounds 必须是非负数值 `x`/`y`/`width`/`height`；类型不明或负尺寸必须 blocking，不能静默丢弃布局证据。
补充：`kind: character_atlas` 必须携带 parts；每个 part 的 id、pose、expression、layer 和 fallback 必须是 safe symbol，anchor/crop 必须是数值矩形，mouth/eye state compatibility 必须是 boolean。缺 parts 或 part 字段不合规则必须 blocking，不能把合并差分图当单张立绘继续转换。
补充：默认 `Title.png`/`Game.png` 视觉参考必须校验固定尺寸和 hash；缺文件、PNG 不可读、hash mismatch 或 dimensions mismatch 必须让 `tsuinosora.visual_reference_report.v1` 和 Stage 3 gate blocking，report 仍只能写 hash、尺寸、区域 id、layout metric 和 diagnostic。
补充：`tsuinosora.route_graph_report.v1` 和 `tsuinosora.script_source_map_report.v1` 中同一 `route_id` 不能映射到多个 terminal/choice signature；冲突时必须 blocking，不能继续生成 NativeVN story 或 scenario refs。
补充：同一 route 内的 `choices` 必须唯一；重复 choice id 必须 blocking，不能生成重复 `player_input choose` 或 `.astra` option key。
补充：`stage3-gate` 只能在 route graph 缺失时使用 `tsuinosora.script_source_map_report.v1` fallback；如果存在 route graph sidecar 但 payload、symbol、coverage 或 duplicate 检查失败，fallback 不能绕过 blocking diagnostic。
补充：`tsuinosora.script_source_map_report.v1` 对 unsupported Lingo bytecode 必须逐个 `Lscr` resource 覆盖；同一个 `director_lingo_map.json` 中只覆盖部分 `script_resource_id`/`script_payload_sha256` 时必须 blocking。
补充：RIFF/RIFX Director container 的 declared size 必须精确匹配可读文件大小；mismatch 时不得记录 resource/tag coverage，也不得抽取 embedded payload。
补充：`tsuinosora.nativevn_package_input_report.v1` 必须重新校验显式传入的 route；不安全 `route_id`/terminal/choice、非 covered coverage、重复 choice 或冲突 route signature 都必须 blocking，且不能写出 story 或 scenario refs。
补充：`local-gate` 不能把显式传入的 routes 当作商业 route coverage；真实本地 gate 必须从 `tsuinosora.route_graph_report.v1` 或 `tsuinosora.script_source_map_report.v1` 派生 routes，显式 routes 只能触发 blocking diagnostic，不能写出 NativeVN package input。
补充：`demo-slice --config` 只能作为私有真实数据切片入口，config 中的 root 只作为运行参数读取，report/package/docs 只能写 alias、相对路径、hash、byte size、coverage 和 diagnostic；config 中显式 routes 必须 blocking，不能替代 route graph 或 script source-map evidence。通过 demo-slice 生成可玩 NativeVN project、Windows/Web bundle 或 patch Windows direct-read，只能证明 demo slice 可玩，不能把完整 TsuiNoSora commercial gate 或 Stage 3 标为 `DONE`。
补充：formal release profile 的 `tsuinosora.manual_signoff.v1` 必须用 `check_id` 字段包含并通过 `manual.full_playthrough`、`manual.audio_listening`、`manual.visual_review` 和 `manual.alias_replacement`；任一 required check 缺失、未执行、失败或存在 blocker 都必须 blocking。
补充：TsuiNoSora package section release gate 必须统一阻断 `text`、`script_text`、`source_text`、`content`、`payload`、`payload_bytes`、`bytecode`、`bytes`、`commercial_text`、`lingo_source`、`raw_payload` 和 `source_payload` 等 payload-like 字段；唯一允许的 `payload` 键是 `redaction.payload: omitted`。
补充：NativeVN package input 写入 `PackageSections/*.json` 前必须清洗 package release gate 禁止的 payload-like 字段；工作区内原始 report 可保留脱敏 redaction 说明，但进入 package section 的 JSON 不能出现 `commercial_text`、`bytes`、`raw_payload` 等字段。
补充：`tsuinosora.asset_analysis` package section 即使 `status: pass`，也必须包含至少一条 analyzed asset evidence；空 `assets` 不能作为 Asset Analysis Gate 完成证据。
补充：`tsuinosora.conversion_manifest` package section 即使所有 routes 都是 `covered`，也必须包含至少一条 converted resource evidence；空 `resources` 不能作为真实转换完成证据。每条 resource 必须包含 source/native 相对路径、classification、source hash、converted hash 和正 byte size，缺字段或 hash 非 `sha256:` 都必须 blocking。
Standalone bundle 只能记录相对路径、section hash、entrypoint、sanitized launch report 和 sanitized route report。Windows/Web bundle 验收必须从已 cook/package 的 `.astrapkg` 构建，并重新通过 player route scenario；不得把存在文件当成可玩证据。Windows bundle 的 route evidence 必须由 bundle 内的 `AstraPlayer.exe` 读取 `AstraPlayer.config.json`、package 和 `scenario.refs` 后输出 `astra.player_route_report.v1`，不能用外部 headless CLI 报告冒充 player host 证据。
TsuiNoSora standalone bundle 还必须把脱敏 `tsuinosora.mount_policy` section 派生成 bundle 内相对路径 `AstraPlayer.mount_policy.json`。Windows/Web player route report 必须校验 `player.mount_policy` 和 `player.mount_policy_hash`，后者要用 bundle manifest 中登记的 path、role、hash 和 byte size 证明 player host 读取到的是未被篡改的 mount policy 文件；`tsuinosora-patch-game` 还必须校验 `player.patch_direct_read`，证明 player host 读取了 bundle mount policy，并且 scenario `mount_aliases` 与 policy aliases/hash_policy/fallback 一致。Windows player 的 patch direct-read 必须有本地读取证据：scenario 用 `mount_probes` 声明 alias、相对 path 和 `sha256`，或用 `mount_assets` 声明 alias、相对 path、role、route id 和 `sha256`，其中 role 必须是 Asset analysis 允许分类且不能是 `unknown` 或 `script`，再通过 `AstraPlayer.exe --route-scenario ... --mount-root alias=path` 读取本地合法数据根；route report 只能记录 `player.patch_mount_probe`、`player.patch_mount_asset` 和状态，不得记录本地 root。Web player 遇到包含本地 `mount_probes` 或 `mount_assets` 的 patch scenario 必须 blocking。不得把 package section 存在、静态文件存在或外部 headless report 当作 patch direct-read 证据。
`tsuinosora.nativevn_package_input_report.v1` 必须为实际写出的 project、story、package section 和 scenario ref 记录 report-relative path、role、`sha256` 和 byte size；不得输出 story 正文、商业素材 payload 或本地路径。

## 6. 变更边界

- NativeVN 公开样例只保留紧凑技术验收内容。15–20 分钟、三终局、中英双语、中文全配音和正式原创资产属于 `Docs/migrations/nativevn-flagship-demo-migration.md`；该 migration 完成许可与产品验收前，不得提交 Windows SAPI/TTS 产物或把旗舰 Demo 标为 Stage 3 完成证据。

- 优先复用成熟库和已有模式，不为单一实现新增抽象。
- 任何新增 public contract 都要同时说明权限、诊断、migration、release gate 和最小测试。
- 旧 VN 兼容不能成为 NativeVN、Editor 或 EngineCore 达标前置条件。
- 不提交商业游戏 payload、未授权截图或可绕过访问控制的说明；测试报告和示例数据不得泄露私有绝对路径。
- 商业视觉参考只允许使用仓库中明确列为参考证据的文件。新的商业截图、文本、音频或影片只能写入 ignored 私有调试目录；可提交 report 只能写 hash、尺寸、区域 id、coverage、diagnostic 和 layout metric。
- Git 提交使用短祈使句，例如 `[docs] Rewrite product architecture`。
