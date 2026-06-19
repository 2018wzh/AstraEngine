# ADR 0009: Legacy VN Emulator and Modernization Plugins

Status: Accepted

## Context

AstraEngine 要支持旧 VN 引擎兼容运行、调试和现代化。许多旧引擎使用预编译脚本、VM、timeline、score 或私有包格式，强制导入或反编译会降低兼容度。

## Decision

Legacy VN 支持通过兼容插件实现：

- PackageReader / VFS mount。
- Legacy asset resolver。
- Legacy ScriptRuntime / VM。
- Opcode decoder / timeline adapter。
- Legacy API Mapper。
- Save extension state。
- Compatibility Inspector。
- Modernization Profile 和 FilterGraph。

不把外部项目默认导入为 Astra canonical source。外部原资产默认 mount-only。

## Consequences

- 兼容模块可以拥有 VM 状态，但必须通过 Save extension state 进入统一存档。
- 旧 API 输出映射为 RuntimeEvent 或 PresentationCommand，不直接调用底层渲染或音频 native handle。
- Cook/package 默认不复制外部原始资产。


