# Coding Style

## 1. Style Identity

AstraEngine 的实现风格是 **modern C++23 / std-first engine code**，不是 UE C++ clone。

`UE-class 2D runtime` 在 Astra 文档中表示工程完备度目标：可验证、可发布、可调试、可扩展、可诊断、可保存回放。它不表示复制 UE 的表层 C++ 风格、宏系统、容器体系或 `UObject` runtime。

优先级：

1. 使用 C++23 标准库和清晰的 RAII ownership。
2. 在 public API 使用 Astra 自有 DTO、stable ID、opaque handle 和 schema-friendly descriptors。
3. 在动态模块和跨语言边界使用 C ABI / POD descriptor。
4. 借鉴大型引擎的生命周期、模块化、诊断、反射元数据和 release gate 思路，但以 Astra 自己的轻量系统实现。

## 2. Technical Baseline

- C++23。
- 优先使用标准库：`std::string`、`std::vector`、`std::span`、`std::optional`、`std::variant`、`std::expected` 或项目等价 `Result`、`std::filesystem`、`std::chrono`。
- 默认使用 RAII：`std::unique_ptr` 表示独占 ownership，`std::shared_ptr` 只在共享生命周期无法避免且有明确 owner graph 时使用。
- 不重造基础容器、字符串、路径、时间、错误容器或智能指针，除非跨 ABI 边界需要 POD descriptor、opaque handle 或 host-owned buffer。
- 不引入 UE-like 基础类型体系，例如 `TArray`、`TMap`、`FString`、`FName`、`UObject`、`UCLASS`、`UPROPERTY` 风格宏或 GC 语义。
- 第三方类型可以在 private implementation 使用；public header 只暴露标准库类型、Astra DTO、Astra handle 和 C ABI 类型。

## 3. What To Borrow From UE-Class Engines

可以借鉴：

- 明确的 module lifecycle：discover、validate、load、initialize、activate、deactivate、shutdown、unload。
- 强诊断与 release gate：机器可读 diagnostic、severity、suggested fix、blocking/fatal policy。
- 反射元数据目标：property descriptor、schema、Inspector metadata、migration、review flags。
- 运行时边界：Editor 不拥有 packaged runtime state，runtime 可 headless 运行和测试。
- 可审计扩展：capability、permission、provider slot、module policy。

不要借鉴：

- 宏驱动的 public object model。
- UE 命名和容器作为默认编码风格。
- 通过继承巨大基类获得生命周期。
- public API 暴露 engine object pointer 或 native handle。
- 让 Editor-only、Renderer、Audio、AI、Legacy VM 概念进入 Core。

## 4. Architecture Boundaries

- Core 不依赖 VN、AI、Live2D、legacy VM、Editor、Renderer backend、Audio backend、SDL 或 OS-specific native API。
- Runtime 不依赖 Editor UI、MCP server implementation、AI provider implementation、renderer/audio native handle 或 legacy VM internals。
- Platform public API 只暴露 engine interfaces；SDL、Win32、POSIX、GPU、audio 和 window handles 留在 private implementation。
- 插件跨 ABI 不传递 STL ownership、C++ Actor/Component 指针、ECS entity、native handle 或 Editor widget。
- Public API 使用 Astra 自有 DTO、opaque handle、stable ID 和 schema-friendly descriptors。
- C ABI headers 只使用 fixed-width scalar、explicit-length UTF-8 string view、POD struct、opaque handle、callback function pointer、result code 和 host-owned buffer policy。

## 5. Naming

- 类型使用 `PascalCase`：`DiagnosticSink`、`PlatformServices`、`ModuleDescriptor`。
- 函数和变量使用 `camelCase`：`emit`、`registerProvider`、`loadPluginDescriptor`、`diagnostics`。
- 常量使用 `kPascalCase`：`kDefaultTimeout`。
- 枚举类型使用 `PascalCase`；枚举值使用 `PascalCase`：`DiagnosticSeverity::Blocking`。
- C ABI symbols 使用 `Astra` 前缀和 C-compatible naming；C function pointers and struct fields 使用 lower snake case：`register_service`、`module_state`。
- 文件名与主要类型保持一致。
- 稳定 ID 使用小写 dotted 或 URI-like scheme，例如 `astra.vn.character`、`native:/Characters/Alice`。

Existing early code may still contain PascalCase member functions such as `Emit` or `Register`. New code should follow the rule above, and cleanup work should migrate old public C++ APIs toward `camelCase` when ABI/API churn is acceptable.

## 6. Public API Shape

- Prefer small DTO structs with explicit fields over implicit engine object graphs.
- Prefer free functions or focused service interfaces for subsystem entry points.
- Prefer `std::string_view` / `std::span` for non-owning input where lifetime is clear.
- Return owning data with standard containers in C++ API when not crossing ABI.
- Avoid hidden global singletons. If a service needs shared context, pass it explicitly through a facade or registry.
- Keep headers minimal: include what is required for declarations, forward declare private implementation details where safe, and use PIMPL for heavy backend state.
- Do not expose template-heavy implementation details as the only extension path; plugin-facing extensibility belongs behind module descriptors, registries, C ABI, or stable C++ service interfaces.

## 7. Ownership And Lifetime

- Ownership must be visible at API boundaries.
- Use references for required non-owning dependencies that must outlive the call.
- Use pointers only for nullable/non-owning relationships or opaque handles; document ownership when it is not obvious.
- Do not store raw pointers to plugin-owned objects across unload unless the lifecycle is guarded by `ModuleManager`.
- Do not save native pointers, C++ object addresses, ECS entity raw values, thread handles, renderer handles, audio handles or Editor-only object references.
- Save/replay state must be serializable through stable IDs, descriptors, values and extension state blobs.

## 8. Errors And Diagnostics

- Recoverable C++ errors return `std::expected` or project equivalent `Astra::Core::Result`.
- Cross-ABI errors return explicit result code and write to diagnostics sink when context matters.
- Do not throw exceptions across ABI boundaries.
- Exceptions from third-party/private code must be caught before crossing public Astra boundaries unless a local implementation has a documented no-throw guarantee.
- Diagnostics are structured data, not formatted log strings. Include code, severity, category, message, source/object refs and suggested fixes when actionable.
- Logs describe temporal observations; diagnostics describe machine-actionable conditions.

## 9. Properties And Reflection

- Astra uses a reflection-lite `PropertySystem`, not UE-style generated reflection.
- Public metadata should be represented as descriptors, flags, schema IDs and stable property IDs.
- Inspector/editor/MCP/release gate behavior should be driven by descriptors and diagnostics, not by C++ macros hidden in runtime types.
- If native C++ types need schema exposure, add explicit descriptor registration or generated descriptor data that still compiles to plain C++/DTO boundaries.

## 10. Dependencies And Includes

- Core must stay dependency-light and must not include Platform, Runtime, Editor, AI, VN, SDL, renderer or audio headers.
- ModuleRuntime may include C ABI headers and platform dynamic library abstractions, but plugin public ABI remains C-compatible.
- Backend-specific includes belong in `Private/`.
- Public headers must not expose SDL, OS, GPU, audio, editor widget or provider SDK types.
- Prefer narrow includes over umbrella includes.

## 11. Tests

- Add focused tests for behavior, not only construction.
- Test names should describe expected behavior, for example `ServiceRegistry enforces required permissions`.
- ABI/public header tests should scan for forbidden public tokens when adding new module boundaries.
- For plugin work, include at least one integration path through `ModuleManager` or a smoke program once the relevant systems exist.

## 12. Documentation

Design documents describe the target state. Development docs may describe target state too, but implementation notes must clearly mark what is implemented now versus planned.

Do not document planned systems as currently working. If implementation lags behind the target architecture, call that out in an implementation note, issue, coverage matrix or TODO rather than silently changing the architecture goal.

## 13. Implementation Slices

- If a runtime implementation file grows beyond roughly 800 lines or mixes several independent workflows, split it into concrete slices under `Private/<Feature>/` with at most two directory levels under the module root.
- Keep slices concrete. Do not leave behind empty forwarding shells or placeholder translation units.
- After splitting one module, update the implementation index in `docs/manual/api/README.md`, rebuild the affected target, and run focused tests before moving to the next split.
- Prefer a small number of feature-aligned slices over fine-grained sharding; split only when the boundary is clear and maintainability actually improves.
