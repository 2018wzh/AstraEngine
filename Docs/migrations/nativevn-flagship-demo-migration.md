# NativeVN Flagship Demo Migration

**状态：`IN_PROGRESS`**

本迁移只记录未来旗舰 Demo 的产品目标，不在 Migration 6/9 与 Stage 3 产品链收口中实现。当前 `Examples/NativeVN` 只保留两路线技术验收内容；此前 WIP 中的三路线剧情、26 条 Windows SAPI 配音和大体量生成素材已从提交候选中移除。SAPI 输出没有明确再分发许可，不能进入公开样例。

## 目标

未来旗舰 Demo 的目标时长为 15–20 分钟，包含三条终局路线、中英双语、中文全配音和正式原创资产。剧情、配音和素材必须在独立 migration 中完成版权、provenance、sidecar、hash、byte size、localization、可访问性与 release review。

## 进入实现前的 gate

- 剧情与全部 localization 文本完成编辑审校，并确认可公开再分发。
- 中文配音采用明确允许再分发的录制或授权方案；不接受系统 TTS/SAPI 产物替代。
- 图片、字体、语音、BGM、SE 和视频均有独立 sidecar、license、provenance、SHA-256 与 byte size。
- 所有资源从 package VFS 读取，构建期不联网，不引用本地绝对路径。
- Windows/Web 使用 formal Stage 3 runner 通过完整路线、系统流、save/restart/load、视觉变化、音频 callback 与 fence。
- 原始录音、工程文件和调试 artifact 只放 ignored 私有目录；Git 只提交获准发布的成品与脱敏 evidence。

## 状态关系

`S3-FLAGSHIP-DEMO-01` 保持 `IN_PROGRESS`。它与 `S3-TSUI-INTERNAL-DEMO-01`、`S3-TSUI-GATE-01` 一起阻断 Stage 3 顶层关闭，但不反向阻断 Migration 6 或 Migration 9 的共享基础。
