# RFVP 原生嵌入差异

## 固定来源

- 上游基线为 `af557db750c7833274f22a6d82b0dc33cdea2f85`；本批从主仓原 pin `dcd39c23ca59b38a9a96b6f301aeb05121aa3e18` 继续，保留完整来源历史。
- 原生生命周期修复为 `7e2016a0e0ef1c0b102185914cc7a8459bb2c9e1`；原生音频 worker 所有权接入为 `3359d8fbea3c53d6d262a9f71b6d4d79888c2368`，与适配分开。
- 原始适配为 `02b66d779f7c4ff6da5f4bc82d935a1088a834e0`；CPU 路径的 ABI 2 硬件验证增量为 [`40fe7c7b11a5c830303c33a975b1f4f7af333837`](https://github.com/2018wzh/rfvp/commit/40fe7c7b11a5c830303c33a975b1f4f7af333837)。曾有的 Windows D3D12 交付增量（自 `3700dc093424d5948c39cd237d34a79ee0d33913` 起）已在 C ABI v3 中整体删除。
- 使用 C ABI v3（`astra-emu-abi` 位于 AstraEngine `emu/abi-v3` 分支；fork manifest 的 git rev 待该提交推送后更新）。描述符声明 `display_name=RFVP`、`family_name=FVP`、`CAP_CPU_SURFACE | CAP_RENDER_EXTENT`。`SessionState.flags=0`；RFVP 无 module-owner 扩展，OpenRequest 的该字段须为 null。包元数据沿用现有 `hosted_fork_*` 字段记录来源，不代表保留 Hosted 运行链。
- 历史单适配提交为 `478448ed9d60778f78d5de911fa5717c48cb201c`，直接父提交为通用基线 `3359d8fbea3c53d6d262a9f71b6d4d79888c2368`；它与当时 Release 源码 `ac13a9ee80b495b02734808b92a7223aba06cc1c` 的 tree 相同。上一公开 `astra` 为 [`ea8b5964fd5b6cde8186eff6a5da1607a3eb6a41`](https://github.com/2018wzh/rfvp/commit/ea8b5964fd5b6cde8186eff6a5da1607a3eb6a41)，包含原生原偏移翻译表及 ASS 消费路径；当前主仓 gitlink 为本地 `astra` 分支的 C ABI v3 提交 `00536b823986a8d9c267b9b1d3e0528b370cc13f`（待推送）；旧 tree 等价记录不代表当前构建源码。生产读档 epoch/错误传播修复保留，旧自动验收驱动已移除。
- Kira 运行层保持 0.10.8，固定 [`d2f10db15d9c451c7b2fd33043ae7732e75bee5d`](https://github.com/2018wzh/kira/commit/d2f10db15d9c451c7b2fd33043ae7732e75bee5d)；设备回归记录增量为 `bb64ffe37f1fa46e7e55c011e102ab28e344fbd3`，不改 mixer 运行实现。独立 workspace 的 CPAL 0.15.3 仍固定 `65ac8a6ac1a635cc26bfc5af331295b052044908`；主仓统一 patch 的 ALSA 0.11 兼容版本 `ecfda841a9140d60bbec1630d47ca91f1a77b8f3` 由主线程验证。
- 原生 `rfvp` 的 `astra-subtitle` 依赖与独立 `Cargo.lock` 均固定 `2018wzh/AstraEngine@c27ac6e107328170deecac428bba7472d3ce2d6c`；这是共享 ASS 实现来源，不替换上述独立嵌入 manifest 的 ABI pin 或 Kira pin。

## 所有权与完整帧

`astra-emu-fvp` 只保留构建依赖和 `astra_emu_query` 注册入口。函数表、配置、探测、输入与 lease 包装位于 fork 的 `crates/astra-embedded`；App、调度、VM、资源、renderer、游戏 UI、音视频与保存继续由 RFVP 原生代码持有。旧 ProviderModule、公共 advance、HostedSession、Host PCM、替代文件系统及相应旧 ABI 测试已移除。

`App::render_frame` 仍先绘制原场景 RenderTarget，再将场景、`legacy_save_load_ui` 和 `exit_confirm_ui` 合成到独立最终离屏目标。CPU 回读只读取这个完整目标；SaveCreate 缩略图和遮罩转场仍在原时点读取原场景目标，不能借用 Host 输出 lease。

窗口外部尺寸 O 不改变 L/R，也不推进 VM。渲染尺寸由 `OpenRequest.render_extent` 指定，描述符声明范围 160×90 – 8192×8192、默认 0×0 并保持宽高比：0×0 使用 1:1 缩放；否则 `scale = min(w/Lw, h/Lh)` 截断到 0.25–4，`R = round(L × scale)`，任一轴超过 8192 返回 `RFVP_RENDER_SIZE_UNSUPPORTED`，并校验设备尺寸上限；`SurfaceInfo.raster` 报告实际 R，未声明在线重配置。原 `render_scale`、`surface` 配置项已删除。输入携带当前会话/输出代次和帧像素坐标，再复用原生窗口到逻辑/UI 映射。保留 `astra.emu.fvp`、文件 stem 游戏身份和 `script_encoding=shift_jis/gbk/utf8`（选项名 Shift-JIS（日文原版）/GBK（简体中文）/UTF-8），不丢已有配置关联。配置标签、分组、说明均为中文。

原生 typed 配置增加默认空的 `translation_table` 与 `subtitle_map`，声明为 `CONFIG_FLAG_FILE_PATH | CONFIG_FLAG_GAME_RELATIVE`，均读取游戏根内的显式相对资源。`AFVPTR1` 翻译表按原 HCB 字符串偏移保存 UTF-8，核对最终 HCB 哈希与真实指令边界；未命中保留原编码，不改写 VM 地址或原脚本。字幕映射按 movie/BGM 资源关联 UTF-8 ASS 和显式字体，由原生媒体时钟驱动并绘入原场景目标；暂停、换曲、结束、seek/读档与输出尺寸变化沿原生命周期更新。该能力不补充游戏字体授权，也不构成实际游戏人工验收。

原生 owner 使用独立 8 MiB 栈线程；平台线程只入队输入/控制及取得完成帧。三槽不可变 CPU 池满时跳过显示发布，VM、音频和存档继续执行。Ready/Pending/Unchanged 不以像素 hash 判断；已交付 lease 可跨关闭持有，归还只销毁纯 CPU owner。

## 暂停、平台与关闭

暂停停止原生推进并暂停原 Kira mixer，恢复移动电影时钟原点，不补跑等待时长。WindowMode、F11 和指针位置/显隐经 typed 平台服务执行并反馈结果；等待期间保留最后一帧，取消独立唤醒平台服务。无窗口 Host 必须明确拒绝窗口请求。本核心不新增文字 Hook。

原 VM worker 增加指令边界取消和显式 join；原生读档成功递增 context epoch，取消旧平台请求和未完成回读。新 App 清空上一会话的 legacy 配置、文字历史、角色表和 UI 请求。关闭停止回读、VM、电影、原设备 watcher 和全部 streaming decoder，再调用原 RFVG 保存；即使关闭出错也完成 worker 回收。库不卸载，下一次启动重建会话资源。

Kira 的 DecoderOwner 独立于播放 handle；handle Drop 语义不变，Sound Drop 只发取消，不在设备 callback join。CPAL watcher 的启动握手、失败路径、唤醒与 join 在同版本 fork 修复；不替换 mixer/decoder/device。非商业复现位于 Kira 的 `tests/native_owners.rs`。

## 读档修复与验收策略

保留 `11ecaa6a51aa545750bbbdec468626aa19e70553` 的两项生产修复：读档后向 Host invalidation 发布新 context epoch；在 shutdown 设置取消标志之前记录原取消状态，避免清理过程吞掉真实请求失败。未改原生 VM、renderer、音视频或存档格式。

按用户最新要求，验收由用户使用 Release 产物手动完成。已移除适配新增的 `native_cpu.rs`、`native_abi.rs`、`tests/common/mod.rs` 硬件驱动、动态 DLL 验收及 HCB fixture，并移除它们专用的 `tempfile` / `libloading` dev-dependencies。generic `3359d8f` 不包含该适配测试目录；原生上游普通测试不变，`config.rs` 和 `input.rs` 的纯逻辑单元测试保留。没有新增自动输入、截图、定时关闭、Headless 或性能采样入口。

此前 Debug 运行记录仅保留为历史证据，不视为当前 Release 的人工验收结论。商业副本、生成存档及私有测试数据库已在前一轮清理；原始资源和旧存档未动。本轮不启动游戏，也不操作用户窗口。

## Release 构建与交付

在独占树根目录构建原生注册 DLL，不初始化其他核心：

```sh
cargo build --manifest-path Emulator/ThirdParty/rfvp/Cargo.toml -p rfvp-astra-embedded --locked --release --features native-dylib-export --lib --target-dir .work/target-rfvp-release -j 4
```

历史构建记录：旧源码曾输出 `release [optimized]` 的 `.work/target-rfvp-release/release/rfvp_astra_embedded.dll`，并复制为 `.work/release-fvp/astra_emu_fvp.dll`，导出 `astra_emu_query`；当时仅做构建和静态导入/导出检查。当前原生核心从 `00536b823986a8d9c267b9b1d3e0528b370cc13f` 构建，旧路径与旧 tree 记录不作为当前发布包或人工验收凭据。

运行需要兼容 C ABI v3 的 Release Manager、x64 Windows 系统组件、VC++ 运行库及可用 GPU/音频驱动。静态导入包括 `VCRUNTIME140.dll`、UCRT、`mfplat.dll`（Media Foundation）和 `d3dcompiler_47.dll`；原生媒体实现没有额外 FFmpeg DLL 依赖。Vulkan 需驱动提供 loader；帧只经 CPU lease 交付。

资源目录仍须与旧存档隔离，原生保存写入游戏目录下的 `save/`。字体与媒体来源见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
