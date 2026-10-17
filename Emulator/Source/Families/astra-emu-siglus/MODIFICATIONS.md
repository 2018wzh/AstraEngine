# Siglus 上游差异

## 固定来源

- 完整 fork：[xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs)，主仓 submodule：`Emulator/ThirdParty/siglus_rs`。
- 上游 HEAD（2026-10-17 核对，1.0.0）：`5f358ef1360b0439e92aed2b749e8f25d8721954`。
- 唯一适配提交：[`29fb9582751fc8ddfc23b374d428d6eba30bcb40`](https://github.com/2018wzh/siglus_rs/commit/29fb9582751fc8ddfc23b374d428d6eba30bcb40)，直接父提交为上述 HEAD；统一适配分支 `astra` 已推送 GitHub，远端 SHA 与主仓 gitlink 一致。
- 来源历史、原生平台与许可证保留；不改写旧分支。

## 核心差异

- `astra-hosted`：Family 关闭默认 feature，接入原 Kira mixer 的 stereo F32 输出；原桌面默认 CPAL 与解码 features 保留，配置限于消费 crate。
- Renderer 的 surface 可选，离屏与窗口共用初始化和 `render_frame_to_view`。保留原 sprite/wipe/shader/Emote/几何及 D3D9 byte-space 合成语义；离屏选择硬件 adapter。
- 原生离屏成功绘制后递增 generation，跨 target resize 连续。Family 无新提交时不回读，不用 VM tick 或像素 hash 代替。
- 单 staging 回读采用 `poll_frame_rgba(&mut Vec<u8>)` / `cancel_frame_readback`：首 poll 提交 copy/map，后续仅 `Maintain::Poll`；返回捕获时的尺寸与 generation。原子代次隔离取消后的晚回调，resize/Drop 取消旧 map。
- 每 mixer 独立取消，先解除阻塞写再 join；替换 mixer 不取消 session sink。共享注册表使用 parking_lot，诊断复用原 log。
- 成功 load、返回菜单和 restart 增加一次性 reset 通知；原 VM/过程流与实时时钟保留，不启用 replay-only virtual-clock。
- WMV decoder 包版本 0.2.1 区分同名路径包，库产物名 `siglus_wmv_decoder` 避免与 RFVP 的 rlib/cdylib/staticlib 相互覆盖；VM 依赖别名和原生 CLI 目标保留，三个 CLI 的库引用随目标名迁移。解码算法与 features 不变。此前整组 Release 的 LNK2019 已复现，改名后相同四包命令构建通过且不再出现产物 collision。
- 三个保留的原生 decoder CLI 已启用 `desktop-bins` 实际构建；原 `wmv-decoder` 入口解码单 ASF packet 的自制 WMV2 样本，输出五张 16×16 PNG、1920 字节原生 YUV，退出 0。此为改名后的真实调用验证，不把它当作商业电影流程或 WMV3/WVC1 验收。

## Family 与 Host 边界

Family 负责 ABI、输入、PCM 队列和 CPU lease，调用原 `new_with_renderer_sync`；三个 `Arc<Vec<u8>>` 槽在提交前预留，外部持满三个 lease 时返回 Pending，不复制或扩为第四槽。map 使用同一五秒期限，resize/poll 不续期；reset/close 取消预留。boot 复用相同 poll 路径，每次未完成 sleep 1 ms。原 global/config/read 持久化在启动/关闭边界调用 `persist_global_if_changed`，不逐帧 hash 或复制 codec。Manager owner 一次性保留 8 MiB 栈，创建、推进、取帧和关闭保持同线程。

## 结论与验证入口

RTX 4070 Laptop GPU / Vulkan 硬件回归验证非对齐行与 resize 像素、捕获 generation、三槽背压/指针复用/旧 lease 不变、跨 resize 固定期限、超时与取消恢复，共三项；原生 map 主动中止/取消后再提交一项通过。Family lib 六项通过，实际 Rewrite+ 输入/画面/PCM/关闭重开一项通过。样本执行原版 `Gameexe.dat` / `Scene.pck`，副本附带的汉化 V2 `.chs` 文件未启用。Release Manager 实跑 3000 帧，观察到 1280×720 动画标题，退出 0，NullAudio 收到非零 PCM。Hosted 整改前后成本、真实设备音频、Sandbox 窗口及长期资源验收见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)及[实施计划](../../../../Docs/status/implementation-plan.md)。原版性能测试按用户决定取消。许可及自备密钥边界见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
