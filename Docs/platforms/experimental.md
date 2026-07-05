# Experimental Platforms

实验平台参考 `D:/Workspace/rfvp` 的多平台经验，允许 PSP、PS2、PS3、PSV、3DS、Wii、WiiU、UEFI 等作为独立 host module。实验平台不阻塞 v1.0 硬目标。

## Rules

- 实验平台可以缺少 Editor、AI、联网和完整硬件解码。
- 必须保留 Runtime deterministic contract、package reader、input mapping、save/load 和 report export。
- 平台限制必须写入 capability report，Release Gate 根据 profile 判断是否可发布。

实验平台不改变六平台 v1 gate。host trait 仍按 [Platform Host Blueprint](../implementation/platform-host.md) 实现，缺失能力必须进入 capability report。
