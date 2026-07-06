# Creator Manual

创作者从 Project Wizard 创建 AstraVN 项目，导入资产，编写 `.astra`，选择 Luau 策略包，用 Graph/Timeline 调整演出，在 PIE 中调试，最后 Cook、Package、Release Gate。需要扩展项目时，通过 Plugin Manager 启用插件并处理依赖诊断。

## 最短流程

1. 选择 AstraVN template。
2. 导入背景、角色、语音、BGM、字体和 filter profile。
3. 编写 `.astra` story，给生产命令添加稳定 `#@id`。
4. 选择官方策略包或第三方策略包，在 Plugin Manager 确认 load phase、依赖、权限、命令 provider、Inspector 控件和 Timeline track 都能预览。
5. 用 Graph/Timeline 查看同一 story IR，不创建第二套运行逻辑。
6. 在 PIE 中跑 full-flow YAML scenario。
7. 通过 Package/Release Gate 面板生成二进制 package。

脚本机制、Luau 策略和可视化规则见 [AstraVN Script Spec](../modules/astra-vn-script.md)。演出模型、标准命令和系统 UI 见 [AstraVN Presentation Model](../modules/astra-vn-presentation-model.md)、[AstraVN Standard Command Library](../modules/astra-vn-standard-commands.md) 和 [AstraVN System UI Profile](../modules/astra-vn-system-ui-profile.md)。样例见 [AstraVN Script Sample](../samples/astra-vn-script/README.md) 和 [AstraVN Advanced Presentation Sample](../samples/astra-vn-advanced/README.md)。

## AI

项目授权后，AI 可以直写 `.astra`、Luau 策略和 Graph/Timeline 派生层。每次写入都必须生成 audit event、patch 或 graph diff、undo checkpoint 和 release check。

## Reference

| 任务 | 入口 | 验收 |
| --- | --- | --- |
| 创建项目 | Project Wizard | 生成 `project.yaml`、source tree、policy binding |
| 导入资产 | Content Browser / Import Wizard | sidecar、import audit、cook artifact 都可追踪 |
| 编辑脚本 | Script Editor + Graph/Timeline | source map identity 保持一致 |
| 管理插件 | Plugin Manager / Project Settings | dependency graph、enablement、extension point、diagnostic jump 可解释 |
| 调试 | PIE + Debugger | diagnostic 能跳到 source_ref |
| 发布 | Package / Release Gate | `astra.release_report.v1` pass |
