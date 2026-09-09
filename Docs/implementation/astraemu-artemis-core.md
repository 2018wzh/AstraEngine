# AstraEMU Artemis 后续接入

Artemis 是 FVP 之后的后续 Family，本轮不进入活动 workspace。PFS/PF6/PF8、boot、text/tag、legacy Lua 与 `.iet`/`.ast`/`.asb` 的研究事实可继续参考；它们不代表实现已完成。

未来接入必须实现 [独立 Family ABI](../contracts/astraemu-ipc.md)：Family 自行持有 VM、原生文件、媒体、最终帧和原生存档；Host 仅管理 session、输入、音频设备与最终帧滤镜。旧 RuntimeWorld/LegacyRuntimeProvider、StateMachine effect、统一 snapshot、Host VFS 和通用 Hook 设计已被取代，不作为新接入基础。

对格式与行为的未知部分应明确报错，不能用猜测性支持或替代画面隐藏。具体源码复用、完整游戏范围和文本替换能力在授权 Artemis 实施时确定；当前只完成 FVP，端到端翻译等待 Minori。

设计决策见 [ADR 0019](../adr/0019-astraemu-independent-host.md)，当前范围见 [重构方案](../migrations/astraemu-independent-host.md)。
