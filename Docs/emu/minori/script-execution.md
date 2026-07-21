# Minori Script Execution

当前已有受限 VM，执行 `set`、`setglobal`、`label`、`goto`、`if`、`wait`、`message`、BGM/SE、`playvoice *`、`transition`、无 stand 的 `stage`、`effect CrossFade2`、`.panel 1`、`chain` 和 `end`。状态保存 PC、local/global 变量、等待、消息身份、图层、transition、effect timeline、message panel、音频和系统页等字段，并通过 postcard snapshot round-trip 验证。`select`、非控制型 `playvoice`、stand position、其他 panel mode 和其余演出命令遇到执行路径时返回稳定 blocking diagnostic，不会被跳过。

`chain` 的语义已经由原程序反编译纠正。它不是 call，也没有 return frame；处理函数结束当前脚本，把参数写入全局 `NEXT`，随后由外层装载下一个脚本。运行时据此做尾链式 VFS 切换：目标只允许 `minori:/scr/` 根下的直接 `.sc` entry，脚本切换时清空 local 变量，保留 global 变量。路径穿越、缺 entry、解析失败和 hash 漂移都会毒化 session 并阻断执行。

`set`/`setGlobal` 不采用复合赋值语法。IDA 证明 handler 只接受 3-token 直接赋值和 5-token 表达式；当前 VM 实现整数、local/global 引用以及 `|`、`&`、`+`、`-`、`*`、`/`、`%`。读取变量时先查 local，再查 global。字符串赋值和原程序的除零结果尚未得到足够样本证据，因此继续阻断。

`wait` 的整数单位已经确认是 10 ms timer tick。VM 在 family state 中同时保存原始 tick 和换算后的 milliseconds，提交给公共 Await contract 时使用后者；换算溢出直接阻断。Await request 是边沿事件，只在创建等待时提交一次；后续等待 tick 不重发相同 token，避免把持续状态误当作新的公共 request。

`message` 按原程序的 space/tab tokenizer 和 handler contract 解析：每个分隔符都产生一个位置，连续分隔符保留为空字段。四个起始字段依次为 integer id、voice identity、speaker 和正文首段，余下 operand 用单个 ASCII space 拼回正文。因此 `id` 后的三空格表示 voice、speaker 都为空，而不是可折叠的排版空白。少于四个 operand 时仍执行原程序的空默认更新，不把坏输入悄悄跳过。确定性 state 只保存 id 与正文、speaker、voice 的 hash；正文和 speaker 通过 `LegacyEffect::TextCapture` 的单次有界 lease 交给 host，取走后立即失效，不进入 snapshot、report 或日志。

同一 effect batch 先用 `astra.emu.text_presentation.v1` 提交不含正文的 `LegacyTextPresentationV1`，再按 lease id 与 `TextCapture` 关联。这个结构保留现有 `LegacyEffect` v1 wire layout，避免在 ABI fingerprint 不变时偷偷改变 postcard 数据。Minori 只声明语言、字体 family、文字区域、字号、行高、最大行数与颜色，Host 必须用显式绑定的 `cosmic_text_cpu` 和仓库内 Noto Sans JP 完成 shaping，再交给 Astra `Renderer2D` 合成。缺 provider、字体绑定不一致、stage 不是 1280×720、区域越界、重复或悬空 layout、shaping diagnostic 都会阻断；没有系统字体或字符宽度估算 fallback。消息提交后建立非零物理输入 mask 的 await，等待 confirm、space 或主指针输入。

音频资源后缀按原程序的 `resource[volume,pan]` 规则解析。普通 BGM/SE 引用生成稳定 `minori:/...` URI，并在发出公共 `LegacyAudioCommandV1` 前由绑定 VFS `stat` 核对存在性和大小；host 后续仍通过 session resource channel 读取，商业字节不进入 effect。BGM 使用固定 loop stream，三个 SE command 使用独立 bus；非循环 SE 使用确定性 stream id。`*` 停止对应固定 stream，并保留原程序的 fade-out 参数。

`transition` 只配置后续 stage，不自行提交替代帧。`stage` 按已确认顺序更新前景、背景和 stand state。family effect 只保存 VFS URI、编码 hash、尺寸与绘制指令；Headless/Manager Host 通过 session resource channel 读取编码数据，并交给显式绑定的 Astra `DecodeProviderRegistry`。解码后的 RGBA 只存在于 Host 临时渲染帧，不进入 effect、snapshot 或 report，也没有 Minori 私有 renderer。stand position 尚未证明为像素坐标，因此含 stand 的 stage 会返回 `ASTRA_EMU_MINORI_STAGE_STAND_POSITION`。

`effect CrossFade2` 的 handler 接收 effect id、冒号分隔的资源序列和两个时间参数；第三个整数沿用构造器默认值 `-1`。资源序列中的 `*` 是空帧。原效果对象按更新时间阈值推进 0..255 混合量，并在相邻资源之间循环。VM 用固定 `delta_ns` 更新同一状态，snapshot 同时保存下一次更新所需的 accumulator 与最后实际提交的 frame，避免 panel 叠加或恢复时反推可见 alpha；Host 仍只收到 resource-frame URI、hash、尺寸、顶点和 alpha。当前只接受 IDA 已确认的 `CrossFade2`，其他 effect id、非法资源、零时间参数或非默认第三参数均返回 `ASTRA_EMU_MINORI_RUNTIME_EFFECT`。

`.panel` 已确认调用 `CMessagePanel`。第一个整数是 `!panel_Mode`，资源名以 `!panel_Filename` 保存；原程序的 mode 1 分支选择 `msgPanel.png`。当前 runtime 只接受精确的 `.panel 1`，把 `minori:/sys/msgPanel.png` 作为最上层 resource-frame，并与最后实际显示的 CrossFade2 frame 合成。mode 1 的 x 使用 panel 全局坐标，y 按 `viewport_height - image_height + 64` 计算；超出 viewport 的底部 64 px 由 renderer clip。mode 0、2–10、第二个过渡参数和自定义文件名仍缺完整语义，统一返回 `ASTRA_EMU_MINORI_RUNTIME_PANEL`。

全包 census 已确认 89 个脚本、33728 行、33695 个 command 和 29 个 command token，catalog 范围内 unknown opcode 为 0。资源契约迁移后，签名动态 Minori plugin 已通过真实八包 Headless E2 的前 372 个 fixed tick：入口 tail-chain、BGM、SE、黑底 stage、竖排标题 stage、6 秒 wait、可见 CrossFade2、`.panel 1` 和首条 message 都成功。运行实际提交 8 帧，形成 5 个不同 checkpoint，snapshot round-trip 成立且 diagnostic 为 0。人工检查确认 panel 的底部位置和 clipping，首条日文正文也没有缺字、横向裁剪或拉伸。该证据不代表完整 effect 周期、路线、系统 UI 或 transition 动画完成。

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

Snapshot schema 当前为 `astra.emu.minori.runtime_state.v6`，包含 VM state、当前脚本 URI/hash、pc、message/backlog、已提交 presentation layer、transition 配置、CrossFade2 timeline 与可见 frame、message panel、audio bus 的 URI/loop/volume/pan/continuation 状态和 patch mount manifest。恢复时 host 必须重新从绑定 VFS 读取当前脚本并核对 hash，不能信任 snapshot 中的脚本身份。Snapshot 不包含解密 payload。

## Determinism

随机数、auto/skip、voice replay 和 movie end event 都必须进入 trace。联网或系统时间不参与脚本决定。
