# Player 片段预览控制

Editor 继续增量 cook、构建 package/bundle，再启动独立真实 GPU Player。`--preview-control` 只为这个子进程打开 JSONL stdin/stdout 控制，日志仍走 stderr。没有端口、常驻服务或持久化开关。普通 Player 启动不变；不能与 automation report 模式组合。

## 身份与接口

Rust 协议定义在 `astra-vn-script::PreviewRequest`、`PreviewCommand`、`PreviewResponse`，版本为 `astra.vn.preview.v1`。第一条请求必须在十秒内 Attach。`PreviewIdentity.project_hash` 必须等于 Player 实际打开的 compiled project hash；相对文档路径、version 和内容 hash 来自 Editor 在 cook 前固定的作者快照。Editor 修改任一文档后必须停止旧进程并重新 cook，不能把旧 hash 换成新版本继续用。generation 由 Editor 每次启动递增，全部后续请求重复同一 identity。

sequence 从 1 开始严格递增。状态推送使用 sequence 0；对请求的响应携带原 sequence。Ready 才表示已连接实际 Player，会话退出发送 Stopped，断管或启动失败也必须由父进程按退出处理。单消息上限 65,536 字节；identity 上限 32 KiB、256 个文档；两向队列各四条。超限或无法继续交付时关闭预览，不无限缓存。

Pause 和 Resume 在同一主循环边界切换设备音频与逻辑时钟，暂停期间不消费剧情输入；窗口关闭仍有效。恢复播放重新建立计时 deadline，暂停时长不会补跑成逻辑欠账。请求接收可以取消，接收后的状态修改完整执行，不能由另一个 select 分支丢弃一半恢复操作。

## 片段和检查点

片段 ID 使用当前 pending wait 对应的编译 source ID。没有合法 source map 项的执行区间不能定位。任何 VN 命令提交改变 fixed step，即使仍在同一 source ID，也会清空旧检查点；这避免跨越系统操作或脚本副作用。Player 每隔至少 100 ms 的已完成逻辑边界记录一次，最多保留 64 个、合计 32 MiB 编码预算，状态推送给出每个 checkpoint ID 及精确 presentation_time_ns。

SeekWithinFragment 必须处于 Pause 状态，只接受当前片段仍保留的 checkpoint ID。没有按任意时间猜测最近点的行为。它复用当前 Player 的 Runtime、Stage 和媒体显式状态恢复，不创建第二个 VnSession，不回放剧情命令、按键、Luau 栈或外部 IO。恢复重建作用域，使旧 worker 完成失效。检查点只留在进程内存，不写游戏存档槽；不改变 Player v9 存档格式。

恢复先检查身份、当前片段、精确 checkpoint、媒体资产和候选 Stage。提交前拒绝保留当前状态和任务。提交后的设备恢复或渲染失败终止预览，不伪装成可继续的拒绝；资源所有者仍执行取消、关闭和 join。设备重建保留 paused 标记，定位过程中不会暂时开始输出声音。旧身份、重复请求、跨片段、缺检查点、不可恢复状态分别有 typed 拒绝码。

## 开发与验收

[Player 回归](../../Engine/Source/Programs/astra-player-vn/src/native_vn_host/preview/tests.rs)覆盖重复与旧身份、取消代次、精确位置恢复、不执行剧情、损坏候选不改活动状态；[管道回归](../../Engine/Source/Programs/astra-player/src/preview_transport.rs)检查父进程仍持有 stdin 时子进程也能取消并 join。Windows 使用可取消同步管道 IO，Unix 使用非阻塞 stdio；队列、线程和句柄由进程会话拥有。

Editor 消费者接入、真实素材的连续定位、GPU/音频和 Linux/macOS 本机检查分别验收，局部测试不代替这些流程。开发命令见[开发手册](../manual/development.md)。

设计参考 UE 的独立 [PIE 会话](https://dev.epicgames.com/documentation/unreal-engine/ineditor-testing-play-and-simulate-in-unreal-engine)与 [Sequencer 的播放区间和明确评估边界](https://dev.epicgames.com/documentation/unreal-engine/sequencer-cinematic-editor-unreal-engine)。本接口保持 VN 已记录片段内恢复，不把它扩展成任意脚本时间旅行。
