# RFVP 原生嵌入差异

## 固定来源

- 上游基线为 `af557db750c7833274f22a6d82b0dc33cdea2f85`；本批从主仓原 pin `dcd39c23ca59b38a9a96b6f301aeb05121aa3e18` 继续，保留完整来源历史。
- 原生生命周期修复为 `7e2016a0e0ef1c0b102185914cc7a8459bb2c9e1`；原生音频 worker 所有权接入为 `3359d8fbea3c53d6d262a9f71b6d4d79888c2368`，与适配分开。
- 原始适配为 `02b66d779f7c4ff6da5f4bc82d935a1088a834e0`；CPU 路径的 ABI 2 硬件验证增量为 [`40fe7c7b11a5c830303c33a975b1f4f7af333837`](https://github.com/2018wzh/rfvp/commit/40fe7c7b11a5c830303c33a975b1f4f7af333837)。Windows GPU 增量从兼容 flate2 提交 `3700dc093424d5948c39cd237d34a79ee0d33913` 继续，保留已发布历史。
- C ABI 2 固定共享实现 `3c7762ad533c8641af51b8aff81794dc377e23a9`，D3D12 extension 单独使用 v1。`SessionState.flags=0`；RFVP 无 module-owner 扩展，OpenRequest 的该字段须为 null。包元数据沿用现有 `hosted_fork_*` 字段记录来源，不代表保留 Hosted 运行链。
- 当前 GPU 适配为 `1f78df1a28467c4fa0bddaf6d0d322c14b2aa11e`，分支 `codex/emu-dx12-surface-20261018`；主仓以 submodule gitlink 固定该源代码。
- Kira 运行层保持 0.10.8，固定 [`d2f10db15d9c451c7b2fd33043ae7732e75bee5d`](https://github.com/2018wzh/kira/commit/d2f10db15d9c451c7b2fd33043ae7732e75bee5d)；设备回归记录增量为 `bb64ffe37f1fa46e7e55c011e102ab28e344fbd3`，不改 mixer 运行实现。独立 workspace 的 CPAL 0.15.3 仍固定 `65ac8a6ac1a635cc26bfc5af331295b052044908`；主仓统一 patch 的 ALSA 0.11 兼容版本 `ecfda841a9140d60bbec1630d47ca91f1a77b8f3` 由主线程验证。

## 所有权与完整帧

`astra-emu-fvp` 只保留构建依赖和 `astra_emu_query` 注册入口。函数表、配置、探测、输入与 lease 包装位于 fork 的 `crates/astra-embedded`；App、调度、VM、资源、renderer、游戏 UI、音视频与保存继续由 RFVP 原生代码持有。旧 ProviderModule、公共 advance、HostedSession、Host PCM、替代文件系统及相应旧 ABI 测试已移除。

`App::render_frame` 仍先绘制原场景 RenderTarget，再将场景、`legacy_save_load_ui` 和 `exit_confirm_ui` 合成到独立最终离屏目标。CPU 回读只读取这个完整目标；SaveCreate 缩略图和遮罩转场仍在原时点读取原场景目标，不能借用 Host 输出 lease。

窗口外部尺寸 O 不改变 L/R，也不推进 VM。`render_scale` 为启动配置，范围 0.25–4，输出报告实际取整后的 R，并校验设备尺寸上限；未声明在线重配置。输入携带当前会话/输出代次和帧像素坐标，再复用原生窗口到逻辑/UI 映射。保留 `astra.emu.fvp`、文件 stem 游戏身份和 `script_encoding=shift_jis/gbk/utf8`，不丢已有配置关联。

原生 owner 使用独立 8 MiB 栈线程；平台线程只入队输入/控制及取得完成帧。三槽不可变 CPU 池满时跳过显示发布，VM、音频和存档继续执行。Ready/Pending/Unchanged 不以像素 hash 判断；已交付 lease 可跨关闭持有，归还只销毁纯 CPU owner。

### Windows D3D12 交付

`surface=cpu` 保留原默认路径；`surface=cpu_d3d12` 使用原 CPU 交付和 D3D12 原生设备，供同 backend 对照；`surface=d3d12` 使用 `astra.emu.surface.d3d12` v1。Manager 从已有 Slint wgpu 29 设备取得 LUID、格式和限制，通过扩展 open 传入。RFVP 在同一个原生 owner 中选择对应 LUID 的 DX12 adapter，创建独立 wgpu 0.19 device；缺少兼容 Host、格式或设备时明确失败，不回退 CPU。

GPU 路径有两次 GPU copy：包含两个原生 UI 的最终目标 → 三槽共享 export；Host import → 私有滤镜输入。正常交付没有 CPU 回读或上传。共享资源与原场景、缩略图、转场历史分离，Host 仍复用 PixelPresentation、FilterEngine 和 Slint StageTextureUpdate。Headless 截图只在 Host 完成这些步骤后回读，并单独记录 `host_readback_bytes`。

每个 generation 只 acquire 一次。ready/done 为两个共享 D3D12 fence；资源在设备之间以 COMMON 状态交接。Host 在自己的队列等待 ready、复制、恢复 COMMON，再 signal done。原生端同时确认 lease 已归还和消费 fence 已完成才复用槽，不向原生队列提交外部无限 Wait。池满只丢显示发布；关闭只排空原生写入，不等待 Host 持有的旧 lease。Host 的已提交 imports/fences 保留到真实 done 完成或设备丢失。

Host 检查导入资源的实际 DXGI 格式、尺寸、mip/sample、heap、共享 fence 和所属设备。wgpu 29 的 `create_texture_from_hal` 使用 `TextureClearMode::None`；不能用普通未初始化 texture 包装导入资源，否则延迟清零可能覆盖原生像素。

Host graphics owner 直接持有有界 retirement queue，不启动后台退役线程。正常关闭等待实际 done；五秒超时会返回 GPU 清理错误并禁止同进程重开，同时继续取消和回收原生 VM/服务。未完成的最多三个 imports/lease 不会被提前销毁，最终退出只能留到进程回收；实际 device-lost 则按 API 返回退役。超时不记作成功关闭。


## 暂停、平台与关闭

暂停停止原生推进并暂停原 Kira mixer，恢复移动电影时钟原点，不补跑等待时长。WindowMode、F11 和指针位置/显隐经 typed 平台服务执行并反馈结果；等待期间保留最后一帧，取消独立唤醒平台服务。无窗口 Host 必须明确拒绝窗口请求。本核心不新增文字 Hook。

原 VM worker 增加指令边界取消和显式 join；原生读档成功递增 context epoch，取消旧平台请求和未完成回读。新 App 清空上一会话的 legacy 配置、文字历史、角色表和 UI 请求。关闭停止回读、VM、电影、原设备 watcher 和全部 streaming decoder，再调用原 RFVG 保存；即使关闭出错也完成 worker 回收。库不卸载，下一次启动重建会话资源。

Kira 的 DecoderOwner 独立于播放 handle；handle Drop 语义不变，Sound Drop 只发取消，不在设备 callback join。CPAL watcher 的启动握手、失败路径、唤醒与 join 在同版本 fork 修复；不替换 mixer/decoder/device。非商业复现位于 Kira 的 `tests/native_owners.rs`。

## 既有 CPU 基线验证

在独占树执行最终检查，未使用主线程商业副本。原生 Cargo.lock 已记录 ABI 2、Kira 与 CPAL 的精确 Git 来源。

- `--locked` adapter 测试：4 项普通测试、3 项显式硬件 `native_cpu` 测试通过。包含不让出的 VM 取消、两个原生 modal UI、三槽背压、暂停恢复、跨关闭 lease、同进程重开。
- Native RFVS 保存/加载通过：槽 0 保留 `global[0]=99`，读取后 context epoch 增加；完整 UI 为 960×720，原 96×72 缩略图仍是进入菜单前的黑色场景，不包含菜单。
- Native adapter DLL 构建成功；实际加载 DLL 并查询符号执行 ABI 2 用例。DLL 驻留，epoch 41–43 连续打开/关闭，旧 CPU lease 在新会话启动后归还；关闭前后像素保持不变。输入 WindowState 报告 O=1920×1080、scale factor 2 后，R(864,664.5) 仍命中 L(576,443) 的菜单按钮；这是 headless 几何回归，不是 OS 窗口 resize 验收。
- 无窗口 Host 对 F11 的 WINDOW_FULLSCREEN 返回实际 UNSUPPORTED，核心进入失败态；错误关闭完成回收，下一次打开成功，没有伪造窗口成功。
- 实际 GPU：NVIDIA GeForce RTX 4070 Laptop GPU，Vulkan，驱动 610.88。L=640×480，验证 R=640×480/960×720。
- Kira `native_owners` 4 项测试通过，包括真实 CPAL 输出的三轮关闭重开。选定 Logitech G431、F32、48 kHz、8 声道；暂停时播放位置不变，恢复后继续，decoder 已销毁且 watcher 已 join。该夹具静音，不代表可听音频验收。

检查命令（RFVP 项目下选择 `rfvp-astra-embedded`）：

```sh
cargo test -p rfvp-astra-embedded --locked --features dynamic-plugin-export --lib --test native_cpu -- --include-ignored --test-threads=1 --nocapture
cargo build -p rfvp-astra-embedded --features native-dylib-export --lib
cargo test -p rfvp-astra-embedded --features native-dylib-export --test native_abi -- --ignored --test-threads=1 --nocapture
```

Kira fork 使用 `cargo test -p kira --test native_owners -- --include-ignored --test-threads=1 --nocapture`。运行保留 `VK_LAYER_KHRONOS_validation` 不可用告警及 RFVP 编译 warnings，未关闭检查或改写原 renderer。未运行 fmt、Clippy、商业游戏、电影播放或可听音频/结局验收；这些仍由主线程整合后的产品流程覆盖。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

## Windows D3D12 硬件验收

2026-10-18，NVIDIA GeForce RTX 4070 Laptop GPU，驱动 610.88。RFVP wgpu 0.19 与 Manager wgpu 29 分别创建 D3D12 device，并检查 LUID、真实共享资源描述和双向 fence。

- 真实 RFVP DLL 的 LoadFile、ExitDialog 各连续重开三轮。L=640×480、R=320×240；GPU 导入与同 generation 在 Host 边界回读后的 CPU 归一化逐字节一致，另与 `cpu_d3d12` 原生 CPU 基线比较。每次 GPU copy 为 307,200 字节，正常交付 CPU 字节为零。
- 同一生产 PixelPresentation/FilterEngine 验证 2× 缩放后 O=640×480、切回 O=320×240；产出的真实 GPU texture 成功导入 `slint::Image`。Host resize 不改变 R，Escape 实际关闭原生 UI。三槽全被持有时不再发布第四个资源；未导入的旧 lease 在 native close 后仍能打开共享句柄并还原原 UI。
- 用独立 Host gate fence 阻止消费队列：done 保持未完成时 native cancel/close 两轮用时 34/65 ms；放行后 done=1，旧帧继续可读。没有向 native queue 放入 Host 外部 Wait。
- 对本次 Host device 调用真实 `ID3D12Device5::RemoveDevice`：Host 返回 GPU 清理失败并拒绝重开；原生 wgpu 分别在 UI 资源创建或队列提交处报告 validation panic / `Lost`，owner 捕获后返回 `RFVP_CLOSE_FAILED / RFVP_NATIVE_WORKER_PANICKED`。保留这些诊断，不将失败关闭写成成功回收。
- 生产 Manager `--headless`，100 ms 观察间隔、60 次观察、同进程三轮重开：L=640×480，R=O=960×720。180 次观察、141 次 Host copy（389,836,800 字节）、493 次 native export copy（1,363,046,400 字节），每次 copy 为 2,764,800 字节。15 个 Host 检查点共回读 41,472,000 字节；这部分是截图验收开销，不是交付流量。三轮最终画面均为 Escape 关闭菜单后的黑场。
- 正常生产运行无 ERROR；保留 24 条原生 downlevel capability / `CREATE_SAMPLER_COMPARISON_FUNC_IGNORED` 告警。观察次数、截图时间和 copy 数不作为 FPS 成绩。
- 最终 RFVP 独立 workspace `--locked` 验证 7 项通过，包含原 CPU modal、保存缩略图、取消和重开；Manager 普通回归 78 项通过，三个 D3D12 硬件用例另以独立进程通过。Manager/manager-core 的 `Clippy --all-targets -- -D warnings` 通过。

可复用的硬件测试在 Manager 的 `headless_presenter_tests.rs`，自行生成非商业 HCB。实际命令如下，工作目录为独占树根目录：

```sh
export ASTRA_EMU_RFVP_D3D12_PLUGIN=D:/Workspace/AstraEngine-emu-dx12-20261018/.work/target-d3d12/debug/astra_emu_fvp.dll
export ASTRA_EMU_DXC_PATH=D:/Workspace/AstraEngine-emu-dx12-20261018/.work/dxc/dxcompiler.dll
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-fvp --features dynamic-plugin-export --target-dir .work/target-d3d12 -j 4
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-manager --bin astra-emu-manager rfvp_d3d12_production_presenter_pixels_leases_cancel_reopen --target-dir .work/target-d3d12 -- --ignored --test-threads=1 --nocapture
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-manager --bin astra-emu-manager rfvp_d3d12_bidirectional_fence_does_not_block_native_close --target-dir .work/target-d3d12 -- --ignored --test-threads=1 --nocapture
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-manager --bin astra-emu-manager rfvp_d3d12_device_loss_reports_failed_cleanup_and_blocks_reopen --target-dir .work/target-d3d12 -- --ignored --test-threads=1 --nocapture
.work/target-d3d12/debug/astra-emu-manager.exe --headless .work/acceptance/gpu.json
```

设备丢失测试必须单独进程运行。生产截图与诊断留在 ignored `.work/acceptance`。桌面处于锁屏状态，未验收可交互 Slint 窗口、实际桌面 Present、鼠标手感、可听音频或商业结局。
