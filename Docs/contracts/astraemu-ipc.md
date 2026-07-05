# AstraEMU IPC Contract

AstraEMU Manager 和 compat core 分离。Core 是独立进程，持有旧引擎 family 的权威状态机和私有 VM 状态；Manager 负责窗口、配置、provider selection、插件分发、报告和 Astra Runtime bridge。

## IPC

控制和 trace 走 framed local RPC，媒体块走 shared memory。协议必须支持：

- `ProbeContent`
- `LoadCase`
- `Step`
- `ApplyInput`
- `SaveSnapshot`
- `LoadSnapshot`
- `Shutdown`
- `ReadTraceBatch`
- `MapSharedMediaBlock`

Core 输出 RuntimeEvent、PresentationCommand、AudioCommand、TextCaptureEvent、StateMachineTrace、LegacyVmSnapshotRef 和 diagnostics。Manager 不解析 core 私有 VM 内存。

## Family 顺序

v1 可用 family 是 Artemis。后续按通用性排序扩展：KrKr/KAG/TJS、BGI/Ethornell、SoftPAL、FVP、Siglus。SoftPAL 参考 `D:/Workspace/sena-rs`，FVP 参考 `D:/Workspace/rfvp`，Siglus 参考 `D:/Workspace/siglus_rs`，BGI 参考 `ethornell-rs`。

## Report

Local case report 只包含 hash、coverage、diagnostics、命令、family feature 和脱敏 metadata，不包含商业 payload、私有绝对路径或未授权截图。
