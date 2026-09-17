# 本地 Codex 重构交接

交接日期：2026-09-16。本文是接手入口，目标仍是完成整个已确认的产品重构，不是仅完成真实游戏测试。**项目尚未完成；目前最缺的是授权商业游戏、真实设备和完整产品流程的验证。**

## 1. 接手的代码与权威资料

- 分支：`codex/architecture-rebuild`；原始基线：`28f89d88`。
- 最新实现提交：`04d9b6dd`（NativeVN 删除转发 FSM）；交接文档在其后单独提交。
- 用户已授权常规实现、修复、测试和本地提交。没有要求推送、发布或向他人发送消息。
- 用户明确要求主线程实施，不使用子智能体。此前子任务已整合的内容在本分支中；其他历史 worktree 不归接手实例所有，不再次盲目 cherry-pick。
- 每个实例独占 worktree、branch、target 和服务。接手前核对 Git 状态、构建进程和设备连接，不复用其他实例的 target。
- 先读 [AGENTS.md](../../AGENTS.md)、[总体架构](../product/architecture.md)、[重构契约](../contracts/rebuild.md)、[实施状态](implementation-plan.md)、[开发手册](../manual/development.md)。最近 Runtime/Player 的变更见 [迁移说明](../contracts/presentation-tick-migration.md)。
- 新宪章取代旧约束中冲突的 Qt、强制 Headless、通用产品 ABI、强制 FSM、通用 replay、Luau sandbox 和 evidence 审批要求。旧源码/旧文档的存在不表示新架构完成，也不能据此恢复旧设计。

若通过附带 Git bundle 交接，它包含从上述基线之后的本分支提交，不含 target、游戏资源、私有配置和测试日志；接收仓库须已有基线对象。先核对 bundle，导入新本地分支，再创建独占 worktree：

```bash
git bundle verify '<bundle-file>'
git fetch '<bundle-file>' codex/architecture-rebuild:codex/local-game-validation
git worktree add ../AstraEngine-local-game-validation codex/local-game-validation
```

将占位符替换为实际 bundle 文件。目标分支或 worktree 已存在时先核对归属，不强制覆盖。若 verify 报缺少 prerequisite，先取得原仓库基线历史。原远程为 `2018wzh/AstraEngine`；本次交接不假定重构分支已推送。

## 2. 用户已经确定的目标与取舍

| 领域 | 确定的方向 |
| --- | --- |
| 顺序 | EMU/SDK → Engine/VN → GPUI Editor/Agent → 终之空 → 四平台整体验收 |
| 仓库 | 单仓产品分区；根 workspace 管 Engine/VN/Player/共享库/工具，Emulator 独立 workspace/lockfile/target；Editor 在实际 GPUI 实现时独立 |
| Engine | 可嵌入 2D 核心，Actor/Component；flat FSM 可选，业务修改不强制通过 action；不要求 World 带 package/registry |
| NativeVN | Rust typed API 编译期组合，删除内部通用动态 gameplay provider/UI ABI；保留业务能力，不只做 facade 或 fixture |
| 时间/失败 | 60 Hz 逻辑、独立呈现；无每帧全状态复制/回滚；错误终止会话，可靠存档恢复；新演出替换同属性旧任务，其他轨道继续 |
| 异步 | 明确所有者、作用域和取消/完成结果；读档/替换/退出使旧异步结果失效；耗时 IO 异步 |
| 保存 | 保存显式业务状态；删通用 replay/history hash 链，保留 VN backlog、语音/CG 回放；旧内部格式可拒绝/重建，原商业存档不能覆盖 |
| 创作 | `.astra` 是 Story/Scene/Sequence/角色/UI 的权威源，成熟 lexer/parser/CST/AST，保留注释/source map，图/时间线写回同一源 |
| 脚本 | 可信 Luau 经宿主访问文件/网络；不保存闭包/协程/native handle；预览 seek 不重复外部 IO |
| UI/编辑器 | 游戏 UI Yakui、EMU Slint、Editor GPUI；桌面 Editor 独立真实 GPU 预览，共用真实 VN session |
| Agent | ACP 外部 Agent + MCP 版本化编辑、取消、冲突检测、批量撤销，自主/逐批确认模式；不擅自另写模型 Agent 循环 |
| EMU | 独立薄 Family API v2，核心自己持有 VM/文件/媒体/存档；CPU 最终帧、有界可取消 PCM、可选翻译；库驻留至进程退出，session 关闭仍须 cancel/join |
| SDK | 文字、媒体、绘制、字节源等可独立使用，不迫使 Musica 创建 Engine/VN session；优先成熟库 |
| 平台 | Windows/Linux/macOS/Android；Editor 三桌面。Web/iOS、RPG/TRPG、Runtime AI、完整 Live2D、通用节点编程延后 |
| 测试 | 普通 unit/integration test 不强制 Headless；视听才开宿主。实际操作游戏并检查画面/声音/存读档，不能用窗口存在或假报告证明完成 |

FVP 保持 RFVP 0.6.0 revision `304e773387a9920c9db091ec1fd937c717aea949`，共用上游 GlobalSaveDataV1/RFVG；不复制 hosted 存档。Musica 短期采用核心自有存档，原版兼容不是当前发布要求。GARbro scheme 继续使用纯 Rust 两阶段 NRBF reader，不引入 BinaryFormatter、managed helper 或启发式 fallback。

## 3. 当前实现与证据

| 模块 | 已落地 | 仍未证明/未完成 |
| --- | --- | --- |
| 规则与构建 | 新宪章、轻量文档检查、xtask、独立 Emulator workspace；普通测试迁出强制 Headless | 旧私有抽象、超大文件、未迁移文档仍需随实现清理 |
| EMU/FVP | typed 启动配置、可选翻译、进程驻留库、独立 API v2/FVP 编码适配 | Manager 内真实安装与播放、原生存档/音视频/完整结局、多平台 |
| SDK/Musica | 独立 astra-text；PAZ/profile/纯 Rust importer；独立 session、CPU 画面、PCM、输入、AMINSV02 保存与音频渐变恢复 | 完整 opcode、choice/系统页/影片、ANI/SQZ session 播放、长媒体流式解码、真实结局/Android |
| Runtime | 无包 World、typed Actor/Component 保存；删除整帧回滚与通用 replay/hash chain；TaskScope/scoped completion/cancel | 其他平台资源取消、完整可信 Luau/产品异步 IO 接入 |
| Player/媒体 | 原位呈现；fence all-of；视频作用域；产品存档包含音视频/timeline；播放时钟、冷启动 PCM、output 重建、pending open/close 回收 | 真实 GPU/audio/decoder 组合、长流程、暂停/恢复/冷启动及性能 |
| NativeVN | Player 直接持有 NativeVnSession；typed input/output/view/config；共享 Arc 剧情；原生 SaveBlob；原位 VnRuntime；失败取消/恢复；本轮删除转发 FSM/控制锁 | serialized policy descriptor、其他 generic provider/FFI/UI ABI 消费者、DSL/可信 Luau/标准产品能力仍需完成 |
| Package/发布 | PackageRuntimeSelection；构建/读取一致性与未知 runtime 拒绝；产品入口匹配编译描述；发布检查直接执行原生 session 并验证恢复后推进 | 序列化 policy/registry/target 的整体改版与残留契约清理；不能把局部发布检查当真实产品验收 |
| Editor/Agent | 有目标与契约，部分旧代码仍在 | GPUI 产品实现、统一源码编辑、预览、ACP/MCP 工作流尚未完成 |
| 终之空 | 旧研究/转换资料与样例可供核对 | 新 `.astra` 工程、Classic/Modern 全路线、私有四平台包未验收 |

最新 Engine 全量检查于交接时确认正常结束：**738 passed、0 failed、9 ignored**，并通过 docs、fmt、Clippy 和 build。223 页文档链接/卫生检查通过，文档检查器另有 7 项 Python 回归。忽略用例仍未执行，不能算通过。

Emulator 历史完整检查为 169 passed、3 ignored；后续 Musica/CLI all-feature 定向记录为 50 + 10 passed。这不是交接时最新完整 EMU 重跑，更不是真实商业游戏验收。本地应重新执行其全量检查。

源环境缺少可用真实 GPU/测试游戏/目标设备，没有完成任何四平台商业游戏验收。代码 fixture 中真实格式的图片、字体、PCM 解码只证明对应局部路径；不是实际设备输出和完整兼容性证明。

## 4. 最近实现的关键细节

| 提交 | 内容 |
| --- | --- |
| `b74c0c59` | Player 直接拥有 NativeVnSession，旧 provider map 仅留 ABI 边界 |
| `a894205b` | 原生 Runtime SaveBlob/LoadReport；Player 存档 v8，拒绝 v7 |
| `df6ea264` | 原生 step 使用 Runtime TickInput/TickMode；close 消费会话 |
| `4af26102` | 原生执行配置脱离 ABI executor/mode |
| `5105a5d0` | Package 原生选择与直接 session 发布检查，保留编译描述漂移拒绝 |
| `7052fb64` | 会话长期持有 VnRuntime；每步不复制完整历史；失败阻断 step/save，restore 后恢复 |
| `04d9b6dd` | 删除 NativeVN 专用 FSM/action、控制 Mutex 和字符串 PlayerInput 映射；直接提交事件与 host await |

当前主要入口：

- `Engine/Source/Modules/AstraVN/astra-vn-runtime-provider/src/native_open.rs`、`native_session.rs`、`native_step.rs`、`native_session_step.rs`、`native_session_save.rs`、`restore.rs`。
- `Engine/Source/Modules/AstraVN/astra-vn-core/src/runtime/session_step.rs`：原位 apply_deferred 与匹配当前 wait 的绑定，不公开可变状态。
- `Engine/Source/Runtime/astra-runtime/src/world/tasks.rs`：create_host_await 与 scoped completion；通用 FSM 仍保留。
- `Engine/Source/Programs/astra-player-vn/src/native_vn_host/` 以及 `product_media_host/`：产品执行、存档、媒体/作用域。
- `Engine/Source/Runtime/astra-package/src/runtime_selection.rs`：只读 NativeVn/target/profile；序列化 descriptor 仍未删除。

NativeVN 的 tick 先处理已有完成，随后直接提交业务事件与新 await；事件供下一 tick 消费。新存档无专用 FSM；旧存档含非空 StateMachineStore 时返回 `ASTRA_NATIVE_VN_RESTORE_LEGACY_MACHINE`，在提交前拒绝。损坏恢复保持当前状态；失败会话只有成功恢复才解除 failed。不要重新加回全帧 clone 或转换成第二套 effect 应用层。

内部格式迁移还包括 Runtime save_blob v5、Player v8、Media snapshot v3、演出 coordinator v5、Musica AMINSV02。它们不是原商业存档迁移方案；旧内部文件明确报错，不静默覆盖。

## 5. 本地接手后优先做什么

1. **确认代码和环境。** 从本分支创建独占 worktree，读取新宪章；记录 OS、实际 CPU/GPU/内存、驱动和音频设备。检查目标测试游戏的合法本地目录、版本、启动入口、既有原生存档；不要在报告中写私有绝对路径。仅追问阻塞信息，不重复询问上面已确定的设计。
2. **先复验 Emulator，再运行真实 FVP。** 目标《樱花萌放》。检查 Manager 中插件安装/配置、启动、文字、图像、BGM/语音/SE、输入、系统页、原生存读档、关闭重开和至少一个结局。原商业存档先保留独立备份，确认没有读失败后覆盖文件。遇到 RFVP 问题做最薄适配，并更新 Family 的 MODIFICATIONS。
3. **盘点并打通真实 Musica。** 目标《夏空的英仙座》。使用下列 CLI 与说明核对 archive/profile、SC/媒体；明确入口脚本，不能因默认 `test.sc` 不存在而猜测 fallback。当前 choice/系统页/movie 等 opcode 不完整，应按真实阻塞补齐并加脱敏最小回归，完成至少一结局、存读档/冷启动、关闭资源回收。不能把 unsupported 改成静默跳过。
4. **继续 Engine/VN 重构。** 用 NativeVN package 真实验证 UI、字体、演出、完整视频、音频和保存恢复；补齐真实平台缺口，清理 policy/ABI 剩余消费者，完成可信 Luau、异步 IO 和 `.astra` 前端。保持 Actor/Component、typed 组合和明确生命周期。
5. **完成 GPUI Editor/Agent。** 统一文本/图/时间线版本与撤销，复用真实 session/GPU 预览，接通 ACP/MCP 两种模式；seek 不重放外部 IO。
6. **完成终之空与四平台收尾。** 重做 `.astra` 工程、Classic/Modern 私有包，按下表验收；最后检查旧实现/双轨是否真正删除，而不是只增加新入口。

每一轮先修可复现产品缺陷，再运行受影响测试与对应产品 workspace 检查，更新实施状态。阶段完成必须有真实产品行为；不能以测试数量、文件存在或报告生成替代。

## 6. 游戏、平台与性能验收

| 对象 | 必须覆盖 |
| --- | --- |
| FVP《樱花萌放》 | 真实 Manager 播放，音视频/输入/系统页、原生保存与恢复、冷启动/关闭重开、至少一个结局 |
| Musica《夏空的英仙座》 | 实际脚本与媒体、choice/系统页、自有保存与恢复、冷启动/退出、至少一个结局；自有格式不宣称原版兼容 |
| 终之空 | Classic/Modern 全部 37 路线；Windows 长流程，Linux/macOS/Android 代表流程；本地转换与私有四平台包 |
| NativeVN | 字体 shaping/fallback、资源/媒体、UI/选择/系统页、演出并行与替换、保存恢复、暂停/冷启动/取消与退出 |
| 性能 | VN 桌面 1440p120、移动 1080p60；逻辑 60 Hz。EMU 按游戏原生帧率，分别测核心与 Host |

用户只指定“主流配置”，尚未固定机器型号；按实际硬件与固定场景测量，不把这句话当成精确基线。尽量远程操作；无法远程验证的环节再请用户手工执行。不能用软件 GPU、NullAudioDevice、仅 meter 或第一帧播放证明真实设备体验。

商业源、profile/key、正文、截图、音频、视频、原存档和转换中间产物只留 ignored 私有工作区；不进入 Git、package report 或公开样例。可提交记录只保留脱敏 hash、覆盖范围、诊断和必要指标。无需建立新的 evidence/signoff 审批体系。

## 7. 可用命令与入口

从接手 worktree 根目录运行；Emulator 有自己的 target，禁止与其他 worktree 共享。

```bash
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cargo xtask check --workspace engine
cargo xtask check --workspace emu
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-musica --all-features
cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-musica --all-targets --all-features -- -D warnings
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-musica --features dynamic-plugin-export
```

需要调试器符号时可以移除上述减小产物的环境变量并重建。真实平台构建前检查系统库/工具链，不能把本环境 Linux 编译通过当成 Windows/macOS/Android 编译通过。详细入口：

- [开发手册](../manual/development.md)：xtask 和 workspace 检查。
- [Musica CLI](../../Emulator/Source/Programs/astra-emu-musica-cli/README.md)：scan-archives、import-garbro-scheme、census-scripts、census-media；profile 含私有密钥，不可提交。
- [Musica core](../../Emulator/Source/Families/astra-emu-musica/README.md)：启动配置、已知缺口、F5/F9 自有保存、解码预算。
- [FVP 资料](../emu/fvp/README.md)、[FVP 薄适配](../emu/fvp/thin-fork.md)：与当前新契约交叉核对，旧架构章节不作为恢复旧设计的依据。
- [NativeVN 样例](../../Examples/NativeVN/README.md)、[终之空资料](../samples/tsuinosora-modernization/README.md)：旧资料仅作现有实现与素材研究入口。

全量 Engine 测试有一个故意让 UI component host 崩溃的隔离测试，可能产生 `Engine/Source/Programs/astra-ui-component-host/core.*`。确认它来自当前实例的测试二进制后再删除，不能清理别人的进程/文件。交接时本轮测试进程已结束，自己的 core 文件已清理。

## 8. 尚待澄清但不应阻止独立工作的事项

- 本地实际游戏版本、目录、入口脚本、设备与远程可操作范围，由接手实例只针对缺失项确认。
- 用户指定优先 OpenAI Responses/Completions，但尚未说明是外部 Agent 后端还是改为自建模型循环。现行决定仍是 ACP 外部 Agent + MCP，实施到此再明确冲突，不能自行把 API 名称当成替换整个 Agent 架构的授权。
- 没有已验证的交付期限或精确硬件基线，不凭空补出。

## 9. 给接手 Codex 的启动指令

> 请先阅读本文件及链接的新宪章/重构契约，核对 `codex/architecture-rebuild` 的提交。由主线程在独占 worktree 继续整个重构，不使用子智能体。优先利用本地合法《樱花萌放》《夏空的英仙座》和终之空素材，完成真实游戏与设备验证，按实际阻塞补齐实现；不要把局部 fixture、旧 Stage 状态或已有测试数量当成完成。常规实现、测试、修复和本地提交已获授权，无需重复确认。保留原商业存档与私有素材边界；每阶段同步真实进度。先报告代码/设备/游戏可用性和第一个具体执行项，然后开始工作。
