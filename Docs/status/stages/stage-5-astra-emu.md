# Stage 5 AstraEMU Work

Status: `IN_PROGRESS`

2026 年 8 月 23 日，AstraEMU 已硬切到 Product Runtime Provider ABI v4、Family ABI v9 和 Extension ABI v1。当前分支不提供 v7/v8 compatibility shim。FVP 固定为 `Ported + SingleLayer`，Minori 固定为 `Native + MultiLayer`；错误组合在 descriptor 校验阶段直接阻断。

本页只记录 v9 当前状态。旧 scene transaction、family snapshot/save/restore、text lease、session resource presentation、step budget 以及 frame/audio/route/session/input 等运行时语义 hash 已退出当前契约，历史报告不能再用于关闭 v9 验收。

已完成的实现包括：

- `astra-media-core` retained `Layer2D`、三态 damage、typed `FilterGraph`、事务回滚与稳定排序。
- Host-owned writable surface lease；RGBA8/BGRA8 sRGB 均使用 premultiplied alpha，generation 在 step 成功后原子发布。
- 同步 Hook 与 UTF-8 translation companion；正文、payload、secret 和文本 hash 不进入日志、SQLite、report 或 package。
- per-game writable root 与安全相对路径文件 API；FVP、Minori 的原生存档由 core 自己组织。
- RFVP fork 直接实现 Family ABI v9，AstraEngine 的 `astra-emu-fvp` 只保留 dylib、身份、构造、shutdown、panic containment 和错误边界。
- Manager、CLI、Headless 消费公共 `Layer2D`。Manager GPU 路径按层执行 bloom、fade、color-matrix，并复用双缓冲纹理和 uniform；静态 generation 不重复执行。
- AstraEMU 自定义 E2/E3 报告改用计数、明确状态、coverage ID 和真实 artifact；只保留 package、plugin binary、schema、build、profile、artifact-file 等完整性 hash。

尚未关闭的门禁是 Performance E2、真实 Windows Manager E3、完整 workspace clippy/test、Android target 修复后的全量构建，以及授权游戏的长流程、音频、视频和冷启动原生存档复跑。因此本 Stage 仍为 `IN_PROGRESS`，不得宣称可合入或发布。

## S5-GAME-RUNTIME-01 AstraEmuRuntimeProvider gameplay runtime

**Status:** `IN_PROGRESS`

`AstraEmuRuntimeProvider` 声明唯一 `Layer2D` presentation lane。共享 Product Runtime save/restore 对 AstraEMU 返回 unsupported；Manager 不再提供 F5/F9，CLI 不再提供 checkpoint/resume。Family output 通过 step-scoped surface、Layer2D、audio、wait、input 和 control DTO 进入 RuntimeWorld。

## S5-EMUCORE-SM-01 EmulatorCore VM state-machine mapping

**Status:** `IN_PROGRESS`

RuntimeWorld 继续负责 Actor/Component、StateMachine、ordered ingress 与 typed action。family-private VM 状态不再包装成 Host snapshot，也不生成逐 tick state hash。FVP/Minori 在自己的 session 内持有执行状态，并通过 typed output 提交可观察结果。

## S5-LEGACY-VFS-01 Legacy pack VFS mounts

**Status:** `IN_PROGRESS`

只保留显式 mount、stat/range read、source revision 与安全边界。probe 已删除 entry/metadata 策略预算；整数溢出、越界、所有权和系统分配错误仍 fail-fast。源文件、archive entry、schema 和 artifact 完整性 hash 保留，不恢复 per-read content hash。

## S5-MANAGER-01 Manager RuntimeWorld bridge

**Status:** `IN_PROGRESS`

Manager 已接入 v9 surface pool、原子 staged generation、Hook binding、writable-file 单写 session 和 Layer2D GPU compositor。pool 暂时耗尽时重试同一 generation并节流告警，不丢帧、不临时分配、不切换 presentation mode。

E3 现在直接检查输入消费计数、画面变化、非静音音频、terminal 事件、coverage ID 和有序 shutdown；不再使用 session/input/frame/audio/route hash。

## S5-MANAGER-UI-01 Slint Manager 与 runtime overlay

**Status:** `IN_PROGRESS`

Slint 继续持有窗口、事件循环和共享 wgpu device/queue。游戏 underlay 与 overlay 不做 CPU 整帧回读或跨设备复制。翻译设置只保存显式启用状态、唯一 provider 配置和 `u32 timeout_ms`；缓存与正文持久化已删除。

真实 Windows 输入、画面、音频和 shutdown 仍需独立 E3。

## S5-FAMILY-01 LegacyRuntimeProvider facade

**Status:** `IN_PROGRESS`

Family ABI v9 descriptor、writable surface、Layer2D transaction、Hook 和 writable-file ports 已落地。v7/v8 module、错误 core-kind/mode 和旧 ABI fingerprint 必须拒绝。Git/path 双份 `astra-emu-family-api` package identity 视为构建阻断。

## S5-AUTOPROBE-01 Manager auto probe

**Status:** `IN_PROGRESS`

显式 case profile 始终优先，默认 probe 顺序不变。probe 使用表示安全的文件上限，不再接受调用方策略预算。真实多 family 冲突、坏 manifest 与大目录诊断仍需复跑。

## S5-METADATA-01 作品识别、元数据、游玩记录与兼容性库

**Status:** `IN_PROGRESS`

本次 ABI 迁移未改变 metadata 的 HTTPS、license、consent 和隐私边界。metadata payload/cache 的完整性 hash 属于内容与 artifact 校验，不是 Runtime 语义 hash。真实网络、商业许可和 UI 自动化仍开放。

## S5-SCRIPT-01 Trusted Luau patch/decode runtime

**Status:** `IN_PROGRESS`

Trusted Luau 不再提供 `text_hook`。翻译只能通过 Extension/Family Hook。媒体替换使用安全相对 URI 显式匹配；patch intent 只记录 kind 与 payload 字节数，不记录正文、路径、payload 或替代 hash。sandbox 的内存、指令和输出边界继续作为安全约束保留。

## S5-TEXT-01 Text dump and translation provider

**Status:** `IN_PROGRESS`

独立文本 lease、Host overlay、翻译 cache 和文本 hash 已删除。FVP/Minori 在 framebuffer acquire 前同步调用 `astra.emu.translation.text.v1`，未绑定、timeout、认证、限流、网络、协议、缺字或布局失败时保留原文并返回 typed diagnostic。成功结果由 family core 自行完成 fallback、shaping、换行和绘制。

Extension ABI v1 已提供 descriptor、instance/session lifecycle、同步 invoke 与 loader fail-fast。CLI/Headless 可通过 `--extension-library` 和可选 `--extension-timeout-ms` 显式绑定预配置 extension；默认 timeout 为 2000 ms，0 表示立即超时。

## S5-FILTER-01 AstraEMU FilterGraph presets

**Status:** `IN_PROGRESS`

Layer state 可携带 typed `FilterGraph`。Manager GPU compositor 支持 bloom、fade 和 color-matrix，按层使用可复用双缓冲输出；无 CPU fallback、无 runtime hash、无逐帧资源创建。最终画面 preset 仍保留 none、grayscale、crt-soft、warm。

filter visual golden 和正式 GPU 性能证据尚未形成。

## S5-ARTEMIS-01 Artemis family plugin

**Status:** `PLANNED`

必须直接迁移 Family ABI v9；不得从旧 snapshot、scene transaction 或 text lease 恢复兼容路径。

## S5-KRKR-01 KrKr family alpha profile

**Status:** `PLANNED`

后续接入必须选择 `Native + MultiLayer` 或 `Ported + SingleLayer`，并使用 v9 Host ports。

## S5-BGI-01 BGI family plugin

**Status:** `PLANNED`

后续接入必须直接实现 v9，不接受 v7/v8 shim。

## S5-SOFTPAL-01 SoftPAL 接入门槛

**Status:** `PLANNED`

保持在 probe/research 阶段，不阻塞 FVP 首发门禁。

## S5-FVP-01 FVP 接入门槛

**Status:** `IN_PROGRESS`

RFVP fork 当前固定 revision `f4f64a5bb726c1759350a666a35e0a454b810f61`，provider 已声明 `Ported + SingleLayer` 并能走 Host writable surface、Hook 和 writable-file 路径。AstraEngine 侧 adapter 已收缩为 dylib/export、identity、panic 和错误边界；正常目标像素路径只有“RFVP 写 Host lease → Host 上传”。但 pinned fork 仍保留 hosted semantic-delta、snapshot/restore 和旧 policy-limit API，详见 [RFVP fork audit](../../emu/fvp/rfvp-fork-audit.md)，因此 fork thinness gate 当前为 `BLOCKED`，不能把旧路径写成已删除。

`astra-emu-fvp` 已收缩为 dylib/export、build identity、provider 构造/shutdown、panic containment 和最终错误映射。fork provider 聚焦测试已通过；fork 全量测试、真实游戏 oracle、翻译布局、冷启动存档和 Performance E2 仍开放。

## S5-SIGLUS-01 Siglus 接入门槛

**Status:** `PLANNED`

Siglus v8 不属于本分支，必须先迁移到 Family ABI v9，不能直接合并旧实现。

## S5-GATE-01 AstraEMU release gate

**Status:** `IN_PROGRESS`

唯一预算型阻断门禁是 Performance E2。800×600 local/static damage 与 1920×1080 full damage 都要求 1200 warmup、72000 samples、presentation p99 不超过 8.33 ms、零 deadline miss；前者还要求无变化零 upload、稳态零 allocation。Runtime 保持 60 Hz，presentation 为 120 Hz，在线翻译 latency 单独报告。

Headless 只形成 E2。Windows Manager 的真实输入、画面、音频与 shutdown 必须另做 E3。

## S5-PROGRAM-TARGET-01 AstraEMU Manager 与 CLI Program target

**Status:** `IN_PROGRESS`

Manager、CLI 与 Headless 已使用相同 v9 family host services、Layer2D 和 writable-file contract。CLI 不再暴露 checkpoint/resume；Windowed/Headless 自定义报告不生成 runtime semantic hash。共享 Headless v3 artifact contract 中的 package/profile/build/artifact-file hash 继续作为完整性绑定。

## 验证状态

已通过的聚焦验证包括 Family API、Extension API、FVP provider、Minori provider、Manager/CLI/E3 的增量 check/test，以及 schema 生成。提交前仍必须运行：

```bash
python Tools/check_docs.py
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p astra-headless
cargo test --workspace
```

本机 Android target 组件尚未完成修复。在全量门禁、Performance E2 和 Windows E3 完成前，本分支不得标记为可合入。
