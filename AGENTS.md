# AstraEngine 实施宪章

## 1. 仓库定位

AstraEngine 仓库是 AstraEngine 系列的产品总入口，负责维护跨仓架构、共享契约、状态矩阵、验收口径和代理实施规则。系列仓库按 lockstep release 协同：

| 仓库 | 职责 |
| --- | --- |
| AstraEngine | EngineCore、Runtime、Asset、Media、Script、插件 ABI、公共测试框架和跨仓契约 |
| AstraVN | `.astra` canonical story、VN preset、商业 VN 基线系统、Lua 扩展和发布样例 |
| AstraEditor | Qt/QML creator editor、PIE、Inspector、Graph/Timeline、Package/Release Gate UI |
| AstraEMU | 旧 VN manager、独立进程 compat core、family adapter、Lua patch/decode runtime |
| AstraPlatform | 桌面、移动、Web、实验旧主机平台壳和原生能力适配 |

实现时先更新本仓共享契约，再改子仓。不能让子仓私有设计反向污染 EngineCore 边界。

## 2. 架构硬约束

- Runtime 权威模型是 Actor/Component + StateMachine；局部 ECS 只用于可证明的热点批处理，不能作为 creator-facing 对象模型。
- EngineCore 不依赖 Editor UI、MCP server、AI provider、Lua runtime、legacy VM、平台 GPU/audio handle 或具体 renderer/audio backend。
- Runtime 可使用 Tokio，但 deterministic state 不直接依赖 task completion order。任何可挂起 action 必须落成可序列化 `AwaitToken`，结果在固定 tick 边界进入有序事件队列。
- 插件采用 Rust-facing `abi_stable` 风格 ABI。插件可以加载和卸载，不支持热重载。插件 binary 必须匹配 engine version、rustc fingerprint、feature fingerprint 和 provider descriptor。
- Provider 只能通过 ServiceRegistry、ExtensionRegistry、EngineModuleSlot 暴露能力。不能跨 ABI 传递对象所有权、Actor 指针、Editor widget、GPU/audio native handle。
- `.astra` 是 AstraVN canonical story source。Graph、Timeline 和 Editor layout 只能保存作者元数据，必须能往返到同一 IR、source map 和 debug symbol。
- AstraVN Core 持有 dialogue、choice、backlog、save/load、read-state、voice replay 等权威语义；Rust 插件提供机制，Lua policy 提供表现、系统页和复杂演出策略。
- Lua 5.4 通过 `mlua` 进入 AstraVN/AstraEMU。默认 capability sandbox，无文件、网络或系统调用；EMU 只提供 patch/decode runtime 和 API，不负责绕过 DRM、商业保护或访问控制。
- Save 和 package 是自描述二进制容器，section payload 使用 `postcard`/serde。外部 YAML descriptor 只作为 text-first source，Cook 后不得成为 runtime 必需文件。
- Renderer2D 后端可替换，wgpu 是默认 provider。平台解码优先，桌面可通过 vcpkg 接 FFmpeg fallback。视觉 FilterGraph 和 AudioGraph 分离。
- Runtime AI 与 Editor AI 同等重要。联网 Runtime AI 可发布，但输出通过 IntentValidator 后必须固化进 save/replay，回放不重新请求 provider。
- AstraEMU compat core 是独立进程，core 持有旧引擎权威状态机，Manager 通过本地 RPC 和 shared memory 接收 trace、media block、TextCaptureEvent 和 snapshot。

## 3. 文档规则

- 中文主体，API、type、crate、command 和文件名保留英文。
- 文档结构从产品到实现：`docs/product`、`docs/contracts`、`docs/modules`、`docs/platforms`、`docs/status`、`docs/manual`、`docs/references`、`docs/adr`。
- 每个模块必须能从设计页走到 contract、public API、data format、test scenario、release gate 和 manual link。
- 设计页只写目标和契约；当前实现状态放在 `docs/status`。
- 修改页面结构时，同步更新最近的 README 或索引。
- 中文技术文档按 `humanizer-zh` 处理：去掉翻译腔、堆砌列表和空泛结尾，事实和实现状态不得拔高。
- 不写营销文案，不把 planned work 写成 implemented behavior。

## 4. Rust 与脚本风格

- Rust 采用 idiomatic Rust：`snake_case` 函数和变量，`PascalCase` 类型，`SCREAMING_SNAKE_CASE` 常量。
- 必须运行 `rustfmt` 和 `clippy`；公共 API 变更需要对应 contract 和 migration 说明。
- derive 宏可以生成 PropertySystem、serde、schema、Inspector、save/replay、MCP patch glue 和注册样板。宏必须支持 `cargo expand` 调试路径，不得生成隐藏继承、全局对象系统或不可见生命周期。
- 跨平台脚本使用 Python，不使用 PowerShell 编写项目脚本。
- YAML descriptor 和 scenario 必须配 JSON Schema 或 Rust schema 类型。

## 5. 测试与验收

提交前至少执行：

```bash
python tools/check_docs.py
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

当前仓库尚未实现 Rust workspace 时，文档改动至少执行：

```bash
python tools/check_docs.py
```

该脚本同时检查文档断链、状态页覆盖矩阵和历史标记残留。

无头玩家测试使用 YAML scenario。测试以完整玩家流程断言为主，state/event/presentation hash、截图和音频采样作为定位证据。Release Gate 必须输出 machine-readable report。

## 6. 变更边界

- 优先复用成熟库和已有模式，不为单一实现新增抽象。
- 任何新增 public contract 都要同时说明权限、诊断、migration、release gate 和最小测试。
- 旧 VN 兼容不能成为 NativeVN、Editor 或 EngineCore 达标前置条件。
- 不提交商业游戏 payload、未授权截图或可绕过访问控制的说明；测试报告和示例数据不得泄露私有绝对路径。
- Git 提交使用短祈使句，例如 `[docs] Rewrite product architecture`。
