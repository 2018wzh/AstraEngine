# RFVP 最小原生适配记录

## 来源与整合边界

核心为 [`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp) 的完整 fork，保留上游历史、workspace、工具、平台与许可证，以 Git submodule 集成于 `Emulator/ThirdParty/rfvp`。本次核对的上游 main HEAD 为 `af557db750c7833274f22a6d82b0dc33cdea2f85`（2026-10-16），crate 版本仍为 0.6.0。唯一适配提交为 `d1ff965d79ba2cdc956aad73ae100cc44bc7ce90`，其直接父提交即上游 HEAD，提交计数为 1；分支为 `codex/astra-upstream-head-20261016`。主仓以 `160000` 精确 gitlink 固定，未发布远端。

旧适配保存于独占旧分支提交 `a721a8afc62059010004a6afbd20668b91288ed1`，不重写来源或已发布历史。旧基线差异与测试不作为新 HEAD 的验收证据。

## 核心保留与必要改动

- 保留上游原 VM、协程、GPU primitive renderer、mask 管线、NVSG/字体、视频解码/合成、音轨和保存实现。`hosted` 使用原 std 运行依赖与原生视频，不再启用另一套 no_std 图像/字体实现。原桌面默认 feature 保留。
- `no_std_core` 只补充嵌入需要的实际生命周期、输入/时间边界、字体注入、资源读取、原生音频命令后端与 session-owned globals。成功读档通过显式 reset 通知 Host；普通帧 elapsed 规则不放宽。
- `rfvp_render/hosted.rs` 调用同一 `GpuPrimRenderer` 与 `RenderTarget`，复用有界 GPU 回读资源并分开提交/完成。保存缩略图在脚本绘制新状态之前读取原 render target，沿用原 Triangle 缩放；mask 使用上游已有 snapshot 接口，没有额外 shader 或第二 renderer。
- 原桌面 `app.rs` 的保存回读调用迁移到同一可复用 readback 接口。`gpu_prim.rs` 两处固定容量 visit scratch 改为栈数组，绘制算法不变。
- 原视频 worker 的所有者在关闭时先终止接收、发出停止并 join，避免有界帧队列阻塞退出；不复制 decoder 或更换播放时间线。
- Native Movie 独立使用保留 stream ID，不挤占四个原生 BGM slot。Family mixer 继续限制活动声音，暂停和淡出仍占容量。
- `na_mpeg2_decoder` 包版本调整为 0.1.1，仅区分同一 workspace 中 Siglus 的同名 0.1.0 路径包；解码实现未改。

## 保存契约：不增加旧存档处理

直接沿用上游 `SaveStateSnapshotV1` / `MotionManagerSnapshotV1`、`GlobalSaveDataV1`、RFVS/RFVG 与原生读写流程。当前尚未投入使用，因此删除旧 V2 扩展、独立 hosted codec、旧格式识别/迁移/兼容 reader 和专门版本报错；不修改既有商业存档。

原记录将旧 fork 的 V2 演出布局误称为上游布局，现予纠正：上游旧基线 `304e7733` 与本次 `af557db7` 均为 V1，旧 fork `b2f385da` 为 V2。不会将旧扩展重新带回当前上游。

本次回归另复现两个上游 V1 的实际缺陷：从资产路径重载覆盖运行时坐标，且调色后仅保存路径会丢失实际像素。必要修复只复用 V1 已有 RGBA、坐标及尺寸字段：修改过的像素仅在保存边界捕获，读档优先已有像素，源资产重载后恢复保存的几何。不新增 schema、版本分支或逐帧状态复制。这是当前存档正确性修复，不是旧存档兼容。

## Family 职责

Astra 专用命令收集与帧变化跟踪保留在 `src/embedding/`，不放入核心。跟踪只保留原生绘制元数据与资源身份，不复制纹理 payload、不计算像素 hash、不重建场景渲染。删除旧 Evidence、指令跟踪、文本事务队列、复制计数与无消费者的 scene-delta/矩形 damage 接口。

`NativeFileSystem` 提供实际资源和原子保存；系统字体由 fontdb 解析原生 slot 所需的字体及 TTC index，核心保留原 optional-slot fallback。最终帧使用硬件 GPU 与不可变 CPU lease；alpha 声明为 Ignored，字节不改写。音频 worker 使用原 SoftAudioMixer、有界 PCM 和独立控制通道；关闭先取消受阻 sink，再等待 worker。影片不再由 Family 的第二个 WMV 播放器覆盖游戏帧，全部经过核心原生视频合成与音轨路径。

诊断经共享 Family 日志桥进入 Manager tracing，不另建核心文件 sink。Family ABI 为 v13，配置保留 `script_encoding`。未声明翻译 capability。

## 验证与未验收范围

本轮迁移的实际命令、失败前/修复后证据、最终提交与检查结果统一记录在[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md#上游-head-单适配提交整合进行中)。历史完整 workspace 通过不能替代新上游 HEAD 的检查。商业结局、设备听音、四平台与原生/Hosted 同场景性能对照未完成，不以非商业夹具或静态检查冒充验收。

实际商业游戏补齐了小 GPU 夹具未覆盖的 CPU 驻留边界：便携 host 的元数据通道不能授权像素回收。现在只在同一原 `GpuPrimRenderer` 确认对应 generation 已上传后调用原 `evict_uploaded_cpu_pixels`；缓存缺失使用原恢复函数，不永久保留 CPU 副本、不改写原 GPU cache。完整 Family 的 GraphLoad→原 GPU 像素回归先失败后通过；动态模块 25 项及另 3 项硬件 GPU 回归通过，实际游戏从黑屏修复为可见原生标题菜单并正常关闭。
