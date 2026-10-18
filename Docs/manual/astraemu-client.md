# AstraEMU 客户端

完整运行包包含 `astra-emu-manager`（Windows 为 `.exe`）、`cores/`、许可说明和本手册。解压后直接启动 Manager，不需要设置环境变量或手工复制核心到用户数据目录。只移动可执行文件会丢失随包核心。

首次启动点击“添加游戏”，选择原生游戏目录；也可选择包含多个游戏的父目录。扫描完成后选择游戏，点击“启动”。多个核心都能识别同一目录时，先在游戏详情中选择核心。游戏不需要转换为 AstraVN package。

游戏输入、原生菜单和原生存读档由核心负责。关闭游戏回到资料库；重新启动创建新的会话。资料库、设置和运行日志保存在系统用户数据目录，不写入程序目录；游戏的既有存档仍按核心自身规则处理。联网元数据和翻译需要在设置中单独启用。

核心自行打开真实音频设备；设备不可用或格式不支持时显示原生错误，不自动切换 NullAudio。Manager 不再驱动核心 Tick 或接收核心 PCM。

FVP 当前原生 App 路径仍嵌入上游四个 MS 命名字体文件，分发授权尚未核验。完整说明见 [第三方声明](../../Emulator/THIRD_PARTY_NOTICES.md)，当前开发产物不能直接当作可分发 Release。

客户端从程序相邻的 `cores/` 和用户数据目录的 `cores/` 同时发现核心。核心文件损坏、ABI 不符和重复 plugin ID 都会显示诊断；重复核心不会按目录顺序覆盖。替换核心后需要重启客户端。

新 Manager 只加载 C ABI v2 核心，旧 Family vtable 不兼容。核心自行运行 VM、原生菜单、渲染、音视频与保存；Manager 接收包含文字和游戏 UI 的完整帧。CPU lease 与 RFVP 的 D3D12 扩展共用呈现滤镜链。D3D12 使用独立 Device、共享图像及双向 fence，包含两次 GPU copy，不称零拷贝。

当前 RFVP 已验证原生剧情、选择、菜单/缩略图、存读档和关闭重开，以及 D3D12 导入、滤镜、resize、旧租约和设备丢失。Siglus 已通过新 ABI 的标题、影片、正文、存读档和同 DLL 重开代表流程。KrKr 已删除 child/IPC，公开 TJS/PNG/OpenAL 与重开验证通过。七核心可玩范围及已知阻塞统一见[实施计划](../status/implementation-plan.md)；本轮不要求每核心通关，旧 Hosted/Sandbox 结果不代替本次验收。

Musica 提供同步文字 Hook；共享串行 Luau VM 执行已启用扩展。原生取消不排在 Luau 队列后。可安全重试的错误显示重试和结束，其他失败只允许结束。只安装可信本地扩展；CLI 为 `extension-install <id> <source.luau>`、`extension-enable <id> <true|false>`、`extension-configure <id> <configuration.json>`、`extension-list`。本批不提供在线扩展市场。

验收使用 Release Manager 和 Release 核心，由用户在实际窗口中手动操作。Manager 不再提供脚本式 headless、自动输入、截图或关闭重开入口；扩展管理 CLI 保留。BGI 已由用户确认人工通过，其他核心按用户实际结果记录，不以构建成功代替人工验收。

历史开发证据保留在 Git 和 ignored 输出中。已知问题见[实施计划](../status/implementation-plan.md)；早期 Debug 或锁屏消息级结果不外推为本次 Release 性能、前台手感或听音结论。

## 构建运行包

下面以 FVP 为例。先初始化主仓固定的完整 submodule；不要换回旧 revision 绕过失败。参与核心保留不同原生 Lua/音频依赖，分别构建，不将 Artemis Lua 5.1 和 Manager Luau 合并进同一次 Cargo 调用。

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

Musica 动态核心须同时启用 `dynamic-plugin-export,ffmpeg-vcpkg`，并配置 FFmpeg vcpkg 环境。构建完成后立即把该 DLL 及匹配的 FFmpeg 依赖复制到独立运行目录，再启动 Manager；之后对同一 target 的 CLI/静态依赖构建可能覆盖未启用导出的同名 DLL。`astra_emu_query` 必须能协商 ABI v2，不能把普通 rlib 依赖构建产生的 DLL 当作产品核心。

KrKr 原生运行目录用 `Tools/AstraEMU/package_krkr_runtime.py --native-runtime <目录> --output <新目录>` 组装，包含实际 DLL、Resources 和运行依赖；不再接收旧 Rust child 产物。该工具不代替商业流程和许可证核验。
