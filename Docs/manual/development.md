# 开发与测试

## 日常开发

按当前任务定位代码、实施修改、验证结果即可。常规编辑、构建、测试和本地提交已有授权；独立任务可以委派，由发起者整合。远端推送、发布和主线整合沿用当前任务的授权，涉及未授权的破坏性操作再确认。

文档按需查阅，与代码一起更新，不是开工审批。任务明确时直接推进；文档滞后就修正相关段落，不要求先整理全部文档、冻结全部设计或重启会话。尚未确定且会改变产品范围的取舍单独确认，其他工作继续。

协作方式按实际冲突选择：串行修改可使用当前工作树，并行改动重叠时使用独立 worktree/分支；构建产物、服务端口也按需要隔离。保留用户和其他任务的修改，合并重叠改动时明确整合者，不把每项任务新建工作树作为固定流程。

实现优先复用现有代码和成熟依赖，按职责拆分。公共接口变化同步相关消费者，兼容策略遵循该接口的实际要求；不为局部修改扩大全仓重构。跨平台工具优先 Python，文档示例使用 bash/sh。数据格式见[数据契约](../contracts/data-formats.md)，日志复用 [astra-observability](../contracts/logging-observability.md)，生命周期和原生核心适配见[重构契约](../contracts/rebuild.md)。

## Workspace 与常用命令


在仓库根目录运行 `cargo xtask`。Engine/VN/Player/共享库由根 workspace 管理，EMU 使用 `Emulator/Cargo.toml` 和独立 lockfile/target；Editor 使用 `Editor/Cargo.toml` 和独立 lockfile/target，入口见 [Editor 手册](../../Editor/README.md)。共享库通过路径依赖复用，不复制源文件或产物。

```bash
cargo xtask docs
cargo xtask check --workspace engine
cargo xtask check --workspace emu
cargo xtask check --workspace editor
cargo xtask test --workspace engine -p astra-core
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-family-api
```

CI 对三桌面的 Engine、EMU、Editor 分别执行 `check`，EMU job 检出时递归初始化固定提交的子模块；各 job 拉取 Git LFS 资产。`check` 执行链接检查、格式、clippy、产品构建和完整测试；`fmt` 只处理所选 workspace 的成员，不格式化第三方路径依赖。`test -p` 是普通定向测试，不启动或预构建 Headless。完整产品测试先构建所选产品的程序，供真正的 CLI/宿主测试使用。

EMU 的 `Source/Families` 成员（Musica、CMVS 等自有核心）由 xtask 分成独立 Cargo 构建/Clippy/测试单元，避免把互斥的运行时 feature 合并进 Manager 的依赖图。动态核心仍在同一 Manager 进程运行；这不是进程隔离。静态组合与后续平台需要另外验证，不能用独立 DLL 的编译结果代替。

外部核心不是 EMU workspace 成员，`cargo xtask check --workspace emu` 不构建也不测试它们。适配代码全部位于各 fork `astra` 分支上的单个提交，主仓只固定 submodule。检查外部核心时在各自 fork manifest 上单独构建，`--target-dir` 指向盘符根下的短目录（发布工作流用 `t\<名称>`），以免嵌套原生构建超出 Windows MAX_PATH；测试把 `build` 换成 `test`，manifest、包名与 feature 不变：

| 核心 | 命令 | 前置环境 | 产物 |
| --- | --- | --- | --- |
| rfvp | `cargo build --manifest-path Emulator/ThirdParty/rfvp/Cargo.toml --locked --release -p astra-emu-rfvp --features dynamic-plugin-export` | `ASTRA_SUBTITLE_VCPKG_ROOT` | `astra_emu_rfvp.dll`，另需 `fribidi-0.dll` |
| siglus_rs | `cargo build --manifest-path Emulator/ThirdParty/siglus_rs/astra/astra-emu-siglus_rs/Cargo.toml --locked --release -p astra-emu-siglus_rs --features dynamic` | 无 | `astra_emu_siglus_rs.dll` |
| art3m1s-core | `cargo build --manifest-path Emulator/ThirdParty/art3m1s-core/Cargo.toml --locked --release -p astra-emu-art3m1s-core` | `VCPKG_ROOT`（FFmpeg 9）、`LIBCLANG_PATH` | `astra_emu_art3m1s_core.dll`，另需 FFmpeg 9 DLL |
| sena-rs | `cargo build --manifest-path Emulator/ThirdParty/sena-rs/Cargo.toml --locked --release -p astra-emu-sena-rs --features diagnostic-bridge` | 无 | `astra_emu_sena_rs.dll` |
| ethornell-rs | `cargo build --manifest-path Emulator/ThirdParty/ethornell-rs/Cargo.toml --locked --release -p astra-emu-ethornell-rs --features diagnostic-bridge` | 无 | `astra_emu_ethornell_rs.dll` |
| kirikiroid2-web | CMake `-DKRKR2_ASTRA_EMU=ON`，目标 `astra_emu_kirikiroid2_web`；测试 crate 为 `Emulator/ThirdParty/kirikiri2/astra/Cargo.toml`，步骤见 fork 的 `astra/README.md` | MSVC、WinFlexBison、KrKr vcpkg | `<build>/astra/astra_emu_kirikiroid2_web.dll`，另需 FFmpeg 3.3.9 DLL、`Resources/` 与 `vcomp140.dll` |

外部核心的 plugin ID 等于上游仓库名，适配 crate 命名为 `astra-emu-<上游仓库名>`。Rust 适配统一以 Git 依赖固定 AstraEngine `6d9d691dcc8f3d768e40b5be1d6f37366cca62d8` 的 `astra-emu-abi`；EMU workspace 的 `[patch]` 把同一来源映射到本地路径，Android Manager 静态链接的 `astra-emu-rfvp` 也走这条映射。

全量开发测试会产生较大的调试符号和增量缓存。CI 设置以下环境变量以控制空间，本地空间有限时也可使用；它们不改变 Release 配置。需要调试器符号时移除 debug 设置并重建。遇到 rustc/LLVM OOM 时先限制编译并发，不修改生产逻辑或跳过失败用例；GPU/窗口测试也应串行占用设备。

```bash
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
cargo xtask check --workspace engine
```

只限制并发、保留已有 Debug 配置时，Engine/Editor 可执行 `cargo build --workspace -j 1`，再执行 `cargo test --workspace -j 1 -- --test-threads=1`；EMU 使用 `cargo xtask build --workspace emu`、`cargo xtask test --workspace emu`，或按核心分开执行 `cargo -p`，不要合并互斥的运行时 feature。完整检查与定向检查不要同时使用同一 target；先等前一项结束，或显式使用本工作树内的独占目录，不清理其他正在使用的构建目录。

## 按改动选择验证

局部 Rust 改动运行相关测试、格式检查和受影响 crate 的 Clippy，并实际运行改变的路径；跨模块改动扩大到对应消费者。完整 workspace 检查用于跨 workspace 变更、CI 或准备交付，不要求每次提交或每个阶段都重跑全部产品。单纯文档修改运行 `python Tools/check_docs.py`。

GPU、音频、窗口和设备测试按改动需要运行。开发时可使用 Headless、软件 adapter、NullAudio 或 fixture 定位问题；结果标明覆盖范围。设备或素材缺失时继续不依赖它们的工作，将相关验收留待可用环境执行。各产品的硬件、平台和结局要求见[实施计划](../status/implementation-plan.md)，局部测试通过不等于整项产品验收完成。

## 数据与提交

商业资源、正文、截图、存档、私有 profile/key 和转换产物放在 ignored 工作区；测试使用副本或新槽，避免覆盖用户原数据。读取失败保留原文件，清理仅针对本任务产生且不再使用的临时产物。

提交保留许可证与修改归属，检查是否混入无关修改、secret、私有路径或商业正文。缺失能力、错误及未运行的检查如实说明。上游问题与适配改动分别记录，性能测量见[性能审计](../status/performance-upstream-audit-2026-10-12.md)。

文档以中文为主，维护有用的接口说明、当前状态和操作方法；旧过程留在 Git。文档检查只查链接、控制字符和私有绝对路径，不要求报告 schema、审批文件或固定状态表。

## 平台与打包

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

## EMU Release 人工验收

AstraEMU 仅使用实际 Release Manager 和各核心 Release 动态库进行人工验收。启动产品窗口后，由用户选择游戏、推进剧情、操作菜单与存读档并退出重开；不得自动输入、截图或关闭用户窗口。Manager 的脚本式 headless 验收入口和额外硬件驱动已移除。

FVP 使用固定 RFVP 来源的原生 RFVS/RFVG 保存路径和格式；Astra 不添加旧格式 reader 或迁移层。保护原商业存档，开发时使用隔离副本与新槽；读取失败保留原文件。

Native VN 的 GPU Headless 使用 `render_policy: checkpoints` 时，独立资源变更可合并提交；同一资源再次变更前会先提交前一个待绘制帧，保留释放与重新上传的跨帧顺序。这些中间提交不执行像素回读，检查点仍读取最新画面。逐帧视听与性能测试使用 `all`。

`astra-headless` 的日志过滤使用 `ASTRA_LOG`。长流程定位可设置 `ASTRA_LOG=info,astra_headless=trace,astra_platform_headless=debug`，查看等待完成的输入序号、tick 与资源提交边界；这些日志不包含剧情正文。


## 核心适配与 Manager 日志

适配优先启用核心现成的 GPU feature 和平台能力，仅修改嵌入入口、Family API 和必要生命周期边界；复用原生渲染、媒体、VM 及存档，不另建竞争路径。自有核心的差异写入各 Family 的 MODIFICATIONS.md，外部核心的差异写在其 fork `astra` 提交内的说明文件。CPU 最终帧是交付格式，不代表使用 CPU 渲染。

动态核心通过 `astra-emu-abi` 的诊断桥接入 Manager，静态核心复用宿主 subscriber。按各核心说明启用动态导出 feature，将 `astra_emu_`（Unix 为 `libastra_emu_`）动态库及其依赖放入数据目录 `cores/`；Manager 冷启动扫描，按文件保留加载诊断。更新需替换文件并重启，不热加载，也不加载旧 abi_stable 协议。

视觉滤镜使用锁定的 DirectXShaderCompiler 版本；Windows 发布目录需同时携带 `dxcompiler.dll` 和 `dxil.dll`，并与 Manager 可执行文件放在同一目录。Manager 默认按可执行文件目录定位 `dxcompiler.dll`，不会依赖启动时的工作目录或系统 PATH；`ASTRA_EMU_DXC_PATH` 只用于显式指定并校验一个替代位置，缺失或校验失败会直接报告滤镜诊断。

```sh
RUST_LOG=info,astra_emu::family=debug astra-emu-manager
cargo build --manifest-path Emulator/ThirdParty/rfvp/Cargo.toml --locked --release -p astra-emu-rfvp --features dynamic-plugin-export --target-dir "$SHORT_TARGET_ROOT/rfvp"
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

## 外部原生核心接入

核心直接导出 `astra_emu_query`／`CoreApi`，声明 typed 配置，自持调度和音视频，交付完整 CPU/GPU 帧；Host 投递输入、读取状态及帧，不调用 advance。动态导出及日志桥按各核心构建说明启用，静态链接使用相同 Query 函数表。

CPU owner 不依赖会话，Host 可以在关闭后归还持有的不可变 lease。实际游戏功能由用户在 Release Manager 中人工检查，不保留公开假核心或脚本自动验收入口。

## NativeVN 嵌入接口

使用 `astra_vn::{VnSession, VnSessionConfig, NativeVnStepInput}` 创建和推进会话，保存/恢复传递 `astra_runtime::SaveBlob`，关闭消费 VnSession。原 `astra-vn-runtime-provider` 依赖须改为 `astra-vn`，不提供旧名称别名。静态 NativeVN 包已删除 gameplay descriptor 和动态 Factory/ABI request 链；通过现有 Cook/package 入口生成包。EngineSession 的任务作用域由会话持有，调用方不得在退出后继续提交结果。

本接口的增量回归包括 `cargo test -p astra-vn -p astra-runtime` 和 `cargo test -p astra-player-vn --lib`；包检查修改另运行 `cargo test -p astra-release --test release_report`。这些测试不代替 Windows/Android 实玩。

## Editor Player 预览控制

增量 cook/package/bundle 后，以 `astra-player --preview-control` 启动独立 Player，stdin/stdout 仅用于 `astra.vn.preview.v1`，stderr 接收日志。父进程须先发送 Attach、等 Ready 后显示运行；变更文档版本时停止并回收旧进程，不能复用旧 identity。Pause/Resume 和片段内精确 checkpoint 定位见[预览控制契约](../contracts/player-preview.md)。用户游戏存档槽不参与预览定位。


### Headless 退出与缩略图调试

CLI 的 run/serve 必须持有 HeadlessThreadOwner，并通过 factory.with_thread_owner 绑定会话线程。owner 的作用域包住产品/会话：正常关闭先执行产品 shutdown，失败返回或取消时再取消并 join 剩余 Host。只有资源所属线程完成析构后，CLI 才能退出；不能以 process::exit 或超时强杀代替此收尾。 同一 owner 还跨会话保留原生 GPU Instance，供各 factory 使用；session 的 renderer、device、queue 和纹理仍各自创建和释放。直接调用 WgpuOffscreenRenderer 时也必须传入长期 graphics owner 持有的 Instance，不能在反复打开的 session 内重新创建整个 owner。

Player 的物理输入入口使用 NativeVnHostCommandSource::prepare_ui_input，先按批内顺序执行命令，再把 Captured 结果交给 cache_gameplay_surface，最后处理 UI save request。新增宿主不要恢复仅按右键判定截图的逻辑。NativeVN 输入工具的 --save-restore 覆盖写槽与读取；--cold-restore 用另一进程读取已存在的一份测试存档，不能与写槽或影片输入选项合并。

`xtask` 在产品 build 步骤排除正在运行的自身可执行文件，避免 Windows 文件锁阻止覆盖；自身仍参加格式、Clippy 和测试。平台内部故障注入命令的类型在各 feature 组合下保持一致，客户端注入入口仍由 `platform-test-driver` 控制，未开启的宿主明确拒绝注入请求。

## Linux GStreamer 定向验证

WAV 解码依赖 GStreamer 的 playback/typefind、wavparse、audioconvert 和 audioresample。开发机安装对应平台插件包，或用 `GST_PLUGIN_PATH` 指向本机私有插件目录；不将插件复制进源码，也不在失败后更换 decoder。以下用例实际解码公开构造的 16/24-bit PCM，并覆盖重复请求、重新创建和损坏流，默认 ignored，必须显式执行才计通过：

```bash
cargo test -p astra-platform-linux --lib decode::tests -- --include-ignored
```

无显示会话的 runner 可单独验证 Host 启动拒绝；这不是窗口播放成功：

```bash
cargo test -p astra-platform-linux --features platform-test-driver --test unavailable missing_display_rejects_real_host_start -- --ignored
```

正式产品仍须在有显示与音频设备的 Linux 环境验证完整播放、系统页、存读档、退出及重新启动。WAV 局部通过不代表视频已验收；完整视频测试见下节。

## Linux 视频流与 GPU 验证

视频测试使用 GStreamer 的 appsrc/appsink、videoconvert、VP8 encoder/decoder 与 WebM mux/demux 生成公开最小 fixture。安装所需插件后，显式执行流式、取消、截断和 GPU 用例；没有硬件 GPU 不运行 GPU 用例，不用软件 adapter 替代：

```bash
cargo test -p astra-platform-linux --lib decode:: -- --include-ignored --skip complete_local_movie_streams_through_worker_and_gpu --test-threads=1
```

实际本地视频测试须事先独立核对容器帧数、时长与 codec；环境变量只供这条 ignored 测试选取本地素材，不进入产品 API。每帧经过 session worker 和硬件 renderer，检查 PTS/时长、像素及结束计数。`ASTRA_LINUX_VIDEO_TEST_ARTIFACT_DIR` 可选，用于保存首/中/尾 GPU RGBA 回读，仅指向 ignored 私有目录。

```bash
ASTRA_LINUX_VIDEO_TEST_FILE='<authorized-video-file>' \
ASTRA_LINUX_VIDEO_TEST_CODEC=mp4 \
ASTRA_LINUX_VIDEO_TEST_FRAMES=288 \
ASTRA_LINUX_VIDEO_TEST_DURATION_US=12000000 \
cargo test -p astra-platform-linux --lib complete_local_movie_streams_through_worker_and_gpu -- --ignored
```

示例数字对应 12 秒、24 fps 的影片，其他素材必须填写独立核对后的真实值。H.264/MP4 另需所选 GStreamer 环境提供对应 decoder/demux 插件；不因缺插件改用其他 Astra provider。编码输入、单帧/音频输出和队列边界见 [重构契约](../contracts/rebuild.md)。本测试不验证原生窗口、设备声音或正式性能预算。
