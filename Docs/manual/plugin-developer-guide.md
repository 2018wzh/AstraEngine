# Plugin Developer Guide

插件通过 Rust-facing `abi_stable` 风格 ABI 接入。插件可以提供 renderer、text layout、audio、decode、script runtime、presentation library、asset importer、cook processor、editor panel、MCP tool 或 AI provider。

## 插件最小文件

```text
plugin.yaml
src/lib.rs
tests/load_unload.rs
manual.md
```

`plugin.yaml` 必须声明 id、version、engine version、rustc fingerprint、feature fingerprint、capability、permission 和 packaged eligibility。

## 禁止项

插件不得跨 ABI 保存 host object ownership、Actor 指针、GPU/audio native handle、Editor widget 或 unload 后 callback。需要 runtime state 时，注册 save section 和 migrator。
