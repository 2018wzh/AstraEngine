# Coding Style

## 1. 技术基线

- C++23。
- 优先使用标准库：`std::string`、`std::vector`、`std::span`、`std::optional`、`std::variant`、`std::expected`、`std::filesystem`、`std::chrono`。
- 不重造基础容器、路径、时间或错误容器，除非跨 ABI 边界需要 POD descriptor。

## 2. 架构边界

- Core 不依赖 VN、AI、Live2D、legacy VM、Editor、Renderer backend。
- Runtime 不依赖 Editor。
- 插件跨 ABI 不传递 STL ownership、C++ Actor 指针、native handle 或 Editor widget。
- Public API 使用 Astra 自有 DTO、opaque handle、stable ID。

## 3. 命名

- 类型使用 `PascalCase`。
- 函数和变量使用 `camelCase`。
- 常量使用 `kPascalCase`。
- 文件名与主要类型保持一致。
- 稳定 ID 使用小写 dotted 或 URI-like scheme，例如 `astra.vn.character`、`native:/Characters/Alice`。

## 4. 错误与诊断

- 可恢复错误返回 `std::expected` 或等价 Result。
- 跨 ABI 错误返回 result code，并写 diagnostics sink。
- 不跨 ABI 抛异常。

## 5. 文档

设计文档描述目标态。Development 文档也按目标态书写；若实现尚未达到目标，在代码 README 或 issue 中说明差距，不在目标文档中把旧实现作为中心。
