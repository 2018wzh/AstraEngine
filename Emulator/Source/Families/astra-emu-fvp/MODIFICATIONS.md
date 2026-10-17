# RFVP 原生嵌入差异

## 固定来源

- 上游基线为 `af557db750c7833274f22a6d82b0dc33cdea2f85`；本批从主仓原 pin `dcd39c23ca59b38a9a96b6f301aeb05121aa3e18` 继续，保留完整来源历史。
- 原生生命周期修复为 `7e2016a0e0ef1c0b102185914cc7a8459bb2c9e1`；原生音频 worker 所有权接入为 `3359d8fbea3c53d6d262a9f71b6d4d79888c2368`，与适配分开。
- 新适配提交及主仓 pin 为 [`02b66d779f7c4ff6da5f4bc82d935a1088a834e0`](https://github.com/2018wzh/rfvp/commit/02b66d779f7c4ff6da5f4bc82d935a1088a834e0)，发布在 `codex/emu-rfvp-embedded-20261018`。历史 `astra` 分支未改写；主线程整合时仍须按契约整理 fork/master 与 astra 基线。
- C ABI 使用主仓共享实现 `94cd66ee9051d3643f51f4a61fed0466bf320280`。包元数据沿用现有 `hosted_fork_*` 字段记录来源，不代表保留 Hosted 运行链。
- Kira 保持 0.10.8，固定 [`d2f10db15d9c451c7b2fd33043ae7732e75bee5d`](https://github.com/2018wzh/kira/commit/d2f10db15d9c451c7b2fd33043ae7732e75bee5d)。CPAL 保持 0.15.3，workspace 固定 WASAPI 关闭修复 `65ac8a6ac1a635cc26bfc5af331295b052044908`；主仓 workspace 须应用同一 patch。

## 所有权与完整帧

`astra-emu-fvp` 只保留构建依赖和 `astra_emu_query` 注册入口。函数表、配置、探测、输入与 lease 包装位于 fork 的 `crates/astra-embedded`；App、调度、VM、资源、renderer、游戏 UI、音视频与保存继续由 RFVP 原生代码持有。旧 ProviderModule、公共 advance、HostedSession、Host PCM、替代文件系统及相应旧 ABI 测试已移除。

`App::render_frame` 仍先绘制原场景 RenderTarget，再将场景、`legacy_save_load_ui` 和 `exit_confirm_ui` 合成到独立最终离屏目标。CPU 回读只读取这个完整目标；SaveCreate 缩略图和遮罩转场仍在原时点读取原场景目标，不能借用 Host 输出 lease。

窗口外部尺寸 O 不改变 L/R，也不推进 VM。`render_scale` 为启动配置，范围 0.25–4，输出报告实际取整后的 R，并校验设备尺寸上限；未声明在线重配置。输入携带当前会话/输出代次和帧像素坐标，再复用原生窗口到逻辑/UI 映射。保留 `astra.emu.fvp`、文件 stem 游戏身份和 `script_encoding=shift_jis/gbk/utf8`，不丢已有配置关联。

原生 owner 使用独立 8 MiB 栈线程；平台线程只入队输入/控制及取得完成帧。三槽不可变 CPU 池满时跳过显示发布，VM、音频和存档继续执行。Ready/Pending/Unchanged 不以像素 hash 判断；已交付 lease 可跨关闭持有，归还只销毁纯 CPU owner。

## 暂停、平台与关闭

暂停停止原生推进并暂停原 Kira mixer，恢复移动电影时钟原点，不补跑等待时长。WindowMode、F11 和指针位置/显隐经 typed 平台服务执行并反馈结果；等待期间保留最后一帧，取消独立唤醒平台服务。无窗口 Host 必须明确拒绝窗口请求。本核心不新增文字 Hook。

原 VM worker 增加指令边界取消和显式 join；原生读档成功递增 context epoch，取消旧平台请求和未完成回读。新 App 清空上一会话的 legacy 配置、文字历史、角色表和 UI 请求。关闭停止回读、VM、电影、原设备 watcher 和全部 streaming decoder，再调用原 RFVG 保存；即使关闭出错也完成 worker 回收。库不卸载，下一次启动重建会话资源。

Kira 的 DecoderOwner 独立于播放 handle；handle Drop 语义不变，Sound Drop 只发取消，不在设备 callback join。CPAL watcher 的启动握手、失败路径、唤醒与 join 在同版本 fork 修复；不替换 mixer/decoder/device。非商业复现位于 Kira 的 `tests/native_owners.rs`。

## 验证边界

本批 worker 按分工未运行 build、fmt、Clippy、测试或真实游戏。新增普通回归覆盖配置身份、非法配置和输入代次；`crates/astra-embedded/tests/native_cpu.rs` 的显式硬件用例覆盖原生菜单完整帧、R=1.5、暂停/窗口状态分离、三槽背压、跨关闭 lease、同进程重开，以及不让出的 VM 取消。Kira 的显式设备用例检查整机 mixer 暂停和 device/decoder join；静音夹具不代表可听音频验收。

主线程整合后更新 workspace/native lockfile，运行上述用例及真实硬件产品流程，覆盖原存档、电影、可听音频、自由 resize/指针映射和结局。历史 Hosted 测试及旧商业截图不能作为本次原生路径验收。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
