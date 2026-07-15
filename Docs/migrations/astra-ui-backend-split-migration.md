# Migration 12：AstraVN Yakui UI 与 Script-declared UI 迁移

本页记录从旧固定 UI 命中路径迁到生产级 AstraVN UI 的实施顺序。架构真源见 [UI Contract](../contracts/ui.md)、[UI Component Plugin Contract](../contracts/ui-component-plugin.md)、[UI Backend 实施规格](../implementation/ui-backend.md) 与 ADR 0014–0017。

Migration 12 当前状态为 `IN_PROGRESS`。backend-neutral contract、Yakui adapter、Script UI、AstraText、Scene2D Mesh、产品 ViewModel、Classic/Modern profile、签名 component host 与 Headless E2 主路径已经落地；最终 workspace gate、Windows/Web E3、性能与无障碍正式 evidence 尚未闭合。在这些 evidence 通过前不得标记 `DONE`。

## 迁移边界

本 migration 实现 AstraVN Yakui shipping backend、shared UI contract、`.astra` UI frontend、Luau Controller、Scene2D/Mesh2D 输出、component ABI、产品页、开发工具和 Windows/Web gate。

AstraEMU Manager/overlay 使用 egui 的决策写入 Stage 5 设计，但不在本 migration 创建 AstraEMU 产品代码。Editor 保持 Qt/QML；只更新 PIE 对 shared runtime UI contract 的依赖。iOS/Android 只关闭共享 touch contract 和 Headless semantic evidence，真实设备仍属 Stage 6。

## Step 0：工具链与依赖 preflight

1. 增加根 `rust-toolchain.toml` 的 `stable` channel，删除 workspace 和成员 crate 的 `rust-version`。
2. 扩展 isolated build identity，记录实际 rustc/Cargo、lockfile、target/profile/feature、commit/dirty state。
3. 在该工具链上选择最新兼容 Yakui core/widgets；验证 license、Windows、wasm32、依赖图、文本依赖和 API，再在同一提交精确锁定。
4. 明确阻断 `yakui-wgpu`、`yakui-winit`、`yakui-app`、第二份 wgpu/winit 和第二套正式文本 authority。
5. 锁定官方 `luau-analyze` 与 jco，并把 executable/package hash 写入 build identity。

**验证：** 新鲜 isolated Windows/wasm32 dependency build；identity mismatch、依赖重复、license 或 tool hash mismatch 必须失败。

## Step 1：`astra-ui-core`

新增 backend-neutral viewport、physical input、input disposition、semantic tree、action envelope、theme token、texture delta、mesh primitive、repaint 和 performance DTO。所有 collection、string、texture、vertex/index 都有显式上限；schema 由 Rust 生成。

**验证：** serde/schema roundtrip、invalid enum、duplicate id、index/clip/texture bounds、premultiplied alpha、oversized DTO 和 redaction tests。

## Step 2：Scene2D/Mesh2D 主路径

在当前 `PlayerHostCommand::PresentScene` 和 platform `HostCommand::PresentScene` 上增加 UI texture、clip 和 indexed Mesh2D command。Windows wgpu 与 WebGPU 分别实现相同 resource generation、transactional update、scissor、premultiplied blend、release 和 context restore。

AstraVN UI 不得发送 `PresentRgba`。保留该通用命令的其他调用者不能成为 VN fallback；VN 的旧 bitmap/headless presenter 不得恢复。

**验证：** resource collision、stale generation、failed upload rollback、clip/index bounds、resize、context loss/restore、readback 和同 Scene order。

## Step 3：Yakui adapter

新增 `astra-ui-yakui`，将 Astra input 转为 Yakui event，将 Yakui paint 转为 `UiRenderFrame`。实现 stable id bridge、focus scope、modal capture、gamepad navigation、IME、touch、repaint 和 full texture resync。上游不支持的必需能力在 preflight 阻断，不能静默丢弃 event。

**验证：** pointer/keyboard/gamepad、Windows/Web IME、Web touch、Consumed/Bubble、modal 不穿透、DPI、fixed clock、context restore 和无 Yakui public type leak。

## Step 4：AstraText 与虚拟集合

实现 `AstraText`/`AstraRichText`，以 Astra TextLayout 生成 measure、glyph plan 和 semantic bounds。完成横排多语言、font fallback、grapheme、CJK kinsoku、ruby、CJK vertical glyph、tate-chu-yoko 和 vertical ruby。实现 `VirtualList`、`VirtualGrid` 与 thumbnail LRU。

**验证：** `zh-Hans`/`ja`/`en` 横排、`zh-Hans`/`ja` 竖排、font asset/hash、wrap/clip/ellipsis、10,000 Backlog、1,000 Gallery、locale/font-scale 切换和 atlas restore。BiDi/RTL 不作为 Migration 12 closure evidence。

## Step 5：Script UI frontend

为 `.astra` source 增加 `Story`/`Ui` role。UI grammar 包含 `ui_view`、`ui_bind`、`ui_component`、widget property、repeat 和 `on event -> action`。binding 只允许 literal、`$model/$item/$event/$state`、text key、asset ref 和 theme token。

实现 `UiSymbols -> UiWidgets -> UiModels -> UiActions -> UiBindings -> UiCapabilities -> UiBundle`，并保持 token-level source map、formatter 和 language-service roundtrip。

**验证：** typed path、unknown widget/action、event argument、duplicate stable id、unsafe binding root、localization/asset/theme missing、capability mismatch、format idempotence 和 diagnostic span。

## Step 6：直接迁移 `CompiledVnProject`

新增 `compile_astra_project(...) -> CompiledVnProject` 并迁完所有 workspace caller。Package 写入 `vn.compiled_project` root 及独立 story/UI/binding/source-map/controller/theme/backend/component section。Target 直接迁到 `astra.target_manifest.v2`，带 UI 的 target 必须声明唯一 `ui_provider`。

所有 caller 和 fixtures 完成迁移后，删除公开 `compile_astra_sources`、`compile_astra_sources_with_options`、旧 `vn.compiled_story` reader、target v1 reader和兼容测试。旧 package 必须 recook，不做 runtime migration。

**验证：** root/child hash、duplicate/missing section、provider/profile mismatch、old package/target rejection、全 workspace compile caller 和 bundle recook。

## Step 7：`astra-vn-ui` 权威桥接

实现 Message、Choice、Title、Config、Save/Load、Backlog、Gallery、Replay、Voice Replay、Route Chart、Localization Preview 和 text-input fixture 的 Rust ViewModel。实现 command-specific、system-page、surface、profile 四级唯一 binding resolver，以及 action router。

UI action 只产生 request。save/load 使用既有 host transaction；unlock、route jump、replay、config 和 return flow 由 Core/host 校验。

**验证：** stable option/slot/command/item/node id、locked item、invalid config、save overwrite/abort/load migration、route jump denial、return savepoint 和 UI 输入不触发 advance。

## Step 8：Luau Controller

新增 `astra.ui.controller.register` 与可序列化 effect：forward、open/close modal、focus、set session state、animation preset 和 redacted trace。生成 `.d.luau`，以锁定的 `luau-analyze` 全量 typecheck。state 只允许 `none/session`，不进入 save。

**验证：** unknown action/effect、type error、budget、forbidden authority API、snapshot rejection、load/profile/locale generation rebuild 和 effect deterministic hash。

## Step 9：Theme 与 Classic/Modern profile

实现 backend-neutral font/color/metric/texture/nine-slice/focus/motion/accessibility token，完成 Classic 与 Modern 两个正式 profile。两者共享同一 Core state、save/replay 和 action router；profile 切换只更新 binding/theme/presentation generation。

**验证：** missing/duplicate token、asset hash、locale/font scale/high contrast、profile switch Core hash 不变、无 backend default skin fallback。

## Step 10：产品页闭环

按以下顺序接入同一主路径：Message/Choice；Title/Config/Modal；Save/Load；Backlog/Voice Replay；Gallery/Replay；Route Chart；Localization Preview；text-input fixture。正式产品页不得依赖 plugin fixture。

**验证：** 每页 physical input -> semantic target -> action -> authority result -> Scene2D capture；Backlog/Gallery 必须证明虚拟化和 LRU。

## Step 11：UI component ABI

新增 `astra-ui-plugin-abi` 与 host adapter。实现 typed slot、bounded lifecycle DTO、session state、签名和 trust manifest。Windows 使用签名 dylib fixture；Web 从同一 Rust schema 生成 WIT，用锁定 jco 生成并验证 ES module/core wasm。

固定上限为 depth 32、4096 nodes/view、1024 instances/view、4 MiB/DTO、1 MiB state、256 effects/call、64 MiB Web memory；release profile声明 time/fuel。

**验证：** signer allowlist、fingerprint、mount/update/event/snapshot/restore/unmount、permission/user gesture、bounds、panic/trap/timeout、hash drift 和 session termination。不得生成替代组件或切换 provider。

## Step 12：开发工具与热更新

实现 `astra ui check/preview/snapshot/matrix`。Preview 使用真实 Blueprint/Yakui/AstraText/Scene2D，不以静态 JSON 或矩形截图冒充。Dev UI 编译失败立即终止 UI session、暂停 Core 并显示 host diagnostic overlay；成功 rebuild 创建新 generation。发布 runtime和 plugin binary不热更新。

**验证：** fixture schema、matrix identity、PNG/semantic/mesh/report hash、compile failure stop、Core state preserved、successful restart 和路径/payload redaction。

## Step 13：Headless、Windows 与 Web gate

Migration 11 完成的 Headless host 使用同一 cooked package、provider binding、physical input、UI semantic/render contract 生成 E2 preflight。Windows 和 Web 再以同 build/profile/package/provider/session 形成 E3。

Windows 覆盖 pointer/keyboard/gamepad、IME/clipboard fixture、context restore、signed dylib 和 hardware capture。Web 覆盖 pointer/keyboard/touch、IME/clipboard fixture、WebGPU、signed component output 和 browser capture。

**验证：** identity chain、Consumed/Bubble、semantic/render/capture hash、required checkpoint model review、performance artifact 和 automatic comparison。Headless 不能替代真实平台失败。

## Step 14：删除旧 UI 双轨

仅在 Steps 0–13 全部通过后删除 `SystemUiModel`、固定矩形 hit-test、旧 system UI tests/feature、旧 compile/package/target reader 及所有兼容入口。更新 Player、sample、Cook、bundle、Editor PIE 和 docs，使它们只认新 project/UI contract。

**验证：** `rg` 不再发现产品 `SystemUiModel`/旧 compile API/旧 package reader/target v1；全 workspace tests 和 Windows/Web release scenario 通过。不能用 deprecated feature 保留旧主路径。

## 阻断式性能门槛

formal run 必须同时满足：update+layout p95 <= 2.0 ms、paint conversion p95 <= 1.0 ms、stable frame texture update = 0 bytes、draw calls <= 128、vertices <= 250,000、active UI texture <= 64 MiB、10,000 Backlog instantiated <= 64 + overscan。任何超限阻断发布。

## 完成定义

Migration 12 只有在以下条件全部满足后才可标记 `DONE`：

1. `S2-UI-BACKEND-01`、`S3-UI-SCRIPT-01`、`S3-UI-EXT-01` 和扩展后的 `S3-SYSTEM-01` 有实现与对应 evidence。
2. Classic/Modern 全部产品页通过 Headless E2、Windows E3、Web E3；iOS/Android 不被错误计入。
3. signed Windows/Web component fixture 通过，但正式页不依赖 fixture。
4. 旧 UI/compiler/package/target 双轨已删除。
5. 全部 budget、privacy、accessibility semantic、context restore 和 model review gate 通过。
6. `python Tools/check_docs.py`、`cargo fmt --check`、workspace clippy/test 和正式 release scenario 在新鲜 build identity 下通过。

在此之前，相关状态保持 `SPEC_READY` 或 `IN_PROGRESS`，不得用 contract、fixture、Headless capture 或单页 preview 声明产品 UI 完成。
