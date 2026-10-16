# Siglus 最小原生适配

## 固定来源与提交结构

- 上游：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)，2026-10-16 核对 main HEAD 为 `5f358ef1360b0439e92aed2b749e8f25d8721954`，版本 1.0.0。
- 完整 fork：[2018wzh/siglus_rs](https://github.com/2018wzh/siglus_rs)，本地分支 `codex/astra-upstream-head-20261016`。
- 唯一适配提交：`8bf5024fb3975bb8dc375874380f4574ecedf4d8`；唯一父提交为上述上游 HEAD，范围内提交数实测为 1。尚未推送，不能声称远端干净克隆已经可取得它。
- 主仓通过 `Emulator/ThirdParty/siglus_rs` submodule 固定精确提交，不复制源码。旧缺失对象、恢复起点和旧基线验证保存在[审计文档](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)，原来源和发布历史未改写。

## 必要核心差异

- 使用显式 `astra-hosted` feature；Family 关闭 default features。原桌面默认仍使用 Kira/CPAL，未选择 hosted 时保留原 VM、过程流、输入、保存、解码和原生窗口渲染。
- 为原 wgpu renderer 增加无窗口硬件 GPU 目标。仅把 surface 改为 optional、共用初始化和同一 `render_frame_to_view`，不另写 sprite、wipe、shader、Emote 或几何路径。离屏仅接受独显/集显/虚拟硬件 adapter，拒绝 CPU/未知类型。
- 最终纹理与 GPU readback buffer 按尺寸持有，映射完成队列初始化一次后复用；实际拉帧才读回，不逐帧新建 readback buffer。RGBA 非 sRGB render target 保持原 D3D9 byte-space 合成语义；不增加全像素 alpha 认证。
- 复用原 Kira mixer，由每 mixer 的有界 tap worker 交付 stereo F32。取消先打断阻塞写入再 join，替换 mixer 不取消整个 session。新增失败诊断经原 log 通道，不新建文件 sink；新增共享注册表使用 parking_lot。
- Kira backend feature 调整只放在 `siglus_scene_vm` 消费者，不改变整个上游 workspace 的 Kira 默认配置。原解码 features 和桌面 CPAL 保留。
- 原生成功 load-completed、返回菜单与场景 restart 仅增加一次性 timeline-reset 通知；不复制旧分支过程流修复或修改新上游 VM。
- 保留上游原生实时时钟。删除旧适配的 hosted_clock/deterministic_frame_clock 及其消费者，不在真实游戏启用上游注明仅供 replay 的 virtual-clock。
- 原生离屏提交成功后才增加 hosted frame generation；VM tick、读取与 resize 本身不冒充新绘制。Family 据此跳过没有新提交时的 GPU 回读和 CPU 发布，generation 保存在 renderer 中以跨目标 resize 连续，避免重建目标造成旧 frame identity 别名。未新增像素 hash、场景比较或另一绘制路径。
- 原 WMV 解码 crate 版本标为 0.2.1，只解决同一 Host lockfile 中独立 path 包的同名同版本冲突；不更换 codec、算法或媒体行为。

核心差异共 11 文件、+557/−67，包含清单、lockfile、窗口消费者迁移及新增离屏模块；不把全部称为性能改动。Parent Family 负责 ABI v13、事件翻译、PCM Host 队列、FrameLease/代次与生命周期，不向核心引入 Astra 类型或 SDK 依赖。

## 新 HEAD 实际验证与边界

- 上游原桌面 default-feature library check 和 `--no-default-features --features astra-hosted` library check 均通过。
- Hosted Core Clippy 执行成功，保留上游原有 warning；新 tap-worker 的局部 Clippy 问题已修正，没有为全绿屏蔽警告或清理无关上游源码。
- 使用实际 Family 源码的独立清单：普通 library 测试 3 通过，GPU 项原先忽略；另显式执行加强后的 GPU 像素/resize 回归 1 通过。首次只传短测试名又加 `--exact`，实际筛选出零项，未计通过；随后使用完整模块路径真实执行。
- 实际独立程序使用原 renderer，在 NVIDIA GeForce RTX 4070 Laptop GPU / DiscreteGpu / Vulkan 上执行 `63×47 → 65×49 → 63×47`，全部像素分别精确匹配原色，FramePublisher generation 为 1/2/3；关闭 renderer/publisher 后最初 CPU lease 仍有效。覆盖非 256-byte 行宽的 readback padding 与目标重建，不是商业游戏或正式帧率验收。
- 修改文件单独格式化；全 package fmt check 仍报告未修改的 upstream replay/example/movie/Horizon 等格式差异，未扩大适配提交处理它们。
- 已通过实际 Manager headless 入口运行商业游戏并观察原生标题菜单、非零 PCM 和正常关闭；这不替代路线、原生菜单/槽位、设备听音和四平台验收。完整 Emulator 检查及最后的宿主优化复测见性能审计，不能套用旧基线结果。

许可证和用户自备密钥边界见[第三方说明](THIRD_PARTY_NOTICES.md)。

## Family 与宿主生命周期补齐

Family 改用当前上游已提供的 `new_with_renderer_sync`，避免无实际 await 的包装；同步改动本身未解决 Windows main 栈溢出。Manager 的 GUI/headless 固定 owner 统一一次性保留 8 MiB 栈，实际同一游戏随后正常进入标题菜单并关闭，没有更改原 VM 或为此增加核心提交。

Family 在首个脚本帧前取得原 `persist_global_if_changed(None)` fingerprint，关闭时调用同一个原生 dirty writer，保留原全局/已读/配置编码与错误日志，不另写保存格式；只在启动/关闭边界计算，不逐帧 hash。取消音频后仍执行原生关闭持久化并 drop host，最后清除 tap。

短场景 private bytes 采样和原 D3D12 候选后端/DXGI Live Object 警告均如实记入审计；未证明 GPU 无泄露，不屏蔽警告或以局部像素测试代替长期产品对照。
