# Minori Implementation Checklist

## 当前实现与证据

| 项目 | 状态 | 证据边界 |
| --- | --- | --- |
| family API mount/read_dir/stat/read_range/open_stream/unmount | 已实现 | unit/compile，未宣称所有 family 已迁移 |
| PAZ v0-v2、分卷、zlib、随机读取 | 已实现 | GARbro contract + synthetic tests；真实六包 9837 个 entry 完成首尾随机读取 |
| session Luau decoder registry 与 native buffer intrinsics | 已实现 | Manager Core unit tests |
| plaintext cache 8 GiB/1 GiB、identity、权限、atomic/LRU | 已实现 | cache tests；路径不进 report |
| viewer tree/stat/page/search/text/image/audio/hex | backend 已实现 | backend 分类与 bounds unit tests 通过；Slint Files 面板仍是 session 接线 scaffold，真实 UI 验收待补 |
| desktop verify/extract | 已实现 | Windows 真实 `scr` extract 通过；staging tree 原子提交、失败清理和大小写冲突 tests 通过；macOS 运行证据待补 |
| Linux foreground read-only FUSE | 代码已接入 | 缺真实 Linux FUSE 证据，不标完成 |
| GARbro scheme importer | 严格入口已实现，真实导入阻断 | `nrbf 0.2.2` 对当前合法 `Formats.dat` 的 library state 回溯产生重复 library id；不使用 `BinaryFormatter` 或宽松 fallback |
| `.sc` CP932 lossless IR、CFG、unknown command、census | 已实现 | 89 文件/33728 行/33695 command 全包通过；`select` operand 语义仍 unknown |
| Minori VM、演出、存档、完整模拟 | 未开始 | 下一阶段 |

当前合法样本的六个 archive 均非空。六个 index 与 9837 个 entry 已完成全读，第二轮为 9837 cache hits/0 misses；89 个脚本的 payload-free census 也已通过。Linux FUSE、macOS extract、GARbro scheme import 和 VM 仍各自保留独立证据边界。

## 下一阶段 Archive

- [x] Probe game root and classify `scr/st/sys/se/voice/mov.paz`。
- [x] 通过本地私有 Luau patch 解出六个 index。
- [x] 对每个 entry descriptor 校验 offset、packed size、unpacked size 和 method。
- [x] 拒绝 path traversal 和绝对路径 entry。

## Script

- [ ] 从 `scr.paz` 确认入口脚本；`pragma`/`chain` 只能作为线索。
- [ ] 拆分 message、select、变量、wait 与演出 operand；`label/goto/if/chain` CFG 已确认。
- [x] 未确认 command/operand 保留 raw bytes、source span 和 `Unknown`。
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
- [ ] movie 播放交给下一阶段 runtime/media 接入；缺失、空或不可校验的 `mov.paz` 在 mount preflight 阻断。

## Release Gate

- [ ] 本地 case report 只包含 hash、coverage、diagnostics 和命令。
- [ ] 不包含 payload、截图、音频、视频、完整脚本或 key。
