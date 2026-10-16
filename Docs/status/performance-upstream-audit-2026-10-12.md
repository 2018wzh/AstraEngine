# 全仓性能与 AstraEMU 最小上游适配审计（2026-10-12）

## 文档用途与状态

本文合并本次会话的两轮只读审计，供下一步修改共享运行层、AstraEMU Host、Family 和外部核心 fork 使用。第一轮覆盖全仓性能热点，第二轮深入核对 AstraEMU 接口与上游适配边界。

**初始审计记录和接口建议不是修复完成报告，也不是性能验收报告。** 审计阶段只写文档与索引，隔离实验产物已清理。随后获用户授权由主线程整改；实际完成项与验证范围见文末“整改进度”，未列为完成的提案仍不能作为产品能力。

本文不替代 [总体架构](../product/architecture.md)、[重构契约](../contracts/rebuild.md)、[实施状态](implementation-plan.md) 或 [Family API v13](../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)。第 1–9 节保留 v11 审计时的证据和候选接口；v12 CPU lease、像素语义和 owner 调度以及新增 v13 模态服务以现行契约及第 10 节为准。GPU/native handle 扩展仍未采纳，不能按旧提案视为现行能力。

### 证据等级

- **代码证据**：审计时实际读到的实现与调用链；能证明操作存在，不直接证明真实游戏耗时。
- **隔离实验**：执行了仓库外小程序；结论限于已说明的算法、分配或 ABI 所有权机制。
- **待测风险**：由实现推导出的帧时间、输入延迟、欠载或平台风险，尚无相应产品采样。
- **接口提案**：下一步候选方案，不表示调用方已迁移或跨平台验证已通过。

源码链接和行号用于定位审计时的实现；同一工作树正在发生其他修改，实施前应按符号重新核对。不要把本文当作当前树所有问题均仍未修复的证明，也不要为复现审计基线覆盖用户改动。

## 1. 审计范围与基线

### 1.1 覆盖范围

| 范围 | 本次重点 |
| --- | --- |
| 根 workspace / Engine / Runtime / VN | tick、事件与 FSM、snapshot/save、演出、Player 呈现、资源加载与预取 |
| 公共绘制、文字、媒体、音频、字节源 | GPU atlas、离屏回读、窗口呈现、滤镜、字体布局与驻留、PCM、解码与共享所有权 |
| Emulator workspace | Manager/Slint、Family API、SDK、FVP、Siglus、Artemis、Musica、CMVS；另查排除在活动 workspace 外的 KrKr 适配 |
| Editor workspace | 输入事件、全项目编译、文档事务、undo/redo、预览和面板更新 |
| 其他公共模块与工具 | package、policy、plugin、Cook、观测日志、翻译/元数据 provider、平台事件循环、转换/打包工具的热点检索与定向查阅 |

采用全仓检索加关键调用链精读，不是对每个第三方文件和所有平台分支逐行作无缺陷保证。没有运行商业游戏、真实设备音频、完整路线、四平台性能采样或全 workspace 测试。没有为了审计更新依赖、修复检出或重新验证历史产品记录。

### 1.2 主仓与外部核心基线

两轮审计开始时主仓为 `master`，HEAD 为 `113536a74c47c8846accf25b7825d4497995aaf6`。实际工作树含未提交改动，因此审计对象并不等于该提交的纯净树。以下检出值来自审计中的只读 Git 查询，不代表写档时重新切换或同步了核心。

| 核心 | 主仓固定 gitlink | 审计时本地检出 | 补充证据 |
| --- | --- | --- | --- |
| RFVP | `b2f385da486342de6e23202cccad8141ac6423b6` | `14b7f8a748b6c5eeed48afaed621274417ac530d` | 本地缺固定对象；第二轮从远端取得固定提交源码及 compare |
| Artemis | `6b59d145f5d64a5a543d0877459760b969e5f06d` | `327ec88b30aa605e5e7f9147e8678fea6ff41727` | 本地缺固定对象，远端 compare 返回 HTTP 404；最新改动另参考修改记录 |
| Siglus | `df036631f250bbd42213f07bcecb87357267b64f` | `2be01aee004a3cb9f5c72eac6a73532d125162c3` | 本地缺固定对象，远端 compare 返回 HTTP 404；最新改动另参考修改记录 |
| KrKr | 不作为上述三个已固定活动 submodule 的完成核心统计 | 当前目录未提供完整 NativeOwner 源码 | 只查到 Family/子进程/私有 ABI 消费者，不能据此完成原生 fork 差异审计 |

HTTP 404 只说明本次请求未能取得对象，不能单独证明对象从未存在或已被删除。Artemis、Siglus 修改记录中的最新验证结果没有被当作当前本地检出的实测结果。

只读依赖解析实际执行：

```sh
cargo metadata --offline --locked --format-version 1 --manifest-path Emulator/Cargo.toml
```

结果为 exit 101：`astra-emu-fvp` 要求 `rfvp/hosted-gpu`，当前本地 RFVP 检出不含该 feature。见 [FVP 依赖声明](../../Emulator/Source/Families/astra-emu-fvp/Cargo.toml#L18)。第二轮确认远端固定的 `b2f385d` 包含 `hosted-gpu`；因此这是本地检出错位造成的解析阻断，不能误写成固定提交本身缺少 GPU 入口。

未因该失败执行 reset、checkout、fetch 或覆盖其他工作树。后续实测前须在归属明确的独立检出中取得正确固定核心。

### 1.3 RFVP 参考的有效边界

固定上游为 `304e773387a9920c9db091ec1fd937c717aea949`。远端 [固定适配差异](https://github.com/2018wzh/rfvp/compare/304e773387a9920c9db091ec1fd937c717aea949...b2f385da486342de6e23202cccad8141ac6423b6) 显示 76 个文件、3 个提交；其中 `gpu_prim.rs` 未修改，`app.rs` 已修改。文件数只是核对适配范围的证据，不作为质量或优化目标。

参考价值在于 [GpuPrimRenderer](https://github.com/2018wzh/rfvp/blob/b2f385da486342de6e23202cccad8141ac6423b6/crates/rfvp/src/rendering/gpu_prim.rs)：按 generation 跳过不变纹理、同尺寸动态 RGBA 原位更新、按容量复用顶点缓冲。

必须纠正的旧判断：

- 主仓 FVP 已调用 `use_direct_surface()`，并使用原生 GPU capture；不能继续写“未启用 direct surface”或“正常显示仍纯软件合成”。
- 关闭 capture 不等于不再构建任何绘制命令；详见 E01。
- 正常 GPU 显示不等于所有图像路径均已统一；存档缩略图仍有软件重绘，详见 E02。
- 不能把“原生 GPU 文件未修改”推广为整个 GPU/桌面入口未修改；固定差异明确包含 `app.rs`、mask 管线与 readback 包装。
- 原生 swapchain 与 Hosted CPU 最终帧不是等成本路径，不能只比较 VM 推进频率宣称呈现性能相同。

## 2. 全仓发现

P1 表示优先处理的正确性、持续增长或核心热路径问题；P2 表示应计划处理的性能成本或条件性风险。严重度不是已经测得的帧率损失。

### R01 · P1 · Runtime 保留完整已消费历史，并带入存档

**证据**：[EventQueue::drain_ordered_for_step](../../Engine/Source/Runtime/astra-runtime/src/event.rs#L63) 将 `ready.clone()` 追加到 `trace`；[StateMachineStore::tick](../../Engine/Source/Runtime/astra-runtime/src/state_machine.rs#L296) 追加 action trace。[RuntimeWorld::snapshot](../../Engine/Source/Runtime/astra-runtime/src/world.rs#L1107) 克隆事件、FSM、presentation、mutations；[write_runtime_save_with_sections](../../Engine/Source/Runtime/astra-runtime/src/save.rs#L62) 序列化完整 snapshot。

**影响**：已消费事件与诊断历史随长流程增长，进一步增加保存时复制、编码与文件体积。隔离实验确认事件消费完毕后仍全部保留，见第 6 节。

**建议**：将有界诊断历史与恢复状态分离；保存仅保留恢复需要的权威状态、未消费事件和未完成任务。删除/调整序列化字段涉及内部存档契约，须显式迁移所有消费者，不能破坏真实游戏原生存档或把合法待读事件一起清除。

### R02 · P1 · Musica/CMVS 的 CPU 纹理缓存未转化为 GPU 驻留缓存

**证据**：[Musica Scene::layer_with_blend](../../Emulator/Source/Families/astra-emu-musica/src/scene.rs#L246) 和 [CmvsScene::render](../../Emulator/Source/Families/astra-emu-cmvs/src/scene.rs#L179) 生成临时 `SceneCommand::Texture`。[glyph_atlas.rs](../../Engine/Source/Platform/astra-platform-common/src/glyph_atlas.rs#L1267) 为其创建临时资源；提交只保留持久资源 mutation，下一次临时纹理又进入 atlas 更新。

**影响**：动画引发重绘时，未变化的背景/立绘也可能整图重传。Musica 已有 dirty gate，不能误称静止等待时它必然每 tick 重绘；问题发生在实际重绘路径。

**建议**：复用现有 `UploadTexture`、资源引用绘制、`UpdateTextureRegion` 与 `ReleaseResource`，按资源身份/代次驱动上传。保留缓存淘汰、恢复候选、设备丢失和失败回收边界，不建立第二套 renderer。参考 RFVP generation cache，而不是像素逐帧 hash。

### R03 · P1 · 原生窗口呈现静默忽略 FilterGraph

**证据**：[WgpuPresentationCore::present_scene](../../Engine/Source/Platform/astra-platform-common/src/presentation.rs#L321) 直接呈现 glyph renderer 输出；[glyph_atlas 的 FilterGraph 分支](../../Engine/Source/Platform/astra-platform-common/src/glyph_atlas.rs#L1400) 明确将滤镜留给 offscreen owner，而窗口路径没有执行该阶段。

**影响**：窗口与 Headless/离屏的画面能力不一致。代码链已确认，尚未做真实 GPU 窗口复现。

**建议**：窗口与离屏共享实际滤镜执行阶段，不复制 shader 实现，不静默忽略已接受命令。用相同场景分别检查窗口和离屏实际图像；已有 Headless 滤镜通过不能代替桌面验收。

### R04 · P2 · Manager 每次推进均分配、校验、复制并发布整帧

**证据**：[ActiveFamilySession::advance](../../Emulator/Source/Manager/astra-emu-manager/src/manager_app/session.rs#L153) 无条件 `capture_frame()`；[FrameMailbox::publish](../../Emulator/Source/Manager/astra-emu-manager/src/stage_frame.rs#L96) 新建零填充缓冲，逐行检查 alpha、复制像素并增加 generation；[ManagerStageRenderer::render](../../Emulator/Source/Manager/astra-emu-manager/src/stage_renderer.rs#L256) 只能跳过没有新 mailbox generation 的重绘。

**影响**：相同像素也被当作新帧，静止页面仍支付 CPU 交接与 GPU 上传。隔离实验数据见第 6 节。

**建议**：增加明确帧变化/代次契约，复用有界存储；进一步用不可变 FrameLease 消除 Family→Host 像素复制。不要以取消合法性检查或整帧 hash 代替变更通知。ABI 变更须同步重建全部插件和消费者。

### R05 · P2 · FVP 影片期间的游戏画面 capture 被完整覆盖

**证据**：[FvpSession::render_frame](../../Emulator/Source/Families/astra-emu-fvp/src/provider.rs#L320) 先 GPU capture，再将影片帧 `copy_from_slice` 覆盖到相同缓冲。

**影响**：影片覆盖期间，该次游戏画面绘制/回读对最终显示无贡献。尚无该分支独立 GPU 耗时采样。

**建议**：保留 VM、音频、影片完成和必要内部捕获语义，跳过只为最终输出服务的无用场景 capture；影片结束后的首次场景输出必须重新取得正确画面。不要跳过原生推进或伪造影片完成。

### R06 · P2 · 滤镜参数变化触发 shader/pipeline 变体增长

**证据**：[offscreen::apply_filter](../../Engine/Source/Platform/astra-platform-common/src/offscreen.rs#L871) 将参数写入 WGSL 表达式并参与签名，[pipeline 创建与插入](../../Engine/Source/Platform/astra-platform-common/src/offscreen.rs#L931) 按该签名缓存，未设置淘汰。

**触发条件**：调用方连续改变参数；不是声明当前所有 VN 默认效果均已逐帧改变参数。

**建议**：参数走 uniform，pipeline 按滤镜结构和必要格式状态缓存，避免数值变化导致重编译。用连续参数变化验证 pipeline 数量稳定及画面连续性。

### R07 · P2 · Editor 每次输入同步编译全项目

**证据**：[InputEvent::Change](../../Editor/Source/astra-editor/src/main.rs#L104) 调用 `diagnose()`；[diagnose](../../Editor/Source/astra-editor/src/main.rs#L260) 同步 compile；[AuthoringWorkspace::compile](../../Engine/Source/Modules/AstraVN/astra-vn-editor/src/document.rs#L88) 克隆全部文档文本。

**影响**：输入路径成本随项目规模增长，工作在 UI 线程。未进行 Editor 真实键入延迟测量。

**建议**：按文档版本复用解析结果；诊断去抖后后台执行，拒绝过期版本结果。编辑事务、图/时间线与 `.astra` 源码仍由同一 owner 管理，不另建第二份创作状态。

### R08 · P2 · Editor 撤销历史按完整文本计费，只有条数上限

**证据**：[AuthoringWorkspace::apply](../../Engine/Source/Modules/AstraVN/astra-vn-editor/src/document.rs#L118) 对每个改动文档保存完整旧 snapshot；历史最多 100 条，但没有总字节预算。单字符修改也进入此路径。

**建议**：使用逆向编辑记录或共享文本结构，增加总历史字节预算，保留多文档原子性、版本递增、冲突和 undo/redo 行为。隔离实验验证了历史文本增长，见第 6 节。

### R09 · P2 · 延迟事件队列在无到期 tick 仍重建全部 pending

**证据**：[DelayedEventQueue::drain_due](../../Engine/Source/Runtime/astra-runtime/src/delayed_event.rs#L45) 每次排序、drain 全队列并新建 pending Vec。

**建议**：按到期时间组织队列，无事件到期时不扫描、搬移全体；保持 `(due_tick, sequence, id)` 的稳定顺序、取消与保存恢复语义。不能简单换容器后改变同 tick 事件顺序。隔离分配观察见第 6 节。

### R10 · P2 · FSM 多 worker 路径每 wave 创建系统线程

**证据**：[StateMachineStore::tick](../../Engine/Source/Runtime/astra-runtime/src/state_machine.rs#L247) 重算冲突 wave；[execute_parallel_wave](../../Engine/Source/Runtime/astra-runtime/src/state_machine.rs#L485) 使用 `thread::scope/spawn`，每批 join。

**触发条件**：worker 数大于 1，且 wave 含多个可并行 machine；默认单 worker 不能被描述为也创建这些线程。

**建议**：复用 session/host 所有的有界线程池，缓存不可变定义的访问计划，小批量走串行。保留 action 访问冲突、提交顺序、错误边界和任务取消，不增加全 World 克隆。

### R11 · P2 · Manager 音频设备回调释放 PCM Vec

**证据**：[DeviceConsumer::next](../../Emulator/Source/Manager/astra-emu-manager/src/audio_executor.rs#L448) 在替换或清空 `current` 时析构旧 Vec。

**影响**：堆释放进入实时线程，存在尾延迟/欠载风险；本次没有设备欠载测量，不宣称已复现爆音。

**建议**：参考 [Engine stream 的回收/retirement 队列](../../Engine/Source/Runtime/astra-audio-kira/src/stream.rs#L206)，将缓冲释放和回收移到非设备线程，保持队列有界；设备回调仍不得调用 Family。

### R12 · P2 · 无滤镜路径仍整纹理复制并 submit

**证据**：[FilterEngine::apply](../../Emulator/Source/Manager/astra-emu-manager/src/effects/engine.rs#L191) 在 `FilterPreset::None` 时执行 `copy_texture_to_texture` 和 `queue.submit`。

**建议**：无滤镜直接把输入纹理提供给 Slint；滤镜启用、尺寸变化、恢复和关闭时重新绑定正确输出。不能让旧 texture handle 或 viewport 延续到新会话。

## 3. AstraEMU 深入发现与边界

### E01 · RFVP 正常显示仍有两次场景遍历

固定提交的 `tick → render_game_frame → render_motion_to_host` 先生成绘制命令和 `hit_proxies`；关闭完整 capture 后，RecordingRenderer 仍构建 `DirectFrameCommand`、比较前后帧并计算 damage。随后 Family 调用 `HostedGpuRenderer::render → GpuPrimRenderer::rebuild` 再遍历一次，且没有使用 delta 的 `visual_changed/visual_damage`。

证据：[固定核心推进](https://github.com/2018wzh/rfvp/blob/b2f385da486342de6e23202cccad8141ac6423b6/crates/rfvp/src/no_std_core.rs)、[Recorder](https://github.com/2018wzh/rfvp/blob/b2f385da486342de6e23202cccad8141ac6423b6/crates/rfvp/src/hosted.rs)、[GPU 包装](https://github.com/2018wzh/rfvp/blob/b2f385da486342de6e23202cccad8141ac6423b6/crates/rfvp/src/rfvp_render/hosted.rs)。

先消费已有变更信息；对影片、load、尺寸或输出目标变化另作完整失效处理。**不得直接删除第一遍，它还产生命中数据。** 只有 profiling 证明剩余遍历值得改时，才提取共用构建结果；不要增加第三套 dirty/scene 系统，也不删其他上游平台所需的 portable renderer。

### E02 · RFVP 存档缩略图使用完整软件重绘

固定提交的 `persist_hosted_save → render_hosted_save_thumbnail` 创建舞台大小 SoftRenderer，绘制后缩小，而普通输出走原生 GPU。见 [no_std_core.rs:1214](https://github.com/2018wzh/rfvp/blob/b2f385da486342de6e23202cccad8141ac6423b6/crates/rfvp/src/no_std_core.rs#L1214)。

建议复用原生 GPU 的正确保存捕获点。不能拿上一张 Host 帧顶替，不能把准备缩略图推迟到 VM 已继续执行之后，也不能取消 `SaveCreate/SaveWrite` 之间已有的准备状态。这里需要一个小而明确的捕获生命周期边界，不需要新的保存格式、软件 renderer 或快照复制协议。

### E03 · RFVP GPU 包装把提交、分配、等待和 CPU 输出绑定

[HostedGpuRenderer::render](https://github.com/2018wzh/rfvp/blob/b2f385da486342de6e23202cccad8141ac6423b6/crates/rfvp/src/rfvp_render/hosted.rs) 每次创建 readback buffer，并立刻等待结果；[try_map_to_rgba8](https://github.com/2018wzh/rfvp/blob/b2f385da486342de6e23202cccad8141ac6423b6/crates/rfvp/src/rfvp_render/render_target.rs) 使用 `Poll + sleep(1ms)` 循环以及有界超时。

建议拆分原生提交和捕获完成、复用 readback 存储，让 Family 在 owner 线程管理 pending。保留超时和错误，不以删除超时、无限忙等或扩大 elapsed 上限掩盖问题。Host worker 只能把等待移出 UI，不能凭此消除 GPU 同步成本。

### E04 · Artemis 已有跳过/原生呈现能力，但零返回值含义混合

现有 [CoreRuntime](../../Emulator/ThirdParty/art3m1s-core/src/runtime/mod.rs#L384) 有 `advance_and_render_into`、`advance_without_render`、`set_native_surface`、`advance_and_present`；[render_current_frame](../../Emulator/ThirdParty/art3m1s-core/src/runtime/render.rs#L20) 已处理无变化和 damage。

问题是 `advance_and_render_into` 的 `0` 同时可能来自无变化、缓冲不足或 [readback 失败](../../Emulator/ThirdParty/art3m1s-core/src/runtime/render.rs#L166)。[Family pump](../../Emulator/Source/Families/astra-emu-artemis/src/session.rs#L250) 返回字节数，但普通 advance 不据此报告帧变化。

最小上游调整应是明确区分无变化、成功更新和失败的结果，继续调用已有实现。不能把所有 `0` 都映射成 `Unchanged`，也不能通过分析 warning 文本构造成功/失败协议。

Family 应选择核心已有的平台后端，避免把当前 [NativeVulkan 选择](../../Emulator/Source/Families/astra-emu-artemis/src/session.rs#L201) 推广成所有平台的完成方案；本次没有验证 Metal/Android 等实际运行。

### E05 · Siglus PCM 量化往返和 Host 同格式重排均可在适配层消除

[Siglus callback](../../Emulator/Source/Families/astra-emu-siglus/src/audio.rs#L93) 将 Kira F32 转 I16，[Host](../../Emulator/Source/Manager/astra-emu-manager/src/audio_conversion.rs#L233) 再转回 F32。现有 [PcmChunk](../../Emulator/Source/FamilyApi/astra-emu-family-api/src/audio.rs#L40) 已支持 F32，这项不需改上游或公共 ABI。

[AudioConverter::push](../../Emulator/Source/Manager/astra-emu-manager/src/audio_conversion.rs#L63) 即使采样率和声道数都相同，也先拆 planar 再交织。应提供验证后的同格式直通，保留原有块 owner，只有设备需要时才转换。仅将外来 `RVec` 改成 `into_vec` 并不能保证跨 DLL 零复制，见第 6 节。

### E06 · Host 同步 UI 执行与统一 60 Hz 耦合

[Slint timer](../../Emulator/Source/Manager/astra-emu-manager/src/host.rs#L301) 直接执行 Family 推进；[Manager 固定期限](../../Emulator/Source/Manager/astra-emu-manager/src/manager_app/mod.rs#L40) 使用 `16_666_667ns`。核心 GPU 回读、资源加载和原生存读档会阻塞界面；不同核心没有声明下一次唤醒的入口。

建议先改 Host 的资源所有权和消息调度：在符合核心平台要求的固定 owner 线程创建、使用、关闭 provider/session；UI 只收发有界 typed 消息及帧引用。现有 [FamilySession](../../Emulator/Source/FamilyApi/astra-emu-family-api/src/lifecycle.rs#L312) 没有 Send 要求，不应补 `unsafe Send` 后移动已创建的 VM/GPU 对象。需要主线程的原生窗口工作仍在窗口线程协调。

不要为了 Host 节拍统一而再复制 VM 调度或强迫全部核心使用虚拟时钟。受控时间只在真实消费者要求时保留明确模式；不得随意删除当前 load/pause/reset 所依赖的时序边界。`Waiting` 可能仍有动画、语音、计时等待，不能据此停止 tick。

### E07 · 最终帧 alpha/布局约束造成额外 CPU 整帧处理

[FrameInfo](../../Emulator/Source/FamilyApi/astra-emu-family-api/src/frame.rs#L10) 仅声明 opaque RGBA；[Siglus pull_frame](../../Emulator/Source/Families/astra-emu-siglus/src/session.rs#L194) 保留原生 RGB 后逐像素写 alpha=255，Host 又检查 opaque。

建议显式表达 RGBA/BGRA、行方向与 alpha 是否用于呈现。原生目标中不参与最终显示的 alpha 可以在 Host GPU 呈现阶段明确忽略，不要求核心或 Family 每帧改写全部 alpha。这是契约语义调整，不是默许违反 `Opaque` 声明的数据；选择严格 opaque 的路径仍必须验证。逻辑舞台、资产密度、raster、窗口输出四空间继续分离。

### E08 · Mixer 取消不能等价于 session AudioSink 取消

[Siglus PcmBridge](../../Emulator/Source/Families/astra-emu-siglus/src/audio.rs#L80) 为每个 mixer 创建取消域并通过有界转发队列隔离；[回归用例](../../Emulator/Source/Families/astra-emu-siglus/src/audio_tests.rs#L69) 说明场景替换可能先创建新 mixer 再销毁旧 mixer。

若调整公共音频接口，应区分当前 write/mixer 取消与整个 session 关闭。不能为减少一条转发线程而恢复“旧 mixer drop 取消全局 sink”的错误。accepted/played 水位、已经接纳的 PCM、取消中的块和新时间线应有清晰归属，不能把丢弃数据报告为播放完成。关闭错误也必须继续取消和 join 所有 worker。

FVP 已有 [混音控制与 PCM 写入分离](../../Emulator/Source/Families/astra-emu-fvp/src/audio_worker.rs#L4)，不要重复报告其已修复的“Host PCM 背压阻塞全部控制命令”作为当前根因，也不要在优化中重新合并该阻塞边界。

### E09 · KrKr 原生必要服务缺口不能通过改游戏语义规避

[KrKr 当前说明](../../Emulator/Source/Families/astra-emu-krkr/README.md#L65) 明确嵌入式原生模态请求失败；本地没有完整 NativeOwner 源码可确认最小原生实现差异。

如下一步补宿主服务，应采用有界 typed 请求/响应、请求 ID 与 session 代次，由窗口线程完成；保留原生等待和取消语义。不得替游戏选择默认答案、跳过提示框、强制唤醒 VM 或修改脚本变量。Headless 无相应服务时仍明确拒绝。原生同步调用若需要等待 UI，应由正确 owner 边界协调，不能让 UI 线程阻塞等待自身响应。

## 4. 下一版 Family 接口建议

### 4.1 优先调整的边界

| 边界 | 提案 | 安全与性能不变量 |
| --- | --- | --- |
| 帧状态 | 区分 `Unchanged / Pending / Ready`，携带 session epoch 与 frame generation | 未完成不能冒充无变化；首帧、resize、恢复和新输出目标必须完整失效 |
| CPU 帧交付 | 不可变、可共享的 FrameLease，替代只能在同步回调内借用的最终帧交接 | 像素引用在 Host 使用期间有效；纯 CPU owner，不把 VM/GPU/原生句柄藏在 lease 内 |
| 存储适配 | Family 包装原生 Vec、Arc 或共享像素 owner | 不强迫上游依赖 Astra 类型；禁止为统一容器增加整帧复制 |
| 调度 | 下一次唤醒期限/事件唤醒，推进、取帧与 UI 呈现解耦 | 不丢输入或逻辑步，不以显示降频改变 VM、音频和存档语义 |
| 像素描述 | 明确格式、行方向、颜色与 alpha 呈现语义 | 保留 stride、长度、尺寸和像素语义验证；不能混淆 encoded/linear 混合 |
| 音频 | 原生 PCM 格式直通，取消作用域可区分 mixer/write/session | 设备归 Host；有界背压；回调不调用 Family；关闭取消并 join |

不必将提案中的概念都扩展成独立 crate、通用渲染 ABI 或第二层消息序列化。优先复用现有 abi_stable、typed DTO、共享字节源和 Host 队列。

### 4.2 FrameLease 所有权约束

1. 上游继续返回自己的 Vec、共享缓冲或写入已有 caller-owned buffer；Family 负责 ABI 包装。
2. Host 从获取到上传完成一直保留 ABI owner，不能转回普通 `Vec/Arc` 后假定仍为零复制。
3. 有界双/多缓冲只能在无消费者持有时再次写入；耗尽时明确背压或保留最近完成帧，不能复写仍被读取的内存或无限分配。
4. `RArc<RVec<u8>>` 是已隔离验证的一种实现，不是要求 Musica 的 `Arc<[u8]>` 或其他上游改成该类型。应保留各自像素分配并提供统一不可变访问。
5. CPU lease 不得依赖已关闭 session 的可变对象或线程绑定资源。库驻留保证析构函数仍有效；close 仍停止生产、取消请求并回收所有 session worker，Host 清理残留帧引用。
6. 该机制只消除 Family→Host 的额外复制，不消除原生 readback、必要的行格式处理或 GPU 上传。

### 4.3 Damage、丢帧和恢复

先实现完整帧与无变化，不强制所有核心新增 damage。已有原生 damage 的核心可以作为后续增强。

若增加局部更新，须带 `base_generation`。Host 跳过 B、从 A 直接显示 C 时，C 相对 B 的 rectangle 不能直接覆盖 A；必须合并未消费损伤，或从 C 的完整像素上传全帧。首次输出、尺寸/格式变化、设备重建和 epoch 变化同样需要完整基线。

负面验证必须包括：多个待呈现帧被覆盖、消费者降频、旧 GPU 完成晚到、读档后 generation 重启、失败后仍保留正确的已提交画面。不能通过逐帧整图 hash 推测变化。

### 4.4 GPU 快速路径的决策边界

**CPU-only 最终帧不能保证在任意分辨率持续动画下达到原生 swapchain 性能。** owner 线程减少 UI 阻塞，lease 减少 CPU 交接，二者均不消除 GPU→CPU→GPU 往返。

| 方案 | 收益 | 代价与本次建议 |
| --- | --- | --- |
| CPU FrameLease + 按需捕获 | 保留现有便携边界，直接减少重复分配/复制/上传 | 仍有 GPU 往返；建议先实现并实测 |
| 可选原生 surface 呈现 | 对已有原生呈现入口的核心可能显著减少最终帧搬运 | 需改变当前契约，并解决 Slint 覆盖层、滤镜、窗口/input 与四平台生命周期；不是当前已接受实现 |
| 跨后端共享 GPU 纹理 | 可保留 Host GPU 合成并避免 CPU 回读 | 需同物理 adapter、外部内存与同步、格式/所有权/设备丢失协议，侵入性和平台成本高；不能用裸 handle 冒充完成 |

RFVP 使用 wgpu 0.19，Siglus 使用 0.20，Manager 使用 29，Artemis 使用原生 Vulkan/Metal/GL 等后端。**不跨 ABI 传 Rust `wgpu::Texture`，不为统一 Host 强行升级全部上游 wgpu，也不另写一套核心 renderer。** 若实测表明 CPU 往返无法满足目标，再单独决定是否放开 CPU-only 契约；不能静默取消滤镜、覆盖层或其他平台能力换取局部快速路径。

## 5. 最小上游修改与逐核心落点

| 核心 | 应留在 AstraEMU/Family 的工作 | 上游修改边界 |
| --- | --- | --- |
| RFVP | 帧 owner、变更消费、Host 调度、音频桥、影片输出选择 | 必要 GPU 提交/捕获与保存捕获生命周期；保留命中和原生等待，测量后才考虑共享遍历 |
| Siglus | F32 直通、CPU 帧 lease、owner 线程、独立取消域 | 既有 renderer 输出与必要生命周期通知；不再复制或分叉一套 proc 调度 |
| Artemis | 消费已有 dirty/logic-only 能力、已有平台后端、Family mixer 与服务适配 | 明确 fallible 帧结果；继续复用原生 GPU/视频，不重写媒体管线 |
| KrKr | 保留进程隔离；优化私有传输、父端接收缓冲、帧 lease 和宿主服务 | NativeOwner/PCM/生命周期必要入口；完整源码不可得前不承诺精确 patch 范围 |
| Musica/CMVS | 在 Family/SDK 修复驻留、共享字节与回读，保留各自 VM/save 语义 | 不能用两者的 SDK 需求强迫成熟外部核心采用 SDK |

### 5.1 不应为了减小 diff 删除的内容

RFVP 固定差异包含存档版本、遮罩、文字等待、输入生命周期等修复；它们不是 FrameLease 或 Host 调度迁移本身。不得为实现“一个小提交”的外观而删去已经依赖的功能、恢复旧 reader 或改变真实原生存档。

按来源和目的区分：

- **必要适配**：嵌入入口、最终输出、PCM tap、错误/取消/关闭、最小可诊断通知。
- **核心自身修复**：保留回归依据和原生语义说明，整理上游 issue/修复归属，不混称 Host 接口改动。
- **不再需要的产品耦合**：确认所有消费者后移回 Family 或删除；不要误删其他上游平台依赖。

继续遵循完整 fork、固定上游基线、独立分支整理、精确 gitlink 和 MODIFICATIONS.md。不要重写来源/已发布历史，不用格式化噪声、告警屏蔽或大版本升级扩大适配提交。

### 5.2 已有能力应保留，而非重复实现

- `OwnedByteBuffer`/像素 owner 的 clone 是共享所有权，不应把所有 `.clone()` 都当成像素深拷贝。见 [owned_buffer.rs](../../Engine/Source/Runtime/astra-byte-source/src/owned_buffer.rs)。
- 公共 GPU 已有持久资源、atlas 和可复用顶点缓冲；修复消费者，避免另建并行资源协议。
- 文字已有共享布局与 glyph 生命周期；不以矩形或字符宽度估计代替 shaping/fallback。
- NativeVN 已有 presentation tick gate、UI 帧复用与文字揭示检查；不要退回逐帧整 ViewModel 序列化或只按逻辑步复用文字帧。
- FVP 已分开 PCM 写入与混音控制；Siglus 已有 mixer 取消域；这些是应保留的生命周期边界，不是可直接删除的“多余线程”。
- 可选翻译、Headless 观察与正式设备音频保持各自边界；不让可选增强或强制加速 Headless 迫使所有核心改写原生时钟和调度。

### 5.3 整改执行约束（用户再次确认最小上游修改）

- FrameLease、代次、Host owner 调度、CPU buffer pool、像素呈现、缓存与音频桥留在 Family/SDK/Manager；成熟核心不增加 Astra 类型、RuntimeWorld 或 SDK 依赖。
- 先复用核心已有 GPU/媒体/平台 feature。新增核心入口必须有实际 Family 消费者，且直接复用同一原生执行/绘制实现；不新增执行器、renderer、模型转发协议或无消费者接口。
- Artemis 已将新增接口收敛为原生 GPU 推进与独立 CPU 捕获两个 fallible 入口；删除本轮额外引入但 Family 不需要的 FrameOutput 和另一套 render_into 入口。帧池、lease、Pending/Unchanged 以及 Vulkan/Metal 的 feature 选择均在 Family，不下沉到核心。
- 缺失适配机制的恢复、核心自身缺陷修复和本轮性能适配分别记录来源、必要性与回归。AVIO 生命周期、原生加载等待等修复不能混称接口性能改动；若不影响接入/安全/已确认语义，不借整改扩大修复范围。
- 最终核对以固定上游基线、独占分支差异、真实消费者与运行回归为依据，不以文件数少替代正确性，也不删除已经依赖的原生机制来制造“小 diff”。


## 6. 本次实际验证

### 6.1 队列、文档历史和 mailbox 的隔离执行

使用仓库外临时 Rust 可执行程序，执行相关原始方法体。为独立编译去掉序列化/ABI 元数据并补最小外围类型，分配计数由隔离程序的 allocator 记录。**这些数字不是完整产品进程 RSS、每秒内存带宽、GPU 时间或 FPS，也不保证与完整 crate 的类型布局成本完全相同。**

| 场景 | 观察结果 |
| --- | --- |
| EventQueue 消费 10,000 个事件 | pending=0，retained trace=10,000 |
| 10,000 个远期事件，60 次无到期推进 | 840 次分配，累计申请 246,320,640 bytes（234.91 MiB） |
| 1 MiB 文档，100 次单字符修改 | 历史文本 104,857,600 bytes；undo/redo 结果正确，不含额外元数据 |
| 1280×720 相同帧发布 60 次 | 120 次分配，累计申请 221,186,400 bytes（210.94 MiB） |
| 1920×1080 相同帧发布 60 次 | 120 次分配，累计申请 497,666,400 bytes（474.61 MiB） |
| 3840×2160 相同帧发布 60 次 | 120 次分配，累计申请 1,990,658,400 bytes（1,898.44 MiB） |

Mailbox 场景先发布基线，再发布 60 次相同帧：generation 最终为 61，前后像素相等但分配不相同。累计申请量不等于同时驻留内存；若推算实际每秒成本，必须另给出真实发布频率，不能直接用这张表代替产品采样。

### 6.2 真正跨 DLL 的 CPU 帧所有权实验

仓库外独立 Cargo 项目使用 `abi_stable 0.11.3`、`libloading 0.8.9`，构建一个 cdylib producer 和一个 consumer，离线 Release 构建与实际运行成功。生产端创建 1920×1080 RGBA 缓冲，以 `RArc<RVec<u8>>` 交付。

实际观察：

- Vec 包装、跨 DLL 返回及共享 handoff 均保持原像素地址。
- 60 次共享交接没有新增生产端分配；未将其扩大宣称为整个 Host 的零分配。
- 共享时 `RArc::get_mut` 拒绝修改；独占时允许修改。
- 生产端原引用释放后，另一线程仍可读取完整像素；最终释放后生产端 live bytes 回到基线。
- 另一个外来 RVec 转为普通 Vec 时地址变化，实测发生复制。
- DLL 在所有 owner 析构后才释放；没有测试卸载仍被引用的库，也没有以危险卸载作为目标。

这证明一种候选 CPU lease 的机制可行，不等于真实 Family ABI、readback ring、缓冲池耗尽、设备丢失或四平台产品已验证。[RVec::into_vec](https://docs.rs/abi_stable/0.11.3/abi_stable/std_types/struct.RVec.html#method.into_vec) 和 [RArc::into_arc](https://docs.rs/abi_stable/0.11.3/abi_stable/std_types/struct.RArc.html#method.into_arc) 的跨库转换规则必须纳入实现审查。

### 6.3 PCM 精度与帧代次模型

用 Rust 执行 Siglus 与 Host 的实际转换表达式：输入 `-1.0` 经 I16 往返得到约 `-0.99996948`；`±0.00001` 被量化为零。这里只证明量化损失，没有进行可听音质评价。

独立状态模型验证了：跳过 generation 2 后，仅应用 generation 3 相对 generation 2 的单独 damage 会得到错误画面；检测 base 不匹配后采用完整帧可恢复正确结果。另验证旧 session epoch 的晚到帧即使 generation 较高也必须拒绝。该模型不是 GPU 图像测试。

### 6.4 未运行与证据限制

- 没有运行真实 GPU 游戏场景、显示端帧时间、端到端输入延迟、可听声音或正式四平台性能验收。
- 没有以静态代码或上述隔离数字承诺 1440p120、1080p60 或原生 swapchain 等效性能。
- Emulator 依赖解析受本地核心检出错位阻断；没有把失败改写成通过，也没有重复运行它来确认已知错误。
- rust-analyzer 引用查询曾因服务退出失败；后续使用源码与明确调用链核对，没有声称完成全量语义引用分析。
- 临时算法程序、ABI producer/consumer、PCM 程序和独立 target 均已删除，没有留下测试 scaffold 或向仓库写入商业数据。

## 7. 下一步实施顺序

以下顺序是建议，不是已经执行的迁移。

1. **建立可重现基线**：取得正确固定核心，在独占检出/target 中记录版本、GPU、后端、窗口显示频率和音频设备；原生核心与 AstraEMU 使用相同场景和分辨率。
2. **修正 Host owner 边界**：从 Slint UI 回调移出允许迁移的核心同步工作；保证构造、调用、关闭线程一致，保持原生必须的主线程工作及输入顺序。
3. **先更新共享接口**：定版帧状态、epoch/generation、不可变 lease、调度与取消语义；先验证普通 CPU owner，不夹带 GPU handle 扩展。全量迁移随包插件和 Host，不保留旧 ABI shim。
4. **先做上游零修改的收益**：F32 直通、Host 同格式音频快路、无滤镜直用纹理、利用已有 dirty 信息、Host 帧 owner、设备回调回收，以及 Musica/CMVS 持久 GPU 资源。
5. **收紧必要上游修改**：RFVP 捕获提交/完成与保存捕获；Artemis 明确结果；其余核心只补实际缺失的生命周期/通知。不改 VM、格式、混音语义来迁就统一 Host。
6. **并行范围外的独立修复批次**：R01、R03、R06–R10 按真实消费者分别实施，不塞进外部核心适配提交；这里的“独立批次”不授权启动子智能体或共享工作树。
7. **产品采样决定下一层优化**：若 CPU 往返仍超预算，再单独评审 GPU 互操作/原生 surface；先解决契约与完整产品功能，不以静默降级代替验收。

每批更新对应现行文档、受影响调用方和相关契约测试。普通逻辑验证不强制启动 Headless；涉及画面、声音与平台时使用真实产品与硬件路径。

## 8. 验收清单

### 8.1 性能与资源

- 静止画面没有重复的最终帧 readback、Family→Host 像素复制和 GPU 上传；必要 VM/音频 tick 仍继续。
- 动态画面不存在额外跨 ABI 像素复制；readback、上传、滤镜和 Slint 提交分别计时，不能只报 advance Hz。
- 像素存储、readback slots、队列、历史和 pipeline 数量有界；长流程、反复开关和失败候选之后回落。
- 同格式 PCM 无 F32→I16→F32 量化往返及无意义 planar 重排；设备回调不分配/释放普通块，也不进入 Family。
- 在相同真实场景比较原生与 Hosted 的帧时间分布、输入延迟和欠载；记录 CPU 提交与 GPU 完成的区别。预算值按产品目标和真实硬件确定，不用隔离实验推算“通过”。

### 8.2 正确性与生命周期

- 首帧、静止、动画、影片、转场、resize、输入命中、字体与存档缩略图保持原生语义。
- `Pending`、`Unchanged`、错误可区分；超时和读回失败不显示为成功的旧帧。
- 未呈现中间帧、消费者降频、帧池耗尽与 damage 基线不匹配时画面仍正确。
- 保存捕获对应原生 VM yield 边界；读档失败保护当前状态及原存档，成功读档后旧异步结果失效。
- mixer 替换只取消其自身工作；session close 取消阻塞请求并 join 所有 worker，错误路径不跳过清理。
- 输入、窗口服务、模态请求和关闭不形成 owner/UI 相互等待；无能力 Host 明确拒绝，不伪造结果。
- 保留原生 GPU 路径和成熟媒体实现；软件 adapter、静音测试输出、窗口存在或局部 fixture 不能替代真实产品验收。

### 8.3 上游维护性

- 每处 fork 差异都有必要性、真实消费者、回归边界和来源说明。
- 原生默认入口继续可构建/使用；不因 Hosted 适配改变未选择该模式的行为。
- 不为统一 Host 升级全部 GPU/音频依赖，不复制原生执行器、renderer、shader 或 save codec。
- 核心自身修复与适配分别追溯，完整历史保留；准确更新 MODIFICATIONS.md 与固定 gitlink。
- 未运行的平台和长流程仍标未验收，不把新接口或编译通过记作核心完整可玩。

## 9. 后续实施的阅读入口

- [全产品重构契约](../contracts/rebuild.md)
- [现行 Family API v13 契约](../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)
- [RFVP 修改记录](../../Emulator/Source/Families/astra-emu-fvp/MODIFICATIONS.md)
- [Siglus 修改记录](../../Emulator/Source/Families/astra-emu-siglus/MODIFICATIONS.md)
- [Artemis 修改记录](../../Emulator/Source/Families/astra-emu-artemis/MODIFICATIONS.md)
- [KrKr 适配说明](../../Emulator/Source/Families/astra-emu-krkr/README.md)
- [KrKr 最小原生修改记录](../../Emulator/Source/Families/astra-emu-krkr/MODIFICATIONS.md)
- [实际实施状态](implementation-plan.md)
- [开发与验证入口](../manual/development.md)

## 10. 整改进度（2026-10-13）

整改位于独占分支 `codex/performance-main-20261013`；公开源码基线快照为 `6348ff51bef6ea7b56a86ba95612cfa9fbb49069`。来源工作树的 staged/unstaged 修改未覆盖，尚未将整改合入来源分支。此节记录已经执行的检查，不改写上面的原始审计数据。

| 项目 | 当前结果 | 已执行验证 |
| --- | --- | --- |
| R01 | 独占工作树已实现：四类诊断历史各保留最近 1024 条，恢复快照排除历史，runtime.world 升级 v6，presentation 序号独立保存 | 两个新增回归在修改前均失败；修改后 astra-runtime 69 项测试通过。另运行链接真实 crate 的程序：发出 100,000 条演出记录后保留 1024 条，存档修改前后均为 605 bytes，恢复后的下一个序号为 100,000 |
| R02 | Musica/CMVS 提交边界使用共享 SDK SceneTextureResidency，将同一不可变 owner 的 Texture 绘制变为既有 UploadTexture/Sprite/ReleaseResource；没有新增 renderer、上游修改或逐帧像素 hash | 三 crate all-target 严格 Clippy 通过；普通测试 428 通过/18 忽略，显式硬件场景回归 9 通过；新 GPU 回归覆盖共享资产移动、不重传整幅像素、清空后重现。独立 DX12 discrete GPU 程序对照旧 transient 路径和真实 SDK 驻留路径，连续 60 帧逐字节相同，清空/重新上传通过；临时程序已清理，结果不是商业游戏帧时或完整产品验收 |
| R09 | 独占工作树已实现：插入/恢复边界维护有序队列，无到期 tick 只检查首项 | 延迟事件、诊断历史和存读档定向 9 项通过；真实 crate 冒烟保留 10,000 个远期事件，执行 1,000 次无到期推进，allocator 调用为 0，到期后正常交付 |
| R10 | 独占工作树已实现：按注册代次缓存访问计划，并行 wave 复用 World-owned Rayon pool；注册声明固定，重注册失效 | 新回归修改前在 32 tick 创建 64 个 worker ID，修改后通过；Runtime 格式、all-target 严格 Clippy 和 72 项测试通过。真实 crate 冒烟执行 100 tick、4 machine、400 action，仅使用 2 个 worker，保存恢复权威状态一致 |
| R07 | 独占工作树已实现：单后台 compiler、150 ms 去抖、最新 pending、共享版本快照、重复请求缓存及提交前项目/版本校验 | authoring 10 项回归及 Editor all-target 集成检查通过。真实 crate 冒烟对大于 1 MiB 的源码捕获 60 次快照，仅分配 960 bytes 元数据，编译得到 4 个 source locations，未变结果复用、关闭 join/拒绝新请求通过。实际 GPUI 大文档增量编辑/后台编译状态已观察；完整预览/外部 Agent 未验收，MCP stdin 关闭缺口见本节 |
| R08 | 独占工作树已实现：撤销保存 UTF-8 安全的逆向差量；批次原子提交，最多 100 条、64 MiB 保留预算；超预算明确拒绝 | 修改前 1 MiB 文档连续单字修改保留 105,920,831 bytes；修改后 authoring 14 项回归、authoring/Editor all-target 严格 Clippy 通过。真实 crate 冒烟执行 128 次整段替换，实际新增驻留 1,089,155 bytes，历史预算占用 44,300 bytes；100 次撤销/重做内容及预算一致。实际 GPUI 1 MiB 文档经增量 batch/确认/撤销/重做/撤销，完整内容一致 |
| R03/R06 | 独占工作树已实现：窗口/离屏统一 GPU 滤镜；数值改用 uniform，pipeline 仅按 kind/format 缓存；输入身份变化重新绑定，恢复重建滤镜 | 修改前连续参数第二帧 pipeline 从 2 增至 3，窗口/离屏像素不一致；修改后两 crate all-target 严格 Clippy、5 项离屏硬件回归和 1 项窗口 resize/模拟设备恢复回归通过。真实程序在 NVIDIA GPU（DX12，vendor 0x10de/device 0x2820）执行三种滤镜连续 60 帧、320×180→640×360，窗口/离屏逐字节相同，sRGB 逐 pass 量化曲线正确，pipeline 始终 4，关闭完成 |
| R04/E06/E07 | v12 CPU lease、全部 Family 帧入口、loader、Manager 最新帧/像素呈现及固定 owner 调度已实现；registry/provider/session 在 owner 内构造、使用、关闭，未增加 Send 或移动原生 VM/GPU | API/loader 严格 Clippy 和合计 38 项测试通过（5 忽略），此前 API all-feature 22 项通过。Manager 最新 all-target 严格 Clippy、67 项测试通过/4 忽略；新回归覆盖阻塞开档不阻塞 UI、非 Send provider 全生命周期同线程、输入/关闭错误顺序、不调用 UI tick 时逻辑时钟继续、关闭中不恢复游戏窗口/标题、关闭后 CPU lease 可读。之前真实 GPU 像素测试通过。公开 LFS 字体恢复后 CMVS 216 项/2 忽略、Musica 200 项/15 忽略；未修改字体解析 |
| R11/E05/E08 | Siglus 适配层直接交付原生 F32，移除未消费的 PCM 计数及量化往返；Manager 同格式保留 ABI owner，不重排/复制为 Vec；PCM owner 经 lock-free retirement 在 producer/关闭线程释放，设备析构确认后回收；单次 session 取消通知替代每块 waiter；保留独立 mixer 域，不改上游 | Manager/Siglus all-target 严格 Clippy 通过；两包同时测试的冷编译曾超时，按包分别完成 Manager 65 通过/4 忽略、Siglus 3 通过/1 忽略。实际源码程序中 F32 同格式转换分配地址不变、低于 I16 精度的信号保留，转换及 1,001 次 callback buffer 填充 allocator 的 alloc/free 均为 0，随后在非 callback 回收观察到 free=1；实际 Windows CPAL 48 kHz/8ch 生成音流 played/accepted 均为 9,600，设备关闭 ACK 通过。未宣称听音或立体声核心在该设备验收；临时源码/产物已删除 |
| R12 | FilterEngine::apply 返回实际应呈现的纹理，None 直接共享输入、无需目标/encoder/copy/submit；Manager 仅启用滤镜时分配独立目标，关闭滤镜重新绑定输入/像素归一化纹理 | Manager all-target 严格 Clippy 与普通测试 65 通过/4 忽略；显式完成 3 项 GPU 回归及 1 项真实动态库重启加载回归。实际生产 ManagerStageRenderer 程序在 NVIDIA RTX 4070 Laptop/Vulkan/driver 610.88 运行 6 次 raster 变化、None→Scale×2→None、180 次不变 render、关闭，像素正确且关闭滤镜后的 GPU texture identity 恢复输入。DXC 从 Microsoft 固定 v1.8.2502 官方包取得，保留许可证；原 GPU 测试写死临时路径已改为现行 ASTRA_EMU_DXC_PATH，真实核心测试补齐 ASTRA_EMU_TEST_PLUGIN 前置条件后通过。临时程序及运行产物已清理；未宣称完整 Slint 产品/商业游戏验收 |
| R05/E01 已收敛（保留必要原生遍历） | FVP 按 typed scene change 提交原生 GPU，静态 tick 不做第二次 GPU 构建；影片直接发布解码帧 Arc owner，跳过游戏 capture/覆盖，只转换最后到期帧，结束恢复游戏。全帧模式不计算未消费的矩形 damage/changed-resource IDs；GPU 两个 root 的 4096-entry visit scratch 改用栈数组。变化帧仍保留 hit-proxy/change detection 与原生 GPU 的两套计算，未另写共享 renderer/数学路径 | 最新 FVP all-target 严格 Clippy、40 项普通回归/4 忽略，另 4 项显式原生 GPU 回归通过。实际 root 空场景 60 次 GPU 原生 render 输出相同，owner-thread 4096-byte 分配旧路径 60 次、新路径 0（不是总分配/游戏帧时）。实际公开 full-frame Family/native GPU 程序验证 60 次静态 tick Unchanged/关闭后 CPU lease；此前影片切换及原生保存/GPU异步程序通过。NullAudio/公开夹具；临时产物已清理。剩余变化帧双计算 CPU 成本未测，不声称已经合成单遍 |
| E02/E03 已实施（未集成提交） | FVP 向 RFVP 的同步 SaveCreate 边界显式借用既有 HostedGpuRenderer，缩略图来自该时刻的原生 GPU，而非旧 Host 帧或 SoftRenderer；保留 SaveWrite 使用已准备 payload。原生 readback 单 buffer 按尺寸复用，submit/owner-thread poll 分离；普通 FVP 帧通过 Pending 交付，不 sleep 等待。映射回调用可复用 Atomic generation 拒绝取消后的旧结果，保留 5 秒超时；启动、影片恢复及存档边界仍同步。原生 App 四平台初始化/保存调用迁移 | FVP 最新 all-target 严格 Clippy、普通测试 40 通过/4 忽略，4 项显式 GPU 回归通过。缩略图回归在旧软件路径失败，在 GPU 路径通过；实际公开 FamilyProvider/GPU 程序观察准备 RGB=[188,0,0]、稍后写档画面=[0,188,0]，全部缩略图像素匹配准备画面，原生 RFVS 为 1,417,220 bytes。异步实际程序观察 1 次 Pending，期间推进 VM、完成红→绿 lease、3 次 pending-close/reopen、旧 CPU lease 可读。此前 180 次原生 GPU padding/resize 回读冒烟通过；最新原生 Windows 默认桌面 cargo check 通过，保留上游既有 decoder warning，未抑制/格式化上游。NullAudio；临时源码、夹具、存档及执行文件已清理，不代表听音/商业游戏/其他平台验收 |
| E04 已实施（未集成提交） | 原生 Artemis 提供两个显式 Result 帧入口，Family 分开推进与捕获，静态不读回，短缓冲/GPU/完整性错误不再当作零长度静态帧；Windows/Linux/Android 复用 NativeVulkan，macOS 复用 NativeMetal，不新增 renderer/fallback | all-target 严格 Clippy、12 项普通单元测试与 1 项非 GPU 生命周期测试通过；显式 2 项硬件 Vulkan 回归通过（3 次关闭重开、每次启动后 60 次 Unchanged/旧 lease、短缓冲错误不改写与随后完整回读）。实际 PF6/IET/native FFmpeg 视频/WAV 程序运行 150 tick：7 次像素变化、7 个彩色影片帧、142 次 Unchanged、48,960 个 stereo F32 frame 与非零 PCM；关闭/取消通过。原生选卡排除 CPU/未知类型；NullAudio，不代表听音、完整窗口/商业游戏或其余平台验收。首次冷编译超时后继续缓存完成，5 项上游 warning 保留；临时产物已清理 |
| 核心来源 | RFVP 原固定提交已接入；Siglus 从可取得的最小适配补回缺失机制；Artemis 从已发布 13e55ffa 恢复必要机制并加入帧入口；KrKr 完整固定上游已取得，NativeOwner 重适配中 | Siglus 当前 v12 Family all-target 严格 Clippy 通过，library 回归 3 通过/1 忽略；不等于真实游戏验收。Artemis 原缺失 6b59d145 仍不可取得；当前相对 13e55ffa 为 7 个核心文件、245 插入/149 删除，相对上游 0c06f371 为 15 文件（含主要 lockfile 差异），不将恢复起点误称为缺失提交；实际 Family GPU/媒体验证见 E04，存读档/商业游戏/听音仍未本轮验收。KrKr 独立 workspace 的 Family 严格 Clippy 与 26 项回归通过，不能算尚未完成的 NativeOwner 或真实游戏通过 |
| Manager 像素路径实际冒烟 | 运行链接真实 crate 并调用实际 ManagerStageRenderer 的独立程序，不使用测试 mock 或核心软件重绘 | NVIDIA GeForce RTX 4070 Laptop GPU / Vulkan：原生 stride=12，像素分配地址保持；BGRA、linear、bottom-to-top、ignored alpha 转换后实际 GPU 回读正确；180 次不变帧 render、3 次 GPU 资源重建后输出一致；清空会话后既有 CPU lease 仍可读取。临时源码/可执行文件/链接产物已清理；不代表完整 Slint 产品或商业游戏验收 |
| 固定 owner 原生核心冒烟 | 独立程序直接使用生产 FamilyOwner/ActiveFamilySession、v12 动态加载器和真实 Musica DLL；本次 owner 修改全部在 Manager，无新的 Musica/外部核心改动 | Windows 原生 DX12 discrete GPU，1280×720，完成帧 generation=2、背景像素 `[25,100,220,255]`；开档入队 29 μs（不是启动时长），模拟 UI 阻塞 500 ms 后通过物理 F6/Enter/Escape/F8/Enter 输入实际保存/恢复 slot 20，日志确认 save/load completed；关闭、worker join、旧 CPU lease 可读通过。公开夹具首次因缺少原生存档 UI 资源明确失败；补齐既有测试同款资源后通过，未抑制 VFS 错误。NullAudio，仅验证 GPU/VM/生命周期，不代表听音、完整 Slint 窗口或商业游戏验收；临时源码、夹具、存档和复制 DLL 已清理 |

固定 owner 的命令队列容量为 8，单批及 owner 待处理输入不超过 `MAX_EVENTS_PER_ADVANCE`；队列满明确失败，不静默丢输入。UI 保留 closing 视图直到 owner 完成，期间禁止新开档且不再呈现旧窗口属性；开档/正常关闭不等待 native 入口。最终退出才 join。60 Hz owner 期限独立于 UI 服务/重绘，普通帧计入核心执行耗时，恢复按 reset_clock 在推进/取帧完成后锚定；Waiting 不停止时钟。原生模态宿主服务及其取消仍属 E09，不能把当前异步开档视为 KrKr 已接入。

R02 实际 GPU 对照：vendor `0x10de` / device `0x2820`，同一 512×512 RGBA 资产供两个移动绘制使用，64×64 输出、60 帧。`upload_bytes` 包含顶点上传，不是纯纹理量；稳态分别记录旧路径下界和新路径上界，不代表完整游戏帧时。

| 上传量（bytes） | 旧 transient 路径 | 新驻留路径 |
| --- | ---: | ---: |
| 60 帧总上传 | 141,506,528 | 4,223,104 |
| 稳态逐帧范围 | ≥2,114,048 | ≤480 |

### 新发现：当前原生设备只有 8 声道配置

PCM 实际冒烟中，原生默认设备为 F32/48 kHz/8ch；枚举 `supported_output_configs` 未发现 2ch，初次按核心常见的 stereo 声明 configure 明确返回 `ASTRA_EMU_AUDIO_CHANNEL_LAYOUT_UNSUPPORTED`。这是现有转换器只支持同声道、1→2、2→1 的基础播放限制，不是本次新增的静音 fallback。后续硬件冒烟显式生成并声明 8ch 源流以验证 PCM owner/设备关闭，未伪称已经支持 stereo→8ch。进一步基础播放整改应取得真实设备声道位置/布局并制定映射；不得只按声道数量猜测布局、修改上游 mixer、自动改系统默认设备或以 NullAudio 代替。此问题仍未整改，商业游戏/原生核心设备听音仍未验收。

### R05 冒烟发现：ASF 同一 packet 内剩余视频对象丢失

非商业 FFmpeg 生成夹具为 16×16、5 fps、1 秒 WMV2/ASF。修改前实际解码只返回 PTS 0 后 EOF；`na_wmv_player::AsfWmv2Decoder::next_frame` 在 packet 的 payload 循环内返回首个视频对象，丢弃同 packet 剩余 payload。已将未消费 payload 保存在单 packet 的 `IntoIter` 中，不新增逐帧队列分配；修改后实际返回 PTS 0/200/400/600/800 ms，新多对象边界回归通过（1 项）。这是主仓共享解码器的独立正确性修复，不是 RFVP 上游适配改动；尚未推论 WMA 路径具有相同缺陷或完成其整改。

### KrKr 候选嵌入代码：不能直接复用为已完成核心

已检查恢复的 Artemis fork 中 `crates/art3m1s-krkr/native/upstream`。它保留 native VM/纹理/audio 桥，可作嵌入边界参考，但不能当作当前 Windows KrKr NativeOwner：`CMakeLists.txt` 明确仅支持 macOS，并关闭 `USE_RENDER_GL` / `USE_RENDER_VK`；`headless_platform.cpp::TVPShowSimpleInputBox` 忽略请求参数，直接返回常量 1，未完成真实用户响应。不能复用该 compatibility stub 规避 E09。其文档中的历史 macOS 游戏/GPU/PCM 数据不是本次运行证据；当前 Kirikiroid2-web/Cocos/OpenGL 固定上游与该 SDL3 源码约定不同，重适配须保留当前核心原生 GPU、VM 与模态等待/取消，不能替换成该桥的另一渲染路径或 bootstrap。

当前固定 Kirikiroid2-web 的 Windows `Platform.cpp::TVPShowSimpleInputBox` 本身也只发 warning 后返回 0；其 message box 只支持 1/2 个按钮，使用系统固定 OK/YesNo，不保留任意按钮标签。重适配不能把这类桌面移植缺口当作真实交互结果。原生 `SystemIntf.cpp` 以返回索引 0 判断左按钮/输入确认，必须按实际用户响应填写结果，关闭会话应取消请求而非确认输入。

已确认 Windows 默认音频由 `WaveMixer.cpp::CreateAudioRenderer` 选择原生 OpenAL，声音状态/队列进度由 `tTVPSoundBufferAL` 自持；同文件另有 SDL/Oboe mixer，不能因 `FillBuffer` 边界易接而替换默认成熟 mixer。NativeOwner 的 PCM 接入须复用当前原生输出与进度机制，保留 Host accepted/played、暂停/停止/完成与取消，不用墙钟估计播放完成。本次尚未实现该接入，不能将已通过的 Rust 传输测试视作原生音频/核心通过。

原生 Windows 依赖构建已修复并实际安装 Cocos 3.17.2：bundled minizip 的静态声明误用共享 zlib `ZEXTERN`，引发 MSVC C2491；修复只在现有 overlay 中将 bundled unzip 声明改为 `extern`。随后链接暴露裸 `websockets` 名称错误，改用依赖实际导出的 `websockets_shared` target，保留原有 `websockets` target 的配置。没有改 ZIP/CRC、renderer 或 mixer。原版 FFmpeg 3.3.9 的 Debug/Release 构建及安装已通过：显式指定已安装 Yasm 的路径，未升级 FFmpeg、关闭汇编或替换解码器。完整 manifest 依赖安装完成，不等于 NativeOwner 完成。

继续配置暴露两个构建边界问题：本机默认 CMake 编译器为 MinGW，而依赖使用 MSVC；PATH 中 Bison 2.4.1 不满足原语法 `%require "3.8.2"`。独占构建目录显式使用 MSVC 与官方 WinFlexBison 2.5.25/Bison 3.8.2，不改语法或降低版本要求。UnRAR 6.0.7 overlay 未将上游 `dll.def` 加入共享库，生成无导出 DLL、缺失 import library，导致 imported target 配置失败；已在 Windows shared 构建中加入原版 `dll.def`。修复后完整配置通过，实际加载 Release/Debug 两个 DLL 并调用 `RARGetDllVersion`，均返回原版 ABI 8。依赖原有 libarchive 工具分发 warning 保留，未用 policy 掩盖。

恢复上游固定 `layerExImage` 子模块 `23a9214f9b6e9bc3267b964707efa269c041b919`，不以跳过插件算完整核心。原生 MSVC 编译已通过 VM、渲染、音频与基础模块；随后 motionplayer 的 `__builtin_memcpy` 不支持 MSVC，改为 `<cstring>`/`std::memcpy`，保持原版 MT19937 与 canonical double 的位转换。实际编译并运行该原生源文件的独立程序，对四个固定 seed 的 8,192 个输出逐项比较标准 MT19937 对应的 52-bit mantissa，包含跨重生成边界，全部一致；临时源码/产物已清理。完整核心编译仍未完成，不将上述结果等同 NativeOwner/GPU 游戏验收。

完整插件构建随后暴露两处编译移植边界：motionplayer 的数值索引常量 0 被 MSVC 参与指针重载解析，使用现有 `IndexT` 的非 constexpr 局部值消除歧义，未改属性访问或几何计算；固定 layerExImage 在 `_WIN32` 下假定外部已引入 Windows 的 BYTE/RGBQUAD，而当前构建未引入，修复只在该 translation unit 的 MSVC 编译选项中预包含系统 `windows.h`（NOMINMAX/WIN32_LEAN_AND_MEAN），不修改插件子模块的像素算法。修复后完整源文件编译通过（含固定 layerExImage），最终链接仍未通过。

最终链接暴露依赖接口错误：Cocos 将 `_USRDLL` 等 DLL 导出宏以 PUBLIC 传播到消费者，使静态数据使用导出而非导入声明；已改为 PRIVATE，普通 Windows 平台定义仍 PUBLIC。原有 PVRTC decoder 的函数声明未标 `CC_DLL`，仅补齐 DLL 声明，不另写或复制解码算法。TJS 正则头强制 `ONIG_EXTERN extern` 绕过 Oniguruma 既有 Windows 导入声明，已删除该覆盖，复用依赖头与 typed target 的链接约定。

原版 libbpg 直接使用固定 FFmpeg 的 `ff_hevc_decoder` 私有数据符号，不能链接只导出公共 API 的 FFmpeg DLL；Windows overlay 改为静态链接原版 3.3.9，不升级或另写 decoder，其余平台原有 linkage 不变。原版 7zArchive 使用 C SDK 的 `SzArEx_*`/CRC，而 vcpkg 7zip target 只构建 C++ archive/COM 接口，且共享库不导出这些 C SDK/data 符号；从现有固定 26.4 port（上游 tag 26.04）补入上游原有的 `7zArcIn.c`/`7zBuf.c`/`7zDec.c` 与官方 PPMd 开关，Windows 静态链接，保留原 ASM/CRC 优化及全部其余原包组件。未改 SDK 算法、删档案格式或换成新的 archive reader。上述链接修复在当时仍待实际依赖重建，不计为最终链接通过。

上述 Cocos/SevenZip/FFmpeg static 依赖安装与配置已通过；紧接着实际链接发现 FFmpeg MSVC 的 COFF static archive 仍用 `lib*.a` 文件名，而既有 Windows FindFFMPEG/pkg-config 查找 `.lib`，触发缺少 `avfilter.lib`。仅在 Windows MSVC static 安装边界将原 archive 规范为对应 `*.lib` 名称，数据/解码代码不变；正在重建验证，不以配置成功替代最终链接。

后续安装验证补齐现有 XP3 repacker 所需的 SevenZip 原版 C++ 头目录，原代码编译成功；没有禁用 repacker 或修改其实现。最终链接暴露两项遗漏：新增 PVRTC 声明头没有加入原 overlay 的显式 COPY_FILE 清单，现已补齐；FFmpeg static 的 pkg-config 提取只读公开 `Libs`，未传递 bzip2/lzma/Schannel 的 `Libs.private`，现 static 查询使用 `--static`，不关闭 codec、TLS 或凭空硬编码全局库。MSVC 原注册器链接误用 GCC `-Wl,--whole-archive` 被忽略，已沿用现有插件模式改为 `/WHOLEARCHIVE`，保留全部 ncbind 注册器。原有 `/DEBUG:FASTLINK` 工具链 warning 保留，没有用 suppress 伪称支持；新修复仍待完整链接验证。

### KrKr 原版完整构建与硬件冒烟（2026-10-15）

上面各次失败/安装记录不是最终状态：完整原版依赖、所有核心源文件、固定 layerExImage、全部 ncbind 注册器与原生 executable 现已编译/链接通过。最后 15 项缺失依赖来自旧 `FFMPEG_FOUND` cache 跳过新 static 依赖解析，只清除自有目录的 FFmpeg cache 后通过，没有硬编码缺失库或关闭 codec/TLS。

实际原程序启动另发现 Windows `TVPCheckStartupArg` 将 `CommandLineToArgvW` 的数组误当一条 exe 字符串，再以字符 `:` 当 argv[1]，真实脚本不能从命令行启动。已只修复开档边界：RAII 释放原 argv、一次性正确转换 UTF-16/UTF-8 参数，并在目录分支保留原 selector 的尾分隔符语义。否则配置与 savedata 根落到父目录，原 per-game `renderer=opengl` 未被消费。VM/renderer/存档实现不变；修复后真实原版程序运行公开 Window/Layer TJS 脚本，存档根位于游戏目录，正常关闭。

独立临时程序链接全部真实核心 library，复用 AppDelegate/Director/原生 GPU，运行 120 tick；实际 NVIDIA RTX 4070 Laptop/OpenGL 4.6/driver 610.88，原 renderer 为 OpenGL、`IsSoftware=false`。对原 viewport 640×480 区域 GPU 回读观察到 272,025 个与脚本指定 RGB=[33,78,161] 相同的像素，GL error=0，退出 0。首次临时程序重复 swap 导致捕获失败；核对原 Director 已交换后删除重复 swap，未改核心路径。普通桌面/GDI 对 OpenGL 的捕获不能作为这项像素证据。临时程序、公开脚本、独立偏好与链接产物已清理；这不是 NativeOwner/Manager/听音/商业游戏或完整帧/FPS 验收，E09 仍未完成。


原版编译另发现 `UtilStreams.cpp` 的两处 UnRAR 长度拼接用 32-bit `UnpSizeHigh << 32`（MSVC C4293），存在未定义行为，影响超过 4 GiB 的解压长度/进度；应先提升至 64-bit 再移位，并以大长度边界验证。当前只记录审计结论，未静默修复、抑制 warning 或声称该边界通过。

Engine 全 workspace 检查中 docs/fmt/严格 Clippy/产品 build 已通过；测试在 astra-player-vn 的 3 项 presentation 回归遇到图像解码失败。根因为独占快照中的既有示例 PNG 仍是 LFS pointer，只恢复对应公开 LFS 资源并核对 SHA-256，不改 image parser、运行语义或测试期望。恢复后实际执行全部 5 项 presentation 回归，通过；随后 `cargo test --offline --workspace` 全 Engine 工作区（含 doctest）通过，耗时 632.86 秒，保留原有 binrw 0.15.1 future-incompatibility warning。EMU 的 13 个自有 workspace member 格式检查、全 workspace 严格 Clippy/build/test 已通过，后者耗时 2,218.51 秒；Editor 格式检查、全 workspace 严格 Clippy/build/test 已通过，18 项普通测试通过/3 项产品前置条件未提供而忽略，保留 proc-macro-error2 的 future-incompatibility warning。不等于忽略的硬件/真实 Agent/预览流程通过。

### Editor 实际表面验证与新关闭边界

运行真实 GPUI Editor，打开大于 1 MiB 的公开临时剧情文件（实际 UI 文档 1,129,801 bytes），编译得到 4 个 source location。通过生产 MCP 发送一项增量 edit batch，实际窗口先显示待确认；用窗口自身的鼠标事件点击 Apply batch 后 v1→v2，源码和后台 `Checking source…`→`Compiled · 4 source locations` 同步更新。实际点击 Undo batch→Redo→Undo，v3/v4/v5 文档分别与原文/修改文/原文逐字节一致。通过窗口表面的 PrintWindow 观察真实 Source/Outline/版本/编译状态；普通桌面截图只能得到壁纸，没有拿它冒充窗口证据。没有测试鼠标打字吞吐、游戏预览帧率或外部 Agent。临时项目、缓存、截图、日志已清理。

关闭过程中发现仅 `--mcp` 模式存在输入生命周期缺口：MCP client 持有 stdin 管道时，最后窗口关闭后进程等待超过 30 秒；client 关闭 stdin 后进程以 0 退出。原始默认（不带 `--mcp`）入口实际关闭以 0 退出。尝试加入 GPUI last-window `quit` 订阅没有解决 MCP 阻塞，已经移除，不保留无效修复或以 shutdown_timeout 掩盖未回收任务。后续需对 stdio MCP 输入做可取消、可 join 的关闭；Tokio stdin 的阻塞读取是排查入口。本次 R07/R08 的实际编辑/后台编译/增量历史表面已验证，该 MCP 关闭问题仍未整改，不能声称完整 Agent 生命周期验收。

上述三个 Windows workspace 检查与 KrKr 原版构建/GPU 原语冒烟，不等于四平台、真实游戏、设备听音或正式性能完成；KrKr NativeOwner 与新发现的设备声道、MCP 输入关闭边界仍未验收。

### v13 模态服务与实际窗口验证（2026-10-15）

FamilyHostServices 新增可选、session-scoped `ModalService`；保留原按钮顺序/标签及 UTF-8 输入，显式区分 Submitted 与 Cancelled，不把关闭变成第一个按钮。标题/正文/输入各限制 65,536 bytes，最多 16 个按钮、标签各 256 bytes，拒绝 NUL、越界按钮、输入种类不匹配；无窗口 Host 不提供服务。ABI/schema 升为 v13，全部现有初始化调用者迁移；旧插件须统一重建，不保留 v12 shim。

Manager 新增单个待处理请求与 Condvar 等待，UI 仅在 Show 边沿建立字符串/按钮模型，不每帧克隆或序列化 payload；请求 ID 同时携带 epoch/sequence。close/shutdown 在入队或 join 前取消并唤醒等待，open 失败取消，迟到/重复/旧 epoch 响应明确拒绝。Slint 实际表面支持原标签按钮、文本输入、取消及退出会话；模态期间阻断原游戏的键盘/鼠标/手柄与 pointer capture。连续请求按 Dismiss→Show 排空，避免通知合并后新对话未显示。

实际临时窗口程序复用生产 `host_modals.rs`、Slint ManagerWindow 与真实 ABI ModalBridge（无 VM/媒体替身），依次点击原第二/第三按钮、点击取消，并实际键入 UTF-8 后提交；观察响应分别为 button_index=1/RNone、button_index=2/原初始文本、Cancelled、button_index=1/`原有输入 initiale 修改`。另一次在请求仍阻塞时关闭真实窗口，返回 Cancelled、worker join 与进程退出 0。以自身 PrintWindow 表面确认标题、原标签和输入；单独 WM_CHAR 未产生编辑，实际按键+字符事件产生编辑，未把 SendInput 返回值当作输入成功。此项只证明宿主服务/UI，不是生产 FamilyOwner 开档、KrKr native dialog、原生 PCM、GPU 游戏或性能验收。

新增 2 项 ABI 容量/UTF-8/输入类型边界回归与 3 项 Manager 并发/取消/迟到回答/连续请求回归通过。v13 后完整 EMU workspace 严格 Clippy 与普通 test（含原有未标 ignored 的 GPU 回归）通过；依赖原有 warning 保留，原有 ignored 项未宣称通过。首次完整检查因接续环境缺 LIBCLANG_PATH 失败，恢复现有 LLVM 设置后通过，没有改依赖或抑制诊断。先前 Engine/Editor/EMU v12 完整检查保持历史证据，不等于四平台或真实游戏验收。

KrKr 仍须将真实 native message/input box 接入该服务，并完成原生 NativeOwner/混音输出/关闭；本轮宿主接口与 UI 变更没有新增原核心执行、renderer 或 mixer 修改，E09 未标完成。

### KrKr Windows 取消阻塞修复

v13 后实际重跑独立 KrKr 包，旧 `shutdown_releases_a_pending_read_on_a_cloned_stream` 卡住；终止自有测试后串行定位仍阻塞，不能沿用此前某次通过记录。独立 WinSock 实验确认：对原句柄或 duplicated socket 执行 shutdown 都不能释放已在进行的同步 recv；直接对同步 recv 调用 CancelIoEx 也不能证明取消完成。

仅修改 Rust transport：Windows worker 使用 WSASend/WSARecv overlapped I/O，各 clone 复用一个 completion event、同 lane 共用一次性 cancel event；shutdown 先发取消，等待中准确取消并 drain 原 operation 后才释放借用缓冲。没有新增周期性 socket 读超时/poll、per-I/O event 分配、payload 拷贝或原核心修改；Unix 保留现有 TcpStream。原阻塞回归现在有有界失败路径，不再无限 join 掩盖失败。

独立 KrKr all-feature/all-target 严格 Clippy、26 项单元与 9 项真实子进程 transport 集成回归通过。临时实际程序调用同一生产 Windows transport，完成 4,194,304 bytes 逐字节正确的大块传输；停止未读输入/大块阻塞输出后分别在约 181 μs/2.80 ms 内完成 join，进程退出 0。它们是单次实际取消观察，不是吞吐/FPS 或真实 KrKr VM/GPU 验收。临时程序清理，原核心没有因这项修复增加差异。

另定位到尚未整改的 KrKr 旧帧边界：`family/frame.rs` 仍按旧 centered viewport 对每个像素验证 letterbox，`wire/frame.rs` 与 native output 回归仍允许 raster/stage 不同宽高比；与现行“不在 Family 内 letterbox”的要求冲突。NativeOwner 重适配应直接交付原核心最终游戏画布、显式传原像素语义并移除旧全帧扫描，不能简单去掉检查后继续发布旧截图。公共 Opaque alpha 验证也有像素遍历；已经原生合成、不消费 alpha 的路径应使用现行 Ignored 声明，而不是先改写 alpha 再扫描。此处只登记证据和整改边界，未声称已移除这些成本。

v13 插件已完成全 EMU workspace 产品 build；最新文档检查为 190 个文档、0 项链接/卫生错误。KrKr 包和上述独立程序通过不等于 NativeOwner 已完成，E09 继续保留未完成。

### KrKr 帧边界收敛与通用约束的实际影响

随后仅在 KrKr 边界完成上述收敛：wire/native output 在分配 body 前拒绝 raster/stage 宽高比不一致；Family 移除 centered viewport 的逐像素扫描，对已经最终合成的 RGB 声明 Ignored alpha，保留原 stride/owner/alpha bytes，不再重复 Opaque alpha 扫描。损坏几何/短 body 仍不能覆盖最后完成帧。没有裁剪旧截图冒充原游戏画布；真实 NativeOwner 仍须交付正确原生最终画布。

实际临时程序调用生产 `publish_native`，发布 1920×1080、stride=7688 的 8,303,040-byte CPU 帧；原 allocation、全部 bytes/alpha 与 padding 保持，单次发布约 18.8 μs，异比例在像素分配前明确拒绝。只是 CPU adapter 冒烟，不是 GPU/native game FPS。KrKr 本包 26 项单元通过；后续真实 child 回归揭示另一项取消竞态：Host 取消已触发，稍后 native failure 回包仍覆盖调用结果。现以同一终态锁序列化显式取消与首个 native failure，并在阻塞接收后重查终态；保留先发生的原始错误，拒绝用迟到结果覆盖取消。不为特定错误码设特例。修复后 all-feature/all-target 严格 Clippy、26 项单元及 9 项 child transport 回归通过；相关同步状态锁使用 parking_lot，原核心不变。

曾尝试在公共 FrameInfo 统一加入宽高比拒绝；完整回归首先在 Musica Eden 原生导入失败，随后共享 GPU 测试锁中毒。实际源码仍允许独立 logical/render extents，Eden 原生存档明确要求 1024×640；不能靠改 VM/save 几何、修改回归期望或忽略这些测试来“通过”。已撤回本轮新增的全局拒绝和对应试验测试，只保留 KrKr 局部边界；公共 lease 工作完成不等于 Musica 固定舞台/四空间契约已完成。应在独立画布迁移中显式解决原版存档与固定 1280×720/允许 render scale 的冲突。本轮没有修改 Musica 已验证的机制。

### 原 KrKr 最终游戏画布实际取回（无新增核心修改）

新增独立临时 native 程序，仍链接固定原版全部核心/插件，使用原 AppDelegate/Director、per-game opengl 配置与公开 Window/Layer 脚本。通过 `TVPMainWindow → GetDrawDevice → tTVPDrawDevice::GetManagers().back() → GetDrawBuffer() → GetTexture()` 取得原核心最终合成纹理，再调用原 `GetPixelData/GetPitch`；不是屏幕截图、重绘、Cocos UI crop 或另写 renderer。

实际 NVIDIA RTX 4070 Laptop/OpenGL：`IsSoftware=false`，raster=640×480、原 draw-device logical=640×480、pitch=4096、native format=4、首像素 `[33,78,161,255]`，全 307,200 个可见像素均匹配脚本指定颜色，GL error=0、进程退出 0。说明实际原生 row pitch 大于 width×4，后续适配必须保留，不能假定紧密行或复制为另一个 packed canvas。全画布来自游戏纹理，不含原 selector/统计 UI 或窗口 letterbox；其他 draw-device、媒体/音频、NativeOwner/Manager 完整接入及关闭 worker 仍未验收。

探针第一次编译仅因自身 windows.h/WinSock include 顺序冲突而失败；只修探针 header 顺序/Windows 宏后编译链接通过，保留原 FASTLINK warning，没有改上游。临时程序、脚本、偏好与链接产物清理。保留实际可复用的原核心访问边界，原 VM/GPU/mixer 不变。

撤回全局宽高比试验后，实际保留的 v13 全 EMU workspace 严格 Clippy、产品 build 与完整 test 再次通过；Musica 原 Eden 导入回归恢复通过，没有调整其脚本、原存档几何或回归期望。先前失败仍按上节记录，不把它改写为未发生。

### 最终 RGB 的 alpha 认证成本与 release 实测

继续检查发现 Artemis/CMVS/FVP/Musica 仍将已经最终合成的帧声明为 Opaque，公共发布入口因此每次逐像素验证 alpha。四个适配层改为现行 Ignored 声明，不改核心、RGB、原 alpha bytes、VM 或媒体语义；Siglus/KrKr 已使用 Ignored。公共 Opaque 验证仍保留，不能以删除所有边界验证代替正确元数据。

这不是“CPU 扫描消失就免费”：Ignored 在现有 Manager pixel presentation 中可能增加一次 GPU alpha 归一化 pass。为验证取舍，实际 release 程序直接使用生产 FrameLease 发布与 `stage_pixels.rs`，同一 NVIDIA RTX 4070 Laptop/Vulkan/driver 610.88，每种 raster 交替四轮、每轮 8 帧预热和 64 帧测量；相同 RGB/alpha 输入、每帧 upload/submit/等待完成，输出回读 `[33,78,161,255]`。下表为各轮均值的范围，不是产品 FPS、吞吐或 GPU timestamp。

| raster | 原 Opaque 发布 μs | 新 Ignored 发布 μs | 原发布/上传/呈现总 ms | 新发布/上传/呈现总 ms |
| --- | ---: | ---: | ---: | ---: |
| 1920×1080 | 475.278–501.659 | 0.189–0.309 | 1.564–1.604 | 1.114–1.164 |
| 3840×2160 | 1961.864–3739.980 | 0.612–1.034 | 6.279–8.123 | 4.448–4.539 |

该硬件上的净成本下降，包含额外 GPU 归一化；其他平台/设备以及真实游戏仍未测，不能外推为四平台性能验收。独立测量首次 offline 解算选到未缓存的更新依赖而停止；复用现有 workspace lock 后完成，没有升级正式依赖。四个包 fmt、完整 EMU workspace 严格 Clippy、产品 build/test 通过（保留原有依赖 warning/ignored）。临时测量工程与产物清理，不将纯元数据转发写成新的永久测试。

### 宿主内存异常排查（2026-10-15 13:41–13:43）

用户询问是否泄露后暂停新的构建/视听探针，检查 Windows 内存计数与进程归属。13:41:29 实测 `python.exe` PID 59880 私有提交 115,454,361,600 bytes（约 107.5 GiB）；宿主总提交 166,078,746,624 bytes / 169,015,128,064 bytes 上限，AvailableBytes 733,184，已存在严重系统内存压力。该进程命令为 `python.exe -`，创建于 13:37:00，父 PID 34708 为另一 omp；本次 eval PID 27696 的父 PID 是 38364。没有证据将它归为 AstraEMU/native child，也没有终止这个非本实例所有的进程。

13:42:36 起三次定向测量该 PID 已不存在；13:42:58 宿主提交降为 49,800,052,736 bytes（约 46.4 GiB），AvailableBytes 恢复为 41,602,408,448。实测支持“另一个工具进程的异常大额提交已经释放”，不证明分配属于泄露、Python 仍持续增长或 AstraEMU 存在同类泄露；高占用时仅取得一次样本，进程退出原因未知。DWM 仍约 5 GiB 私有提交，单点同样不能称泄露，未终止系统或用户进程。

此前 Sandbox 官方 doctor 通过（wsb 0.8.107.0 / IronRDP 0.1.0）。4 GiB、vGPU enabled、网络/剪贴板 disabled 的新实例中，测试程序返回 `0xC0000142`，日志为空，随后实例从原生列表消失；8 GiB 重建也在启动后消失。仅共享自有非商业输入为只读，用户另明确授权一个自有日志输出目录可写。没有软件 renderer 替代，未取得有效 Sandbox GPU/交互验收。异常 Python 在首次失败之后才创建，所以不能用这个样本解释全部 Sandbox 故障；它与后续失败时间重叠，但因果仍未证实。连接已释放；应在内存恢复后继续隔离验证，不能把 Sandbox 失败写成产品泄露证据。

### KrKr 原生模态 IPC 与原脚本 headless 实证

私有 wire/native callback 同批升至 v2，旧 v1 不兼容。复用原控制 lane 增加独立 modal kind/sequence，不另建网络服务；一个请求等待于 Condvar，PCM/control/frame worker 不依赖 UI 回复。parent 直接调用 session-scoped typed Host 模态服务，无服务时明确 Unavailable，仅实际发请求才失败。控制回包保留原 session/sequence，拒绝错误/重复/迟到响应；取消先唤醒 Host/native 等待，再 join。仅扣除实际 Host 用户等待的超时时间，后续通过 reset_clock 显式重锚定，不扩大正常 elapsed 上限。

原核心只增加一个可选 `EmbeddedModal.h` 宿主钩子和 Win32 两个原 message/input 入口的分派。默认独立核心行为不变；嵌入时原 caption/message/button 文本在 Family 的 `HostModal.h/HostModalAbi.h` 转为有界 UTF-8 回调，最多 16 按钮/256-byte 标签、65,536-byte 正文或输入。取消返回原 API 的 -1，inputString 因而按原脚本绑定返回 void；不自动确认、不替代 TJS，也不改 GPU/mixer/存档。callback 错误独立保留 `ASTRA_EMU_KRKR_MODAL`，不被通用 Boot/Advance/清理错误覆盖。删除此前仍藏在 native capture 中的第二次 Opaque alpha 扫描；像素长度/stride/几何边界继续校验。

实际先跑生产 Slint 窗口、ModalBridge、parent/child IPC、Rust native callback、Family C++ adapter 和原 TJS：原三按钮 inform 的第二按钮返回 1，原 inputString 初始值显示正确；随后窗口关闭，原请求得到取消并返回 void，child 清理完成。临时 UI 探针只接受正常完成的断言因此失败，这次不计完整输入验收。没有把窗口关闭原因猜成产品错误或隐瞒该失败。

用户允许 headless 后，改用同一生产 ModalBridge/transport/callback 与真实原 Cocos/OpenGL/TJS 的交互控制台；仅隐藏原窗口，不换 renderer。逐个读取实际请求再明确选择：原 inform 返回 1，原 inputString 返回 UTF-8 `真实输入修改`，第三个请求等待超过 12 秒后明确取消，原脚本日志为 `typeof(cancelled)=void` 并继续完成。实际原 GPU 为 NVIDIA RTX 4070 Laptop、`IsSoftware=false`。原 TVPSystemUninit 和 Application 析构执行完成，临时 parent 退出 0；故意不发布 NativeReady/假帧，最后 NativeUnavailable 只标记探针不是完整 NativeOwner。最初 shell 环境的一次尝试只到 NativeUnavailable，未见 VM 标记，不计为运行成功。

新增 child 请求形状/迟到回包回归及 2 项真实子进程模态回归；后一组证明模态等待期间 PCM 仍交付、Host 取消可关闭并 join、缺服务只在请求时失败。移除临时探针入口后，本包 fmt、all-feature/all-target 严格 Clippy、27 项单元、2 项 modal 和 9 项 transport 回归通过；原版 Windows 全 core/plugin executable 完整重链接通过。临时源码、公開脚本、独立偏好、UI/console/DLL 探针产物清理。此项完成模态边界，不等于完整 NativeOwner、原生 OpenAL→Host PCM 或实际商业游戏验收。

内存恢复后再次新建 Sandbox，仍在运行测试包之前消失。Windows Error Reporting 在对应时段记录 `ContainerCrashDump`、P1=59/P2=3221225477（0xC0000005）；这是容器故障证据，尚未分析 dump，不能指认具体驱动、核心泄露或将所有故障归因于此前内存压力。Sandbox 控制连接已释放，不降级 GPU；实际游戏验证后续采用用户允许的 hardware headless 路径。

### KrKr 正式 NativeOwner 与原 OpenAL 输出（2026-10-15）

新增 Family 自有 `native/NativeOwner.cpp`、`NativeSession.*`、`NativeInput.*` 与正式 CMake shared-library target；固定核心仅提供可选嵌入钩子：原最终画布更新通知、原 renderer 选择、原退出/标题/全屏分派，以及复用原键盘/鼠标事件逻辑的入口。独立 executable 保留原默认路径。VM、合成、解码、存档、字体与插件均仍来自同一完整原核心，不使用替代 renderer/mixer。CMake 通过可选外部 adapter 目录接入，继承原目标的真实依赖、编译选项和插件 WHOLEARCHIVE，正式 staging 交付 dependency DLLs/Resources。

NativeOwner 固定一个 owner thread、每会话 fresh child；真正出现原游戏最终画布且原 renderer 非 software 后才返回 NativeReady。读取原纹理的物理/逻辑尺寸和 pitch，不截窗口、selector 或 letterbox。保留 stride，一次复制可见行，仅清零 padding，保证成功回调初始化整个借用 span；Rust 因而移除全帧零初始化和重复 boot capture。原 EndUpdate/最终画布通知提供 `frame_changed`，静态 tick 不再触发 Family 捕获，不扫描像素判定变化。原标题变更按事件转发，不逐 tick 复制 caption。

音频复用原 WaveMixer 与 OpenAL 的 `ALC_SOFT_loopback`：48 kHz、stereo I16、480 source frames/block。一个可取消 producer 交付 Host 后等待实际观察的 `played_source_frames`，不以 accepted/mixed 或墙钟冒充消费；最多两块/960 source frames（20 ms）领先已观察消费。仅等待刚提交块会必然制造 underrun，故保留一块排队。原单个 source 的播放偏移仍来自原 OpenAL，未声明能逐 source/pitch 精确映射设备 timestamp。借用 context 不由原 mixer 释放；停止 producer 并 join、原 VM/wave 关闭、原 image loader join、释放借用 renderer wrapper 后才关闭 context。原 sound-buffer 析构先删 source 再删其 queued buffers，避免 OpenAL buffer 仍被引用。

实际独立原 mixer 探针在 Host 消费停滞时观察 accepted=960、consumed=0、原 source=960，非零输出 40,196 samples，AL error=0，阻塞消费等待的取消/join 约 198 μs。正式 NativeOwner 的原 TJS/Window/Layer/WaveSoundBuffer 另在 NVIDIA RTX 4070 Laptop/OpenGL 上通过：raster/stage=640×480、pitch=4096，307,200 个可见像素均为指定蓝色；键盘 A 进入原脚本并变红，原 pointer 点击 (123,234) 进入原脚本并变绿，所有 padding 为零。修正嵌入输入曾绕经 Cocos world/stage 往返造成 x=122 的精度损失：改为直接复用原逻辑坐标事件入口，独立窗口仍走原转换，不修改渲染数学。20 个静态 advance 均未声明变化；原 caption 和 typed SetFullscreen(true) 已观察。原 WAV 解码/混音产生非零 PCM，直接 native 启动/关闭退出 0。

随后使用正式生产 Rust child、三个独立 transport lane、打包后的真实 DLL/Resources 运行三次 fresh-child 开启/捕获/键盘变化/关闭：原蓝色和红色全可见像素逐字节匹配，静态不捕获、全屏命令交付。Host 明确使用 TestConsumption，而不是设备听音；三次 accepted source frames 为 45,600 / 45,120 / 47,040，非零 I16 samples 为 76,577 / 75,617 / 77,315，close 分别 129.6632 / 147.1881 / 136.0601 ms，全部正常退出。不能把这些公开夹具结果称为商业游戏、视频/BPG、原生存读档、听音、长期内存稳定或四平台验收。

首次真实 child close 失败，未计通过。定位到取消已生效后 PCM acknowledgement sender 关闭仍被当 Protocol，并经 native callback 变为 Audio；现阻塞接收返回后重查取消，取消引起的队列关闭不制造新失败，真正此前错误仍由首错状态保留。同步修正 progress 辅助 channel 关闭不能令 control writer 提前退出、丢失随后 Stop 的风险。三次真实生产 close 在修复后通过；前两次失败与临时诊断不改写为成功，诊断 scaffolding 随后移除。完整实际游戏流程与正式 Manager 集成仍待下一步，不提前加入已验收核心清单。


### 实际 KrKr 游戏黑屏与原路径对照

使用现有 Limelight Lemonade Jam 的独占 ignored 副本，未改来源素材/脚本或既有商业存档，按用户允许的 headless 方式运行。观察原唯一信息按钮后明确确认，不默认回答；真实原 VM、脚本/字体/插件初始化继续完成，原 NVIDIA/OpenGL 交付 raster/stage=1920×1080、pitch=8192。实际输出始终黑色，输入后有更新通知与非零原混音 PCM，不能据此声称可玩、音画同步或存读档通过。原生游戏结束前的多次 Host close 均正常退出，单次约 34–72 ms；前两次诊断中断不计正常退出。

没有把黑色直接归因为帧缓存：临时只读 GPU 比较对原纹理执行直接采样，与原 GetPixelData 的黑色一致，GL error=0；原 Cocos framebuffer 随启动由白色过渡为黑色，后续也为黑色。该 framebuffer 只用于诊断，不作为 Family 输出，不做窗口裁剪、软件 fallback 或第二套 renderer；相关临时 GPU 诊断代码已经从正式 NativeOwner 移除。

另运行同一固定核心及必要 Windows 构建边界改动的原路径对照：不安装 EmbeddedSession/EmbeddedAudio，不调用 NativeOwner 或 transport，不使用 Host PCM，保留原 OpenAL 设备输出；仅保留模态钩子以明确答复原提示。原 AppDelegate/Director/原文件选择入口与 per-game opengl 配置实际运行，NVIDIA RTX 4070 Laptop、`IsSoftware=false`、Application active=true。启动完成后按真实时钟推进 3,600 tick/约 60 秒，原最终画布 2,073,600 个可见像素的 nonzero RGB 数为 0，原 SystemUninit/Application/Director 关闭、进程退出 0。首个对照探针把模态等待计入 deadline 并补跑 tick，未用于性能结论；修正等待与启动时钟后才取得上述有正常 5 秒间距的对照。此证据排除了“仅因新增 Host PCM/IPC 或新帧 lease 才出现”的简单解释，但尚未定位游戏兼容性根因，不把比较等价于原始未修改上游或完整故障归因。

公共补充场景实际执行原 PNG 解码、两层 GPU alpha 合成与 Arial 原字形绘制：opaque/half-alpha/transparent 区域分别读出 `[199,45,25,255]`、`[116,61,93,255]`、`[33,78,161,255]`，真实输出可见文字。首个临时脚本漏设子 layer 逻辑尺寸，只显示底层；显式设置尺寸后通过，只改夹具，没有改核心或期望掩盖故障。故简单填色成功不是全部图像/文字路径的替代证据；PNG/alpha/字形通过也不能代替实际游戏黑屏验收。下一步应沿原核心定位游戏兼容性/资源或呈现问题，优先整理可复现上游缺陷，不扩写 VM、shader 或另一套渲染。

第一次实际游戏诊断在 NativeOwner 返回后导入 Pillow→NumPy 阻塞，Python faulthandler 显示停于该导入，尚未调用帧复制；不是已经证明的 native capture 卡死。LLDB-DAP 在调试命令后断开并使该次进程结束，后一次只终止了自有阻塞探针，均未计正常关闭。改为原进程仅输出 raw pixels、在外部解码后完成实际 GPU/关闭观察，没有为此修改核心、降级 GPU 或修改系统库。

同一后续实际游戏进程的私有提交采样如下；该场景没有连续增长证据，但三点、单场景/单会话不能排除其他生命周期或媒体路径泄露。工作集也不是 GPU 驻留总量。

| 时间 | PrivateUsage bytes | WorkingSet bytes |
| --- | ---: | ---: |
| 18:12:56 | 729612288 | 317788160 |
| 18:13:49 | 650792960 | 328192000 |
| 18:15:14 | 625147904 | 322678784 |

### 私有 v3 与原生日志元数据（过程记录）

PCM consumption-wait callback 改变 BootOptions/Callbacks layout，changed-frame 回复改变 wire payload；本阶段曾将私有 native ABI/wire 同批升为 v3，公共 Family 仍为 v13。旧 v1/v2 必须整套重建，在版本检查/读取 body 前拒绝，不沿用 v2 的相同版本号调用不同布局。当前私有版本已因下节可写目录契约升为 v4；各次运行证据按实际版本记录。

继续核查真实输出发现原 `[error]` 被按 stdout 降为 Info、Debug 未按原 Windows main 启用。Family adapter 现安装一个共享 spdlog→既有 stdio drain 的有界元数据桥；按原 main 启用 Debug，每条物理行显式携带原 source/level，普通字符串、Debug/Display 与含分隔符/换行的正文正常转发，不对 payload 作白名单或敏感字段筛选，不新增文件 sink。parent 在调用 Host enabled 前恢复 typed level/source，critical 映射到 tracing/现有 DiagnosticLevel 的最高 Error；原始未标记依赖 stdout/stderr 仍按已有原始流诊断处理，不从任意 message 猜级别。超长 source 或截断 message 计入 dropped_fields；队列丢弃仍有独立 records 计数。桥初始化失败阻止 boot。


私有 v3 的生产运行包另完成三次 fresh child：蓝色/键盘红色画布、静态无变化、原 WAV 非零 PCM 和正常关闭；第三次由原 TJS `System.exit()` 到达真实 Finished，再正常关闭，不退出整个 Host。原 native Error/Debug/source 均通过生产 parent 以 typed 诊断观察，非文本猜级别。三次实测如下；仍为 TestConsumption，不是设备听音或正式性能。

| v3 session | close ms | accepted source frames | 非零 I16 samples |
| --- | ---: | ---: | ---: |
| 501 | 205.701 | 46560 | 71313 |
| 502 | 144.859 | 46080 | 71088 |
| 503 | 228.803 | 48480 | 70841 |

### 原生可写目录、初始焦点与私有 v4

发现 Cocos Win32 `getWritablePath` 调用 `SHGetFolderPath(CSIDL_LOCAL_APPDATA)` 并按 executable basename 创建目录；单独覆盖 `LOCALAPPDATA` 环境变量不能隔离该路径。此前探针的环境变量设置不构成完整全局偏好隔离证明，不清理归属不明的用户目录。当前改为 parent 为每个 child 持有独占 TempDir，经 private boot 明确传入，adapter 在 AppDelegate/偏好初始化前调用原 `FileUtils::setWritablePath`；目录只在 child 完全退出和全部 parent worker join 后释放，包括硬杀失败路径。只隔离原 Cocos UI/内部缓存，不重定向 per-game preference 或原游戏 savedata；无新增核心文件/存档实现。

private BootOptions 增加 typed UTF-8 writable-root span，Callbacks 仍 56 bytes、BootOptions 现为 120 bytes；native ABI 与 wire 当前同为 v4，v1/v2/v3 均拒绝，公共 Family ABI 保持 v13。高层 parent Boot 只接受 game/window，实际进度与可写目录由唯一 owner 构造，调用者不能伪造进度或借用全局可写目录。使用已有 tempfile crate，未升级依赖版本；C++ 只调用原 FileUtils 能力。

实际 transport 回归令 child 在该目录保持一个 Windows 独占文件并忽略 Stop：Host 取消后约 5 秒超时，parent 杀死/reap child、join worker 后才移除目录。该回归首先揭露“显式取消后真实硬杀仍返回 close 成功”，失败已保留；现将本地 kill/wait/join 的失败独立于迟到 native reply 的取消优先级，close 必须报告 Shutdown，修复后同一回归通过。原始 native 首错仍优先，未用延长 timeout、隐藏错误或不 join 处理。

另实际复现 initial_window.focused=false 时原 Application 仍 true；adapter 现在在启动脚本前调用原 OnActivate/OnDeactivate，不保留无消费者的 focused/visible 副本。实际原 GPU 场景修复前 GetActivating=true，修复后 false，均正常关闭。普通事件仍调用原激活语义，不修改 VM 或全局输入实现；不可把隐藏 native window 的 OS 焦点当 Host 焦点。

v4 已通过真实动态 Family DLL→生产 child→原 NativeOwner 的三次 open/frame/advance/close：初始真实帧、20 个静态 tick 的 Unchanged identity、跨会话 epoch=1/2/3 与关闭后仍有效的旧 CPU lease 均观察通过。Windows canonical `//?/` 路径在该实际读写中可用，没有因为推测添加前缀剥离或新路径依赖。随后原 TJS Dictionary binary save/load 在独占游戏 savedata 中跨三个 fresh child 读回并递增为 4/5/6；session UI/cache 目录不同且被清理，原存档仍在。只是原存档原语与适配生命周期证明，不等于商业游戏菜单/槽位/完整路线验收。

保存探针首次误把原静态 Dictionary.saveStruct 当实例成员调用，原模态明确报 Member does not exist；只修夹具，复用仓内 `incontextof` 约定，未改原 codec。该次启动失败还揭露 PCM 提前断开/ack abort 抢先覆盖最终 native Modal 错误为 Protocol；现在启动阶段的 PCM I/O 断开先取消输出、等待 control 提供原始失败，运行阶段断开仍明确 Audio，损坏协议仍失败，进度失效不能返回 NativeReady。实际诊断修复后保留 Modal；随后保存原语成功。另一次三次会话本身正常，但临时观察器未考虑原 console message 的前导空白而未计数，探针断言失败；仅修观察器，最终输出 4/5/6，不改生产日志 payload 或隐瞒失败。

上述 v4 运行保留原核心对公开夹具缺少可选 system/plugin 目录的 Error 输出，已验证级别来源正确转发，不抑制原警告来获得“干净”输出。实际游戏黑屏的先前失败继续保留，不能以公共保存原语或 DLL 成功加载代替游戏验收。

版本记录校正：原文字把复制的 SevenZip port 误记为 24.09；实际 `e456309491fd875487bf02b8a0dca76c1b1b00cc:ports/7zip/vcpkg.json` 与当前 overlay、安装日志均为 26.4，上游 archive/tag 为 26.04。这是文档纠错，不是本次重新升级依赖；FFmpeg 仍固定 3.3.9。


### KrKr child 已发送帧缓冲复用

进一步收敛新 NativeOwner 的热路径：原 capture 每次为 stride×height 新建 Vec，尽管 child 的帧数据发完即可复用。现在由同一 frame writer 在真正完成 IO 后交回缓冲，VM owner 通过容量 2 的返回队列获取，按需保留容量、清空长度后让原同步 native copy 初始化完整 span。不增加线程、不阻塞 owner 等待缓冲、不共享 parent lease 的可变存储；initial/in-flight 时才按需分配。队列和单帧 64 MiB 边界保持，静态未变帧仍不回读。仅修改私有 Rust buffer 所有权，不改 native/wire v4、原 VM、GPU 或上游合成路径。

实际动态 Family/生产 child/原 NVIDIA GPU，公开原 TJS 通过 A/B 键触发 50 次交替红/蓝画布，最终 generation=51；每次全部可见像素正确，持续持有的最初蓝色 parent lease 在全部更新及 close 后仍正确。临时 child System allocator 计数只统计 Rust 的 ≥1 MiB alloc/alloc_zeroed/realloc，未统计 C++、GPU、Host 或设备音频，不能据此宣称全链路零分配。

| 同一 50 次原生变化场景 | child Rust ≥1 MiB 申请次数 | 整个观察循环 ms |
| --- | ---: | ---: |
| 修改前 | 51 | 3980.526 |
| 修改后 | 1 | 4223.842 |

分配目标已实证达成；单次总时间包含显式 pacing、完整像素检查、IPC 与 GPU，不是吞吐/FPS 测试，修改后该次总时间反而更长，不能写成产品提速或四平台达标。首次临时 helper 错用不存在的 KeyModifiers::default 导致编译失败，只改为现有 typed 字段，没有修改公共 API。临时全局 allocator/计数输出已从正式 child 入口移除，后续检查使用正常 allocator。

### 原启动对照补正与标题等待定位

继续比较原 Windows 启动链，发现嵌入入口取消 selector 的 `launch` 后，漏调用其中原有的 `TVPGlobalPreferenceForm::Initialize()`；原 Cocos Windows 主循环还在 `Director::mainLoop` 后调用 `GLViewImpl::pollEvents()`。Family adapter 现恢复这两个原调用，不改核心 VM、渲染或调度实现。正式 native target 编译通过，实际游戏仍使用原 NVIDIA GPU，补齐后黑屏仍存在，不能把这次修正称为黑屏修复。

此前“原路径对照”同样取消了 launch 并手动推进 Director，也漏了这两个平台步骤。因此该对照不是完整原平台循环，不能独立证明故障属于上游，也不能排除两个入口共有的启动适配问题；其黑色输出和正常关闭测量仍是真实观察，归因范围必须收紧。

临时有界只读诊断确认：原 VM 已从 first.ks 推进到 title.ks 的 wait 标签，conductor.status=0；原窗口只有一个，且为 visible/current/main，最终画布仍为 1920×1080 黑色，不是误选辅助窗口。原 SystemHook.currentState 为 title，已观察到标题 start hook 对象；不能因通用 kag.hookMap 中没有标题键就断言 hook 未注册。此轮原游戏会话显式确认已观察的信息框后继续运行，正常关闭 150.250 ms、parent 退出 0；仍未达到可玩、视频或完整存读档验收。

以上窗口、成员与资源读取仅为临时诊断，素材与读取结果留在独占 ignored 工作区，不进入公开文档。正式交付前须删除这些诊断钩子并重建；本节不将含诊断的运行包记为最终发布验证。

### 迁移上游 HEAD 前的适配规模与“最小”判断

本节对独占整改工作树实际计数，不代表已提交或可发布版本，也不包含其他实例尚未整合的实现。口径如下：

- Family 为各 crate 的 `src/` 与 KrKr `native/` 源码物理行，包含注释、空行和 src 内测试；排除独立 `tests/`、`examples/`、文档、构建配置、生成物。它是当前源码总量，不是本轮新增量，也不是剔除注释后的 SLOC。
- 核心差异使用固定基线到当前工作树的 `git diff --numstat --no-renames`，加上尚未跟踪的实际源码文件；仅计 Rust/C/C++/头文件等源码，不计 lockfile、清单、CMake、文档和 patch 文本。插入与删除分别列出，不能用净增长掩盖改动面。
- “本轮”比较恢复起点，不代表全部是性能修改：同时包含必要恢复、嵌入和生命周期正确性修复。未取得的 Siglus/Artemis 原 gitlink 不能作为可复现统计基线。

| 引擎 | Family 当前源码行 | 核心相对上游：源码文件 / 插入 / 删除 | 本轮核心差异：源码文件 / 插入 / 删除 |
| --- | ---: | --- | --- |
| FVP / RFVP | 4,686 | 74 / +6,016 / −610 | 6 / +297 / −83 |
| Siglus | 980 | 10 / +605 / −95 | 3 / +85 / −55 |
| Artemis | 2,056 | 11 / +295 / −151 | 6 / +244 / −148 |
| KrKr | 7,541 | 15 / +320 / −68 | 15 / +320 / −68 |
| Musica | 25,974 | 非外部原核心 fork，当前为自有实现 | 不适用同口径 |
| CMVS | 28,520 | 非外部原核心 fork，当前为自有实现 | 不适用同口径 |

上游基线分别为 RFVP `304e773387a9920c9db091ec1fd937c717aea949`、Siglus `e762f9f9c1c94cb490fb13f1367fd06f3e1367b4`、Artemis `0c06f37160961c9ff75d4937d5e6bb0500d0bef9`、KrKr `7105b50822097d11b7b1f5894bf541509fd51350`。本轮起点分别为 `b2f385da486342de6e23202cccad8141ac6423b6`、`c6c4f99ccac855966b726909756afe3200488374`、`13e55ffa98b8ea2fec4b294d210bd50567106a08` 与同一 KrKr 上游提交。

KrKr Family 其中 Rust 6,643 行、native C++/头文件 898 行；Rust 还承担进程隔离、三 lane IPC、Windows 可取消 IO、模态和诊断，不能把全部算成侵入原核心的代码。核心差异的 15 个源码文件包含 Cocos overlay 的 23 行 PVRTC 导出头；剔除此依赖边界为原核心 14 文件、+297/−68。未修改 layerExImage 子模块。公共 Family API、SDK、Manager 不重复分摊进每个引擎，故本表也不是整个 EMU 产品的总行数。

**结论：不能一概宣称已达到性能优先的最小适配。** 当前主要设计遵循必要边界，最终“最小”仍需要逐文件职责与实际产品回归，而不是行数排名：

- **RFVP 累积差异最宽，仍需重点收敛。** 历史 hosted/no_std_core、资源与保存生命周期构成大部分差异；不能将其全称为本轮性能改动。也不能为缩小数字删除原生存档、文字等待和已依赖机制。本轮限定在 6 个原核心源码文件，复用原 GPU 捕获、分离/复用 readback 和必要保存边界，没有另写场景 renderer。
- **Siglus、Artemis 的核心侵入较窄。** Siglus 复用原 wgpu/Kira，桥接离屏输出、PCM 与取消；Artemis 复用原 GPU/FFmpeg，Family/SDK 承担核心原本没有的实际 PCM mixer。原虚拟时钟、媒体释放、读档正确性分别属于已有嵌入或核心修复，不把它们包装为免费的性能收益。
- **KrKr 把主要复杂度留在 Family，原核心修改集中于可选服务/事件和构建边界。** 不另写 VM、shader、decoder、mixer 或保存格式；静态帧不重复回读，已发送的 child 缓冲按所有权复用。但三 lane IPC 与原 GPU→CPU→Host GPU 路径仍有真实成本，不是端到端零拷贝。商业游戏仍黑屏，不能称为已验收的最小适配。
- **Musica、CMVS 是重实现，不是少量嵌入补丁。** 两万余行包含 VM、格式、脚本与呈现业务，不能与成熟核心 Family 行数直接比较；性能策略应优先复用 SDK/公共 GPU、文字、媒体与字节源，并另审重复实现，不为统一 Host 再写成熟核心路径。
- 已观测的 CPU 发布、alpha 声明与帧缓冲分配改善不等于原生等效 FPS。当前未完成原生与 Hosted 同场景的帧时间分布、输入延迟、设备欠载及四平台对照，因此“性能不回退”仍未整体验收。

本轮统计前已删除临时 `InspectWindows.h`、NativeSession 中的帧诊断和文件触发检查，正式 native target 重建通过。正常 allocator 的生产 child/动态 Family/原 NVIDIA GPU 再次执行 50 次交替变化：generation=51、全部可见像素正确、最初 parent lease 跨更新及关闭保持有效，退出 0。本次无分配计数器，不能从空计数数组声称零分配；临时交互 example 与测试素材仍待最终清理，不计入表中适配源码。

### 用户范围调整：跳过 KrKr

用户在本轮统计后明确要求“跳过 krkr 的适配，进行下一步”。停止 KrKr 新增适配与游戏黑屏调查，保留独占工作树已有代码、失败与实测记录，不回滚来源或改写为适配通过；不将 KrKr 列为本批核心/gitlink 整合与验收目标。前一轮独立 executable 完整构建已执行，但尚未启动新的完整原平台循环对照，不虚报运行结果。后续优先完成 FVP、Siglus、Artemis 的提交整合和定向/工作区验证，公共接口、Manager、SDK、Runtime、Editor 的既有性能整改继续保留。

### 上游 HEAD 单适配提交整合（进行中）

用户随后明确要求：当前适配必须直接基于上游 HEAD，仅新增一个适配提交，并以 submodule 集成到 ThirdParty。本要求替代本轮此前固定旧上游提交的整合目标；KrKr 仍按上一条范围调整跳过。核对时间为 2026-10-16 00:49:15 +08:00，以下为本次解析的不可变上游基线，不使用浮动 branch 作为最终 gitlink：

| Family | 上游默认分支 | 实时 HEAD |
| --- | --- | --- |
| FVP | xmoezzz/rfvp main | `af557db750c7833274f22a6d82b0dc33cdea2f85` |
| Siglus | xmoezzz/siglus_rs main | `5f358ef1360b0439e92aed2b749e8f25d8721954` |
| Artemis | Alphaly2K/art3m1s-core master | `ad90a62cb192b119b7cf8589aa7f87a94bd747c8` |

旧实现分别保存在原独占分支的新本地快照 `a721a8afc62059010004a6afbd20668b91288ed1`、`f9bd2e1b503e8c9e009d7fd2050c04d13def072f`、`e6e26529f1327c2797d692d616e4f83d38b6027e`，原发布/来源历史未重写。新的 `codex/astra-upstream-head-20261016` 分支直接从各自上游 HEAD 创建，正在处理真实 API 差异；这些保存快照不是最终单适配提交，不能用它们更新最终 gitlink。

Siglus 上游已升至 1.0.0，渲染、过程流及原生 load-completed 入口显著更新。迁移保留新上游的实际 VM/渲染和原生实时时钟，不复制旧过程流、不在游戏 session 启用上游注明仅供 replay 的 virtual-clock；Family 原有 hosted_clock/deterministic_frame_clock 消费者随之删除。离屏输出继续调用同一原生绘制函数，读回目标与完成队列按输出尺寸持有复用，而非逐帧新建 GPU readback buffer。尚未完成新基线回归，旧统计和旧基线测试不能覆盖本次迁移。

新 Artemis 上游使用 ffmpeg-next 9.0。已撤销旧适配将其降为 8.1 的清单改动，保留上游媒体版本；根/EMU 共享依赖同步到 9.0，以解决 Cargo 的原生 links 单版本约束，并须重新验证公共媒体消费者，不把依赖更新称性能收益。本机实际 libavcodec major=63，使用对应原生安装；Artemis 原生 Vulkan/FFmpeg 9 的 library check 通过，保留上游 warning。此前错误的 shell 引号启动失败、离线缺失 feature 解析失败，以及临时 FFmpeg 8 check 均不作为最终通过证据。

Artemis 新增的可选 Siglus 桥原来引用相邻目录并要求私有 `art3m1s-host-audio` feature；不能让它意外消费独立 Astra Siglus Family 的上游 1.0 核心，也不能删除该 feature 或伪造实现来消除解析错误。已核实原作者公开 fork 实际提供该 feature，改为精确 Git revision `Alphaly2K/siglus_rs@28917920a3e5eeb9a5fa60eac31400422cd41fe2`。这是保留可选原功能的依赖闭合，不将该桥启用为 AstraEMU Siglus 的另一执行/渲染路径。

三个核心的单适配提交、父提交和单提交计数已核对，主仓通过模式 `160000` 的精确 gitlink 固定，见下表。只作本地整合，远端发布与完整产品/平台验收不是本次检查的通过结论。

### 必要适配收敛与旧存档范围澄清

用户再次要求“尊重上游代码，仅适配必要部分，且高性能最小”，并明确当前尚未投入使用，不需要额外旧存档报错代码。本批直接沿用新上游 V1 保存/读取流程，不保留旧 V2 扩展、不增加旧格式识别、迁移、兼容 reader 或专门错误分支；上游原有校验和错误行为保留。旧适配与保存对象仍在原独占分支，不修改任何既有商业存档。

源码复核纠正此前记录：上游 `304e7733` 和当前 `af557db7` 均声明 `MotionManagerSnapshotV1` / `SaveStateSnapshotV1`；旧 fork `b2f385da` 及其保留快照 `a721a8af` 实际声明 V2。因此此前修改记录中“已恢复完全相同的上游 motion layout”的表述不准确，不能作为新基线原生兼容证据。已撤去旧 V2 演出容器序列化和重复 hosted codec，删除旧版本专用错误枚举、格式化分支及纯文案测试，没有将失败改写成成功。

RFVP 正在复用新上游统一的图像、保存和媒体实现：撤去 13 个文件中的旧演出/图像 ownership 和软件 framebuffer 包装差异；原生纹理 trait 回到上游借用接口，最终跨 Family 的不可变 CPU lease 仍保留。旧适配单独实现的 mask pipeline/shader 已移除，遮罩准备和上一帧 snapshot 改调用上游 GpuPrimRenderer 现有接口。保存缩略图按新上游实际调用顺序捕获“脚本新状态绘制之前的原 render target”，使用同一 Triangle 缩放，不再将旧基线的重绘当前状态规则强加给新 HEAD。

原生 hosted-gpu library check 已通过，原有 warning 正常显示，旧适配带入的 `allow(warnings)` 已删除。Family 正在删除旧影片命令转发/重复播放路径，启用原生视频解码、合成与音轨，再补齐新的回归；尚不将编译或待接通消费者写成完整通过。

### 已完成的单提交与新基线实证

| 核心 | 唯一适配提交 | 精确父提交 / 上游 HEAD | parent 索引中的模式 |
| --- | --- | --- | --- |
| FVP / RFVP | `d1ff965d79ba2cdc956aad73ae100cc44bc7ce90` | `af557db750c7833274f22a6d82b0dc33cdea2f85` | `160000`，ThirdParty 精确 gitlink |
| Artemis | `b802ab8eb397d12c623df793ad9feb8d3ae0b5a9` | `ad90a62cb192b119b7cf8589aa7f87a94bd747c8` | `160000`，ThirdParty 精确 gitlink |
| Siglus | `8bf5024fb3975bb8dc375874380f4574ecedf4d8` | `5f358ef1360b0439e92aed2b749e8f25d8721954` | `160000`，ThirdParty 精确 gitlink |

三者 `HEAD^` 均等于列出的上游 HEAD，`rev-list upstream..adaptation --count` 均为 1；最后再次 `ls-remote HEAD` 核对三上游未变化，三核心工作树均 clean。尚未推送，不能声称远端全新 clone 已可重现。

Artemis 新 HEAD 下原生库 339 项通过/1 忽略，PF8 11 项通过/3 忽略；实际 Family 源码独立清单 13 项通过，另显式 2 项硬件 GPU 回归通过。实际 NativeVulkan/FFmpeg 9 非商业 PF6 冒烟推进 150 次，18 次变化、132 次 Unchanged、15 个彩色帧、237,120 个 PCM samples（233,848 非零）；正常关闭、取消及旧 CPU lease 均观察通过。明确为 TestConsumption，不是听音/商业路线/四平台验收。

Siglus 1.0.0 的原桌面默认 feature 和 hosted feature library check 均通过；Family library 3 项普通回归通过，另显式 GPU 像素/非对齐行宽 resize 回归通过。实际独立程序在 NVIDIA RTX 4070 Laptop / DiscreteGpu / Vulkan 上运行 `63×47 → 65×49 → 63×47`，全像素分别正确、generation 为 1/2/3，关闭后旧 lease 保持。初次 `--exact` 使用短名筛选零项未计通过，完整路径重跑实际执行 1 项。两核心 Clippy 执行成功并保留上游 warning；fmt 检查中未修改的上游文件差异如实保留，没有为全绿扩大 diff。完整 workspace、新基线真实游戏及平台验收仍待完成。

### RFVP V1 与最小核心收敛实测

新基线下删除 Family 的第二套 WMV 播放器（原 `video.rs`）、旧 V2 保存测试，以及未被编译或消费的 hosted global codec 与版本/变量计数专门拒绝分支。只使用上游统一保存流程，不额外识别或迁移旧文件。无消费者的 Evidence/指令 trace ring、文本事务队列、复制遥测和 scene-delta 协议一并撤去；原 VM 热路径不再保留额外指令记录调用，打印文本不再为未使用的事务队列复制字符串。

Astra 专用命令收集和变化检测已从核心 `hosted.rs` 移到 Family `embedding` 模块。只观察原生元数据和资源身份，不复制纹理、不计算像素 hash、不绘制另一场景。当前只需要整帧输出，故删除无消费者的矩形 damage 与捕获模式；复用两组命令 Vec，借用播放状态，避免每帧临时 recorder 的状态分配和无效 Tick 命令分配。核心只保留必要的原生嵌入、GPU/音频/字体与生命周期边界。

回归实际复现上游 V1 的两个可见错误：路径重载将保存坐标 `(3,5,1,1)` 覆盖为零；GPU 调色图读档后从 `[127,63,191,255]` 变为白色。不是旧版本报错。修复复用 V1 已有 RGBA 与几何字段：修改像素仅在保存边界捕获，读档优先已有像素，原资产重载后恢复几何；没有新增保存 schema、迁移或逐帧 snapshot。失败前输出保留，修复后原地/冷恢复的真实 GPU 像素回归通过。

迁移后的 FVP Family 普通回归 23 项通过、3 项 GPU 按原条件忽略；随后显式执行 3 项 NVIDIA 硬件 GPU 回归全部通过，覆盖原生文字恢复、色调像素与原 mask 管线。Family 全目标严格 Clippy 通过，上游 warning 未屏蔽。实际原生 WMV 异步解码/墙钟播放、后续变化帧、EOF 恢复游戏帧及关闭后旧 lease 已由回归执行；原测试强行要求首次 tick 立即呈现并通过虚拟 elapsed 跳至 EOF，已改为等待真实解码和播放，不改变核心时钟。

独立原桌面 default feature library check 通过。过程中发现旧 app 仍引用已删除的第二套 mask，已恢复上游 app 的实际渲染流程，仅迁移三处复用 readback 的调用和其所有者初始化。原生库全测试尝试在未修改的 `texture.rs` 测试中因缺少 `std::io::Read` 导入出现六处 E0599，且该组测试依赖原 testcase 文件；未删测试、忽略或扩大核心修改来凑全绿，不计全测试通过。全 workspace fmt 首次检查还显示大量未修改上游格式差异，后续限定活动产品 package，保持 fork 差异最小。

原 bare `no_std` 检查同时报告 cdylib 所需 allocator/panic handler/abort 条件及旧适配遗留的 BGM 参数分支。已恢复原生 BGM syscall，删除无消费者的 load-signal/pending-count helper；用 `cargo rustc --no-default-features --features no_std --lib --crate-type rlib -- -C panic=abort` 实际编译通过。没有为桌面嵌入向核心添加全局 allocator/panic handler，也不把 rlib 编译作为原 PSP/PSX 平台运行验收。

本次相对各自最新上游 HEAD 的最终整提交规模（含 Cargo 清单、lock 和必要回归，不与历史“仅源码”表混算）：

| 核心 | 修改文件 | 插入行 | 删除行 |
| --- | ---: | ---: | ---: |
| RFVP | 35 | 1,533 | 158 |
| Siglus | 11 | 557 | 67 |
| Artemis | 15 | 1,358 | 274 |

RFVP 的差异已不包含旧 V2 容器、第二 mask shader、Family WMV 播放器或 Astra hosted 事务协议。仍保留 session globals、真实 GPU/音频/字体与保存生命周期，不能只为减少行数删除可靠性。此表证明差异规模和边界，不证明同场景帧时间不回退。

Editor 在公共媒体依赖同步后再次完成 fmt、全目标严格 Clippy、build 与完整测试：18 项通过、3 项按原条件忽略；原 `proc-macro-error2` 未来兼容警告保留。商业 FVP 素材已复制到 ignored 独占运行目录，未复制或修改原商业存档；后续从实际 Manager headless 入口运行，不以夹具替代真实游戏。

### 实际游戏暴露的像素所有权缺口与修复

使用实际 Manager `--headless`、重建的 FVP 动态库、原 HCB/资源与 NVIDIA RTX 4070 Laptop / Vulkan / 610.88 驱动，在 ignored 独占副本运行商业游戏，未复制原存档。首次运行 1,200 次 advance 虽正常退出并交付非零 PCM，但实际检查保存的画面全黑，不能记为视觉通过。

根因是新上游 portable Core 会按 host texture cache 的“已上传”标志回收原资产 CPU 像素，而 Family 的通道只观察元数据，并不拥有像素。它在原生 GPU rebuild 前错误授权回收，原 GPU 从未得到纹理。已补完整 Family `GraphLoad→PrimSetSprt→原 GPU` 回归，首先复现黑像素失败；修复后准确白色纹理、相邻黑底、静态 generation 与关闭后旧 lease 全部通过。

修复不替换 GPU 或永久保留 CPU 图片：便携元数据 cache 不再授权 GPU 路径的回收；原 `GpuPrimRenderer.is_graph_uploaded` 才是权威。使用原恢复/回收函数，在原 GPU rebuild 真正上传后回收 pristine CPU 像素，与原 app 的策略一致。最终仍直接基于上游 HEAD 的一个适配提交，只有必要的所有权边界变化。

修复后同一真实入口运行 1,200 次 advance，57.10 秒后退出 0；已实际查看原生标题菜单，记录 1,932,000 rendered audio frames、2,543,969 非零 samples，关闭成功。动态导出模式 25 项普通回归及另 3 项硬件 GPU 回归通过，Family 全目标严格 Clippy 通过。57.10 秒包含 debug 执行、回读、截图与初始化，不将它写成 60 FPS 或同场景性能无回退。

本次每秒采样 private bytes 峰值 271.88 MiB，后段连续采样维持约 271.88 MiB，进程正常退出；没有观测到该短场景持续无界增长。未采样 GPU 驻留，也未完成长时间/跨路线循环，因此不能据此断言完全没有内存泄露。错误黑屏场景的更小占用不能作为性能收益。

Siglus 实际游戏首次在 Windows main 线程因 `0xC00000FD` 栈溢出退出，GPU 小场景通过不能覆盖完整 VM 构造。Family 正改用新上游已经提供、异步包装实际直接调用的同步构造函数，消除无用 Future/阻塞 executor 层；不为此改原 VM 或新增兼容 Host，待实际复测再判断。

Artemis 实际商业 PFS 副本已成功挂载、选择原 NativeVulkan、打开并输出首帧，随后产生 typed 窗口请求。无窗口 headless 按契约明确返回 `ASTRA_EMU_HEADLESS_WINDOW_COMMAND_UNAVAILABLE`，正常取消/关闭；本次未记录具体请求 variant，不将其猜作全屏或游戏失败。这不是完整游戏流程通过，也不通过吞掉窗口请求或假窗口绕过。原商业存档未读写，完整窗口流程仍未验收。

Engine 在共享媒体版本同步后重新执行 fmt、全目标严格 Clippy、build 与完整测试：870 项通过、22 项按原条件忽略；保留 `binrw` 未来兼容警告。Emulator 首轮新基线严格 Clippy/build/test 为 626 项通过、37 项忽略，之后实际游戏新增的驻留/同步构造修复须补最终验证。独立旧 decoder 工具的 3 项回归通过；当前 FVP 不再直接依赖它，打包已撤去错误的旧 decoder 归因，改保留实际 Family 与原 fork 许可记录。

Siglus 同步构造的单行改动本身未解决 Windows main 栈溢出，已保留该失败，不将其虚报修复。最终由 Manager 给 GUI 与 headless 固定 owner 一次性保留 8 MiB 栈，headless 的加载、创建、推进、取帧与关闭全部留在该 owner，不跨线程移动 VM/GPU、不改原核心。实际同一游戏推进 600 次，55.23 秒退出 0；已查看原生标题菜单，TestConsumption 记录 1,748,160 rendered audio frames、2,532,902 非零 samples。Manager 定向严格 Clippy、build 与 68 项当时的回归通过，4 项原条件忽略。

Siglus private bytes 峰值 479.56 MiB，末段约 475–479 MiB；这只是短场景有界采样，不是长期无泄露证明。实际日志还保留了初始化候选 D3D12 后端的 resource-state 错误及正常关闭后 DXGI `Live Object : 18` 警告；随后实际选择 NVIDIA Vulkan 并正确绘制。尚未确定警告对象类型、是否随进程内重复开关增长，不能屏蔽警告或据 CPU 采样宣称 GPU 无泄露。该原后端/驱动问题列为后续原生对照与上游 issue 候选，不以重写核心渲染解决。

实际 headless 路径此前还为每个变化帧立即分配/转换整幅 RgbaImage。已改为仅保留最终不可变 CPU lease，真正请求截图和最终 PNG 时才执行原行序、通道、色彩空间和 alpha 转换；Ready/Pending/Unchanged 的校验和有界最新帧所有权不变。删除对应的纯复制回显测试，保留输入/限额/错误边界；最终产品复测与全 workspace 检查继续补齐，不提前报告加速比例。

最后复核新 HEAD 迁移还发现 Siglus Family 将每次逻辑 step 都标为 frame_dirty，缺少原生绘制边界会导致无新提交也回读。最终小钩子只在同一原生离屏绘制成功后推进 generation；Family 观察该值，不用 VM 帧数代替绘制、不做像素 hash。generation 位于 renderer 而非可重建 target，`63×47 → 65×49 → 63×47` 的真实 GPU 回归同时确认精确像素和不会与旧 CPU frame identity 别名；3 项普通及显式 1 项 GPU 回归、严格 Clippy 与动态库构建通过。Siglus 仍为上游 HEAD 的一个适配提交，未增加核心执行或绘制路径。

### 最终本地验证记录与边界

按需转换后，FVP 同样运行 1,200 次推进、相同截图请求，退出 0，总耗时 24.20 秒；最终 `1280×720` RGBA 与先前 57.10 秒运行逐字节完全相同，差异 channel samples 为零。它证明删除了测试 Host 的无用逐帧转换且实际输出不变，不作为隔离原生 FPS、帧时间分布或加速比例；两轮不以编译/驱动负载完全相同为前提。Private bytes 峰值 268.91 MiB，关闭正常。

物理 pointer/button 输入在原标题菜单选择 New Game。最初只观察到 1,800 帧内的黑色转场，未把它记为剧情成功，也未新增跳过或特例；同一原输入继续到 3,000 帧、53.09 秒后正常退出，已实际查看场景、原生正文与游戏功能面板。商业正文/截图只留 ignored 运行区，不入本报告或 Git。

Siglus 按需 PNG 与原绘制 generation gate 后，600 帧运行 16.07 秒退出 0，实际画面处于原开场媒体；不能与此前 55.23 秒时已到标题菜单的同帧号图比较为状态回归，因为原生实时时钟被保留，没有启用 virtual-clock。关闭实际生成原生 config/global/read 等文件；再用它们冷启动推进 2,400 帧、50.08 秒退出 0，已查看标题菜单，TestConsumption 交付 1,553,280 rendered audio frames、2,330,342 非零 samples。冷运行 private bytes 峰值 396.77 MiB、末段约 390–397 MiB，仍不作为长期/GPU 无泄露结论。

真实 `package_client.py` 使用实际 Manager/FVP 动态库完成本地打包；从包内 executable 加载包内 Family，再对独占真实游戏运行 120 帧并正常关闭、输出 PNG。包内保留完整 RFVP 许可与当前 Family 说明，旧独立 decoder 的错误归因目录不存在。使用本机明确的原生运行库环境，不宣称这已覆盖其他机器的自包含发行或分发许可审定。临时程序清单、源码迁移 scaffolds 和测试客户端包已清理；原始目录/存档未修改。

最终 Emulator 活动 package fmt、全目标严格 Clippy 和完整 build 成功。整条 Cargo test 命令包含 package-cache 锁等待并在总期限结束，输出停在未修改的 Musica 用例；不能据最后一行认定该用例本身死锁。隔离该用例，带正确原生运行库环境实际 5.06 秒通过。随后绕开 Cargo cache/build 等待，按原 package cwd、相同运行库环境和 `--test-threads=1`，直接执行本次已编译的全部 26 个单元/集成 test executable：627 通过、0 失败、37 按原条件忽略，290.73 秒完成，Musica 为 200 通过/16 忽略。没有删除超时用例、改变忽略条件或缩小执行集合；两组空 doctest此前已成功执行零项。区分原 Cargo 综合命令超时与当前完整可执行测试集合通过，不将前者伪写成一次全绿。超时后自有测试进程已被清理，未杀其他实例；临时 cache-independent runner 随后清理。

构建仍保留两个原 decoder 包同名 lib target 的非哈希输出文件 collision 警告；版本区分解决 lockfile 包身份，不等于重命名上游 C/static/dynamic 输出。实际 Family Rust 依赖与两核心游戏已分别运行，不把额外 helper library 作为产品 ABI 或打包输入；未来 Cargo 兼容/原生构建命名需单独上游对照，未通过删掉上游 targets 或改 codec 消除警告。
