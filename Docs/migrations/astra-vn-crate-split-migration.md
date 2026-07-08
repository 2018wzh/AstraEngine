# AstraVN Crate Split Migration

本计划把已经存在的单 crate `astra-vn` 拆成多个功能 crate，并让 `astra-vn` 只保留 facade、`rlib`/Rust ABI `dylib` 和兼容 re-export。逻辑拆分在 [AstraVN Module Layout Migration](astra-vn-module-layout-migration.md) 完成后执行；本页用当前路径标识待拆文件，前置搬迁后对应到 `Engine/Source/Modules/AstraVN/astra-vn/src/...`。

## 现有实现入口

- `Engine/Source/Runtime/astra-vn/src/types.rs`：`AstraSource`、`CompiledStory`、route graph、VN runtime state、backlog、wait、player command、save blob 和 profile manifest。
- `Engine/Source/Runtime/astra-vn/src/parser.rs`、`compiler.rs`：`.astra` parser、compiler、diagnostic 和 source map 相关逻辑。
- `Engine/Source/Runtime/astra-vn/src/runtime.rs`：VN cursor、step、save slot、system controls 和 presentation/audio command emission。
- `Engine/Source/Runtime/astra-vn/src/luau.rs`、`policy_bundle.rs`、`standard_policy.luau`：Luau sandbox、policy state、mutation/query/trace 和 policy bundle gate。
- `Engine/Source/Runtime/astra-vn/src/presentation.rs`、`presentation_execution.rs`、`presentation_provider.rs`、`standard_commands.rs`：演出模型、headless execution、presentation provider 和标准命令。
- `Engine/Source/Runtime/astra-vn/src/system_ui.rs`、`save_container.rs`、`package.rs`、`commercial_baseline.rs`、`advanced_presentation.rs`：系统 UI、save/package section 和 VN release evidence。
- `Engine/Source/Runtime/astra-vn/src/plugin_extensions.rs`、`editor_metadata.rs`：VN extension slots 和 Graph/Timeline metadata。
- `Engine/Source/Runtime/astra-vn/src/lib.rs`：当前 monolithic facade。

## 目标 crate graph

目标目录统一位于 `Engine/Source/Modules/AstraVN/`：

| Crate | 职责 |
| --- | --- |
| `astra-vn-script` | `.astra` source、parser、compiler、`CompiledStory`、source map、debug symbol、route graph、story/variable/command manifest |
| `astra-vn-presentation` | StageModel、layer/camera/video/audio/timeline/fallback、headless presentation execution、presentation provider manifest |
| `astra-vn-core` | `VnRuntime`、VN command cursor、choice、call/return、backlog、read-state、voice replay、wait state、system state、replay UI |
| `astra-vn-policy` | Luau sandbox、policy state、mutation/query/trace、policy bundle manifest、source cache、`standard_policy.luau` |
| `astra-vn-commands` | standard command library、command schema、usage validation、command manifest |
| `astra-vn-system` | system stories、save/config/backlog/gallery/replay/route chart/localization profile |
| `astra-vn-save` | `vn.runtime_state`、`vn.policy_state` save sections、save blob、hash、migration glue |
| `astra-vn-package` | `vn.*` package section plans、profile manifest、commercial baseline、advanced presentation manifest、package evidence |
| `astra-vn-plugin` | VN extension points、extension manifest、provider slot ids |
| `astra-vn-editor` | Graph/Timeline authoring metadata、source round-trip metadata、NativeVN `RuntimeEditorMetadata` |
| `astra-vn-runtime-provider` | `NativeVnRuntimeProvider` composition，负责 prepare/probe/open/step/save/restore/package/release/editor metadata |
| `astra-vn` | facade crate，只 re-export 上述 crate 并保留 `rlib`/Rust ABI `dylib` |

依赖方向固定为：`astra-vn` facade 依赖各功能 crate；功能 crate 不依赖 `astra-vn` facade。需要共享类型时，向更底层 crate 下沉，不能通过 facade 回引。

## 分步迁移

1. 建立 sibling crate skeleton。
   在 `Engine/Source/Modules/AstraVN/` 下新增上述功能 crate，每个 crate 只配置实际需要的 workspace dependency。先让它们编译通过，不搬业务代码。
2. 拆出 `astra-vn-script`。
   迁移 `AstraSource`、`CompiledStory`、story/variable/command manifest、route graph、parser 和 compiler。`compiler_runtime`、`compiler_diagnostics` 先挂到该 crate；facade re-export 保持 `astra_vn::*` 兼容。
3. 拆出 `astra-vn-presentation`。
   迁移 StageModel、AudioCommand、PresentationTimeline、Timeline task、presentation provider manifest 和 headless execution。`presentation_model`、`presentation_execution`、`await_gates` 中的演出断言迁到该 crate。
4. 拆出 `astra-vn-core`。
   迁移 `VnRuntime`、runtime state、choice/backlog/read-state/voice replay/wait/system state 和 player command。该 crate依赖 `astra-vn-script` 与 `astra-vn-presentation`，但不依赖 Luau policy。
5. 拆出 `astra-vn-policy`。
   迁移 Luau sandbox、policy state、mutation/query/trace、policy bundle manifest、source cache 和 `standard_policy.luau`。该 crate 只通过 serde DTO 与 Core 交互，不暴露 Luau VM handle。
6. 拆出 `astra-vn-commands`。
   迁移 standard command manifest、descriptor、usage validation 和 command schema。它可以依赖 script/presentation DTO，不能依赖 runtime provider。
7. 拆出 `astra-vn-system`。
   迁移 SystemStoryManifest、system UI profile、save migration policy、unlock source 和 localization coverage。
8. 拆出 `astra-vn-save`。
   迁移 `VnRuntimeStateSave`、`VnPolicyStateSave`、save blob hash 和 postcard glue。它只保存可序列化 state，不保存 Luau function/thread/userdata。
9. 拆出 `astra-vn-package`。
   迁移 profile manifest、commercial baseline、advanced presentation、package section plan 和 VN package evidence。release gate 仍在 `astra-release`，这里只提供 schema/evidence DTO。
10. 拆出 `astra-vn-plugin`。
    迁移 VN extension manifest、required extension ids 和 provider slot helpers。真实 plugin loading 仍属于 Stage 1 `astra-plugin`。
11. 拆出 `astra-vn-editor`。
    迁移 Graph/Timeline authoring metadata、patch manifest 和 NativeVN `RuntimeEditorMetadata`。该 crate 不依赖 Qt/QML，也不能传递 Editor widget。
12. 新增 `astra-vn-runtime-provider`。
    组合 script/core/policy/presentation/commands/system/save/package/plugin/editor crate，实现 `NativeVnRuntimeProvider` 的 prepare/probe/open/step/save/restore/package/release/editor metadata。
13. 收缩 `astra-vn` facade。
    `astra-vn/src/lib.rs` 只保留 `pub use astra_vn_*::*;`、facade 文档和 dylib smoke。删除业务实现模块，确认 feature crate 没有依赖 `astra-vn`。
14. 下游逐步改用窄依赖。
    `astra-test`、`astra-release`、`astra-cli` 可以先继续依赖 facade；后续按职责改为依赖 `astra-vn-script`、`astra-vn-core`、`astra-vn-package` 或 `astra-vn-runtime-provider`。

## 验收命令

```bash
python Tools/check_docs.py
cargo metadata --no-deps
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p astra-vn-script
cargo test -p astra-vn-core
cargo test -p astra-vn-policy
cargo test -p astra-vn-presentation
cargo test -p astra-vn-commands
cargo test -p astra-vn-system
cargo test -p astra-vn-save
cargo test -p astra-vn-package
cargo test -p astra-vn-plugin
cargo test -p astra-vn-editor
cargo test -p astra-vn-runtime-provider
cargo test -p astra-vn --test vn_dylib_facade
cargo test -p astra-vn --test vn_plugin_extensions
cargo test -p astra-test --test vn_scenario
cargo test -p astra-release --test release_report release_gate_
cargo test -p astra-cli --test target_platform nativevn_sample_cooks_packages_validates_and_runs_full_playthrough
```

## 不得修改项

- 不改变 `.astra` canonical story source、source map、debug symbol、VN package section schema、release check id 或 public sample behavior。
- 不让功能 crate 依赖 `astra-vn` facade。
- 不把 heavy decode/text/media provider 依赖拖入 `astra-vn` facade。
- 不把 Luau VM handle、renderer/audio native handle、Actor 指针、Editor widget、本地 root、商业 payload 或未脱敏脚本文本写入 public DTO、save、package 或 report。
- 不把拆分本身标为玩法 runtime 已完成；`NativeVnRuntimeProvider` 仍由 `S3-RUNTIME-PROVIDER-01` 单独验收。
