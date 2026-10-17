# 实施状态与下一步计划

更新：2026-10-16。当前代码基线 `3c3194a365b14bac41104f6dd889f5e4ba4aff5d`，独占分支 `codex/performance-main-20261013`。总体设计见[架构](../product/architecture.md)和[重构契约](../contracts/rebuild.md)，问题与运行证据统一见[性能审计](performance-upstream-audit-2026-10-12.md)。本页替代按日期追加的进度流水账和旧本地交接。

## 当前结论

- 本轮性能改动已本地整合；**严格最小适配和全产品验收均未完成**。
- FVP、Siglus、Artemis 已按上游 HEAD＋一个适配提交固定为 `Emulator/ThirdParty` submodule。结构通过，内容仍需处理审计 R01–R09。完整 SHA 和差异规模只在审计及各 Family MODIFICATIONS 维护。
- 保持 CPU lease、原生 GPU/媒体/存档、固定 owner 与有界 PCM；先建性能基线，不先定任意容忍百分比。本批只定位内存风险，不承担长时间耐久验收。
- CMVS、KrKr 跳过本批适配/整合，保留既有提交和未提交成果。其他来源工作树不归当前实例清理。
- 已发布独立原生贡献：RFVP [V1 图像 #51](https://github.com/xmoezzz/rfvp/pull/51)、[文字等待 #53](https://github.com/xmoezzz/rfvp/pull/53)、[scratch 分配 #54](https://github.com/xmoezzz/rfvp/pull/54)；Artemis [onLoad #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)、[PF8 #7](https://github.com/Alphaly2K/art3m1s-core/pull/7)、[AVIO #9](https://github.com/Alphaly2K/art3m1s-core/pull/9)。产品及完整适配分支保持本地。

## 产品状态

| 产品/模块 | 已具备 | 尚未完成 |
| --- | --- | --- |
| Engine/Runtime | 独立 EngineSession、typed 状态/任务/保存；有界诊断、worker/access plan 复用；静态 gameplay/UI 消费链 | 完整产品的作用域/媒体/恢复与平台组合验收，性能基线 |
| AstraVN/Player | VnSession、Yakui、公共 Canvas、完整媒体流、typed 输入/存读档、Luau 显式 IO/动作延续 | 授权 IO 的实际产品能力、真实设备音频、跨平台输入/媒体/挂起恢复及长流程 |
| Editor/Agent | GPUI、版本化事务、增量撤销、后台诊断、独立真实 Player 预览和 ACP/MCP 通道 | `--mcp` 持有 stdin 的关闭挂起；图/时间线/Agent 完整工作流、片段 seek 与三桌面验收 |
| FVP | 原 RFVP GPU/mask/视频/保存；portable/hosted 输入、V1/文字原生归因与保存 PC/像素恢复；非阻塞 read-into 和三槽背压 | R09 原生配对与真实槽位流程；功能 PR 合并、至少一结局 |
| Siglus | 原 renderer/Kira/实时时钟；单 staging Poll、三槽与取消/超时；真实 Rewrite+ 窗口标题及独立完成轮询 | R09 三轮窗口/原生配对、新基线菜单/槽位/代表流程、GPU 警告归因 |
| Artemis | 原 Vulkan/FFmpeg；PF8/AVIO 原生修复、硬件策略和恢复通知；功能控制组已自然经过电影到 2025ver 标题 | 功能 PR 合并；R09 商业窗口、存读档、必要宿主请求及现存 BGM crossfade 能力缺口 |
| Musica/SDK | 公共文字/媒体/画布与纹理驻留；标题、剧情、选择、系统页、自有保存已有实现与局部运行记录 | 完整机制整合回归、至少一结局、设备听音；eden 原版互通仅静态片段，旧 1024×640 逻辑尺寸整合待处理 |
| CMVS | 原格式/VM/归档、SDK 画布/文字/音频与 GPU 所有权已接入 | 最近实测首 advance 停在 command 548 / offset 200462，ACTION_UNBOUND；先打通首 tick，再验收剧情、媒体、保存与退出 |
| 终之空转换 | 原生 Story IR、37 路线输入生成、Classic/Modern 私有包与部分硬件运行记录 | Classic/Modern 全部 37 路线及系统/演出/视听验收；不能把历史少量路线结果计为完成 |
| 平台/发行 | Windows 为当前主要实测环境；Linux/Android 有历史构建和部分运行记录 | 当前整合基线四平台代表流程；macOS、Android 真机、可听音频、发行依赖/许可检查 |

历史验证只在对应构建上成立；当前工作以审计记录的最新基线为准。完整测试数量、局部 GPU 夹具和显式 NullAudio 均不代替产品验收。

## 下一步整改顺序

### 1. 修复适配引入的正确性问题

- **R01 已完成**：RFVP 输入统一在 VM 前每 tick 提交一次，删除重复键映射。
- 同一自制 HCB 的按下/保持/松开/repeat/wheel/焦点/激活点击序列，真实 portable 与 hosted `boot/push_event/tick` 均通过；portable 保留首 tick 失败前/通过后证据。

### 2. 将原生功能修复与嵌入钩子分开

- **R02/R03 已归因**：RFVP V1 图像/几何、旧文字等待；Artemis PF8/AVIO 均有独立原生失败前/通过后证据与 PR。RFVP SaveCreate 保留必要 hosted 捕获边界。
- Artemis packed stereo 的纯上游和当前 tuple 控制组均精确保留非对称 PCM 与 WAV 长度，归类为 typed stereo 表达，不报告未复现的缺陷。onLoad PR #5 仍等待合并。
- 上游待合并不阻止 Family/Host 独立整改。保留当前功能，标清依赖；不在 Family 复制核心功能，不撤回必要修复换取小 diff。

### 3. 清理非必要适配

- **R05 已完成清理**：删除 RFVP 无消费者 sha2/热路径 trace，恢复原 partial-read 循环；scratch 优化原生 Release 实测 100 次 build 减少 100 次 4096-byte 分配，作为独立 PR #54 保留。
- **R06 已完成隔离**：上游默认 Any 排序保持原样，Family 使用显式 HardwareOnly；候选筛选与真实硬件生命周期回归通过。
- 每个新增核心入口必须有实际消费者，说明为什么原有 API 不足。保持原桌面/便携入口和原图形库，不为统一 Host 重写 VM、renderer 或 decoder。

### 4. 补齐恢复、退出和诊断边界

- **R07 已实现并验证**：原生成功 load/reset 通知一次，失败保持既有成功通知；真实 Family pointer→load 清 remainder。Manager 的恢复锚点延后到首个 Ready/Unchanged 完成，Pending 期间继续接收关闭，普通 Pending 不停止逻辑时间；调度与窗口/音频生命周期七项回归通过。
- **R04 已完成有界回读**：Siglus 单 staging 使用 Poll 与取消，RFVP 完成路径改为 read-into；两个 Family 使用三个不可变 CPU 槽，提交前预留，持满返回 Pending。硬件像素、跨 resize/取消、超时、背压和旧 lease 生命周期已运行。Manager 增加独立完成期限，避免只随 VM tick 轮询造成隔帧发布。
- **R08 已完成**：仅精确 Family event target 保留有界原值；局部 subscriber、真实 DiagnosticSink→最终 JSON 以及 Release Manager/FVP 的 wgpu 日志和像素烟测通过。普通 target 与 span 继续脱敏。
- 验证失败启动、受阻 PCM、媒体替换、关闭错误后的全部回收、旧帧 lease 与迟到结果。当前仅定位长期泄露风险，不据短时平稳给出“无泄露”。

### 5. 建立性能基线，再实施剩余优化

按下一节执行 **R09**。两核心整改前后各 15 组 headless 样本已采集，包含三轮实际游戏标题/开场和自制静态对照；真实窗口已取得进程级 GPU 完成、present CPU 和新图像上传数据。原版性能测试按用户决定取消，保留原生功能复现。正式验收使用 Sandbox，覆盖正文/电影、新槽恢复及退出；主机记录单列。用户已恢复 `AllowVGPU=1`，重建后 Sandbox 在产品运行前退出，同时间 WER 记录 `0x3B/0xC0000005`；宿主 `26200.9711` 的相关报告见[性能审计](performance-upstream-audit-2026-10-12.md#资源与环境限制)。先恢复 Sandbox 运行环境，再继续硬件验收。保留静态跳过、纹理驻留、F32 owner 直通、非实时回收和按需 PNG。

### 6. 重新固定并验证核心整合

- 三核心已在新独占分支整理为实时 upstream HEAD 加一个适配提交，Family revision 与 `160000` gitlink 已核对一致，具体 SHA 见[性能审计](performance-upstream-audit-2026-10-12.md#固定核心基线)。RFVP PR #51/#53/#54、Artemis PR #5/#7/#9 仍待合并；合并后剔除重复修复，严格最小钩子内容验收保持待完成。
- 核对直接父提交、提交计数、gitlink、Family 依赖 revision 和 MODIFICATIONS；有对象缺失先恢复精确对象，不降级核心或关闭 GPU 来消除解析错误。
- 检查受影响 crate 的 fmt/Clippy/测试，运行 Manager 实际游戏路径；阶段末完成三个活动 workspace 检查。上游原测试错误、依赖缺失、decoder 产物重名分别记录。
- 更新本页状态和审计证据，替换旧记录，不再追加整段过程日志。发布产品/完整适配分支须另按授权执行。

### 7. 继续产品重构与最终验收

EMU 正确性和最小适配收敛后，继续 Engine/VN 的实际 IO/媒体/保存、Editor 图/时间线/Agent 完整流程与关闭问题，再推进终之空路线和四平台。相关实现契约留在各产品文档，本页不复制 API 明细。

## 性能基线与最终验收

**对照条件：** 相同硬件、驱动、分辨率、Release 构建和游戏版本；分开冷启动与预热。本批性能比较限定 Hosted 整改前后，原生入口用于功能归因。按语义场景/真实时间窗口对齐，不按 headless 逻辑帧号强配实时时钟。

**分项记录：** VM advance、GPU submit/completion、readback 等待与 depad、CPU 分配/字节数、Host 上传/呈现、p50/p95/p99、丢帧、音频欠载，以及测量开关自身开销。静态场景、动态文本/转场、影片、读档与退出分别覆盖。

**当前架构不变量：** 静态无新帧不重复回读/上传；无滤镜不额外全纹理复制；没有每帧全状态快照；设备回调不释放外部 PCM owner；队列、缓存和在途结果有界。数值阈值在配对数据形成后确定。

**产品验收：**

- Windows：FVP《樱花萌放》、Musica《夏空的英仙座》各一结局；新增核心代表流程；AstraVN 终之空 Classic/Modern 全部 37 路线。
- Linux/macOS/Android：代表流程及真实输入、媒体、存读档、挂起/恢复、关闭与性能。Editor 为三桌面；Android 游戏导入管理目录，核心静态编译。
- VN 目标桌面 1440p120、移动 1080p60；EMU 保持原生节奏。分别报告核心和 Host 成本，记录实际设备。
- 原生窗口/模态、设备听音、商业长流程与长期 GPU/资源稳定性各自验收。Artemis headless 的 typed 窗口拒绝不能作为商业视觉通过；Siglus DXGI Live Object 警告继续归因。

## 工作与检查入口

主线程使用独占 worktree/target，保留来源和用户已有存档。操作入口见[开发手册](../manual/development.md)、[AstraEMU 客户端](../manual/astraemu-client.md)、[Editor](../../Editor/README.md)、[NativeVN 样例](../../Examples/NativeVN/README.md)、[终之空资料](../samples/tsuinosora-modernization/README.md)。

文档改动执行 `python Tools/check_docs.py`；代码改动按影响范围执行检查与真实运行。商业运行产物只留 ignored 区域；采样完成后移除临时探针。本批按用户要求使用 Sandbox 验收，当前待启动崩溃解决后继续硬件 GPU 预检。主机数据保留为独立证据，软件 GPU 或无声输出不替代对应设备验收。
