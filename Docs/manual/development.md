# 开发与测试

在仓库根目录运行 `cargo xtask`。Engine/VN/Player/共享库由根 workspace 管理，EMU 使用 `Emulator/Cargo.toml` 和独立 lockfile/target；Editor 使用 `Editor/Cargo.toml` 和独立 lockfile/target，入口见 [Editor 手册](../../Editor/README.md)。共享库通过路径依赖复用，不复制源文件或产物。

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

Android Player 的构建入口是 `Tools/build_android.py`。AGP 9.3.0 配合 Gradle 9.5.0，构建脚本固定使用 JDK 17；设置 `JAVA_HOME` 时，校验与 Gradle 都使用该目录的 Java，而不是 PATH 中另一版本。SDK、NDK、Gradle 缓存只对当前构建进程设置，独占工作树的输出不能指向其他实例。

Android 游戏包必须包含对应 Android host profile；不能把 Windows profile 的包改扩展名后当作 Android 包。Player 从包的 `player.display_config` 读取逻辑舞台尺寸，实际窗口尺寸和安全区由设备事件更新。设备验收应覆盖触控、系统返回、前后台、声音、存读档与重新启动，编译成功不代表这些流程已通过。

Android Player 启动时先进入 Activity 事件循环，每轮最多读取 1 MiB 包数据；界面显示已读字节和当前阶段。后台仅校验 owned bytes，一次完整 storage audit 后将同一 PackageReader 交给 session，保留 section 读取校验。加载期间可返回，挂起会取消加载；重试沿用当前事件循环，关闭会等待工作线程回收。运行期挂起冻结演出与媒体时间、暂停设备输出，并仅释放系统 surface；恢复沿用 GPU 资源并重设调度期限。新加载 UI、无障碍事件防护、触控和前后台恢复仍需真机复测，本轮已按用户要求暂停设备验收。


Windows 独立 Player 包通过 `--windows-runtime` 显式提供与 Player 构建工具链匹配的 Microsoft VC x64 CRT 可再发行目录。打包器使用 object 解析器验证全部 DLL 为 x64 PE 动态库，再复制并加入既有 bundle 文件清单；截断 PE、错误架构或非 DLL 文件在复制运行库前失败。缺少 `msvcp140.dll`、`vcruntime140.dll` 或 `vcruntime140_1.dll` 时失败。目标机器无需依赖开发机已安装的 VC Runtime。目录须使用 Microsoft 允许随应用分发的运行库，不能用调试版 DLL 代替。

```sh
astra package bundle .tmp/game.astrapak \
  --out .tmp/windows-game --target game --profile classic --platform windows \
  --windows-player target/debug/astra-player.exe \
  --windows-runtime "$VC_CRT_REDIST" \
  --crash-reporter target/debug/AstraCrashReporter.exe
```

## EMU GPU Headless 调试

`astra-emu-manager --headless .tmp/run.json` 为调试入口显式指定 Family 插件，以 60 Hz 推进并将最后一帧写入 PNG；这条入口不写入 Manager 的 `cores/` 注册来源。插件继续使用自己的渲染器；FVP 要求硬件或 Sandbox 虚拟 GPU，拒绝软件 adapter。音频使用按采样时钟消费 PCM 的 Null 后端，因此此入口不验证实际声音。

配置包含 `plugin`、`game`、`configuration`、`frames` 和 `output`。路径相对运行目录解析，游戏副本与输出放在 ignored 目录，测试前保护原存档。`frames` 范围为 1–36000。

可选 `capture_frames` 在同一会话内保存指定帧，适合比较输入推进和存读档前后的画面。例如 `"capture_frames": [120, 240, 360]` 在各帧输入和推进完成后截图；帧从 0 开始，必须小于 `frames`，最多 128 项且不能重复。文件名在 `output` 文件名后追加 `.frame-120.png` 等后缀，最后一帧仍写入 `output`。游戏提前结束而未到达要求的截图帧，或截图缺失、写入失败时，测试明确失败并关闭会话；不会用最后一帧冒充未执行的帧。截图只能保存在 ignored 私有目录。

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

动态核心的 tracing/log 由 Family API v7 日志桥接入 Manager。构建动态插件时开启 `dynamic-plugin-export`，再把生成的 `astra_emu_`（Unix 为 `libastra_emu_`）动态库和其依赖放入 Manager 数据目录 `cores/`；Manager 冷启动自动扫描并在加载失败时按文件给出诊断。更新核心需重启 Manager，运行期间不热加载。静态核心使用宿主订阅器。

视觉滤镜使用锁定的 DirectXShaderCompiler 版本；Windows 发布目录需同时携带 `dxcompiler.dll` 和 `dxil.dll`，并与 Manager 可执行文件放在同一目录。Manager 默认按可执行文件目录定位 `dxcompiler.dll`，不会依赖启动时的工作目录或系统 PATH；`ASTRA_EMU_DXC_PATH` 只用于显式指定并校验一个替代位置，缺失或校验失败会直接报告滤镜诊断。

```sh
RUST_LOG=info,astra_emu::family=debug astra-emu-manager
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-fvp --features dynamic-plugin-export
```

`core_target` 保留核心来源，`event` 是稳定事件名；`fields` 为有界结构化字段，超限字段计入 `dropped_fields`。日志桥不做内容脱敏或字符串白名单过滤，但产品日志仍不得主动记录游戏正文、路径或未经审计的整体 Debug 对象；若只看到 `family.unstructured_log`，应在对应核心根因边界增加安全的稳定事件和数值字段。日志桥初始化失败会阻止加载；读取损坏存档仍必须返回错误且保留原文件。

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

## NativeVN 嵌入接口

使用 `astra_vn::{VnSession, VnSessionConfig, NativeVnStepInput}` 创建和推进会话，保存/恢复传递 `astra_runtime::SaveBlob`，关闭消费 VnSession。原 `astra-vn-runtime-provider` 依赖须改为 `astra-vn`，不提供旧名称别名。包元数据使用 `astra_vn::native_vn_descriptor()`；无需注册 Factory 或传递 ABI request。EngineSession 的任务作用域由会话持有，调用方不得在退出后继续提交结果。

本接口的增量回归包括 `cargo test -p astra-vn -p astra-runtime` 和 `cargo test -p astra-player-vn --lib`；包检查修改另运行 `cargo test -p astra-release --test release_report`。这些测试不代替 Windows/Android 实玩。

## Editor Player 预览控制

增量 cook/package/bundle 后，以 `astra-player --preview-control` 启动独立 Player，stdin/stdout 仅用于 `astra.vn.preview.v1`，stderr 接收日志。父进程须先发送 Attach、等 Ready 后显示运行；变更文档版本时停止并回收旧进程，不能复用旧 identity。Pause/Resume 和片段内精确 checkpoint 定位见[预览控制契约](../contracts/player-preview.md)。用户游戏存档槽不参与预览定位。
