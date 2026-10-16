# RFVP 上游差异

## 固定来源

- 完整 fork：[xmoezzz/rfvp](https://github.com/xmoezzz/rfvp)，主仓 submodule：`Emulator/ThirdParty/rfvp`。
- 上游 HEAD（2026-10-16，0.6.0）：`af557db750c7833274f22a6d82b0dc33cdea2f85`。
- 唯一适配提交：`d1ff965d79ba2cdc956aad73ae100cc44bc7ce90`，直接父提交为上述 HEAD；分支 `codex/astra-upstream-head-20261016`，本地整合、未发布。
- 来源历史、workspace、工具和许可证保留；不改写旧分支。

## 差异分类

| 类别 | 当前内容 | 处置 |
| --- | --- | --- |
| 嵌入入口 | hosted 使用原 std 类型；字体/TTC 注入、资源读取、session globals、原音频状态接线、成功恢复通知 | 按实际消费者保留必要钩子，保持原操作语义 |
| GPU 输出 | 同一 GpuPrimRenderer/RenderTarget；可复用 staging buffer、分离 submit/poll、期限和代次取消；原目标捕获保存缩略图 | 保留；动态 CPU Vec 分配纳入基线 |
| 资源与关闭 | 仅原 GPU cache 授权像素回收；原 Movie worker 停止接收后取消/join；独立 Movie stream ID | 保留实际所有权，不加第二播放器或永久 CPU 副本 |
| 通用功能修复 | V1 修改像素/几何恢复、TextResume/协程取消、SaveCreate 捕获时点 | 审计 R02：逐项归因，原生缺陷独立上游修复 |
| 非必要差异/回归 | 无消费者 sha2、额外 tracing、通用 scratch/文件策略、格式差异；纯 no_std 缺输入提交 | 审计 R01/R05：恢复原路径，清理越界改动 |
| 构建身份 | na_mpeg2_decoder 版本 0.1.1 区分同名路径包 | 不改解码算法；不把包身份区分当作产物命名问题已解决 |

## Family 与保存边界

Family 的 `src/embedding/` 收集原音频命令和绘制元数据，后者仅作变化/hit-proxy 观察；原 GPU 负责像素上传与绘制。NativeFileSystem 提供资源与原子保存；fontdb 解析实际系统字体和 TTC face。最终输出为不可变 CPU lease、有界 PCM，日志走共享桥。

保存沿用上游 `SaveStateSnapshotV1` / `MotionManagerSnapshotV1`、RFVS、`GlobalSaveDataV1`/RFVG。旧 fork 的 V2 不是上游格式；旧扩展、重复 codec、版本识别/迁移/专门报错已删除。当前 V1 图像修复仍须按功能修复分类，不因此成为纯嵌入钩子。

## 结论与验证入口

**尚未通过严格最小适配验收。** 单提交/submodule 结构通过；当前硬件 GPU 游戏可到标题及剧情，不能覆盖 R01 的原 no_std 路径或完整结局。定向测试、实际游戏、原生全测试编译限制和未验收范围统一见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)，下一步见[实施计划](../../../../Docs/status/implementation-plan.md)。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
