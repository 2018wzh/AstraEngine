# Artemis 最小原生适配

## 固定来源与提交结构

- 上游：[Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core)，2026-10-16 核对默认 master HEAD 为 `ad90a62cb192b119b7cf8589aa7f87a94bd747c8`。
- 完整 fork：[2018wzh/art3m1s-core](https://github.com/2018wzh/art3m1s-core)，本地分支 `codex/astra-upstream-head-20261016`。
- 唯一适配提交：`b802ab8eb397d12c623df793ad9feb8d3ae0b5a9`；其唯一父提交为上述上游 HEAD，范围内提交数实测为 1。尚未推送，不能宣称远端干净克隆已可取得此对象。
- 主仓使用 `Emulator/ThirdParty/art3m1s-core` submodule，不复制核心源码。旧来源、缺失对象、恢复过程和旧基线实测保存在[审计文档](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)。旧适配历史未改写。

## 必要差异

- 保留上游 VM、Lua 5.1、NativeVulkan/NativeMetal、文字和 FFmpeg 9。没有恢复旧 Luau 替换、强制唤醒、修改游戏变量、模拟影片结束或第二 renderer。
- 两个 fallible 原生帧入口 `try_advance_frame`、`try_capture_frame_into` 分开推进和捕获；静态不读回，短缓冲和 GPU 失败不混同 Unchanged。FrameInfo/lease/代次和 Host 呈现仍在 Family。
- PFS 索引与查询统一分隔符，保留原大小写语义；跨平台路径测试使用原生 Path。
- 原生 Vulkan 拒绝 CPU/未知 adapter；不添加软件 fallback。
- 原 FFmpeg 自定义 AVIO 保留明确分配/释放与失败处理。视频音频提取归播放实例所有，停止/替换/关闭先取消并 join，未消费临时 WAV 由原 session 清理。
- 原生读档先恢复正确的输入等待，再执行 onLoad 标签；回调后恢复保存文字。不替换原保存格式。
- 独立 workspace 与精确可选依赖闭合：原 RFVP 桥使用 `Alphaly2K/rfvp@ec204312e123b4839cec8e69e6237fb0374e5518`；原 Siglus 桥使用实际提供 `art3m1s-host-audio` 的 `Alphaly2K/siglus_rs@28917920a3e5eeb9a5fa60eac31400422cd41fe2`。AstraEMU 不启用这两条桥，不让它们意外消费相邻独立 Family 的核心。

核心适配共 15 文件、+1,358/−274，包含 lockfile、清单和回归；不是纯性能代码量。未统一格式化上游文件。删除了本轮只有像素长度断言的弱 GPU 测试，使用 Family 的状态/错误/lease 回归及实际 GPU 场景验证，不以加载或缓冲长度冒充产品通过。

## 新 HEAD 实际验证

- 原生 Vulkan/FFmpeg 9 library check 通过；lib 单元 339 通过、1 忽略，PF8 11 通过、3 忽略。
- 复用真实 Family 源码的独立清单：普通测试 13 通过、2 GPU 项忽略；随后显式硬件运行这 2 项，通过静态 generation、三次关闭重开、旧 CPU lease、短缓冲拒绝及目标不被改写。
- 实际 FamilyProvider/native GPU/FFmpeg 9 非商业 PF6 视频音频程序推进 150 次：18 次变化、132 次 Unchanged、15 个彩色帧，交付 237,120 个 PCM samples，其中 233,848 个非零；关闭取消与旧 CPU lease 保持有效。Host 明确为 TestConsumption，不是设备听音、商业游戏、完整路线或四平台验收。
- 原生 Clippy 执行成功，保留上游 warning；本次新增的无效 `drop(pipeline)` 已删除。修改文件已单独格式化。全仓 fmt check 仍发现未修改的上游 `asb-interpreter/src/interpreter.rs` 格式差异，没有为获得全绿扩大适配 diff。
- 完整 Emulator workspace、共享 FFmpeg 9 消费者和最终 parent 提交尚在整合验证，以上独立清单结果不代替整仓通过。

Family API 为 v13；最终原生 RGB 使用 Ignored alpha，不扫描或改写原 alpha bytes。原核心没有 PCM mixer，Family 继续复用 SDK Symphonia/Kira 处理原音频命令和独立影片通道。BGM crossfade 与不支持的原生 UI 请求仍明确失败，不伪造完成。

许可证与归属见[第三方说明](THIRD_PARTY_NOTICES.md)。
