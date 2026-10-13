# Plugin Developer Guide

机制插件通过 `abi_stable` 注册和加载，支持范围以实际 Rust ABI、registry 与消费者为准，不以规划 trait 列表代替实现。Gameplay/VN UI 使用编译期组合；Editor ACP/MCP 和 AstraEMU Family 分别使用独立接口。

## 插件文件

```text
plugin.yaml
src/lib.rs
policy.luau
tests/load_unload.rs
manual.md
```

`plugin.yaml` 必须声明 id、version、engine version、rustc fingerprint、feature fingerprint、capability、permission、load phase、extension point、dependency graph、enablement scope 和 packaged eligibility。Luau policy bundle 还要声明命令 schema、Editor metadata、hook、mutation scope、save migrator、performance budget 和依赖锁定策略。

## 禁止项

插件不得跨 ABI 保存 host object ownership、Actor 指针、GPU/audio native handle、Editor widget 或 unload 后 callback。需要 runtime state 时，注册 save section 和 migrator。

插件只发 `tracing` span/event，不安装 subscriber 或自建日志文件。事件使用稳定 plugin/provider category 和 `event` 字段：load/unload、provider register/session lifecycle 用 `INFO`，选择和映射用 `DEBUG`，高频调用用 `TRACE`，允许继续的明确 fallback 用 `WARN`，最终拒绝加载或 ABI/provider 操作失败由 host 边界记录一次 `ERROR`。字段只能包含 plugin/provider/action id、fingerprint hash、状态、计数和 diagnostic code；不得记录 payload、secret、native handle、绝对路径或完整 descriptor `Debug` 输出。

复杂演出插件采用 Rust 机制、Luau 策略。Rust 侧提供高性能 node/provider/host API；Luau 侧决定时序、参数、fallback、预设和可视化元数据。

## Extension Reference

descriptor、permission、load/unload、provider 注册与报告见 [插件 ABI](../contracts/plugin-abi.md) 和 [Provider And Plugin API](../implementation/provider-plugin-api.md)。没有消费者的 AI provider、动态 gameplay/UI slot 和旧 LegacyFamilyPlugin 不属于现行扩展入口。

游戏 UI 使用编译期组合的 Yakui；作品通过 `.astra` View、Binding 和 typed Luau Controller 定义界面。详见 [UI Contract](../contracts/ui.md)。

Editor 使用 ACP 外部 Agent 与 MCP 批量编辑，不通过机制插件绑定模型。AstraEMU Family 的原生生命周期、窗口请求和日志桥见 [EMU 接口](../contracts/astraemu-ipc.md)，不能使用本页的 Engine registry 代替。

## 验收命令

```bash
cargo test -p astra-plugin descriptor_gate
cargo test -p astra-plugin load_unload
cargo test -p astra-plugin extension_registry
cargo test -p astra-release --test release_report
```
