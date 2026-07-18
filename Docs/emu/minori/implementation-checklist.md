# Minori Implementation Checklist

## 当前实现与证据

| 项目 | 状态 | 证据边界 |
| --- | --- | --- |
| `family-core` mount/read_dir/stat/read_range/open_stream 契约与 manifest v2 | 已实现 | unit/compile；`family-api` 已硬迁移为 ABI DTO，不保留 VFS re-export |
| PAZ v0-v2、分卷、zlib、随机读取 | 已实现 | GARbro contract + synthetic tests；真实六包 9837 个 entry 完成首尾随机读取 |
| 纯 Rust `MinoriPazDecryptProvider` | 已实现 | Blowfish、RC4 skip、archive XOR、zlib、movie transform；没有 Luau callback 或 fallback |
| Trusted Luau v2 private profile | 已实现 | data-only 一次注册、sandbox 与预算 tests；Minori Luau 不执行逐 entry 解密 |
| 公共 plaintext cache、identity、权限、atomic/LRU | 已实现 | corruption/identity/LRU tests；Windows owner-only DACL 与 Unix mode 失败即阻断 |
| 公共 viewer tree/stat/page/search/text/hex/media binding | backend 已实现 | image/audio/video 必须显式 `DecodeProviderRegistry` binding；Manager UI 接线和真实预览验收待补 |
| 公共 desktop verify/extract | 已实现 | Windows manifest v2 全量 verify 已通过；extract contract 已接入，macOS 运行证据待补 |
| Linux foreground read-only FUSE | 代码已接入 | 缺真实 Linux FUSE 证据，不标完成 |
| GARbro scheme importer | 已实现 | 独立 CLI 使用纯 Rust 两阶段 NRBF reader；原子生成 patch/profile，不使用 managed helper 或 fallback |
| `.sc` CP932 lossless IR、CFG、unknown command、census | 已实现 | 89 文件/33728 行/33695 command/29 token，unknown opcode 0；`select` operand 语义仍 unknown |
| Minori VM、演出、存档、完整模拟 | 未开始 | 下一阶段 |

当前合法样本的六个 archive 均非空。纯 Rust GARbro scheme importer 生成的私有补丁已完成六个 index 与 9837 个 entry 全读；manifest v2 verify 共执行 29720 次 range read、读取 2403596354 个 decoded bytes，同 identity Release 复核全部命中 cache，聚合 hash 一致。89 个脚本的 payload-free census 也已通过。Linux FUSE、macOS extract、Manager media preview 和 VM 仍各自保留独立证据边界。

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
