# Minori Implementation Checklist

## 当前实现与证据

| 项目 | 状态 | 证据边界 |
| --- | --- | --- |
| `family-core` mount/read_dir/stat/read_range/open_stream 契约与 manifest v2 | 已实现 | unit/compile；`family-api` 已硬迁移为 ABI DTO，不保留 VFS re-export |
| PAZ v0-v2、分卷、zlib、随机读取 | 已实现 | GARbro contract + synthetic tests；真实八包 14502 个 entry 完成 decoded full verify |
| 纯 Rust `MinoriPazDecryptProvider` | 已实现 | Blowfish、RC4 skip、archive XOR、zlib、movie transform；没有 Luau callback 或 fallback |
| Trusted Luau v2 private profile | 已实现 | data-only 一次注册、sandbox 与预算 tests；Minori Luau 不执行逐 entry 解密 |
| 公共 plaintext cache、identity、权限、atomic/LRU | 已实现 | corruption/identity/LRU tests；Windows owner-only DACL 与 Unix mode 失败即阻断 |
| 公共 viewer tree/stat/page/search/text/hex/media binding | backend 已实现 | image/audio/video 必须显式 `DecodeProviderRegistry` binding；Manager UI 接线和真实预览验收待补 |
| 公共 desktop verify/extract | 已实现 | Windows 八包 manifest v2 full verify 已通过；extract contract 已接入，macOS 运行证据待补 |
| Linux foreground read-only FUSE | 代码已接入 | 缺真实 Linux FUSE 证据，不标完成 |
| GARbro scheme importer | 已实现 | 独立 CLI 使用纯 Rust 两阶段 NRBF reader；原子生成 patch/profile，不使用 managed helper 或 fallback |
| `.sc` CP932 lossless IR、CFG、unknown command、census | 已实现 | 89 文件/33728 行/33695 command/29 token，unknown opcode 0；`select` operand 语义仍 unknown |
| ANI/SQZ container 与 `bg`/`bgm` census | adapter 已实现 | 2655 PNG、1951 ANI/6723 frames、9 SQZ/224 frames、49 Ogg 真实读取通过；渲染/播放尚未验收 |
| Minori deterministic VM state 与 control-flow slice | E2 slice | `set/setglobal/label/goto/if/wait/message`、BGM/SE stop、`playvoice *`、`transition`、无 stand `stage`、`chain/end`、budget、一次性文本 lease、连续 tick、尾链 VFS 切换和 postcard snapshot/restore；select、普通 voice、stand 与主要演出仍 blocking |
| Minori runtime provider / `cdylib` ABI | E2 slice | 签名动态 plugin 经通用 `--family minori --mount-profile` composition 启动；真实八包 Headless 到达首条可见 message，共 372 tick、8 个 frame、5 个 checkpoint，音频 evidence 与 snapshot round-trip 通过 |
| Minori 演出、系统 UI、完整模拟 | 未完成 | 已验证黑场、竖排标题、CrossFade2、message panel 和首条日文正文；transition 动画、人物、UI、存储和路线仍开放 |

当前合法样本包含八个非空逻辑 archive 和 18 个物理文件。纯 Rust GARbro scheme importer 生成的私有补丁已完成八包 manifest v2 full verify：14502 个 entry、43818 次 range read、6624958365 个 decoded bytes。验证轮显式关闭 cache；启用 cache 的运行因平台缓存卷空间不足阻断，因此新的八包 cache identity 仍没有第二轮命中实证。89 个脚本的 payload-free census 已通过。Linux FUSE、macOS extract、Manager media preview 和 VM 仍各自保留独立证据边界。

## 下一阶段 Archive

- [x] Probe game root and classify `bg/bgm/scr/st/sys/se/voice/mov`，包括 `bg.pazA` 至 `bg.pazJ`。
- [x] 通过本地私有 Luau patch 解出八个 index。
- [x] 对八包执行 decoded full verify；cache 关闭的完整读取与首尾复读已通过。
- [ ] 在具备足够私有存储空间的环境复核八包 cache identity 与第二轮全命中。
- [x] 对每个 entry descriptor 校验 offset、packed size、unpacked size 和 method。
- [x] 拒绝 path traversal 和绝对路径 entry。

## Script

- [x] 从 `scr.paz` 与原程序候选确认入口文件 `test.sc`；多脚本时 CLI 要求完整稳定 URI `--entry minori:/scr/test.sc`，不接受裸文件名，也不隐式选择。
- [ ] 拆分 select、普通 voice、stand 与其余演出 operand；音频 `*`、message、BGM/SE、变量、wait、`label/goto/if` CFG、transition/stage 前部和 `chain` 尾链语义已确认。
- [x] 未确认 command/operand 保留 raw bytes、source span 和 `Unknown`。
- [ ] 完成全部资源引用映射；BGM/SE、stage 前景/背景和 stand role 已有严格映射。

## Runtime

- [x] boot 到首个 message；正文经一次性 lease、CosmicText 和 Renderer2D 形成真实 checkpoint，未进入 snapshot/report。
- [ ] 用户推进、auto、skip、backlog 不破坏 pc。
- [ ] choice 写入变量并跳转。
- [ ] save/load 后 state/event/presentation hash 一致。

## Media

- [ ] 背景、立绘和系统 UI 分 layer 输出；当前背景/前景已输出，stand 与系统 UI 未完成。
- [ ] BGM、SE、voice 分通道；BGM/三个 SE bus 已验证，message voice 尚未绑定。
- [ ] voice replay 不推进 VM。
- [ ] movie 播放交给下一阶段 runtime/media 接入；缺失、空或不可校验的 `mov.paz` 在 mount preflight 阻断。

## Release Gate

- [ ] 本地 case report 只包含 hash、coverage、diagnostics 和命令。
- [ ] 不包含 payload、截图、音频、视频、完整脚本或 key。
