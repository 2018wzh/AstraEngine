# Emulator Core 执行边界

原 RuntimeWorld 与 StateMachine 桥接设计已由 [独立 Host 重构](../migrations/astraemu-independent-host.md) 取代，不再实现或保留兼容入口。

Family 持有旧 VM 的权威状态，以 advance(elapsed) 接收时间和有序物理输入。脚本线程、等待、转场、音频、解码与原生存档由 Family 自行实现；Host 只消费最终 CPU 帧与混合 PCM。窗口焦点与尺寸是传入的事件，不替代 Family 的暂停或恢复规则。

接口由 [Family contract](../contracts/astraemu-ipc.md) 定义；当前接入状态见 [Stage 5](../status/stages/stage-5-astra-emu.md)。
