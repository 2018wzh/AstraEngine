# Game Runtime Provider Migration

本计划只迁移已经存在的 AstraVN runtime facade、VN extension manifest、package sections 和 release checks，使它们对齐 [Game Runtime Provider Contract](../contracts/game-runtime-provider.md)。执行前先完成 [AstraVN Module Layout Migration](astra-vn-module-layout-migration.md) 和 [AstraVN Crate Split Migration](astra-vn-crate-split-migration.md)。AstraEMU/AstraRPG 代码尚未存在，不列为迁移对象。

## 迁移前实现入口

- `Engine/Source/Runtime/astra-vn/src/lib.rs`：迁移前 Rust ABI dylib facade 和兼容 re-export。
- `Engine/Source/Runtime/astra-vn/src/parser.rs`、`compiler.rs`、`types.rs`：`.astra` parser/compiler、CompiledStory、source map、debug symbol、route graph 和 VN state 类型。
- `Engine/Source/Runtime/astra-vn/src/runtime.rs`、`save_container.rs`、`system_ui.rs`：VN runtime cursor、choice/backlog/read-state/voice replay、wait state、system state 和 save section。
- `Engine/Source/Runtime/astra-vn/src/luau.rs`、`policy_bundle.rs`、`standard_policy.luau`：Luau policy host、policy state、policy bundle manifest 和 source cache。
- `Engine/Source/Runtime/astra-vn/src/presentation.rs`、`presentation_execution.rs`、`presentation_provider.rs`、`standard_commands.rs`：演出模型、headless execution、presentation provider 和标准命令。
- `Engine/Source/Runtime/astra-vn/src/package.rs`、`commercial_baseline.rs`、`advanced_presentation.rs`、`plugin_extensions.rs`、`editor_metadata.rs`：VN package section、release evidence、extension manifest 和 Editor metadata。
- `Engine/Plugins/Fixtures/vn-extension-provider`：VN extension provider slots。
- `Engine/Source/Developer/astra-release`：`vn.*`、`tsuinosora.*`、`player.full_playable` release checks。
- `Engine/Source/Programs/astra-cli`：NativeVN cook/package/bundle/test route flow。
- `Engine/Source/Runtime/astra-player-core` 和 `Engine/Source/Programs/astra-player`：live automation script/input transcript/report validation。

前置的 module layout 和 crate split 迁移完成后，目标实现入口变为 `Engine/Source/Modules/AstraVN/astra-vn-runtime-provider`，并组合 `astra-vn-script`、`astra-vn-core`、`astra-vn-policy`、`astra-vn-presentation`、`astra-vn-commands`、`astra-vn-system`、`astra-vn-save`、`astra-vn-package`、`astra-vn-plugin` 和 `astra-vn-editor`。`astra-plugin-abi` 提供 runtime provider DTO+FFI entrypoints；`astra-test`、`astra-package` 和 `astra-release` 通过 target-level `runtime_provider: native_vn`、`provider.policy` 和 `plugin.extension_registry` 选择并校验 provider。

## 目标设计

`NativeVnRuntimeProvider` 包装现有 VN 能力：

- `prepare`：编译 `.astra`、policy bundle、system stories、command manifest 和 presentation profile。
- `probe`：校验 target/profile、package sections、scenario refs、mount policy 和 player route model。
- `open`：创建 VN runtime cursor、policy state、presentation state 和 save section cursor。
- `step`：推进 dialogue、choice、system page、wait、presentation command、audio command 和 Luau policy effect。
- `save/restore`：读写 `vn.runtime_state` 和 `vn.policy_state`。
- `package_sections`：继续输出 `vn.*` package sections。
- `release_checks`：继续声明 `vn.commercial_baseline`、`vn.system_ui_profile`、`vn.advanced_presentation`、`player.full_playable` 等 check。

VN Core 继续持有 dialogue、choice、backlog、read-state、save/load 和 voice replay 权威语义。Provider 只是玩法 runtime selector，不改变 VN 语义。

## 分步迁移

1. 新增 `astra-vn-runtime-provider` crate。
   定义 `NativeVnRuntimeProvider` descriptor、session id、prepare/probe/open/step/save/restore/shutdown DTO，并组合 `astra-vn-script`、`astra-vn-core`、`astra-vn-policy`、`astra-vn-presentation`、`astra-vn-system`、`astra-vn-save`、`astra-vn-package`、`astra-vn-plugin` 和 `astra-vn-editor`。
2. Target manifest 显式绑定 `native_vn`。
   `nativevn-game`、AdvancedVN 和 TsuiNoSora synthetic/demo slice target 都要写 runtime provider binding；缺 binding 时 release gate blocking。
3. 迁移 VN extension binding。
   现有 Luau policy bundle provider、VN command provider、presentation command provider、Graph/Timeline metadata provider 和 release check provider 挂到 `NativeVnRuntimeProvider` selection。
4. 迁移 package sections。
   `vn.compiled_story`、`vn.profile_manifest`、`vn.policy_bundle_manifest`、`vn.extension_manifest`、`vn.standard_command_manifest`、`vn.presentation_provider_manifest`、`vn.commercial_baseline_manifest`、`vn.system_story_manifest`、`vn.system_ui_profile_manifest` 和 `vn.advanced_presentation_manifest` 保持 schema，增加 runtime provider evidence。
5. 迁移 scenario runner。
   `astra test run` 通过 selected gameplay runtime provider 推进 VN action/assertion；未知 provider、未知 action、缺 package 或 target mismatch 继续 blocking。
6. 迁移 release gate。
   增加 `runtime_provider.binding` 和 `runtime_provider.native_vn` check，校验 target binding、`provider.policy`、plugin registry、provider descriptor、package section continuity、release check declaration、save/load/replay hash 和 player report profile/target match。
7. 保持 facade 兼容。
   `astra-vn` 只作为 Rust ABI dylib facade 和兼容 re-export；跨插件稳定边界仍是 `.astra`、package section 和 Stage 1 plugin ABI。功能 crate 不得依赖 `astra-vn` facade。

## 验收命令

```bash
python Tools/check_docs.py
cargo test -p astra-vn --test vn_dylib_facade
cargo test -p astra-vn-plugin --test vn_plugin_extensions
cargo test -p astra-vn-save --test vn_save_container
cargo test -p astra-plugin-abi runtime_provider_abi
cargo test -p astra-plugin runtime_provider_registry
cargo test -p astra-vn-runtime-provider --test game_runtime_provider
cargo test -p astra-vn-runtime-provider --test runtime_provider_ffi
cargo test -p astra-test --test vn_scenario
cargo test -p astra-release --test release_report runtime_provider
cargo test -p astra-cli --test target_platform nativevn_sample_cooks_packages_validates_and_runs_full_playthrough
```

## 不得修改项

- 不把 AstraVN Core 抽成 AstraEMU/AstraRPG 的基类。
- 不改变 `.astra` canonical story source、source map、debug symbol 和 package section schema 的兼容口径。
- 不把 Luau VM handle、renderer/audio native handle、Actor 指针、Editor widget、本地 root 或商业 payload 放入 provider DTO。
- 不用 direct VN command、DOM click、JS callback 或 route report 冒充 `player.full_playable`。
