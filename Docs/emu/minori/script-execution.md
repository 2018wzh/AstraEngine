# Minori Script Execution

The active Minori runtime section is `astra.emu.minori.runtime_state.v23`.
Any older snapshot is a migration-rejection input and is never restored.

当前 VM 已覆盖首条路线实际经过的控制流、message、choice、stage/character、effect、BGM/SE、movie、panel、chain 和 end。message 自带 voice 通过公共 audio command 通道读取 `voice.paz`；非控制型独立 `playvoice` 在样本 census 中没有出现，遇到时继续阻断。

`.char` 的动态路径按原程序 `CCharLayerManager` 收紧为四种已确认操作。`load` 创建 signed slot，`pos` 更新中心/底部锚点，`vis` 使用原生首字符布尔规则切换可见性，`trans <slot> <duration_ms> <opacity>` 在当前位置对透明度做线性插值。`trans` 在完成前提交可序列化时间等待；fixed tick、起止透明度、已过时间和完成状态都进入 snapshot。当前样本只使用即时 `load/pos/keep`，没有观察到 `trans/vis`，因此真实路线不会人为注入动态效果，也不会按资源名猜测呼吸动画或 ANI 播放。

`chain` 的语义已经由原程序反编译纠正。它不是 call，也没有 return frame；处理函数结束当前脚本，把参数写入全局 `NEXT`，随后由外层装载下一个脚本。运行时据此做尾链式 VFS 切换：目标只允许 `minori:/scr/` 根下的直接 `.sc` entry，脚本切换时清空 local 变量，保留 global 变量。路径穿越、缺 entry、解析失败和 hash 漂移都会毒化 session 并阻断执行。

`set`/`setGlobal` 不采用复合赋值语法。IDA 证明 handler 只接受 3-token 直接赋值和 5-token 表达式；当前 VM 实现整数、local/global 引用以及 `|`、`&`、`+`、`-`、`*`、`/`、`%`。读取变量时先查 local，再查 global。字符串赋值和原程序的除零结果尚未得到足够样本证据，因此继续阻断。

`wait` 的整数单位已经确认是 10 ms timer tick。VM 在 family state 中同时保存原始 tick 和换算后的 milliseconds，提交给公共 Await contract 时使用后者；换算溢出直接阻断。Await request 是边沿事件，只在创建等待时提交一次；后续等待 tick 不重发相同 token，避免把持续状态误当作新的公共 request。

`message` 按原程序的 space/tab tokenizer 和 handler contract 解析：每个分隔符都产生一个位置，连续分隔符保留为空字段。四个起始字段依次为 integer id、voice identity、speaker 和正文首段，余下 operand 用单个 ASCII space 拼回正文。因此 `id` 后的三空格表示 voice、speaker 都为空，而不是可折叠的排版空白。少于四个 operand 时仍执行原程序的空默认更新。voice 使用 `resource[volume,pan]` 语法；原程序与全样本绑定共同确认 `[` 前部分是 archive identity。每条新 message 先停止 stream 4，非空 voice 再以单次播放加载。确定性 state 保存 voice URI、volume、pan 与 hash；正文和 speaker 只进入 local-private snapshot/backlog，并通过一次性 lease 交给 host，report 和日志不记录这些 payload。

Family ABI v9 不再传递 text lease 或 text presentation。Minori 在调用任何 framebuffer acquire 之前，把 speaker 和正文交给同步 `astra.emu.translation.text.v1` Hook；`Unbound` 明确保留原文，`Completed` 只接受有界 UTF-8，timeout、失败和畸形输出都会阻断本次 step。随后 family 使用 `CosmicTextLayoutProvider`、仓库打包的 Noto Sans JP 与 Astra CPU Renderer2D 生成 premultiplied RGBA，写入 Host-owned `minori.surface.text`，并把 `minori.layer.text` 作为 retained Layer2D 提交。Config 的 `text_shadow` 仍只控制既有 2 px 黑色 outline。缺字体、布局 diagnostic、区域越界、surface lease 冲突或 Hook 错误均 fail fast；没有系统字体、位图文字、字符宽度估算或旧 ABI fallback。消息提交后建立非零物理输入 mask 的 await，等待 confirm、space 或主指针输入。

活动消息中切换 Auto 或进入有效快进不会创建第二个等待。VM 保留同一 token，并把等待 modality 从 `Input` 重绑定为 `Time`；切回 Normal 或在尚未完成前释放 Control 时执行反向重绑定。Host 只允许这两种同 token 互换，重复的同类等待或其他 kind 仍直接阻断。Config 的 Auto 速度值 `0` 和 Skip/Control 消息快进都映射为一个 10 ms timing unit，避免制造零时长公共等待；设置值本身不被改写。movie、presentation 和 provider fence 不参与重绑定。该契约让持久 Auto 与受 gate 约束的 Control 快进可以推进当前消息，同时不放宽 Await 的唯一性和正时长约束。

音频资源后缀按原程序的 `resource[volume,pan]` 规则解析。普通 BGM/SE 引用生成稳定 `minori:/...` URI，并在发出公共 `LegacyAudioCommandV1` 前由绑定 VFS `stat` 核对存在性和大小；host 后续仍通过 session resource channel 读取，商业字节不进入 effect。BGM 使用固定 loop stream，三个 SE command 使用独立 bus；非循环 SE 使用确定性 stream id。`*` 停止对应固定 stream，并保留原程序的 fade-out 参数。

资源审计有一个显式的 full policy：`astra.resource_audit=full`。CLI 的 `--audit-all-resources` 只对 Minori 注入该选项；open 阶段通过 Host 的 bounded `enumerate_by_extension` 收集全部 `.sc`，按同一 parser 和 operand validator 收集资源 URI，再逐项 `stat` 并核对非空和 `1 GiB` 上限。审计只保留计数、长度、revision 和 URI identity digest，不读取目录、不把商业字节写进 report；VFS 未提供枚举、脚本损坏、资源缺失、源 revision 漂移或边界异常都会返回稳定 blocking diagnostic。没有该显式选项时仍保持按执行路径的 lazy stat，不能把普通运行误报成全资源覆盖。

`transition` 只配置后续 stage，不自行提交替代帧。`stage` 按已确认顺序更新前景、背景和 stand state。family effect 只保存 VFS URI、编码 hash、尺寸与绘制指令；Headless/Manager Host 通过 session resource channel 读取编码数据，并交给显式绑定的 Astra `DecodeProviderRegistry`。解码后的 RGBA 只存在于 Host 临时渲染帧，不进入 effect、snapshot 或 report，也没有 Minori 私有 renderer。stand position 尚未证明为像素坐标，因此含 stand 的 stage 会返回 `ASTRA_EMU_MINORI_STAGE_STAND_POSITION`。

原程序 parser 将第一个 `effect` operand 绑定为 effect id，第二个 operand 才是可选的冒号分隔资源规格；其后最多三个 operand 以 C 整数读取，缺省值为 `-1`。已在真实执行路径确认 `.effect CrossFade2` 的单 operand 形式：原对象接收空资源规格、替换首层 effect slot，但不会解析出 resource frame。runtime 明确清除活动 effect 并递增确定性 sequence，不提交替代帧或自交叉淡入。四 operand 形式会逐项查询资源；单独的 `*` 是有效的空资源选择，查询不产生资源对象，也不进入双帧路径，因而以同样的无 presentation slot replacement 表达。非空资源序列的后两个整数分别作为 alpha 增量与更新间隔；只有至少两个已解析资源才进入双帧路径。runtime 以同一固定时钟累计间隔、在一次更新中提交当前帧后增加 alpha，并在达到阈值后推进相邻资源。

`.effect2` 复用相同命令对象，但原程序把结果写入独立的第二 effect slot。IDA 已确认当前样本使用的两种形式为 `SnowH` 与 `fadeout`：`SnowH` 建立 50 个横向粒子，绑定 `snowS.png`、`snowM.png`、`snowL.png`；`fadeout` 只衰减第二 slot，不清除第一 slot。runtime 现按该合同保存独立状态、确定性随机数、定点位置、速度、方向、资源级别与 16 ms alpha 累计，并在完整呈现帧的顶层合成。其他 `.effect2` kind 或 operand 形式仍严格阻断。当前 snapshot schema 已随 Config transaction 硬切到 `astra.emu.minori.runtime_state.v23`，旧 schema 不做迁移或回退。

`.panel` 已确认调用 `CMessagePanel`。第一个整数是 `!panel_Mode`，资源名以 `!panel_Filename` 保存；原程序的 mode 0 分支不会加载 panel asset，因此 runtime 清除当前可见 panel 并重发同一演出层；mode 1 分支选择 `msgPanel.png`，并把它作为最上层 resource-frame 与最后实际显示的 CrossFade2 frame 合成。mode 1 的 x 使用 panel 全局坐标，y 按 `viewport_height - image_height + 64` 计算；超出 viewport 的底部 64 px 由 renderer clip。mode 2–10、第二个过渡参数和自定义文件名仍缺完整语义，统一返回 `ASTRA_EMU_MINORI_RUNTIME_PANEL`。

backlog 复用当前 `CMessagePanel`，不会切换 panel mode。原程序进入该状态时重新显示 mode 1 面板，把当前 `CLog` 记录交给同一文本排版器；滚轮向上打开并移动到更早记录，滚轮向下关闭。runtime 因此在 message 执行时保留有界、无静默淘汰的历史记录，打开页面后只通过一次性 lease 提交当前记录。游标或页面变化时 Host 先清除旧文字再接收新 lease；没有变化的 idle tick 不清除 retained text，也不重复签发 lease。正文和 speaker 仅进入 local-private snapshot，不进入 evidence、report 或日志。历史上限为 16384 条、单字段 64 KiB、正文与 speaker 合计 16 MiB；越界、hash 不一致、cursor 损坏和冲突滚轮输入均阻断。Headless checkpoint 必须在关闭输入提交后的下一固定 tick 采样，避免把同 tick 的旧 surface 当成恢复结果。

Host restore 会丢弃 live surface retention，因此 family 不能只恢复 VM bytes。Minori session 在 restore 后标记 presentation rebind：system page 执行完整重画；剧情 wait 重建当前 stage、panel、message 或 choice，并以新的 generation 重新 acquire/commit 所需 surface。旧 session surface 由 Host 在 shutdown/restore 边界清理。rebind 失败会保留 pending 状态并返回 blocking diagnostic，不提交部分恢复画面。

标题页 Config 的 Enter/Escape 语义已由原程序按键 handler 与 action switch 确认：Enter 应用并关闭，Escape 恢复并关闭。v23 把 29 类已确认动作纳入 draft transaction，覆盖三条消息速度、字体前后切换、Auto/Skip 偏好、窗口模式、三项视觉开关、三项其他开关、BGM/Voice/SE 音量与静音、试听、五名角色语音和 Apply/Cancel。鼠标命中区与滑块换算使用原程序坐标；拖动只在主键按下时生效，键盘、取消和指针动作同 tick 冲突会阻断。`configBase.png`、`knob.png`、`checkmark.png`、`circle.png` 通过 retained Scene2D 组合，尺寸不符直接失败。音量和静音在公共 audio command 边界应用，试听明确使用 WAV 并在离开页面时停止；BGM 试听 URI 使用 VFS 清单确认的大小写敏感 identity `BGMTest.wav`，不做大小写搜索。文字阴影已接入剧情消息的 typed outline 并通过真实短程 E2；全屏切换、消息逐字速度、其余视觉开关对剧情演出的实际影响和角色语音筛选还没有各自的 Host/VM 行为证据，因此不能把局部 Config E2 写成完整 Config 验收。

全包 census 已确认 89 个脚本、33728 行、33695 个 command 和 29 个 command token，catalog 范围内 unknown opcode 为 0。资源契约迁移后，签名动态 Minori plugin 已通过真实八包 Headless E2 的前 373 个 fixed tick：入口 tail-chain、BGM、SE、黑底 stage、竖排标题 stage、6 秒 wait、可见 CrossFade2、`.panel 1` 和前两条 message 都成功。运行实际提交 9 帧，形成 6 个不同 checkpoint，snapshot round-trip 成立且 diagnostic 为 0。用于完成 input await 的物理按键会在 Host 生成唯一 await result 后被消费，不再重复进入尚未验证的 family raw-input channel。人工检查确认两条日文正文没有缺字、横向裁剪、拉伸或旧文本残留。该证据不代表完整 effect 周期、路线、系统 UI 或 transition 动画完成。

## VM State

Minori core 持有 family 私有状态：

```text
pc
current_script_uri/hash
local_variables
global_variables
flags
message_state
choice_state
presentation_layers
audio_state
resource_cache_refs
```

Manager 只能接收 trace 和 presentation/audio command，不读取私有 VM 内存。

## Tick

每个 tick 执行到以下暂停点之一：

- `Wait(duration)` 未结束。
- `WaitInput` 等待用户推进。
- `ChoiceGroup` 等待选择。
- movie/audio 同步点。
- save/load snapshot 边界。
- fatal diagnostic。

可挂起动作保存为 `AwaitToken`，恢复时在固定 tick 边界进入事件队列。

## Save/Load

Snapshot schema 当前为 `astra.emu.minori.runtime_state.v23`，包含 VM state、当前脚本 URI/hash、pc、message/backlog、message voice URI/volume/pan、`Normal/Auto/Skip` play mode、Config 已应用值和页面内 draft、Control 与指针物理状态、两个 pragma gate、已提交 presentation layer、transition 配置、effect state、message panel、audio bus 的 URI/encoding/loop/volume/pan/continuation 状态和 patch mount manifest。恢复时 host 必须重新从绑定 VFS 读取当前脚本并核对 hash，不能信任 snapshot 中的脚本身份。Snapshot 不包含解密 payload。

Config 的跨 session 持久化不依赖 gameplay snapshot：显式 writable-file binding 开启后，family 以 `astra.emu.minori.config.v1` envelope 保存已应用配置，严格校验 case/package/profile identity，并以临时文件加 atomic replace 写回。缺少文件使用默认值；损坏、越界或 identity 漂移阻断。加载 gameplay slot 时保留当前 installation-scoped config，避免旧 slot 改写当前音量、阴影和 play-mode 偏好。

媒体恢复有独立边界：v9 的公共 `Play` command 不携带 seek 起点，snapshot 中的 `continuation_pts` 只是已验证的状态记录，不能在 restore 时解释成可执行的媒体位置。restore 会重新检查活动资源并提交从起点开始的确定性播放；要求原位置继续的音频/影片场景保持 blocking，直到公共 Host/media contract 增加并验证 seek continuation。

## Determinism

随机数、auto/skip、voice replay 和 movie end event 都必须进入 trace。联网或系统时间不参与脚本决定。
