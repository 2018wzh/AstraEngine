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

2026-10-16 只读核对：三个实时 upstream HEAD 与适配提交的唯一直接父提交一致；核心检出 clean，主仓 `160000` gitlink 与检出一致。以下适配对象为本地整合，尚未发布。

| 核心 | 上游 HEAD | 单个适配提交 | 差异规模 |
| --- | --- | --- | --- |
| RFVP 0.6.0 | `af557db750c7833274f22a6d82b0dc33cdea2f85` | `d1ff965d79ba2cdc956aad73ae100cc44bc7ce90` | 35 文件，+1533/−158 |
| Siglus 1.0.0 | `5f358ef1360b0439e92aed2b749e8f25d8721954` | `8bf5024fb3975bb8dc375874380f4574ecedf4d8` | 11 文件，+557/−67 |
| Artemis 0.4.0 | `ad90a62cb192b119b7cf8589aa7f87a94bd747c8` | `b802ab8eb397d12c623df793ad9feb8d3ae0b5a9` | 15 文件，+1358/−274 |

规模包含清单、lockfile、测试和说明，不代表纯性能代码量或修改必要性。逐核心差异见 [FVP](../../Emulator/Source/Families/astra-emu-fvp/MODIFICATIONS.md)、[Siglus](../../Emulator/Source/Families/astra-emu-siglus/MODIFICATIONS.md)、[Artemis](../../Emulator/Source/Families/astra-emu-artemis/MODIFICATIONS.md)。来源和已发布历史保持不变。

## 待整改问题

P1 为优先正确性/适配边界问题，P2 为整合缺口或待测成本；不是已测得的性能损失。

### R01 — P1：RFVP 原 no_std 输入提交回归

**位置：** `rfvp/crates/rfvp/src/no_std_core.rs:832–852,1195–1273`；`subsystem/resources/input_manager.rs:564–589`。以下核心相对路径均位于 `Emulator/ThirdParty/`。

适配删除 `apply_pending_events_to_game_data()` 末尾无条件 `begin_frame()`，只在 `#[cfg(feature="hosted")]` 下补回。纯 no_std 不再提交 pending/new 输入状态。

**证据：** 独立程序直接编译当前原 `InputManager`，启用 `feature="no_std"`。鼠标按下后输出 `state=0 down=0`，调用原 `begin_frame()` 后输出 `state=20 down=20`。结合条件编译，原便携入口缺少状态冻结；尚未运行完整 no_std 平台 VM。

**整改：** 恢复所有原 feature 路径每帧恰好一次的输入提交，覆盖事件入队、冻结、VM 消费和下一帧清边。这是适配回归，不报为上游缺陷。

### R02 — P1：RFVP 保存与协程语义混入嵌入提交

**位置：** `rfvp/crates/rfvp/src/subsystem/resources/graph_buff.rs:1284–1370`、`subsystem/components/syscalls/saveload.rs:72–77`、`vm_runner.rs:94–110,250–262,412–459`、`subsystem/resources/thread_wrapper.rs:80–89`。

当前差异包括 V1 修改像素/几何恢复、hosted SaveCreate 不立即 break、待捕获时提前返回，以及 TextResume 只唤醒 TEXT、Start/Exit 取消旧文字请求。它们改变保存时点或协程行为，部分同时改变非 hosted 路径。

**整改：** 逐项区分上游缺陷与嵌入时序适配。已复现的原生缺陷独立上游修复；适配造成的时序差异在适配内修正。不得为缩小 diff 恢复已知错误。V1 图像修复复用原字段，虽非旧档兼容，仍不是纯钩子。

### R03 — P1：Artemis 通用功能修复尚未独立上游化

**位置：** `art3m1s-core/crates/pf8/src/reader.rs:24`、`crates/art3m1s-media/src/ffmpeg.rs:136–152,703–715`、`src/runtime/save_io.rs:318–326`。

PF8 分隔符规范化、AVIO `av_freep` 参数及所有权、立体声 packed sample 读取、onLoad 等待绑定均是通用正确性修复。取消/join 和 fallible 输出入口另属嵌入生命周期边界。

**整改：** 纯缺陷按复现拆分上游贡献；不回退错误的 AVIO 释放方式。onLoad 已提交 [PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)，其余仍待分别归因和复现。

### R04 — P2：变化帧回读仍有等待与 CPU 分配

**位置：** `siglus_rs/crates/siglus_scene_vm/src/render/hosted.rs:55–104`、Siglus Family `src/session.rs:190–197,264–270`、Manager `manager_app/owner.rs:307–321`。

Siglus 实际链为 acquire_frame、原生 texture copy、`Maintain::Wait`、`recv`、新 Vec 和 depad。generation gate 已跳过无新绘制帧，GPU staging buffer/完成队列已复用；变化帧仍分配 CPU 像素，等待缺少应用级超时和取消。关闭需 owner 返回后处理命令并 join。[INFERENCE] GPU 受阻可能拖住退出；本轮未注入驱动挂起。

RFVP `rfvp_render/render_target.rs:64–93` 已有 Poll、5 秒期限和代次取消，但完成时仍创建 Vec。Artemis Family `session.rs:310–332` 已用三个 CPU 槽，无空闲槽返回 Pending。

**整改：** 分测等待、depad、分配成本；优先在 Family 复用有界 CPU 槽，核心只给 submit/poll/read-into 钩子。不能覆盖在用 lease，也不能把线程绑定 GPU 对象放入跨线程 lease。

### R05 — P2：RFVP 无消费者依赖与非嵌入改动

**位置：** `rfvp/crates/rfvp/Cargo.toml:33,173`、`vm_runner.rs:199–217,378–389`、`rendering/gpu_prim.rs:964,1011`、`host_api/fs.rs:54–69`。

新增 `sha2` 进入默认运行依赖，但整个 crate 只命中清单声明，无代码消费者。上下文/输入激活时每 opcode tracing 是额外插桩；visit scratch 改栈数组是通用 renderer 优化，1 MiB 分块是通用文件策略。

**整改：** 删除无消费者依赖、无关格式变化及无消费者热路径插桩。通用优化交上游，宿主文件策略能放 Family 就不改核心；不能用少量行数证明必要性。

### R06 — P2：Artemis 产品 GPU 策略影响上游默认行为

**位置：** `art3m1s-core/crates/art3m1s-render/src/backend/vulkan/mod.rs:699–703`。

`_ => 1` 改为仅接受 VirtualGpu、其余跳过，改变原生公共设备选择，而非只限定 Astra 嵌入。

**整改：** 保持上游默认行为，由显式嵌入配置/设备信息钩子承载 Astra 硬件要求。删除无收益的 `ffmpeg-next` 版本范围文字变化 `9.0.0 → 9.0`。

### R07 — P2：Artemis 成功恢复缺少 reset_clock

**位置：** Artemis Family `src/session.rs:336–398`、核心 `src/runtime/save_io.rs:287–392`、Manager `manager_app/session.rs:189–195`。

Family 仅在 suspend 切换返回 `reset_clock=true`，正常 pump 没有消费成功读档/重开始通知。[INFERENCE] 读档耗时会进入下次普通 elapsed；尚未测得对应动画跳变。

**整改：** 原生成功边界增加一次性通知，Family 清 elapsed remainder 并返回 reset_clock；失败不通知。属于必要生命周期钩子，不改存档格式或时间上限。

### R08 — P2：Family 原生日志在 Host 输出阶段整体脱敏

**位置：** Manager Core `src/family_diagnostics.rs`、`astra-observability/src/record.rs:32–72`、Family API `src/diagnostic_bridge.rs:120–138`。

桥已转发字符串/Debug，但 fields 被整体格式化后进入公共 sanitize；包含 URL/路径样式时整个字段变成 `[redacted]`。实际 Manager/FVP 启动的三条 wgpu 日志出现该值，`dropped_fields=0`。这是既有链路残留，不是本轮提交新引入。

**整改：** 在 Family 诊断专属输出边界保持有界 typed 内容、来源和超限计数；不要全局关闭其他产品的隐私规则。

### R09 — 验收缺口：原生/Hosted 性能基线未建立

过去 headless 整程耗时受原生实时时钟、场景阶段和截图影响，不是同场景 A/B。CPU lease 保留 owner，也不能证明帧时间开销可接受。

**整改：** 按[实施计划](implementation-plan.md#性能基线与最终验收)建立分项基线后再定阈值。RFVP 元数据/hit-proxy 遍历与原 GPU rebuild 仍分别存在，列入测量，不为合成单遍另写 renderer。

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
| Engine | 870 | 22 | fmt、全目标严格 Clippy、build、完整测试 |
| Editor | 18 | 3 | fmt、严格 Clippy、build、完整测试 |
| Emulator | 627 | 37 | fmt、严格 Clippy、build；全部 26 个已编译测试程序直接执行 |

Emulator 综合 Cargo test 包含缓存锁等待并超时；末尾 Musica 用例隔离通过，随后按原 cwd/原生运行库/单测试线程执行完整可执行测试集合。不能将原 Cargo 命令写成全绿或将最后一行认定为死锁。以上是代码基线的既有运行记录，不是本次文档修改重新运行。

FVP 普通 25 项及显式硬件 GPU 3 项通过；Siglus 普通 3 项及 GPU 1 项；Artemis 普通 13 项及 GPU 2 项。RFVP 原 default check/portable rlib 通过；原生全 lib tests 因未修改的 texture 测试缺 `Read` 导入出现 6 个 E0599，保持未通过。两个 decoder 的同名非哈希 lib 输出 collision 警告仍在，包版本区分没有解决原生产物命名。

### 游戏与运行证据

| 对象 | 已观察 | 尚未覆盖 |
| --- | --- | --- |
| FVP《樱花萌放》 | 当前硬件 GPU headless 标题、真实 pointer 开始、剧情/正文、非零 PCM、正常关闭；修正元数据误授权像素回收导致的黑屏 | 结局、当前基线完整存读档链、设备听音、原生性能对照 |
| Siglus Rewrite+ | 当前 GPU headless 开场/标题、非零 PCM、原生 global/config/read 写入、冷重开 | 当前基线完整槽位/菜单/路线、设备听音、GPU 警告归因 |
| Artemis《終ノ空remake -2025ver-》Windows | 当前非商业 Vulkan/FFmpeg 媒体与生命周期通过；商业 headless 因 typed 窗口请求明确拒绝 | 当前商业原生窗口流程；初始黑帧不计视觉通过 |
| Musica/CMVS | 共享纹理驻留与既有格式/VM/GPU 回归；Musica 历史短流程及存读档记录 | Musica 结局、CMVS 代表流程、两者当前产品整合和跨平台验收 |

保留两个具体接续点：CMVS 最近实际首 advance 在 command 548 / offset 200462 返回 ACTION_UNBOUND，尚无完整首 tick 证据。Musica `src/provider.rs` 仍暴露 logical/render 两组宽高（106–109、277–280 行），eden 导入涉及旧 1024×640；与固定 1280×720/四档 scale 目标尚未收敛。纹理驻留/alpha 修改没有解决这些语义边界，本次文档整理不改变配置或存档行为。

此前 Artemis 实际窗口发现标题读档文字空白，定位到 onLoad 等待绑定；当前上游独立复现及修复见下节。旧构建的窗口流程不自动成为新 HEAD 验收。

本次审查另运行实际 Manager/FVP 自制 HCB/NVSG 场景：120 次推进，Vulkan/DiscreteGpu/vendor 4318，640×480 输出中指定 4×2 区域全白、相邻像素全黑，退出 0。NullAudio 消费 100,320 帧、0 非零 samples，证明无声夹具路径。R01 的真实 InputManager 程序亦已执行。

### 资源与环境限制

- 短场景 private bytes 平稳不能排除泄露。Siglus 候选 D3D12 resource-state 错误及退出时 DXGI `Live Object : 18` 保留；尚未证明它随进程内重开增长。
- Sandbox 在启动产品前发生 WER P1=59/P2=0xc0000005；本轮已采用用户允许的硬件 GPU headless，未改安全策略或改用软件 GPU。
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

仅这一功能修复分支已推送。其他问题按实际归属和证据分别处理；不把适配黑屏/输入回归报成上游缺陷，不把 Siglus 警告报成已证明泄露。PR 合并后才更新基线、剔除下游重复修复并重新整理单适配提交。
