# 全仓性能与 AstraEMU 最小上游适配审计（2026-10-12）

## 文档用途与状态

本文合并本次会话的两轮只读审计，供下一步修改共享运行层、AstraEMU Host、Family 和外部核心 fork 使用。第一轮覆盖全仓性能热点，第二轮深入核对 AstraEMU 接口与上游适配边界。

**这是审计记录和实施建议，不是修复完成报告，也不是性能验收报告。** 本次交付只新增本文并补充索引；没有据本文修改产品源码、ABI、核心 gitlink、配置或游戏存档。仓库外验证程序、动态库和独立构建目录均已清理。

本文不替代 [总体架构](../product/architecture.md)、[重构契约](../contracts/rebuild.md)、[实施状态](implementation-plan.md) 或 [Family API v11](../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)。接口建议尚未实现、定版或升级指纹；后文的“下一版”不能读作已有 v12。GPU/native handle 扩展会改变现有 CPU-only 边界，必须先明确修改公共契约，不能按本文直接视为现行能力。

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
- [Family API v11 契约](../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md)
- [RFVP 修改记录](../../Emulator/Source/Families/astra-emu-fvp/MODIFICATIONS.md)
- [Siglus 修改记录](../../Emulator/Source/Families/astra-emu-siglus/MODIFICATIONS.md)
- [Artemis 修改记录](../../Emulator/Source/Families/astra-emu-artemis/MODIFICATIONS.md)
- [KrKr 适配说明](../../Emulator/Source/Families/astra-emu-krkr/README.md)
- [实际实施状态](implementation-plan.md)
- [开发与验证入口](../manual/development.md)
