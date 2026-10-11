# AstraEMU 客户端

完整运行包包含 `astra-emu-manager`（Windows 为 `.exe`）、`cores/`、许可说明和本手册。解压后直接启动 Manager，不需要设置环境变量或手工复制核心到用户数据目录。只移动可执行文件会丢失随包核心。

首次启动点击“添加游戏”，选择原生游戏目录；也可选择包含多个游戏的父目录。扫描完成后选择游戏，点击“启动”。多个核心都能识别同一目录时，先在游戏详情中选择核心。游戏不需要转换为 AstraVN package。

游戏输入、原生菜单和原生存读档由核心负责。关闭游戏回到资料库；重新启动创建新的会话。资料库、设置和运行日志保存在系统用户数据目录，不写入程序目录；游戏的既有存档仍按核心自身规则处理。联网元数据和翻译需要在设置中单独启用。

出现 `ASTRA_EMU_AUDIO_DEVICE_UNAVAILABLE` 时，先检查运行环境是否提供音频设备。设置中心的 `NullAudioDevice` 仅供无声音测试，客户端会明确标记；它不表示音频播放已通过。

FVP 保留原生字体语义，要求系统提供 `MS Gothic`；脚本请求其他 MS 字体时也需要对应字体。缺少字体会显示 `ASTRA_EMU_FVP_FONT_MISSING`，不会静默替换字体。运行包不分发 Windows 系统字体。

客户端从程序相邻的 `cores/` 和用户数据目录的 `cores/` 同时发现核心。核心文件损坏、ABI 不符和重复 plugin ID 都会显示诊断；重复核心不会按目录顺序覆盖。替换核心后需要重启客户端。

运行包只包含实际构建的核心，不代表任意游戏已兼容。GPU 离屏测试、NullAudio、真实窗口操作和可听音频是不同检查；实际视听与完整游戏流程未验收时，不声明可通关。KrKr 原生运行包仍等待 Windows Sandbox 验收，当前不作为已验收核心提供。

当前 Windows 开发包已检查 FVP 游戏导入、标题与正文、输入、原生存档写入以及会话关闭重开。该游戏读档后会先恢复到保存菜单，点击游戏菜单的 `EXIT` 可返回所保存的剧情并继续推进；直接返回剧情的行为仍待修正。Sandbox 本次没有可用音频设备，使用 NullAudio，未完成可听音频、结局或性能验收。

其他核心的真实样本仍在检查。CMVS 样本启动后在命令 548 处报告 `ASTRA_EMU_CMVS_ACTION_UNBOUND`，当前部分实现无法继续该游戏。Siglus 的 Debug 包在 Naga 编译 shader 时发生栈溢出，尚待 Release 复测。Artemis 已接入原生窗口标题及 typed 鼠标请求，真实样本尚待重测；未实现的 UI 或媒体请求仍明确失败。开发包的编译结果不能作为可玩或性能结论；实际游戏与性能检查使用 Release 包。调试统计中的 `core_advance_us` 是 Family advance 耗时，`frame_capture_us` 包含 Family 帧访问、可能的 GPU 回读及 Host 复制，`cpu_submission_us` 是 Host 提交 CPU 耗时，不表示 GPU 完成时间。

Musica 样本此前在首次挂载的全归档扫描中长时间停滞；现已改用索引和文件元数据生成归档版本标识，不再启动前扫描全部资产。归档和缓存的增量检查已通过，优化版实际启动、游戏流程及性能仍待复测。

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
