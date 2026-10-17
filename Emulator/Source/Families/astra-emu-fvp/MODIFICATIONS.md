# RFVP 原生嵌入差异

## 固定来源

- 上游基线为 `af557db750c7833274f22a6d82b0dc33cdea2f85`；本批从主仓原 pin `dcd39c23ca59b38a9a96b6f301aeb05121aa3e18` 继续，保留完整来源历史。
- 原生生命周期修复为 `7e2016a0e0ef1c0b102185914cc7a8459bb2c9e1`；原生音频 worker 所有权接入为 `3359d8fbea3c53d6d262a9f71b6d4d79888c2368`，与适配分开。
- 原始适配为 `02b66d779f7c4ff6da5f4bc82d935a1088a834e0`；ABI 2 与最终硬件验证增量、当前主仓 pin 为 [`40fe7c7b11a5c830303c33a975b1f4f7af333837`](https://github.com/2018wzh/rfvp/commit/40fe7c7b11a5c830303c33a975b1f4f7af333837)。均在 `codex/emu-rfvp-embedded-20261018` 追加发布，未改写历史。
- C ABI 2 使用共享实现 `15a39a626b1316cb6137a75e17e14a2ff2d1ba88`，`SessionState.flags=0`；RFVP 无 module-owner 扩展，OpenRequest 的该字段须为 null。包元数据沿用现有 `hosted_fork_*` 字段记录来源，不代表保留 Hosted 运行链。
- Kira 运行层保持 0.10.8，固定 [`d2f10db15d9c451c7b2fd33043ae7732e75bee5d`](https://github.com/2018wzh/kira/commit/d2f10db15d9c451c7b2fd33043ae7732e75bee5d)；设备回归记录增量为 `bb64ffe37f1fa46e7e55c011e102ab28e344fbd3`，不改 mixer 运行实现。独立 workspace 的 CPAL 0.15.3 仍固定 `65ac8a6ac1a635cc26bfc5af331295b052044908`；主仓统一 patch 的 ALSA 0.11 兼容版本 `ecfda841a9140d60bbec1630d47ca91f1a77b8f3` 由主线程验证。

## 所有权与完整帧

`astra-emu-fvp` 只保留构建依赖和 `astra_emu_query` 注册入口。函数表、配置、探测、输入与 lease 包装位于 fork 的 `crates/astra-embedded`；App、调度、VM、资源、renderer、游戏 UI、音视频与保存继续由 RFVP 原生代码持有。旧 ProviderModule、公共 advance、HostedSession、Host PCM、替代文件系统及相应旧 ABI 测试已移除。

`App::render_frame` 仍先绘制原场景 RenderTarget，再将场景、`legacy_save_load_ui` 和 `exit_confirm_ui` 合成到独立最终离屏目标。CPU 回读只读取这个完整目标；SaveCreate 缩略图和遮罩转场仍在原时点读取原场景目标，不能借用 Host 输出 lease。

窗口外部尺寸 O 不改变 L/R，也不推进 VM。`render_scale` 为启动配置，范围 0.25–4，输出报告实际取整后的 R，并校验设备尺寸上限；未声明在线重配置。输入携带当前会话/输出代次和帧像素坐标，再复用原生窗口到逻辑/UI 映射。保留 `astra.emu.fvp`、文件 stem 游戏身份和 `script_encoding=shift_jis/gbk/utf8`，不丢已有配置关联。

原生 owner 使用独立 8 MiB 栈线程；平台线程只入队输入/控制及取得完成帧。三槽不可变 CPU 池满时跳过显示发布，VM、音频和存档继续执行。Ready/Pending/Unchanged 不以像素 hash 判断；已交付 lease 可跨关闭持有，归还只销毁纯 CPU owner。

## 暂停、平台与关闭

暂停停止原生推进并暂停原 Kira mixer，恢复移动电影时钟原点，不补跑等待时长。WindowMode、F11 和指针位置/显隐经 typed 平台服务执行并反馈结果；等待期间保留最后一帧，取消独立唤醒平台服务。无窗口 Host 必须明确拒绝窗口请求。本核心不新增文字 Hook。

原 VM worker 增加指令边界取消和显式 join；原生读档成功递增 context epoch，取消旧平台请求和未完成回读。新 App 清空上一会话的 legacy 配置、文字历史、角色表和 UI 请求。关闭停止回读、VM、电影、原设备 watcher 和全部 streaming decoder，再调用原 RFVG 保存；即使关闭出错也完成 worker 回收。库不卸载，下一次启动重建会话资源。

Kira 的 DecoderOwner 独立于播放 handle；handle Drop 语义不变，Sound Drop 只发取消，不在设备 callback join。CPAL watcher 的启动握手、失败路径、唤醒与 join 在同版本 fork 修复；不替换 mixer/decoder/device。非商业复现位于 Kira 的 `tests/native_owners.rs`。

## 最终验证与边界

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
