# ADR 0009: AstraEMU compat core 独立进程化

Status: Superseded by [ADR 0012](0012-astraemu-engine-native-family-plugin.md).

## Context

旧 VN core 可能包含复杂 VM、解码、脚本补丁和 family-specific 行为。把它们放进主 Runtime 会扩大崩溃和安全面。

## Decision

AstraEMU compat core 作为独立进程运行，持有 family 权威状态机和私有 VM。Manager 通过本地 RPC + shared memory 接收 step output、trace、media block 和 snapshot。

## Consequences

兼容 core 崩溃不直接拖垮 Manager。调试、回放和报告需要跨进程序列化 trace，但 EngineCore 边界更干净。
