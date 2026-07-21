# Minori 移植日志

## 2026-07-21

### 目标

在 Family VFS 公共化基础上建立 Minori runtime 的可信输入面：完整识别真实 PAZ 集合，固定 GARbro 格式 contract，并让 ANI/SQZ 输出进入 AstraEngine 已有图像管线。VM、系统 UI 与 Headless 路线验收尚未到可声明完成的阶段。

### 本次完成

- 分支已 rebase 到当前 `master`，保留 `family-core`、`family-support`、通用 VFS CLI 和 Minori 私有 importer。
- 重新递归扫描授权样本，纠正“只有六包”的旧结论。实际为 `bg/bgm/scr/st/sys/se/voice/mov` 八个逻辑 archive；`bg` 包含主包和 A–J 十个连续分卷，全目录共 18 个物理文件。
- 将这次检查固化为 `astra-emu-minori-cli scan-archives`。扫描器递归检查分卷连续性、重复项、空文件和 symlink；当前样本的 18 个文件合计 5742470010 bytes，inventory hash 为 `sha256:5a5729b8fcaec7cf218fa211e0b76e89af162f3666fd8b3c794e550049e16637`。
- 纯 Rust GARbro importer 现在要求八个 role，并生成与八包集合一致的 private profile。真实挂载解开八个 index，八包 14502-entry decoded full verify 已通过；不再把六包 9837-entry 结果写成全包证据。
- `LegacyMountedVfs::read_dir` 允许合法的 mount root 和单个结尾 `/`，file URI 的严格规则不变。新增 core 与 Minori fixture 回归测试。
- 依据固定 GARbro revision 实现 ANI 与 SQZ 的纯 Rust 有界 adapter。ANI 覆盖 BGRA32、BGR24、BGR565 和 Gray8；SQZ 对 zlib BGRA32 frame 做精确输出长度校验。两者都输出 `image::RgbaImage`，没有 Minori 私有渲染器。
- 新增 `census-media`，并对真实 `bg`/`bgm` 完成逐 entry、逐 ANI/SQZ frame 验证：2655 PNG、1951 ANI/6723 frames、9 SQZ/224 frames、49 Ogg 和 1 个 metadata database。
- 建立可序列化的 Minori runtime state，覆盖 PC、local/global 变量、wait、message/choice 引用、图层、音频、影片、系统页、鉴赏解锁和 deterministic counter。当前执行 `set/setglobal/label/goto/if/wait/chain/end`；未确认命令返回 `ASTRA_EMU_MINORI_RUNTIME_OPCODE`，不会静默跳过。
- 通过 IDA MCP 对原版入口的 command registry、RTTI、vtable 和 handler 调用点做了交叉验证。结果否定了此前讨论的普通 call 假设：`chain` 先执行与 `end` 相同的结束流程，再把参数写入全局 `NEXT`，没有 call stack 或 return path。runtime 已改成尾链式 VFS 切换；snapshot schema 升到 v2，恢复时重新读取 active script 并校验 hash。
- 同一轮反编译确认了 assignment 的 3/5-token 结构、local/global store 查询顺序、`if` 的六种比较运算，以及 `wait` 的 10 ms timer tick。旧 VM 把 `.wait` 参数直接当 milliseconds、把 `.set x += 1` 当原生语法，均与 handler 不符，现已按根因修正。
- snapshot 使用 postcard round-trip，并由测试验证 wait continuation 与 state hash。实现过程中发现 internally tagged enum 无法稳定通过 postcard，已改为二进制格式兼容的 enum 表达，没有保留只可写不可读的 snapshot。
- `astra-emu-minori` 已产出 `rlib`/`cdylib`，实现完整 `LegacyRuntimeProvider` ABI surface 与 host VFS FFI。公共 `LegacyMountedVfsReaderAdapter` 把已解密 mount 绑定到 runtime reader，revision 由 reader、profile、source、entry 和 method identity 派生，不暴露 source path。
- provider lifecycle fixture 已覆盖 open、连续 wait tick、await completion、save、restore 和 shutdown。真实 Headless 长等待暴露了 AwaitToken 重复提交：family 在每个等待 tick 重发同一 token，而 Manager 正确阻断重复 token。运行时现只在创建等待时提交一次 request，后续 tick 只保留 family 状态；回归测试覆盖这一边界，没有放宽公共 Await 校验。
- 原程序 tokenizer 已确认按每个 ASCII space/tab 切分 operand，连续分隔符会保留为空的 positional operand；逗号和引号不具备分隔或引用语义。真实脚本的重复空格只出现在 `message`：双空格 3883 条、三空格 7387 条。parser、typed operand 和 census 现已使用同一规则，避免丢失空 voice/speaker 字段，也不在 parser 层重写商业文本或资源规格。
- `CommandMessage` 的 parse/execute contract 已闭合并进入 VM。确定性 state 只保存 message id 与三个字段 hash；正文和 speaker 通过一次性 `TextCapture` lease 交给 host，消息随后等待显式物理输入。短参数行按原程序构造器默认值执行空更新，不作静默跳过。
- `transition` 与 `stage` 的字段顺序已经由 vtable、parse handler、stage core 调用和入口样本交叉确认。stage 依次接收前景、可选前景坐标、背景、背景坐标和最多十组 stand pair；`*` 表示空层。背景/前景绑定 `bg`，stand 绑定 `st`。stand position 仍是未解释的引擎参数，不能当像素坐标。
- 第二层音频规格已经还原为 `resource[volume,pan]`，包括 C `%d` 数值前缀、默认值、范围夹取和缺右括号行为。BGM/SE 的 fade-in、fade-out 与 SE repeat flag 也由调用路径确认。
- census v3 对 811 条音频引用执行脱敏绑定：401 条非 `*` 引用全部精确、唯一命中 `bgm` 或 `se` entry；410 条未命中项全部为 `*`，没有普通资源缺失或大小写歧义。IDA 已确认 `*` 停止 BGM、对应 SE bus 或 voice stream，并使用各命令的 fade-out 参数。
- 非控制型 `playBGM/playSE/playSE2/playSE3` 已通过稳定 URI 发出公共 audio effect。provider 在发 effect 前检查 VFS stat、非空和 1 GiB 上限；host 仍经 session resource channel 读取内容。snapshot schema 升为 v3，保存 audio pan 与 bus state。
- `astra-emu-cli run/headless` 已硬切到 `--family` 与 `--mount-profile`，Minori 通过显式静态 factory registry 挂载八包，再加载签名动态 `cdylib`。旧 `--engine` 被 CLI parser 拒绝，未保留 alias 或 fallback。
- Headless surface 在 runtime 尚未提交 scene 时可以正常销毁，不再让 `surface.capture` 清理错误掩盖 family 根因；没有为失败路径生成空白替代帧。
- `stage` 只提交 `astra.emu.render_resource_frame.v1`：effect 保存 VFS URI、编码 hash、已验证尺寸和绘制指令，不保存商业像素。Headless 与 Manager 通过 session resource channel 取回编码数据，再交给唯一显式绑定的 Astra `DecodeProviderRegistry`；纯 Rust `ImageDecodeProvider` 是 packaged-eligible 主 provider，不走 fallback。Host 校验编码 hash、RGBA hash 和尺寸后才生成临时 `LegacyRenderFrameV1`。迁移后的真实八包运行前 5 个 fixed tick 已通过：入口 tail-chain、BGM、SE、全黑背景和竖排标题共提交 2 帧，两个 checkpoint 均为 1280×720，视觉发生变化，snapshot round-trip 与音频 artifact 同时成立。新旧路径的 checkpoint hash 与 visual trace hash 完全一致；截图与正文只留在 ignored 私有 artifact。
- IDA 已闭合 `effect` handler 的参数传递和 `CrossFade2` 对象：第二个 operand 按 `:` 拆成资源序列，`*` 形成空帧；两个整数写入混合步进与更新时间阈值。对象在相邻资源间执行 0..255 像素混合并循环推进。VM 保存有界资源序列、更新 accumulator 和最后实际提交的 frame；Host 用同一 resource-frame/DecodeProvider 路径合成，不把像素写入 effect 或 snapshot。
- `.panel` parser、`CMessagePanel` 调用和 mode switch 已确认：最多接收两个整数和一个字符串，缺省值分别为 `0`、`-1` 和空串；mode 1 选择 `msgPanel.png`，mode 与文件名分别以 `!panel_Mode`、`!panel_Filename` 进入存档。runtime 现只实现样本使用的 `.panel 1`，其他 mode、过渡参数和资源覆盖继续阻断。snapshot schema 随可见 effect frame 与 panel state 升为 v6。
- IDA 进一步确认 mode 1 的坐标计算：横坐标取 panel 全局 x，纵坐标为 viewport 高度减图片高度再加 64。真实资源为 263 px 高，因此 720p viewport 中从 y=521 开始绘制，底部 64 px 按原程序语义落在 viewport 外。首次视觉检查发现实现错误地把 panel 放在顶部；现已修正根因并用尺寸回归测试固定，不以视觉容差掩盖。
- 修正 positional tokenizer 后，真实八包 Headless 运行到 372 个 fixed tick：实际提交 8 帧，保存黑场、标题、可见 CrossFade2、panel 和首条 message 五个 checkpoint，消费 12 条物理输入，snapshot round-trip 为 true，diagnostic 为 0。运行读取 10 个资源、35 次 range、4913549 bytes；message frame hash 与 panel 不同。人工查看确认日文字形完整可读，没有缺字方框、横向裁剪或拉伸，正文位于 panel 有效区域。该证据只关闭首条可见 message E2，不代表原版像素一致。
- 公共 API 新增 `LegacyTextPresentationV1` 和 lease binding，只传递 language、显式字体 family、body/speaker region、字号、行高、行数和颜色。它通过现有 `Presentation` effect 发送，没有改动 `LegacyEffect` v1 的 postcard layout 或 ABI fingerprint。正文仍通过一次性 lease 传递，不进入 effect、snapshot、trace 或报告。Headless Host 使用现有 `CosmicTextLayoutProvider`、`TextRenderResourceOwner` 和 `astra-media-core` CPU Renderer2D，把 glyph 合成到最近的无文字 underlay；后续消息不会把上一条正文烘焙进背景。
- IDA 已确认原程序消息字体默认值为 26 px，ruby 为 12 px，默认字体是 CP932 的 MS PGothic。移植按计划显式绑定仓库内 Noto Sans JP，不读取系统字体。首轮 body/speaker region 结合已确认的 panel 几何与外部截图结构制定，真实 checkpoint 检查前只算实现绑定，不声明原版精确坐标。

### 已确认事实

| 事实 | 证据等级 | 说明 |
| --- | --- | --- |
| 八个真实 index 可由同一 private profile 解开 | 本地样本 | mount preflight 与 decoded full verify 通过 |
| `bg=4616`、`bgm=49`，八包合计 14502 entries | 本地样本 | 只记录计数，不记录文件名或 payload |
| `bg.pazA` 至 `bg.pazJ` 是连续分卷 | GARbro contract + 本地样本 | 11 个物理卷完成 bounds 与 encrypted range 读取 |
| ANI/SQZ header、index 与像素 layout | GARbro contract + 本地样本 | synthetic fixture 与 6947 个真实 frame 均通过 |
| `chain`/`end` 控制流 | 原程序反编译 | `CommandChain`、`CommandEnd` 的 RTTI、vtable 和 handler 调用关系一致；普通 call 假设已撤销 |
| `set`/`setGlobal` store 边界 | 原程序反编译 | 两个 command 共用 assignment evaluator，但绑定不同 store；读取顺序为 local 后 global |
| `if`/`wait` | 原程序反编译 | `if` 固定四个 token并支持 `!=/==/>/</>=/<=`；`wait` 参数按 10 ms timer tick 递减 |
| tokenizer/`message` | 原程序反编译 + 本地样本 | 每个 space/tab 都切出一个位置，连续分隔符保留空字段；message 使用 id、voice、speaker 和拼接正文，短参数执行默认空更新 |
| `stage`/`transition` | 原程序反编译 + 本地样本 + Headless | 前景/背景/坐标/stand pair 与 VFS role 已确认；普通 PNG stage 已进入 Astra presentation，stand position 和 transition 动画仍未知 |
| `effect CrossFade2` | 原程序反编译 + 本地样本 + Headless | 冒号资源序列、空帧、混合步进、更新时间阈值和循环索引已进入确定性 state；真实可见帧已形成独立 checkpoint，其他 effect id 仍未知 |
| `panel` mode 1 | 原程序反编译 + 本地样本 + Headless | 已确认 `CMessagePanel`、mode 1 默认资源、存档字段、坐标公式和 effect 上层合成；真实底部 checkpoint 已检查，其他 mode 与过渡参数未知 |
| message 字体与 Host 路径 | 原程序反编译 + contract tests + Headless | 原程序默认正文 26 px、ruby 12 px；移植显式绑定 Noto Sans JP，并复用 CosmicText/Renderer2D；首条真实 checkpoint 已确认无缺字、横向裁剪或拉伸，不声明原版像素一致 |
| BGM/SE resource operand | 原程序反编译 + 本地样本 | token 还包含由原程序解析的资源 metadata；必须先解析再绑定 VFS |
| BGM/SE 非控制资源绑定 | 原程序反编译 + 本地样本 | 401 条引用均精确、唯一命中；`resource[volume,pan]` 在映射前剥离 metadata |
| 音频 `*` token | 原程序反编译 + 本地样本 | BGM、SE1/2/3 与 voice 均停止各自固定 stream；不是资源名 |
| 真实 Minori Headless 启动 | 本地样本 | 签名动态 plugin、八包 mount、372 fixed tick、8 实际呈现帧、5 checkpoint、snapshot round-trip 和音频 artifact 通过；已到首条可见 message |

### 冲突与 blocker

- 旧文档把六个业务包误写成完整集合。八包 full verify 已补齐：14502 entries、43818 次 range read、6624958365 个 decoded bytes。
- 启用 cache 的八包验证因平台私有缓存卷空间不足，在首个写入处阻断。no-cache full verify 已通过，但 cache identity 第二轮全命中仍需单独证据。
- 首次八包挂载会读取约 5.74 GiB source，并为 entry 建立完整性身份；当前一次挂载耗时数分钟。后续需在不削弱 source mutation 检测的前提下合并顺序 hash 工作，不能通过跳过校验提速。
- `stage` 已覆盖无 stand 的普通图像路径，但 stand position 不能按名称猜成像素坐标；遇到 stand 时返回稳定 blocker。transition 配置已保存，动画插值和 fence 尚未接入。
- 真实 Headless 已到首条可见 message，但只检查了选定帧，没有完成整个 effect 周期的逐帧节奏比较。`select`、普通 voice、其他 panel/effect 和后续主要演出仍需逐项确认；assignment 的字符串值和除零行为也仍需脱敏 census。
- AstraEMU runner 现在同时输出专用 `astra.emu.headless_run_report.v2` 和公共 `astra.headless_run_report.v2`；两者绑定同一 manifest hash、输入、checkpoint 和 diagnostic 状态。公共 report 已通过 protocol 定向测试，但尚未在真实八包运行后交给 `prepare-review`/`validate-review`，因此正式 review 仍未闭合。不能用本次模型查看覆盖这项真实 evidence。
- 尚无首条完整路线、全 movie codec inventory、完整 required checkpoint 集合或 Windows E3 证据。

### 本次测试

```sh
cargo test -p astra-emu-family-core -p astra-emu-minori -p astra-emu-minori-cli
cargo test -p astra-emu-family-api
cargo test -p astra-emu-minori -p astra-emu-cli --lib
cargo test -p astra-emu-fvp -p astra-emu-manager-core --lib
cargo test -p astra-platform-headless --test host_contract surface_without_a_submission_can_be_destroyed
cargo check -p astra-emu-cli
cargo clippy -p astra-emu-family-api -p astra-emu-minori -p astra-emu-cli -p astra-emu-fvp -p astra-emu-manager-core --all-targets -- -D warnings
python Tools/check_docs.py
```

此外，ignored 私有样本完成一次签名动态 Minori Headless E2：372 fixed tick、8 个实际呈现帧、5 个 checkpoint、snapshot round-trip 和音频 artifact。自动报告为 passed，消费 12 条物理输入，diagnostic 为空。人工查看黑场、居中竖排标题、可见灯光 effect、底部 panel 和首条日文正文；未见缺字方框、横向裁剪或拉伸。这仍不代表完整 effect 周期、完整 VM、完整路线、正式 Headless review 或 Windows E3 完成。

### 下一步

1. 在具备足够空间的私有缓存卷复核 cache identity，并对 49 个 Ogg 与 5 个 movie 做 provider-level codec inventory。
2. 在下一次真实路线运行中验证公共 run report，并生成、检查、验证正式 review bundle。
3. 继续复核 `CrossFade2` 完整周期并确认 stand position；通过显式 provider binding 接入人物 ANI、MediaPlayback 和平台存储。
4. 扩展物理输入序列到首条完整路线，补齐 required checkpoint；自动门禁通过后再做完整模型视觉审查。
