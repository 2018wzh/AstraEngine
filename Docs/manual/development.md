# 开发与测试

在仓库根目录运行 `cargo xtask`。Engine/VN/Player/共享库由根 workspace 管理，EMU 使用 `Emulator/Cargo.toml` 和独立 lockfile/target；Editor 将在实际 GPUI 实现接入时加入独立 workspace。共享库通过路径依赖复用，不复制源文件或产物。

```bash
cargo xtask docs
cargo xtask check --workspace engine
cargo xtask check --workspace emu
cargo xtask test --workspace engine -p astra-core
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-family-api
```

`check` 执行链接检查、格式、clippy、产品构建和完整测试；`fmt` 只处理所选 workspace 的成员，不格式化第三方路径依赖。`test -p` 是普通定向测试，不启动或预构建 Headless。完整产品测试先构建所选产品的程序，供真正的 CLI/宿主测试使用。

全量开发测试会产生较大的调试符号和增量缓存。CI 设置以下环境变量以控制空间，本地空间有限时也可使用；它们不改变 Release 配置。需要调试器符号时移除这些变量并重建。

```bash
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cargo xtask check --workspace engine
```

并行开发每个实例使用独占 worktree 和 target；不要设置指向其他 worktree 的 CARGO_TARGET_DIR。构建/测试失败必须修复或准确记录根因。

文档检查仅检查链接、控制字符与私有绝对路径，允许 TODO 和未完成状态，不要求报告 schema 或指定 Stage 文字。普通 Rust 测试验证数据和行为，GPU/音频/设备测试按实际需要运行。开发 Agent 辅助真实产品验收，不生成具名审批体系。

Windows 完成长流程，Linux/macOS/Android 完成代表流程和平台特性；没有 GPU、音频设备或商业源时不得声称相关真实流程通过。

Windows 独立 Player 包通过 `--windows-runtime` 显式提供与 Player 构建工具链匹配的 Microsoft VC x64 CRT 可再发行目录。打包器使用 object 解析器验证全部 DLL 为 x64 PE 动态库，再复制并加入既有 bundle 文件清单；截断 PE、错误架构或非 DLL 文件在复制运行库前失败。缺少 `msvcp140.dll`、`vcruntime140.dll` 或 `vcruntime140_1.dll` 时失败。目标机器无需依赖开发机已安装的 VC Runtime。目录须使用 Microsoft 允许随应用分发的运行库，不能用调试版 DLL 代替。

```sh
astra package bundle .tmp/game.astrapak \
  --out .tmp/windows-game --target game --profile classic --platform windows \
  --windows-player target/debug/astra-player.exe \
  --windows-runtime "$VC_CRT_REDIST" \
  --crash-reporter target/debug/AstraCrashReporter.exe
```

## EMU GPU Headless 调试

`astra-emu-manager --headless .tmp/run.json` 加载显式指定的 Family 插件，以 60 Hz 推进并将最后一帧写入 PNG。插件继续使用自己的渲染器；FVP 要求硬件或 Sandbox 虚拟 GPU，拒绝软件 adapter。音频使用按采样时钟消费 PCM 的 Null 后端，因此此入口不验证实际声音。

配置包含 `plugin`、`game`、`configuration`、`frames` 和 `output`。路径相对运行目录解析，游戏副本与输出放在 ignored 目录，测试前保护原存档。`frames` 范围为 1–36000。

FVP 的 RFVS v2 保存原生动画容器及剩余进度，读档后继续透明度、移动等演出。旧 RFVS v1 缺少这部分状态，读取时明确拒绝且保留文件；测试使用新空槽位，不覆盖旧档。RFVG 全局存档格式保持不变。

Native VN 的 GPU Headless 使用 `render_policy: checkpoints` 时，独立资源变更可合并提交；同一资源再次变更前会先提交前一个待绘制帧，保留释放与重新上传的跨帧顺序。这些中间提交不执行像素回读，检查点仍读取最新画面。逐帧视听与性能测试使用 `all`。

`astra-headless` 的日志过滤使用 `ASTRA_LOG`。长流程定位可设置 `ASTRA_LOG=info,astra_headless=trace,astra_platform_headless=debug`，查看等待完成的输入序号、tick 与资源提交边界；这些日志不包含剧情正文。

可选 `inputs` 按零起始帧号排序，同帧保留给定顺序。输入只投递物理键鼠事件，点击需要分别指定按下与释放；坐标使用游戏原始画面像素。键名复用 Manager 输入映射，例如 `enter`、`escape` 和 `space`。以下片段在第 120 帧按下 Enter，下一帧释放：

```json
"inputs": [
  {"frame": 120, "event": {"type": "key", "code": "enter", "pressed": true}},
  {"frame": 121, "event": {"type": "key", "code": "enter", "pressed": false}}
]
```

鼠标移动使用 `{"type":"pointer_move","x":640,"y":360}`；按钮使用 `{"type":"pointer_button","secondary":false,"pressed":true}`，`secondary` 选择右键。非法键名、坐标、帧顺序和越界帧在打开插件前拒绝。此入口用于复现问题；最终帧和进程成功退出不能替代完整产品验收。

## 核心适配与 Manager 日志

适配优先启用核心现成的 GPU feature 和平台能力，仅修改嵌入入口、Family API 和必要生命周期边界；复用原生渲染、媒体、VM 及存档，不另建竞争路径。差异写入各 Family 的 MODIFICATIONS.md。CPU 最终帧是交付格式，不代表使用 CPU 渲染。

动态核心的 tracing/log 由 Family API v3 日志桥接入 Manager。旧插件须重建后重新安装并重启 Manager；FVP/Minori 构建动态插件时开启 dynamic-plugin-export。静态核心使用宿主订阅器。

```sh
RUST_LOG=info,astra_emu::family=debug astra-emu-manager
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-fvp --features dynamic-plugin-export
```

`core_target` 保留核心来源，`event` 是稳定事件名；`fields` 为有界结构化字段。`redacted_fields` 表示未传出的字段数量。若只看到 `family.unstructured_log`，应在对应核心根因边界增加安全的稳定事件和数值字段，不能为排错直接透传游戏正文、路径或整个 Debug 对象。日志桥初始化失败会阻止加载；读取损坏存档仍必须返回错误且保留原文件。

## VN 逐字显示与物理输入

长流程定位可设置 `ASTRA_LOG=info,astra_headless=trace`。`astra.headless.input.started/completed` 按输入序号记录开始与完成，字段仅含输入种类、有效 tick 和推进 tick 数；配合 `astra.headless.await.completed` 区分输入处理中断与等待条件未满足，不记录按键内容、商业文本或观察值。只看到开始事件不能视为该输入成功。

Classic 路线驱动 classic_y_route_acceptance.py 默认验证 Y→K 段；传入 --complete-route 后沿 --route-id 指定的生成路线执行到结局。驱动校验所有选择均已消费，再等待真实 VN session 的 vn.terminal_routes 匹配目标终局。完整路线的输入 tick 包含逐项等待超时预算；私有 Headless profile 的 input.max_tick 和 max_messages 应按生成输入配置。驱动在创建运行目录和启动 GPU 进程前检查这两项预算，不足时输出 required/configured 数值，不自动扩大配置。累计渲染帧与音频预算另按测试时长配置，不能用输入条数替代；限额失败不计路线通过。

批量测试 Classic 时，先用同一生成器导出完整路线输入，再交给现有矩阵执行器。生成器逐条处理，全部通过终局、session、输入序号和关闭检查后才生成最终目录；失败清理自己的临时目录，已有输出不覆盖。stdout 的 `required_max_messages` 和 `required_max_tick` 是全部路线所需输入预算，不是运行完成统计。Modern 仍使用转换器生成的对应输入，不能把两种 UI 的输入混用。

```sh
python Tools/TsuiNoSora/classic_route_inputs.py \
  --story-ir .tmp/tsuinosora/native_story_ir.json \
  --output .tmp/tsuinosora/classic-matrix-inputs
```

将输出目录作为 `headless_route_matrix.py --automation-root`，并显式传入同一 Classic package、GPU profile、build identity 和独立产物目录。矩阵的 `--timeout-seconds` 是每条路线的实际运行时间上限，需按完整流程设置；输入生成成功不计入 37 路线验收。

矩阵仅接受显式 `wgpu_offscreen` 配置，启动前检查每条输入的消息数和 tick 预算；实际运行传入 `--gpu`，共用单路线的原生硬件 adapter、构建、包与检查点校验。旧版 `--resume-report` 已移除，不再仅凭汇总文件跳过路线。输入预算不足或 GPU 校验失败均不得计作通过。

对白进入 pending wait 时，文字可能还在逐字显示。第一次推进输入会补全文字，后续输入才推进剧情。Headless 可通过只读观察项 `vn.text_reveal_complete` 等待当前文字显示完成，再发送物理按键；它来自真实演出状态，不修改剧情游标或显示进度。没有正在显示的文字时为 true。Classic 路线脚本已按此区分对白等待与普通输入等待，不能仅凭 `vn.pending_wait_command` 就假设一次 Enter 足以推进。
