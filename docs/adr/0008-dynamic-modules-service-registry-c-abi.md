# ADR 0008: Dynamic Modules, ServiceRegistry, and C ABI

Status: Accepted

## Context

AstraEngine 需要支持项目级扩展、VN Presentation、Live2D/Spine、FilterPack、AI Provider、Runtime Intent、旧 VN 模拟器、Editor 扩展和 Cook 工具。扩展边界必须稳定。

## Decision

项目级扩展默认使用动态模块。模块通过 `AstraModule` C ABI 进入，通过 `ServiceRegistry` 获取服务，通过 `ExtensionRegistry` 注册扩展。

ABI 不暴露：

- STL ownership。
- C++ Actor/Component 指针。
- renderer/audio native handle。
- Editor widget。
- 内部 ECS entity 或 registry。

## Consequences

- C++ SDK 只是便利包装，稳定边界仍是 C ABI。
- 所有能力必须声明 capability 和 permission。
- Release Gate 校验 ABI、权限、依赖闭包和 packaged eligibility。
- 热重载分层支持，不承诺任意运行中二进制替换。
