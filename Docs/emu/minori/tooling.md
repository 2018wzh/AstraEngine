# Minori 工具状态

Minori 尚未接入 [独立 Host](../../migrations/astraemu-independent-host.md)。旧通用 CLI 已删除；此前的 VFS mount、Luau private profile、Headless 和签名 provider 工作流不适用于当前 Host。

`astra-emu-minori-cli` 的源码作为 inactive 研究工具保留，包含 archive inventory、两阶段纯 Rust NRBF reader、GARbro scheme importer 和脚本/媒体 census。它不在主 workspace 中，不能直接沿用旧的 workspace 运行命令。保留源码是为了后续复用格式解析实现，不代表新 ABI 接入已经完成。

NRBF reader 先收集对象和 metadata，再解析 forward reference；未知 record、断裂 reference、重复 id 和非预期 Musica/PAZ graph 必须报错。后续接入不得改用 BinaryFormatter 或外部 managed helper。旧 importer 输出的 Luau/mount 配置属于旧架构，届时应按新 Family 自有文件边界调整，不保留兼容桥。

## 辅助研究脚本

`Tools/AstraEMU/minori_probe.py`、`minori_paz.py` 和 `minori_sc.py` 仍用于格式研究，不是当前 Host 的生产路径。输入数据库、key、解包内容和 disassembly 保持本地私有。

以往实验结果见 [历史移植记录](porting-log.md)；其中的命令和测试结果不能作为新 Host 的当前状态。
