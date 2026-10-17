# AstraEMU Synthetic Fixtures

这里的 fixture 用于 research tools 的边界检查、最小格式解析和外部 Family 接入开发回归。不要放入商业游戏 payload、完整脚本、图片、音频或视频。

`external_family.rs` 是公开、独立的最小原生 C ABI 核心，由 [接入检查脚本](../../check_external_family.py) 临时编译并交给 Manager 加载。使用方式及自主调度、CPU lease、输入和同进程关闭重开边界见 [开发手册](../../../../../Docs/manual/development.md)。它不是新的产品 workspace，不播放音频，也不模拟商业游戏通过。
