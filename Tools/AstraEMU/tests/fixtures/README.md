# AstraEMU Synthetic Fixtures

这里的 fixture 用于 research tools 的边界检查、最小格式解析和外部 Family 接入开发回归。不要放入商业游戏 payload、完整脚本、图片、音频或视频。

`external_family.rs` 是公开、独立的最小核心源码，由 [接入检查脚本](../../check_external_family.py) 临时编译并交给现有 Manager 加载。使用方式及 CPU 帧/NullAudio 边界见 [开发手册](../../../../../Docs/manual/development.md)。它不是新的产品 workspace，也不模拟商业游戏通过。
