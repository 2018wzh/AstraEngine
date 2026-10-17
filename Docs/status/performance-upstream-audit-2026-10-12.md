# 性能与最小上游适配审计

更新：2026-10-16。代码基线：`3c3194a365b14bac41104f6dd889f5e4ba4aff5d`。范围：该提交的 180 项主仓文件变更，以及 RFVP、Siglus、Artemis 相对各自上游父提交的差异。KrKr 按用户决定跳过。

**结论：性能整改部分有效；当前尚未达到严格最小高性能适配验收。** 三核心 HEAD＋1/submodule 结构正确，但 RFVP/Artemis 仍混入功能修复，RFVP 原 no_std 输入路径存在回归；原生与 Hosted 性能对照尚未建立。

本页维护当前发现与证据；执行顺序见[实施计划](implementation-plan.md)，接口见[Family API](../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)。早期审计、旧基线和逐轮构建日志留在 Git，不作为当前状态。

## 已确认目标

- 全量整改，EMU 深审；由主线程亲自执行。
- 核心严格限嵌入入口、依赖注入、原生输出与生命周期钩子；产品配置、ABI、调度和缓存优先留在 Family/SDK/Manager。
- 保持不可变 CPU lease，不引入 GPU handle，不统一或改写上游 renderer/VM/decoder。GPU→CPU→GPU 是当前架构成本。
- 游戏暴露的引擎自身缺陷独立提交 issue/PR，待上游合并后更新基线。适配回归在适配内修正。
- 先建同场景性能基线，再确定数值门槛。本轮只定位资源风险，不作长期无泄露结论。
- FVP 沿用上游 V1/RFVS、GlobalSaveDataV1/RFVG；不增加旧存档识别、迁移、兼容 reader 或专门报错。

## 固定核心基线

2026-10-17 实时核对：三个 upstream HEAD 未变。每核心在新独占分支整理一个适配提交，直接父提交为对应上游 HEAD，来源与已发布历史保留。以下对象仅本地整合，尚未发布。

| 核心 | 上游 HEAD | 单个适配提交 | 差异规模 |
| --- | --- | --- | --- |
| RFVP 0.6.0 | `af557db750c7833274f22a6d82b0dc33cdea2f85` | `dcd39c23ca59b38a9a96b6f301aeb05121aa3e18` | 37 文件，+1662/−178 |
| Siglus 1.0.0 | `5f358ef1360b0439e92aed2b749e8f25d8721954` | `29fb9582751fc8ddfc23b374d428d6eba30bcb40` | 14 文件，+646/−74 |
| Artemis 0.4.0 | `ad90a62cb192b119b7cf8589aa7f87a94bd747c8` | `ae967d8446fe99b97f6e1df93fb1b61d8ada2040` | 18 文件，+1560/−279 |

规模包含清单、lockfile、测试和说明，不代表纯性能代码量或修改必要性。逐核心差异见 [FVP](../../Emulator/Source/Families/astra-emu-fvp/MODIFICATIONS.md)、[Siglus](../../Emulator/Source/Families/astra-emu-siglus/MODIFICATIONS.md)、[Artemis](../../Emulator/Source/Families/astra-emu-artemis/MODIFICATIONS.md)。来源和已发布历史保持不变。

## 待整改问题

P1 为优先正确性/适配边界问题，P2 为整合缺口或待测成本；不是已测得的性能损失。

### R01 — 已修复：RFVP 原 no_std 输入提交回归

`tick_with_renderer` 在事件入队/清队列后、VM 前无条件调用一次 `begin_frame()`；hosted 的外部 delta 设置仍保持原 feature 边界。删除重复的 `hosted_keycode` 和遮蔽 `engine_key` 的两个分支，不增加键位。

同一自制 HCB 通过 `InputGetState/Down/Up/Repeat/Wheel` 比较预期值，每 tick 在独立 PC 协作 yield；真实 `boot/push_event/tick` 覆盖按下、保持、repeat、松开、清边、wheel、失焦、激活点击抑制及不支持键忽略。portable 修复前首 tick 进入失败 PC；修复后 portable 与 hosted 各 1 项通过。FVP 当前库测试 24 通过、3 项硬件测试忽略，其中原生 GPU 保存/恢复及像素生命周期路径实际执行。

原生全 lib tests 的既有 texture `Read` 编译阻塞未改动；上述入口由 `cargo rustc --crate-type rlib` 构建真实库，再独立链接相同行为回归执行，不把 rlib 编译本身当运行证据。

### R02 — 原生修复已独立提交；捕获边界仍属适配

- V1 图像与几何：[issue #50](https://github.com/xmoezzz/rfvp/issues/50)、[PR #51](https://github.com/xmoezzz/rfvp/pull/51)，独立提交 `783d7d5`。纯上游两个回归均失败，独立修复后两个通过；当前 Family 原 GPU 修改像素/恢复回归 1 项通过。复用 V1 `rgba`，保留明确 VFS 错误，删除不可达 fallback。
- 文字上下文：[issue #52](https://github.com/xmoezzz/rfvp/issues/52)、[PR #53](https://github.com/xmoezzz/rfvp/pull/53)，独立提交 `513b395`。纯原生 VM 的退出后旧完成、非 TEXT 等待及 restart 旧 reveal waiter 三项均失败，修复后三项通过。除队列取消，还取消 TextManager 中属于旧上下文的活跃等待；当前整合保留同一修复。
- SaveCreate：纯上游 native VM 控制组确认捕获 PC 紧随 SaveCreate，未证明这是原生缺陷，不提交通用修复。hosted 保留协作 yield 捕获和 capture_pending 暂停，仍使用原 GPU。扩充 Family 回归实际执行 SaveCreate/yield/菜单/SaveWrite/Load：校验保存 PC、整张恢复像素、原缩略图及恢复后继续，1 项通过。

两个功能 PR 等待上游合并；当前不能声称适配只含嵌入钩子。原生图像修复 Clippy 与文字修复 Clippy 均完成，保留既有 warning。原生完整 lib tests 仍受 texture `Read` 导入问题阻塞；文字复现只在独占树临时注入可访问 crate-private 状态的测试模块，不修改 VM 调度逻辑，提交前已移除注入入口。

### R03 — PF8/AVIO 已提交原生修复；packed stereo 控制组通过

- PF8：[issue #6](https://github.com/Alphaly2K/art3m1s-core/issues/6)、[PR #7](https://github.com/Alphaly2K/art3m1s-core/pull/7)，提交 `186ccc1`。自制真实 PF8 的反斜杠索引条目在纯上游查找失败，修复后斜杠/反斜杠均读到同一内容，Clippy 通过；小写规则保持不动。
- AVIO：[issue #8](https://github.com/Alphaly2K/art3m1s-core/issues/8)、[PR #9](https://github.com/Alphaly2K/art3m1s-core/pull/9)，提交 `348b931`。纯上游非法输入以 `0xc0000005` 退出；LLDB 定位 `_aligned_free → free_avio:125 → FfmpegVideoDecoder::open:242`。独立修复后相同调试运行退出 0，成功解码/关闭、提前停止及非法输入共 8 项通过。保持 buffer 地址释放与 Input 先销毁的顺序。
- packed stereo：48 kHz、8 帧、左右声道各不相同的自制 PCM 经原 decode/export，纯上游及当前 tuple 实现均精确保留交错样本和 WAV 长度。当前 `ffmpeg-next 9.0.0` 按字节数计算 typed plane 长度；原 `plane::<i16>` 未复现丢声道。保留 `(i16,i16)` 明确表达 stereo frame，不将其报告为已证明的上游缺陷。
- onLoad：[PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5) 实时查询仍为 OPEN、`mergedAt=null`，沿用既有独立复现，不重复开 PR。

游戏关联样本为ケロQ《終ノ空remake -2025ver-》Windows（`tsuinosora_remake2025ver.exe`）；本节新增复现使用自制 PF8、PCM、非法媒体与内置 MPEG。三个 Artemis 功能 PR 的合并仍是严格最小钩子验收依赖。

### R04 — 两核心非阻塞 read-into 与三槽已验证

Siglus 保留原 renderer 和单 staging，首 poll 提交 copy/map，后续只 `Maintain::Poll`；完成时 depad 写入调用者 Vec，返回捕获时 generation。取消、resize 和 Drop 使旧回调失效；删除同步 `read_frame_rgba` 和每帧完成 channel。

Family 提交前预留三个 `Arc<Vec<u8>>` 槽之一，无独占槽返回 Pending。首次提交开始五秒期限，普通 poll/resize 不续期；超时取消并返回 `ASTRA_EMU_SIGLUS_READBACK_TIMEOUT`，map 错误取消并沿用 `renderer.readback`。boot 使用同一 poll 路径及 1 ms sleep；reset/close 先取消。真实游戏两个取帧消费者已改为有期限轮询，等待期间保留上一 Ready lease。

RTX 4070 Laptop GPU / Vulkan：非 256-byte 对齐尺寸的逐行/逐列像素、resize、捕获 generation、三槽持满背压、释放后复用原指针、旧 lease 不变、固定期限与超时/取消恢复三项硬件回归通过；原生主动 abort map 后错误及取消后重新提交一项通过。Family lib 六项全通过，实际 Rewrite+ 输入/像素/PCM/关闭重开一项通过。Release Manager 实跑 3000 帧，截图确认动画标题，退出 0；NullAudio 消费 1,620,000 帧、2,564,582 个非零样本。

RFVP 将既有原子代次、五秒 timeout 与 cancel 复用到 `poll_rgba8_into` / `poll_frame_into`；原生同步捕获签名保留。Family 初帧 Vec 直接入首槽，后续提交前预留三槽之一；持满时不启动 map，释放后 acquire 无需 advance 即可提交。reset、原生保存捕获取消和 close 释放预留。旧异步分配入口已无消费者并删除。

FVP lib 29 项实际通过，包含非对齐像素/read-into 指针复用、三槽背压与关闭、原 GPU mask/文字/恢复、电影 EOF 和保存 PC/整帧/继续。原生真实 rlib 外部硬件 probe 两项通过：map 主动 abort、受控 timeout、旧回调代次、输出保留与重提；临时导出已删除，原生全 lib 的既有 texture `Read` 编译阻塞保持记录。两个旧电影/保存测试改为及时释放已消费 lease，继续校验原像素语义。

整组 Release 曾因两个未哈希 `wmv_decoder` 产物覆盖而发生 LNK2019；Siglus 库改名 `siglus_wmv_decoder`，保留 VM 依赖别名、原生 CLI 目标和解码实现后，相同四包命令构建成功且 collision 消失。Release Manager/FVP 自制场景的 4×2 白色及相邻黑色像素精确通过，实际《樱花萌放》开场到正文截图及非零 PCM、退出 0 已观察。整改前后三轮配对仍按 R09 继续。

### R05 — 已清理无消费者差异；scratch 优化已测量并独立提交

删除 RFVP 无消费者 `sha2` 声明及三条 context/input 热路径 trace；Family `build.rs` 的 sha2 保留。精确事件字符串搜索只命中被删除的产生点。文件读取恢复一次 `read_exact_at`，由原循环处理 partial read；真实库烟测验证 `1 MiB + 123` 字节、每次最多 73 字节短读、提前 EOF 和容量超限。

scratch 保留栈数组，独立提交 `cbe9f1f` / [PR #54](https://github.com/xmoezzz/rfvp/pull/54)。Windows Release、RTX 4070 Laptop GPU、Vulkan，预热 10 次后测量 100 次真实 `GpuPrimRenderer::build`：直接父版本的 4096-byte 分配为 100 次，栈数组版本为 0 次。此为分配归属证据；场景帧时收益由 R09 配对测量。

### R06 — 已隔离显式硬件 GPU 策略

`VulkanBackend::new` 保持上游 Any 排序（Discrete=3、Integrated=2、其他=1）。新增显式 HardwareOnly，仅接受 Discrete/Integrated/Virtual；Artemis Family 使用 `NativeVulkanHardware`，原 PlatformDefault、NativeVulkan 与 legacy int 映射保持原行为。FFmpeg manifest 恢复 `9.0.0` 文本，解析版本不变。

候选类型筛选/排序回归 1 项通过；实际 Family 硬件 GPU 生命周期与 capture 两项通过，包含同进程重开三次。`--lib --ignored` 当前运行 0 项，硬件证据来自 `--test lifecycle -- --ignored --test-threads=1`。

### R07 — 已接入成功恢复的一次性时钟通知

CoreRuntime 只在完整 load 与 BOOT 重启成功后置位 `timeline_reset`；Family 启动后消费初始化通知，正常 pump 成功后消费通知、清 sub-ms remainder，并在 Running/Finished 响应中保留 `reset_clock`。

原生硬件回归验证成功 load/reset、合并多次成功、缺失/坏存档、BOOT 失败和成功后失败保留通知；1 项通过。真实 Family 脚本 pointer→load 验证通知一次、`999999 ns` 清零及后续 `499999 ns` 从零积累。Manager 进一步将异步恢复锚点延后到首帧 Ready/Unchanged 完成；独立 Pending 期限不触发额外 VM tick，普通 Pending 仍继续逻辑推进。七项 Host 调度/窗口/音频生命周期回归通过，未扩大 elapsed cap；商业窗口读档的计时观察继续纳入 R09。

### R08 — 已修复精确 Family target 的最终 JSON 输出

只有事件 target 精确等于 `astra_emu::family` 时，FieldVisitor 保留原 tracing 类型和值；Debug fields 仍为现有文本。普通 target、带后缀的相似 target 以及所有 span 继续使用原 sanitizer。typed DiagnosticEvent 的 validate、大小上限、来源/级别及 dropped_fields 保持原样。

局部 subscriber 回归通过；独立进程使用真实 Manager DiagnosticSink 到最终 JSON，验证 URL、Windows 路径、普通 message、Debug、bool/i64/u64/f64、dropped_fields，超限事件被拒绝，其他 target 仍 redacted。

Release Manager/FVP 实跑 120 帧自制 HCB/NVSG：640×480 中指定 4×2 白色像素及相邻黑色像素正确，退出 0；最终 JSON 两条 wgpu 原生日志完整保留，实际设备为 NVIDIA RTX 4070 Laptop GPU、Vulkan、驱动 610.88。无声夹具 NullAudio 消费 86400 帧、非零样本 0。

### R09 — Hosted 整改前后数据已采集；Sandbox 验收待 vGPU

同一 RTX 4070 Laptop GPU / NVIDIA 610.88 / Vulkan，商业场景 1280×720、自制静态对照 640×480；使用独占游戏副本和保留的整改前 DLL。每场景三轮，各取 1800 次 advance、约 30 秒；标题出现后至少预热 10 秒。采样窗内不转 PNG，开始前与结束后各截图一次。下表为三轮分位数的最小值至最大值，单位 ms。

| 场景 | 构建 | advance p50 | advance p95 | advance p99 | acquire 完成 p50 | acquire 完成 p95 | acquire 完成 p99 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 《樱花萌放》动画标题 | 整改前 | 0.1991–0.2117 | 0.3332–0.4148 | 0.6559–0.8001 | 0.0023–0.0024 | 0.0042–0.0054 | 1.6738–1.7380 |
| 《樱花萌放》动画标题 | 整改后 | 0.2147–0.2161 | 0.3735–0.4936 | 0.7482–0.8151 | 0.0026–0.0029 | 0.0055–0.0073 | 1.0937–1.3147 |
| 《樱花萌放》开场演出 | 整改前 | 0.7011–0.7877 | 1.2984–1.6401 | 2.9014–5.3710 | 0.0040–0.0054 | 2.1482–5.9334 | 5.9210–13.1250 |
| 《樱花萌放》开场演出 | 整改后 | 0.7633–0.8319 | 1.4763–1.9059 | 2.6506–3.7290 | 0.0047–0.0066 | 2.0499–3.2245 | 3.6556–4.1669 |
| Rewrite+ 动画标题 | 整改前 | 3.0670–3.2331 | 4.9034–5.0484 | 5.9790–6.4961 | 1.7732–2.5473 | 7.2560–13.3751 | 8.5865–14.7970 |
| Rewrite+ 动画标题 | 整改后 | 2.7156–2.9571 | 3.6447–3.9320 | 4.1662–4.5230 | 0.6887–0.7632 | 0.9328–1.3882 | 1.2560–1.7296 |
| Rewrite+ 开场演出 | 整改前 | 1.9144–2.1612 | 2.9625–4.2885 | 4.1659–5.4683 | 1.7247–2.4410 | 3.4778–7.3928 | 4.6558–12.0048 |
| Rewrite+ 开场演出 | 整改后 | 1.7556–1.7942 | 2.4856–2.5516 | 2.7758–2.9695 | 0.6870–0.7403 | 1.0418–1.4948 | 1.2849–1.8666 |
| 自制 FVP 静态对照 | 整改前 | 0.0846–0.0897 | 0.1224–0.1318 | 0.1858–0.2132 | 0.0022–0.0023 | 0.0030–0.0032 | 0.0040–0.0045 |
| 自制 FVP 静态对照 | 整改后 | 0.0925–0.0991 | 0.1639–0.2223 | 0.2161–0.3441 | 0.0025–0.0028 | 0.0040–0.0054 | 0.0055–0.0076 |

`acquire 完成` 包含 Pending 轮询、GPU map 完成、depad 和 ABI 返回，是 headless 消费边界耗时；GPU timestamp、窗口 upload/present 与设备音频欠载另测。整改前后各 15 组，三轮 Ready 数分别稳定为 70、835、1800、1799；静态对照采样窗无新 Ready。开场截图分别从黑场到正文、从月亮到光带，按演出分类。各轮退出 0，商业样本 NullAudio 有非零 PCM。

Siglus 两场景的 advance 和回读完成分位数均下降。FVP 两场景的回读完成 p99 下降，advance p50 上升；静态对照三轮配对的 advance p50 增量为 0.0028–0.0126 ms。当前数据支持回读长尾改善，未支持整体性能全面提升。测量开关开销仍未量化；原始计时、构建哈希与首尾截图保存在 ignored 工作区。

《樱花萌放》发行/补丁修订未确认。Rewrite+ 本轮选择原版 `Gameexe.dat` / `Scene.pck`；副本附带的 A·T Field 完整汉化补丁 V2 使用 `.chs` 文件。最新范围按用户决定取消原版性能测试，保留原生功能复现及 Hosted 前后数据。正式验收使用 Sandbox；主机数据单列为既有证据。

RFVP 元数据/hit-proxy 遍历与原 GPU rebuild 仍分别存在，列入测量，不为合成单遍另写 renderer。性能采样探针和临时音频设备选择已从产品代码移除，验收构建使用默认路径。

真实窗口暴露并修复了额外的 Host 调度问题：原 owner 只在逻辑 tick 后取帧，异步 Siglus 因而接近隔帧发布。一次 10 秒启动演出记录为 573 次 advance、282 次新图像上传；增加独立 1 ms 完成期限后，30 秒 Rewrite+ 动画标题记录为 1724 次 advance、1714 次上传。此对照验证交付调度，两个场景不用于计算性能收益。完成轮询和关闭仍在原 owner，恢复 Pending 不消耗新时间线。

该窗口使用真实 Realtek 双声道 48 kHz/F32 输出，稳定采样窗内队列空缺帧增量为零，正常退出。系统默认 G431 仅提供八声道，原转换器明确拒绝；测试通过临时、进程内的设备选择探针使用 Realtek，未改系统默认设置。实际端点 loopback 捕获仍需诊断，不以队列计数代替听音。

[PresentMon 2.6.0](https://github.com/GameTechDev/PresentMon/releases/tag/v2.6.0) 的一次 30 秒真实窗口记录包含 GPU 完成证据与 Present API CPU 时间；下表单位 ms。这是整个 Manager 进程（核心与 Host 合计），不是纯核心 GPU timestamp。采集保留未显示帧，不绕过权限或更改图形调度设置；后续轮次使用 QPC 校准，避免日志日期时区差异。

| 进程级指标 | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Present API CPU | 0.3731 | 1.4849 | 2.4126 |
| Present 至 GPU 完成 | 0.5103 | 1.7260 | 2.4516 |
| GPU Busy | 2.2796 | 3.4324 | 4.0091 |
| GPU Time（含等待） | 6.8634 | 14.0831 | 15.5593 |
| 两次 Present 间隔 | 14.3493 | 15.9857 | 19.9870 |


## 已有效的整改

| 范围 | 当前实现 | 保留的成本/边界 |
| --- | --- | --- |
| Runtime | 1024 条诊断历史有界且不入存档；延迟队列空闲跳过；worker/access plan 按 epoch 复用 | 不等于所有 tick 零分配 |
| Editor/VN authoring | 单 owner 后台编译、150 ms debounce、拒收旧 revision；差异 undo/redo 与字节预算 | 实际编译仍复制源码，编辑仍可构造完整文本 |
| 公共呈现 | surface/offscreen 共用滤镜，pipeline 按 kind/format 缓存，参数走 uniform | 每 filter pass 仍独立提交 |
| SDK/Musica/CMVS | 不可变纹理身份、持久上传/Sprite/释放，跟随当前场景驻留 | 保留真实资源预算；成熟核心无需采用 SDK |
| Family/Manager | epoch/generation、Ready/Pending/Unchanged、不可变 CPU owner、latest-frame mailbox、跳过重复上传 | GPU 回读、depad 与上传仍存在 |
| Host 输出 | 无滤镜直接输出；必要像素语义在 GPU 处理；PNG 按需转换 | Ignored alpha/行序转换是必要处理 |
| 音频 | 同格式 F32 owner 直通、非实时回收、有界队列；mixer cancel 与 session cancel 分离 | Siglus 借用 slice 转 owned PCM 仍复制 |
| 成熟核心 | FVP 原 GPU/mask/视频；Siglus 同一 draw-to-view/Kira；Artemis 原 Vulkan/FFmpeg 9 | 继续处理 R01–R08，保留原原生入口 |

删除第二 WMV 播放器、第二 mask、旧 V2 扩展/重复 codec、无消费者 Evidence/事务队列的成果应保留。独立 WMV 工具的 packet payload 修复单列为工具修复，不计作 FVP 适配收益。

## 已验证范围

### 当前整合代码检查

| 对象 | 通过 | 忽略 | 检查边界 |
| --- | ---: | ---: | --- |
| Engine | 871 | 22 | fmt、全目标严格 Clippy、build、完整 Cargo test |
| Editor | 18 | 3 | fmt、严格 Clippy、build、完整测试 |
| Emulator | 627 | 42 | 全部 workspace 包 fmt、全目标严格 Clippy、build、完整 Cargo test |

本轮三个 workspace 的 build、全目标严格 Clippy 和完整 Cargo test 均退出 0。Engine/Editor 完整 fmt 通过，Manager 的 include 模块按 Rust 2021 单独检查通过；Emulator 按全部 workspace 包名检查格式。额外 `fmt --all` 暴露上游 RFVP 未修改的 UEFI/音频模块格式差异，保留原文。硬件与外部 Agent 测试按各自前置条件忽略，产品验收范围见下节。

本轮 FVP 29 项全量回归通过；Siglus 6 项 lib 回归及 Rewrite+ 实际游戏重开通过；Manager 调度/窗口/音频生命周期 7 项通过。Artemis 普通 13 项及 GPU 2 项为此前记录。RFVP 原 default check/portable rlib 通过；原生全 lib tests 因未修改的 texture 测试缺 `Read` 导入出现 6 个 E0599。Siglus decoder 通过独立 lib 名消除非哈希产物冲突，四包联合 Release 与三个原生 CLI 已构建；自制 WMV2 解出五张 16×16 图像。

去除探针后的联合 Release 与四包全目标严格 Clippy 通过。最新默认 lib 测试为 50 通过、10 忽略，Manager 调度/窗口/音频生命周期过滤运行 7 通过；硬件项的既有显式运行结果见上段。新增原生测试和嵌入模块按各自 Rust edition 格式化；RFVP 既有大模块仍有 formatter 差异，未为本次适配整文件重排。

### 游戏与运行证据

| 对象 | 已观察 | 尚未覆盖 |
| --- | --- | --- |
| FVP《樱花萌放》 | 当前硬件 GPU headless 标题、真实 pointer 开始、剧情/正文、非零 PCM、正常关闭；修正元数据误授权像素回收导致的黑屏 | 结局、当前基线完整存读档链、设备听音、Sandbox 硬件流程 |
| Siglus Rewrite+ | 当前 GPU headless 开场/标题、非零 PCM、原生 global/config/read 写入、冷重开 | 当前基线完整槽位/菜单/路线、设备听音、GPU 警告归因 |
| Artemis《終ノ空remake -2025ver-》Windows | 当前非商业 Vulkan/FFmpeg 媒体与生命周期通过；商业 headless 因 typed 窗口请求明确拒绝 | 当前商业原生窗口流程；初始黑帧不计视觉通过 |
| Musica/CMVS | 共享纹理驻留与既有格式/VM/GPU 回归；Musica 历史短流程及存读档记录 | Musica 结局、CMVS 代表流程、两者当前产品整合和跨平台验收 |

保留两个具体接续点：CMVS 最近实际首 advance 在 command 548 / offset 200462 返回 ACTION_UNBOUND，尚无完整首 tick 证据。Musica `src/provider.rs` 仍暴露 logical/render 两组宽高（106–109、277–280 行），eden 导入涉及旧 1024×640；与固定 1280×720/四档 scale 目标尚未收敛。纹理驻留/alpha 修改没有解决这些语义边界，本次文档整理不改变配置或存档行为。

此前 Artemis 实际窗口发现标题读档文字空白，定位到 onLoad 等待绑定；当前上游独立复现及修复见下节。旧构建的窗口流程不自动成为新 HEAD 验收。

本次审查另运行实际 Manager/FVP 自制 HCB/NVSG 场景：120 次推进，Vulkan/DiscreteGpu/vendor 4318，640×480 输出中指定 4×2 区域全白、相邻像素全黑，退出 0。NullAudio 消费 100,320 帧、0 非零 samples，证明无声夹具路径。R01 的真实 InputManager 程序亦已执行。

### 资源与环境限制

- 短场景 private bytes 平稳不能排除泄露。Siglus 候选 D3D12 resource-state 错误及退出时 DXGI `Live Object : 18` 保留；尚未证明它随进程内重开增长。
- Sandbox 当前阻塞为环境启动崩溃。用户恢复策略后已实读 `AllowVGPU=1`，按原硬件配置重建；实例创建成功后从运行列表消失，Manager 与 guest 命令均未执行。同时间 WER 记录 `ContainerCrashDump`：bugcheck `0x3B`、异常 `0xC0000005`、RIP `0xFFFFF80479483152`，未取得可用 dump。宿主为 Windows 11 25H2 `26200.9711`、Sandbox `0.8.107.0`；[Microsoft Windows-Sandbox #131](https://github.com/microsoft/Windows-Sandbox/issues/131) 有相同构建与故障签名的报告，具体故障模块尚未定位。此前软件 GPU 预检由 `ASTRA_EMU_FVP_GPU_HARDWARE_REQUIRED` 明确拒绝、15 项部署哈希一致。当前窗口、输入、媒体、新槽存读档及退出仍未验收，主机结果单列。Agent 未修改安全策略、GPU 驱动或共享虚拟化服务。
- 设备听音、四平台、商业长流程和正式性能仍未验收。Editor `--mcp` 持有 stdin 时关闭挂起仍待整改。
- 许可证/covered-source 范围和其他机器的自包含运行库仍需发行前核验。库驻留至进程退出不是 session 资源泄露的豁免。

## 上游修复

Artemis [issue #4](https://github.com/Alphaly2K/art3m1s-core/issues/4)、[PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)：读档等待误绑 onLoad 辅助脚本。PR 提交 `ad20b8afd01ce2807510fa4e08629afc8e62345b`，直接父提交为 `ad90a62c`；只含 `src/runtime/save_io.rs` 的顺序修正/回归与 CHANGELOG。2026-10-16 远端核对为 open、未合并，当前产品 gitlink 不变。

相关游戏版本已按发行包 readme 核实为 **ケロQ《終ノ空remake -2025ver-》Windows 版**，启动文件 `tsuinosora_remake2025ver.exe`。EXE 无 ProductVersion/FileVersion，不补造补丁号。公开复现为自制脚本和原生 SaveData。

验证直接针对未带 Astra 适配的上游 HEAD：

- 修复前同一回归失败：`onload_helper=None`，期望 `Some(Int(1))`；修复后显式 Vulkan 回归 1 项通过，同时检查 helper 返回、保存 PC 等待及输入后继续。
- 独立 runtime executable 未注入点击，helper 原 `[exit]` 在第 2 tick 执行，正常释放 runtime。
- 所选 lib 测试 339 通过、2 忽略；修改文件 rustfmt、Clippy、CHANGELOG 检查通过，上游 warning 保留。
- 验证清单直接引用独占工作树源码，只启用 Vulkan/Lua 5.1；可选其他引擎桥未构建。临时清单、烟测和场景已清理，PR 分支/回归保留。

上述独立功能修复和 scratch 优化分支已获授权推送，完整产品与适配分支保持本地。不把适配黑屏/输入回归报成上游缺陷，不把 Siglus 警告报成已证明泄露。PR 合并后更新基线、剔除重复修复并重新整理单适配提交。
