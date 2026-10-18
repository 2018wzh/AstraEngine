# AstraEMU 客户端

完整运行包包含 `astra-emu-manager`（Windows 为 `.exe`）、`cores/`、许可说明和本手册。解压后直接启动 Manager，不需要设置环境变量或手工复制核心到用户数据目录。只移动可执行文件会丢失随包核心。

资料库出现后，点击“刷新核心”手动加载核心，再点击“扫描游戏”刷新已有资料库，或点击“添加游戏”选择原生游戏目录；也可选择包含多个游戏的父目录。扫描完成后选择游戏，点击“启动”。多个模拟器能识别同一目录时，在“模拟器配置”中选择并保存。游戏不需要转换为 AstraVN package。

游戏输入、原生菜单和原生存读档由核心负责。关闭游戏回到资料库；重新启动创建新的会话。资料库、设置和运行日志保存在系统用户数据目录，不写入程序目录；游戏的既有存档仍按核心自身规则处理。联网元数据和翻译需要在设置中单独启用。

核心自行打开真实音频设备；设备不可用或格式不支持时显示原生错误，不自动切换 NullAudio。Manager 不再驱动核心 Tick 或接收核心 PCM。

FVP 当前原生 App 路径仍嵌入上游四个 MS 命名字体文件，分发授权尚未核验。完整说明见 [第三方声明](../../Emulator/THIRD_PARTY_NOTICES.md)，当前开发产物不能直接当作可分发 Release。

点击“刷新核心”后，客户端从程序相邻的 `cores/` 和用户数据目录的 `cores/` 同时发现核心。核心文件损坏、ABI 不符和重复 plugin ID 都会显示诊断；重复核心不会按目录顺序覆盖。本次运行只加载一次核心，替换核心或修复加载失败后需要重启客户端，不做热替换。

新 Manager 只加载 C ABI v2 核心，旧 Family vtable 不兼容。核心自行运行 VM、原生菜单、渲染、音视频与保存；Manager 接收包含文字和游戏 UI 的完整帧。CPU lease 与 RFVP 的 D3D12 扩展共用呈现滤镜链。D3D12 使用独立 Device、共享图像及双向 fence，包含两次 GPU copy，不称零拷贝。

当前 RFVP 已验证原生剧情、选择、菜单/缩略图、存读档和关闭重开，以及 D3D12 导入、滤镜、resize、旧租约和设备丢失。Siglus 已通过新 ABI 的标题、影片、正文、存读档和同 DLL 重开代表流程。KrKr 已删除 child/IPC，公开 TJS/PNG/OpenAL 与重开验证通过。七核心可玩范围及已知阻塞统一见[实施计划](../status/implementation-plan.md)；本轮不要求每核心通关，旧 Hosted/Sandbox 结果不代替本次验收。

Musica 提供同步文字 Hook；共享串行 Luau VM 执行已启用扩展。原生取消不排在 Luau 队列后。可安全重试的错误显示重试和结束，其他失败只允许结束。只安装可信本地扩展；CLI 为 `extension-install <id> <source.luau>`、`extension-enable <id> <true|false>`、`extension-configure <id> <configuration.json>`、`extension-list`。本批不提供在线扩展市场。

验收使用 Release Manager 和 Release 核心，由用户在实际窗口中手动操作。Manager 不再提供脚本式 headless、自动输入、截图或关闭重开入口；扩展管理 CLI 保留。BGI 已由用户确认人工通过，其他核心按用户实际结果记录，不以构建成功代替人工验收。

历史开发证据保留在 Git 和 ignored 输出中。已知问题见[实施计划](../status/implementation-plan.md)；早期 Debug 或锁屏消息级结果不外推为本次 Release 性能、前台手感或听音结论。

## 启动与后台加载

Manager 先显示本地资料库、已缓存的标题和游玩信息，封面暂用占位图。启动、首帧和窗口聚焦都不会自动加载核心或刷新游戏库。“刷新核心”只加载核心并显示就绪或失败状态，不识别游戏，也不改动既有游戏绑定；等待手动操作时不显示为忙碌。

核心就绪后，“扫描游戏”在后台完整扫描已配置目录和已登记游戏，“添加游戏”扫描所选目录；界面不阻塞。识别提示保存在用户数据目录的 `probe-cache.json`，根据程序、核心清单、descriptor 和游戏文件元数据决定是否复用；手动完整扫描只复用已登记目录的有效提示，新目录、变化项和复杂或无法可靠校验的目录重新 probe。每次实际启动仍只对所选模拟器执行一次新的原生校验；不匹配时报错，不自动换用其他模拟器。

封面只为可见范围和选中作品异步解码，缩略图缓存有上限；滚动、筛选或重排后，旧任务不能覆盖新卡片。标题、元数据关联和累计游玩信息通过一次批量快照读取，不逐游戏重复查询。

扩展线程和共享 Luau VM 在首次实际需要文字 Hook 的游戏启动时创建，保留到 Manager 退出。游戏纹理与滤镜编译推迟到首个实际游戏帧；资料库阶段只校验效果描述。首次编译失败显示原始错误，不降级；运行中修改滤镜失败时保留旧有效效果。

现有日志新增 `astra.emu.startup.phase`，记录阶段名和距入口的 `elapsed_ms`，包括首帧及用户手动触发后的核心就绪；手动扫描日志记录游戏数及缓存命中数。用户等待手动操作的时间不能算作自动加载耗时。不记录私有配置或脚本文本；首窗、封面和滤镜表现仍由用户在 Release 中手动观察，不运行自动采样或窗口驱动。

## 游戏引擎与模拟器

详情分别显示引擎与模拟器及版本。例如引擎为 SoftPAL，模拟器为 Sena；同一引擎可以安装多个实现，并为不同游戏分别选择。更换模拟器后先保存，再填写该实现的参数。选择随资料库保存，重新扫描与重启不会清除；所选实现缺失时不会自动换用其他模拟器。参数仍按模拟器与游戏/默认范围独立保存。

“模拟器配置”分为“引擎公共设置”和“模拟器专属设置”。公共分辨率是核心送入滤镜之前的原生渲染尺寸：RFVP 提供等比例倍率，Musica 与 Sena 提供实际渲染宽高；它不修改逻辑舞台或窗口大小。未接入该能力的模拟器明确显示不可调整，仍可使用显示滤镜，不以 Host 重采样冒充原生渲染。两组参数共用 `game`（当前游戏）或 `core`（该模拟器默认）配置范围，以及同一组保存/重置按钮；下次游戏启动应用。

## Musica 私有配置

Musica 运行时只读取 `key.toml`（`astra.emu.musica.keys.v2`），包含已确认的 PAZ 参数、八个归档 role 的密钥及可选纹理替换映射。默认从游戏目录读取；也可在该游戏的“模拟器配置”中点击“选择 key.toml 并应用到当前游戏”，校验后保存绝对路径。Manager 不转换格式、不生成密钥，也不修改所选文件。

JSON profile、GARbro `Formats.dat` 和旧 `keys.v1` 只在[开发工具](../../Emulator/Source/Programs/astra-emu-musica-cli/README.md)中转换为新 `key.toml`，不作为运行时输入。旧 key 缺少的算法参数须明确提供。配置、密钥和既有存档不自动改写；不要公开提交或分享私有文件。

Musica 默认 `launch_mode=title`，启动先显示标题菜单，选择开始后再执行 `entry_script`。`direct` 是显式直入脚本模式，原版 eden 存档导入仍须选择它。已有保存的 `direct` 不自动覆盖；需要改为菜单启动时，在游戏的模拟器专属设置中选择 `title` 并保存。更新核心后须手动重启 Manager，已运行的会话不会自动切换。

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
