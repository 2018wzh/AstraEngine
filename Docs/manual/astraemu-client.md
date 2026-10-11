# AstraEMU 客户端

完整运行包包含 `astra-emu-manager`（Windows 为 `.exe`）、`cores/`、许可说明和本手册。解压后直接启动 Manager，不需要设置环境变量或手工复制核心到用户数据目录。只移动可执行文件会丢失随包核心。

首次启动点击“添加游戏”，选择原生游戏目录；也可选择包含多个游戏的父目录。扫描完成后选择游戏，点击“启动”。多个核心都能识别同一目录时，先在游戏详情中选择核心。游戏不需要转换为 AstraVN package。

游戏输入、原生菜单和原生存读档由核心负责。关闭游戏回到资料库；重新启动创建新的会话。资料库、设置和运行日志保存在系统用户数据目录，不写入程序目录；游戏的既有存档仍按核心自身规则处理。联网元数据和翻译需要在设置中单独启用。

出现 `ASTRA_EMU_AUDIO_DEVICE_UNAVAILABLE` 时，先检查运行环境是否提供音频设备。设置中心的 `NullAudioDevice` 仅供无声音测试，客户端会明确标记；它不表示音频播放已通过。

FVP 保留原生字体语义，要求系统提供 `MS Gothic`；脚本请求其他 MS 字体时也需要对应字体。缺少字体会显示 `ASTRA_EMU_FVP_FONT_MISSING`，不会静默替换字体。运行包不分发 Windows 系统字体。

客户端从程序相邻的 `cores/` 和用户数据目录的 `cores/` 同时发现核心。核心文件损坏、ABI 不符和重复 plugin ID 都会显示诊断；重复核心不会按目录顺序覆盖。替换核心后需要重启客户端。

运行包只包含实际构建的核心，不代表任意游戏已兼容。GPU 离屏测试、NullAudio、真实窗口操作和可听音频是不同检查；实际视听与完整游戏流程未验收时，不声明可通关。KrKr 的 Windows 原生 OFF/ON 与 Family/child 生产构建已完成，开发包包含真实原生依赖及同目录 VC/OpenMP runtime。Sandbox 已使用 NVIDIA OpenGL 4.6 进入真实脚本，但样本的原生模态提示框被嵌入接口拒绝，尚未产生游戏最终帧，当前不作为已验收核心提供。

当前 Windows 开发包已检查 FVP 游戏导入、标题与正文、输入、原生存档写入以及会话关闭重开。该游戏读档后会先恢复到保存菜单，点击游戏菜单的 `EXIT` 可返回所保存的剧情并继续推进；直接返回剧情的行为仍待修正。Sandbox 本次没有可用音频设备，使用 NullAudio，未完成可听音频、结局或正式呈现性能验收。

其他核心的真实样本仍在检查。CMVS 样本启动后在命令 548 处报告 `ASTRA_EMU_CMVS_ACTION_UNBOUND`，当前部分实现无法继续该游戏。Siglus 的 Release 包未重现此前 Debug shader 栈溢出；不透明帧与混音器独立取消修复后，实际窗口完成 Rewrite+ 标题、开场 WMV 播放至结束、正文输入、原生存读档，以及 Manager 结束会话后重开并从标题读档。读档直接恢复 CG 与文字并可继续剧情，原生标题 Exit 已正常返回 Manager，随后重开、标题读档、正文推进及 Quit 确认后退出也已完成。正文鼠标 Quit 已直接打开原生确认框，No 返回正文后可继续推进；Title 返回标题、Exit 返回 Manager、重开读档后 Quit→Yes 退出均已完成，无需额外 Enter。完整路线仍未验收。Artemis 已通过原生启动媒体、标题、剧情、输入、普通存档及退出重开；修复加载等待归属后，标题读档直接恢复 CG 和保存处文字，新槽保存、原生退出及重开读取新槽均可继续剧情，完整路线仍未验收。未实现的 UI 或媒体请求仍明确失败。开发包的编译结果不能作为可玩或性能结论；实际游戏与性能检查使用 Release 包。调试统计中的 `core_advance_us` 是 Family advance 耗时，`frame_capture_us` 包含 Family 帧访问、可能的 GPU 回读及 Host 复制，`cpu_submission_us` 是 Host 提交 CPU 耗时，不表示 GPU 完成时间。

本轮 Sandbox 的显示模式报告为 32 Hz，Slint 使用该频率进行窗口节流。实际窗口约 28–30 FPS 与这一限制相符，不能证明达到 60 Hz。核心推进、帧读取与 Host 提交开销分别测量，不通过更改宿主配置或强制重绘替代原生速率验收。

Musica 已移除启动前的全归档扫描，并修复 Git LFS 字体尚未检出时的打包问题。优化包在硬件 DX12 GPU 与 NullAudio 下完成《夏空のペルセウス》的原生标题、正文绘制、键盘推进、快速与普通存读档，以及关闭重开后从标题读取普通存档；没有完成结局或可听音频验收。两次 PresentMon 采集均未输出帧记录，尚无有效呈现性能数据，不能声明达到可玩帧率。

2026-10-12 在实际窗口的已存档剧情场景各采样约 30 秒，采样期间没有重型构建。使用 Release 包、NullAudio、Windows 11 build 26100、AMD Ryzen AI 9 HX 370（24 个逻辑处理器），Host 为 NVIDIA GeForce RTX 4070 Laptop GPU / Vulkan；Musica 核心使用硬件 DX12。下表是运行开销与推进频率，不是显示帧率；p95 单位为 ms。

| 核心 | raster | Host 推进 Hz | 核心推进 p95 | 帧读取 p95 | Host CPU 提交 p95 |
| --- | --- | ---: | ---: | ---: | ---: |
| FVP | 1280×720 | 60.52 | 5.406 | 1.491 | 0.506 |
| Artemis | 1600×1200 | 58.63 | 2.515 | 6.508 | 1.590 |
| Siglus | 1280×720 | 57.82 | 3.802 | 13.082 | 0.865 |
| Musica | 1280×720 | 59.34 | 7.894 | 8.563 | 0.807 |

FVP 原先在 Host PCM 写入受阻时连带阻塞每帧音频控制命令，同一场景仅推进 46.85 Hz，核心推进 p95 为 24.088 ms。现在使用单一 PCM 写入线程，最多保留一个待接收的混音块；混音线程等待 Host 接收时仍处理有序控制命令，下一块混音仍等待实际接收确认。修复后推进间隔 p95 为 18.065 ms、最大 39.958 ms。其他三核心的推进间隔 p95 分别为 23.171、24.287、25.461 ms，仍存在抖动；上述采样不代表完整路线或原生速率呈现验收。

## 构建运行包

下面以 FVP 为例。先初始化主仓固定的 submodule；尚未推送的本地适配提交需要从开发来源恢复，不应换成旧版提交绕过构建失败。

```sh
# 替换为数据盘上的专用工作目录；构建、运行包和临时文件放在其中。
work_dir="<data-disk-work-directory>"
export CARGO_TARGET_DIR="$work_dir/target"
export TEMP="$work_dir/tmp" TMP="$work_dir/tmp"
mkdir -p "$TEMP"
git submodule update --init
cargo build --manifest-path Emulator/Cargo.toml --locked --release -j 1 \
  -p astra-emu-manager -p astra-emu-fvp \
  --features astra-emu-fvp/dynamic-plugin-export
python Tools/Effects/fetch_dxc.py --output "$work_dir/dxc"
python Tools/AstraEMU/package_client.py \
  --manager "$CARGO_TARGET_DIR/release/astra-emu-manager.exe" \
  --core "$CARGO_TARGET_DIR/release/astra_emu_fvp.dll" \
  --runtime-file "$work_dir/dxc/dxcompiler.dll" \
  --runtime-file "$work_dir/dxc/dxil.dll" \
  --runtime-file "$work_dir/dxc/LICENSE-MS.txt" \
  --runtime-file "$work_dir/dxc/LICENSE-LLVM.txt" \
  --output "$work_dir/client"
```

Linux/macOS 分别使用无 `.exe` 的可执行文件以及 `libastra_emu_fvp.so` / `libastra_emu_fvp.dylib`；DXC 示例仅适用于 Windows。`--core` 可以重复传入多个已构建的 Family。`--runtime-file` 指定随包的依赖库或许可文件；Windows 分发还需要加入构建工具链对应的 VC runtime DLL。输出目录必须是新目录，避免上一版核心混入新运行包。该工具组装现有产物，不编译、不启动游戏、不发布。

构建 Musica 前还需恢复其编译期嵌入的字体，避免把 LFS 指针编入核心。运行包会带上字体的 OFL 许可。

```sh
git lfs pull --include="Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf,Examples/NativeVN/Assets/Fonts/NotoSansSC-Variable.ttf" --exclude=""
```
