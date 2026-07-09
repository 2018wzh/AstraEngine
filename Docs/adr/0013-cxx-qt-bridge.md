# ADR 0013: AstraEditor 使用 cxx-qt 作为 Rust↔Qt Bridge

## Status

Accepted.

## Context

ADR 0002 已决定 AstraEditor 使用 Qt/QML shell + Rust core bridge，但未指定 Rust↔Qt 的具体绑定实现。Stage 4 开工前需要明确：

1. Rust 侧如何声明 `QObject` 子类并与 QML 交互（signal、slot、property）。
2. Bridge 层如何向 QML 暴露 `EditorRuntimeBridge` 的运行时数据。
3. `cxx`、`ritual`、`cxx-qt` 等方案的取舍。

候选方案：

| 方案 | 机制 | QML 原生支持 | Qt 6 支持 | 维护状态 |
| --- | --- | --- | --- | --- |
| **cxx-qt** | `cxx` 宏生成 Rust↔C++ 胶水，`#[qml_element]` 声明 QObject | 最好（原生 signal/slot/property）| Qt 6.5+ 完整支持 | KDAB 活跃维护 |
| ritual | 从 Qt 头文件自动生成 unsafe Rust 绑定 | 低层次，需手动封装 | Qt 6 支持极弱 | 不活跃 |
| 手写 C ABI | extern "C" 函数表 | 需要手动写 QObject 胶水 | 不限 | 完全自维护 |

## Decision

AstraEditor v1 使用 **cxx-qt**（KDAB 维护）作为 Rust↔Qt Bridge 实现。

核心设计：

- Bridge 侧用 `#[cxx_qt::bridge]` 模块声明 `EditorBridgeObject`：一个 `QObject` 子类，暴露给 QML 的 property、signal 和 slot 均在此模块内通过宏生成。
- Rust 侧的 `EditorRuntimeBridge` trait impl（`AstraEditorBridge`）通过 `UniquePtr<EditorBridgeQObject>` 在 cxx-qt bridge 模块内调用。
- QML 数据绑定分三类，按场景选型：
  - 列表/树型数据（Content Browser、Plugin Manager）→ `QAbstractListModel`
  - 单对象状态（Project Settings、PIE 状态、active provider）→ `Q_PROPERTY` + `changed` 信号
  - 复杂报告（Release Gate report、AI audit log）→ JSON string property + QML JS 解析
- Qt 版本：Qt 6.5 LTS；Stage 6 根据需要评估升级到 Qt 6.8+。
- Editor crate（`astra-editor-bridge`、`astra-editor`）使用独立 CI job，不纳入默认 `cargo test --workspace`，因为 Qt 不是所有 CI 环境的标准依赖。

构建入口：

```toml
# Editor/Source/Bridge/astra-editor-bridge/Cargo.toml
[dependencies]
cxx-qt = "0.7"
cxx-qt-lib = "0.7"
astra-runtime = { path = "../../../../Engine/Source/Runtime/astra-runtime" }
astra-vn-editor = { path = "../../../../Engine/Source/Modules/AstraVN/astra-vn-editor" }
astra-release   = { path = "../../../../Engine/Source/Developer/astra-release" }

[build-dependencies]
cxx-qt-build = "0.7"
```

```rust
// Editor/Source/Bridge/astra-editor-bridge/src/lib.rs
#[cxx_qt::bridge(namespace = "astra::editor")]
pub mod editor_bridge {
    extern "RustQt" {
        #[qobject]
        #[qproperty(bool, is_project_open)]
        #[qproperty(QString, active_provider_id)]
        type EditorBridgeObject = super::EditorBridgeObjectRust;
    }
    // signals
    unsafe extern "RustQt" {
        #[qsignal]
        fn project_opened(self: Pin<&mut EditorBridgeObject>, report: QString);
        #[qsignal]
        fn compile_result(self: Pin<&mut EditorBridgeObject>, report: QString);
        #[qsignal]
        fn extension_list_changed(self: Pin<&mut EditorBridgeObject>);
        #[qsignal]
        fn pie_state_changed(self: Pin<&mut EditorBridgeObject>, state: QString);
    }
    // slots (QML 调用)
    impl cxx_qt::Threading for EditorBridgeObject {}
    extern "RustQt" {
        #[qinvokable]
        fn open_project(self: Pin<&mut EditorBridgeObject>, path: &QString);
        #[qinvokable]
        fn compile_story(self: Pin<&mut EditorBridgeObject>);
        #[qinvokable]
        fn start_pie(self: Pin<&mut EditorBridgeObject>, game_target: &QString, provider: &QString, profile: &QString);
        #[qinvokable]
        fn stop_pie(self: Pin<&mut EditorBridgeObject>);
        #[qinvokable]
        fn validate_package(self: Pin<&mut EditorBridgeObject>, profile: &QString);
    }
}
```

## Consequences

- cxx-qt 要求 Qt 6.5 LTS 安装在系统上（`Qt6_DIR` 或 `QTDIR` 环境变量），并在 `build.rs` 中通过 `cxx_qt_build::CxxQtBuilder` 触发 moc + C++ 编译。
- Editor CI 需要独立 job（安装 Qt 6.5 LTS，可用 `aqtinstall` 自动化）；普通 `cargo test --workspace` 不需要 Qt。
- packaged runtime（`astra-player`、`astra-cli`）不依赖 cxx-qt 或 Qt；Editor target（`kind = editor, packaged = false`）是唯一依赖 Qt 的 binary。
- 后续如果 cxx-qt 版本变动，需要在本 ADR 中记录 migration 说明，并同步 `Docs/manual/editor-dev-setup.md`。

## Verification

```bash
# Editor 独立 CI job
cargo build -p astra-editor-bridge
cargo test  -p astra-editor-bridge project_wizard
```
