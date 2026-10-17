# RFVP 上游差异

## 固定来源

- 完整 fork：[xmoezzz/rfvp](https://github.com/xmoezzz/rfvp)，主仓 submodule：`Emulator/ThirdParty/rfvp`。
- 上游 HEAD（2026-10-17 核对，0.6.0）：`af557db750c7833274f22a6d82b0dc33cdea2f85`。
- 唯一适配提交：[`dcd39c23ca59b38a9a96b6f301aeb05121aa3e18`](https://github.com/2018wzh/rfvp/commit/dcd39c23ca59b38a9a96b6f301aeb05121aa3e18)，直接父提交为上述 HEAD；统一适配分支 `astra` 已推送 GitHub，远端 SHA 与主仓 gitlink 一致。
- 来源历史、workspace、工具和许可证保留；不改写旧分支。

## 差异分类

| 类别 | 当前内容 | 处置 |
| --- | --- | --- |
| 嵌入入口 | hosted 使用原 std 类型；字体/TTC 注入、资源读取、session globals、原音频状态接线、成功恢复通知 | 按实际消费者保留必要钩子，保持原操作语义 |
| GPU 输出 | 同一 GpuPrimRenderer/RenderTarget；单 staging、submit/poll-into、原期限和代次取消；原目标同步捕获保存缩略图 | Family 三槽预留后提交，完成后沿用同一 Arc 分配发布 |
| 资源与关闭 | 仅原 GPU cache 授权像素回收；原 Movie worker 停止接收后取消/join；独立 Movie stream ID | 保留实际所有权，不加第二播放器或永久 CPU 副本 |
| 通用功能修复 | V1 修改像素/几何恢复、TextResume/协程取消与活跃文字等待取消 | 已独立提交原生 PR #51/#53，保留到上游合并 |
| 构建身份 | na_mpeg2_decoder 版本 0.1.1 区分同名路径包 | 不改解码算法；不把包身份区分当作产物命名问题已解决 |

## Family 与保存边界

Family 的 `src/embedding/` 收集原音频命令和绘制元数据，后者仅作变化/hit-proxy 观察；原 GPU 负责像素上传与绘制。NativeFileSystem 提供资源与原子保存；fontdb 解析实际系统字体和 TTC face。最终输出为不可变 CPU lease、有界 PCM，日志走共享桥。

三个 `Arc<Vec<u8>>` 槽有明确写 owner；外部持满三个 lease 时不提交 GPU map，返回 Pending。释放一槽后 acquire 可在没有新 advance 时提交仍 dirty 的图像。初帧同步 capture 的 Vec 直接移入首槽；reset、原生保存捕获导致的取消、失败和 close 释放预留，旧 lease 保持不可变。同步 `map_to_rgba8/try_map_to_rgba8` 保留原生签名，内部复用同一 read-into 完成路径。

保存沿用上游 `SaveStateSnapshotV1` / `MotionManagerSnapshotV1`、RFVS、`GlobalSaveDataV1`/RFVG。旧 fork 的 V2 不是上游格式；旧扩展、重复 codec、版本识别/迁移/专门报错已删除。当前 V1 图像修复仍须按功能修复分类，不因此成为纯嵌入钩子。

hosted 的 SaveCreate/yield 捕获暂停属于嵌入边界；原生控制组未证明通用缺陷。Family 回归校验保存 PC、缩略图、整张恢复像素与恢复后继续。电影/保存测试按三槽契约及时释放已消费 lease，不要求第四槽。

## 结论与验证入口

原生功能 PR 等待合并，严格最小钩子内容验收仍有上游依赖。当前 Family lib 29 项全部实际通过，包含四项原 GPU 回归、三槽背压与关闭、电影 EOF 和保存/恢复；真实 rlib 外部硬件 probe 两项验证 map abort、受控 timeout、取消代次及 read-into 恢复，临时导出已删除。Release Manager 自制场景精确验证 4×2 白色与相邻黑色像素，实际《樱花萌放》开场到正文截图、非零 PCM 和退出 0 已观察，发行/补丁修订未确认。Hosted 整改前后三轮成本与 Sandbox 验收见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md)及[实施计划](../../../../Docs/status/implementation-plan.md)。原版性能测试按用户决定取消。许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
