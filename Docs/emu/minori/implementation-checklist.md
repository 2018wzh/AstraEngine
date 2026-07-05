# Minori Implementation Checklist

## Archive

- [ ] Probe game root and classify `scr/st/sys/se/voice/mov.paz`。
- [ ] 通过外部 key config 解出 TOC。
- [ ] 对每个 entry 校验 offset、packed size、unpacked size 和 method。
- [ ] 拒绝 path traversal 和绝对路径 entry。

## Script

- [ ] 从 `scr.paz` 定位入口脚本。
- [ ] 反编译 message、choice、jump、call/return、wait。
- [ ] 记录所有未知 opcode 的 raw bytes。
- [ ] 把资源引用映射到 VFS role。

## Runtime

- [ ] boot 到首个 message。
- [ ] 用户推进、auto、skip、backlog 不破坏 pc。
- [ ] choice 写入变量并跳转。
- [ ] save/load 后 state/event/presentation hash 一致。

## Media

- [ ] 背景、立绘和系统 UI 分 layer 输出。
- [ ] BGM、SE、voice 分通道。
- [ ] voice replay 不推进 VM。
- [ ] 缺 movie 或空 `mov.paz` 输出 recoverable diagnostic。

## Release Gate

- [ ] 本地 case report 只包含 hash、coverage、diagnostics 和命令。
- [ ] 不包含 payload、截图、音频、视频、完整脚本或 key。
